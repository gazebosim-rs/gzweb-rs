// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
//! Parked worker recovery. Admission starts a new worker installation. The
//! following connection supplies either another fence or the restored scene.
use super::{Scene, Stamp};
use serde::Deserialize;

fn valid_token(token: &str) -> bool {
    token.len() == 32
        && token
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorRestored {
    version: u32,
    epoch: u64,
    tick: u64,
    token: String,
}

impl OperatorRestored {
    pub(super) fn validate(&self) -> Result<(), String> {
        if self.version == 1 && valid_token(&self.token) {
            Ok(())
        } else {
            Err("Invalid operator restoration receipt".into())
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OperatorFence {
    version: u32,
    pub epoch: u64,
    pub tick: u64,
    pub token: String,
    pub eligible: bool,
    pub reason: String,
}

impl OperatorFence {
    pub(super) fn validate(&self) -> Result<(), String> {
        if self.version != 1
            || !valid_token(&self.token)
            || self.reason.trim().is_empty()
            || self.reason.len() > 4096
        {
            return Err("Invalid operator fence".into());
        }
        Ok(())
    }

    fn stamp(&self) -> Stamp {
        Stamp {
            epoch: self.epoch,
            seconds: self.tick / 1000,
            nanoseconds: (self.tick % 1000) as u32 * 1_000_000,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorReply {
    version: u32,
    request_id: u64,
    accepted: bool,
    error: super::NullableError,
}

impl OperatorReply {
    pub(super) fn validate(&self) -> Result<(), String> {
        if self.version != 1
            || self.request_id == 0
            || match &self.error {
                super::NullableError::None => !self.accepted,
                super::NullableError::Text(reason) => {
                    self.accepted || reason.trim().is_empty() || reason.len() > 4096
                }
            }
        {
            return Err("Invalid operator reply".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OperatorPhase {
    AwaitingScene,
    Parked,
    Requested(u64),
    Admitted,
    Refused(String),
    Disconnected,
    Confirming,
    AwaitingVerifiedScene,
    Unverified(&'static str),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParkedSession {
    pub fence: OperatorFence,
    pub phase: OperatorPhase,
    world_id: Option<u64>,
}

impl ParkedSession {
    pub fn can_recover(&self) -> bool {
        self.fence.eligible && self.phase == OperatorPhase::Parked
    }

    pub fn label(&self) -> String {
        use OperatorPhase::*;
        let state = match &self.phase {
            Requested(_) => "Recovery requested",
            Admitted | Confirming | AwaitingVerifiedScene => "Waiting for verified restart",
            Refused(reason) => reason,
            Unverified(reason) => {
                return format!(
                    "Unverified restart · expected tick {} · {reason}",
                    self.fence.tick
                )
            }
            Disconnected => "Connection lost",
            _ if !self.fence.eligible => "Inspection required",
            _ => "Workers fenced",
        };
        format!("{state} · tick {} · {}", self.fence.tick, self.fence.reason)
    }

    pub(super) fn disconnected(&mut self) {
        use OperatorPhase::*;
        self.phase = match self.phase {
            Requested(_) | Admitted | Confirming | AwaitingVerifiedScene | Unverified(_) => {
                Confirming
            }
            _ => Disconnected,
        }
    }
}

impl super::Session {
    pub fn enable_operator(&mut self, enabled: bool) -> Result<(), String> {
        if self.connected || (enabled && self.replay_enabled) {
            return Err("Worker operator controls require a live connection boundary".into());
        }
        self.operator_enabled = enabled;
        if !enabled {
            self.operator = None;
        }
        Ok(())
    }

    pub fn recover_workers(&mut self, token: &str) -> Result<String, String> {
        let id = self.next_request.ok_or("Operator request IDs exhausted")?;
        let cut = self.operator.as_mut().ok_or("No parked coordinator")?;
        if !self.connected || !cut.can_recover() || cut.fence.token != token {
            return Err("Recovery requires the currently offered eligible checkpoint".into());
        }
        let request = serde_json::json!({
            "version":1,"kind":"recover_workers","request_id":id,"epoch":cut.fence.epoch,
            "tick":cut.fence.tick,"token":token
        });
        cut.phase = OperatorPhase::Requested(id);
        self.next_request = id.checked_add(1);
        Ok(request.to_string())
    }

    pub(super) fn operator_fence(&mut self, fence: OperatorFence) -> Result<(), String> {
        fence.validate()?;
        if !self.operator_enabled || self.operator_restored_seen {
            return Err("Operator fence without live opt-in".into());
        }
        if let Some(old) = &self.operator {
            if !matches!(
                old.phase,
                OperatorPhase::Disconnected | OperatorPhase::Confirming
            ) {
                return if old.fence == fence {
                    Ok(())
                } else {
                    Err("Fence changed on the same connection".into())
                };
            }
        }
        if self.current.is_some() {
            return Err("Parked coordinator must identify its fence before its scene".into());
        }
        self.operator = Some(ParkedSession {
            fence,
            phase: OperatorPhase::AwaitingScene,
            world_id: None,
        });
        Ok(())
    }

    pub(super) fn operator_restored(&mut self, receipt: OperatorRestored) -> Result<(), String> {
        receipt.validate()?;
        if !self.operator_enabled || self.operator_restored_seen || self.current.is_some() {
            return Err("Restoration receipt must precede the first negotiated scene".into());
        }
        if let Some(p) = &mut self.operator {
            match p.phase {
                OperatorPhase::Confirming => {
                    p.phase = if (receipt.epoch, receipt.tick, &receipt.token)
                        == (p.fence.epoch, p.fence.tick, &p.fence.token)
                    {
                        OperatorPhase::AwaitingVerifiedScene
                    } else {
                        OperatorPhase::Unverified("Recovery identity differs")
                    };
                }
                OperatorPhase::Disconnected => {}
                _ => return Err("Unexpected restoration receipt from parked coordinator".into()),
            }
        }
        self.operator_restored_seen = true;
        Ok(())
    }
    pub(super) fn operator_scene(&mut self, scene: &Scene) -> Result<(), String> {
        let Some(parked) = &mut self.operator else {
            return Ok(());
        };
        match parked.phase {
            OperatorPhase::Disconnected => self.operator = None,
            OperatorPhase::Confirming => {
                parked.phase = OperatorPhase::Unverified("Missing recovery identity");
            }
            OperatorPhase::AwaitingVerifiedScene => {
                if scene.stamp() == parked.fence.stamp()
                    && Some(scene.world_id) == parked.world_id
                    && scene.paused
                {
                    self.operator = None;
                } else {
                    parked.phase = OperatorPhase::Unverified(
                        "Restored scene differs from the requested paused checkpoint",
                    );
                }
            }
            OperatorPhase::Unverified(_) => {}
            _ => {
                if scene.stamp() != parked.fence.stamp()
                    || parked.world_id.is_some_and(|w| w != scene.world_id)
                {
                    return Err("Parked scene differs from its checkpoint".into());
                }
                if parked.phase == OperatorPhase::AwaitingScene {
                    parked.phase = OperatorPhase::Parked;
                    parked.world_id = Some(scene.world_id);
                }
            }
        }
        Ok(())
    }

    pub(super) fn operator_reply(&mut self, reply: OperatorReply) -> Result<(), String> {
        reply.validate()?;
        let parked = self
            .operator
            .as_mut()
            .ok_or("Operator reply outside a parked session")?;
        if parked.phase != OperatorPhase::Requested(reply.request_id) {
            return Err("Unexpected operator request ID".into());
        }
        parked.phase = match reply.error {
            super::NullableError::None => OperatorPhase::Admitted,
            super::NullableError::Text(error) => OperatorPhase::Refused(error),
        };
        Ok(())
    }
}
