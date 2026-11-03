// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
//! The bounded local M1 viewer contract; no simulator or transport runtime.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MAX_SCENE_BYTES: usize = 64 * 1024;
pub const MAX_VISUALS: usize = 128;

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Visual {
    pub id: u64,
    pub name: String,
    pub translation: [f64; 3],
    pub rotation: [f64; 4],
    pub shape: Shape,
    pub color: [f64; 4],
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Shape {
    Box { size: [f64; 3] },
    Sphere { radius: f64 },
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Scene {
    pub version: u32,
    pub world: String,
    pub world_id: u64,
    pub epoch: u64,
    pub seconds: u64,
    pub nanoseconds: u32,
    pub sequence: u64,
    pub paused: bool,
    pub visuals: Vec<Visual>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum NullableError {
    Text(String),
    None,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ack {
    pub version: u32,
    pub request_id: u64,
    pub ok: bool,
    pub epoch: u64,
    pub seconds: u64,
    pub nanoseconds: u32,
    pub paused: bool,
    pub error: NullableError,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ServerMessage {
    Scene(Scene),
    Ack(Ack),
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Pause,
    Resume,
    Reset,
}

#[derive(Debug, Serialize)]
pub struct Control {
    version: u32,
    kind: &'static str,
    request_id: u64,
    action: Action,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Stamp {
    pub epoch: u64,
    pub seconds: u64,
    pub nanoseconds: u32,
}

impl Scene {
    pub fn stamp(&self) -> Stamp {
        Stamp {
            epoch: self.epoch,
            seconds: self.seconds,
            nanoseconds: self.nanoseconds,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1
            || self.world != "default"
            || self.world_id == 0
            || self.world_id > u32::MAX as u64
            || self.nanoseconds >= 1_000_000_000
        {
            return Err("Unsupported scene version/world or invalid integer timestamp".into());
        }
        if self.visuals.len() > MAX_VISUALS {
            return Err("Scene exceeds 128 visuals".into());
        }
        let mut ids = BTreeSet::new();
        for visual in &self.visuals {
            let norm = visual.rotation.iter().map(|v| v * v).sum::<f64>();
            if visual.id == 0
                || visual.id > u32::MAX as u64
                || visual.id == self.world_id
                || !ids.insert(visual.id)
                || visual.name.len() > 256
                || visual
                    .translation
                    .iter()
                    .any(|v| !v.is_finite() || !(*v as f32).is_finite())
                || visual.rotation.iter().any(|v| !v.is_finite())
                || !norm.is_finite()
                || (norm - 1.0).abs() > 1e-6
                || visual.color[3] != 1.0
                || visual
                    .color
                    .iter()
                    .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
            {
                return Err("Invalid visual ID/name, pose, unit quaternion or RGBA".into());
            }
            let dimensions: &[f64] = match &visual.shape {
                Shape::Box { size } => size,
                Shape::Sphere { radius } => std::slice::from_ref(radius),
            };
            if dimensions.iter().any(|v| {
                !v.is_finite() || *v <= 0.0 || !(*v as f32).is_finite() || *v as f32 <= 0.0
            }) {
                return Err(
                    "Primitive dimensions are outside the finite positive rendering domain".into(),
                );
            }
        }
        Ok(())
    }
}

impl Ack {
    fn validate(&self) -> Result<(), String> {
        if self.version != 1
            || self.request_id == 0
            || self.nanoseconds >= 1_000_000_000
            || self.ok != matches!(self.error, NullableError::None)
        {
            return Err("Invalid acknowledgement metadata".into());
        }
        Ok(())
    }
}

pub fn parse(text: &str) -> Result<ServerMessage, String> {
    if text.len() > MAX_SCENE_BYTES {
        return Err("Server message exceeds 64 KiB".into());
    }
    let message: ServerMessage =
        serde_json::from_str(text).map_err(|e| format!("Invalid server JSON: {e}"))?;
    match &message {
        ServerMessage::Scene(scene) => scene.validate()?,
        ServerMessage::Ack(ack) => ack.validate()?,
    }
    Ok(message)
}

#[derive(Clone, Copy)]
struct Current {
    world_id: u64,
    stamp: Stamp,
    sequence: u64,
    paused: bool,
}

#[derive(Clone, Copy)]
struct Pending {
    id: u64,
    action: Action,
    stamp: Stamp,
}

/// One coalesced complete scene and at most one control. Callbacks never mutate
/// Bevy entities. Disconnection invalidates the scene before reconnect begins.
pub struct Session {
    pub connected: bool,
    pub status: String,
    pub notice: Option<String>,
    pub clear_scene: bool,
    pub latest: Option<Scene>,
    current: Option<Current>,
    fence: Option<(Stamp, bool)>,
    pending: Option<Pending>,
    next_request: Option<u64>,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            connected: false,
            status: "Disconnected".into(),
            notice: None,
            clear_scene: true,
            latest: None,
            current: None,
            fence: None,
            pending: None,
            next_request: Some(1),
        }
    }
}

impl Session {
    pub fn disconnect(&mut self, reason: impl Into<String>) {
        self.connected = false;
        self.status = reason.into();
        if self.pending.is_some() {
            self.status.push_str("; pending control outcome unknown");
        }
        self.notice = None;
        self.clear_scene = true;
        self.latest = None;
        self.current = None;
        self.fence = None;
        self.pending = None;
    }

    pub fn opened(&mut self) {
        self.connected = true;
        self.status = "Connected; waiting for a complete scene".into();
    }

    pub fn pending_control(&self) -> bool {
        self.pending.is_some()
    }

    pub fn can_control(&self) -> bool {
        self.connected
            && self.current.is_some()
            && self.pending.is_none()
            && self.next_request.is_some()
    }

    pub fn clock(&self) -> Option<(Stamp, bool)> {
        self.fence
            .or_else(|| self.current.map(|c| (c.stamp, c.paused)))
    }

    pub fn paused(&self) -> Option<bool> {
        self.fence
            .map(|(_, paused)| paused)
            .or_else(|| self.current.map(|c| c.paused))
    }

    pub fn control(&mut self, action: Action) -> Result<String, String> {
        if !self.can_control() {
            return Err("A connected scene and no pending control are required".into());
        }
        let id = self
            .next_request
            .ok_or("Request IDs exhausted; reload the viewer")?;
        let text = serde_json::to_string(&Control {
            version: 1,
            kind: "control",
            request_id: id,
            action,
        })
        .map_err(|e| e.to_string())?;
        self.notice = None;
        self.pending = Some(Pending {
            id,
            action,
            stamp: self.clock().ok_or("No committed scene")?.0,
        });
        self.next_request = id.checked_add(1);
        self.status = format!("Waiting for server acknowledgement: {action:?} #{id}");
        Ok(text)
    }

    pub fn receive(&mut self, message: ServerMessage) -> Result<(), String> {
        if !self.connected {
            return Err("Message arrived without an open connection".into());
        }
        match message {
            ServerMessage::Scene(scene) => {
                scene.validate()?;
                if let Some(current) = self.current {
                    if current.world_id == scene.world_id {
                        if scene.epoch < current.stamp.epoch
                            || (scene.epoch == current.stamp.epoch
                                && scene.sequence <= current.sequence)
                        {
                            return Ok(());
                        }
                        if scene.stamp() < current.stamp {
                            return Err("Scene clock moved backwards within its epoch".into());
                        }
                    } else {
                        if self.pending.is_some() {
                            return Err("World changed while a control was pending".into());
                        }
                        self.clear_scene = true;
                        self.fence = None;
                    }
                }
                if let Some((stamp, paused)) = self.fence {
                    if scene.stamp() < stamp || (scene.stamp() == stamp && scene.paused != paused) {
                        return Ok(());
                    }
                    self.fence = None;
                }
                self.current = Some(Current {
                    world_id: scene.world_id,
                    stamp: scene.stamp(),
                    sequence: scene.sequence,
                    paused: scene.paused,
                });
                if self.pending.is_none() {
                    self.status = if scene.paused {
                        "Connected; server paused"
                    } else {
                        "Connected; server running"
                    }
                    .into();
                }
                self.latest = Some(scene);
            }
            ServerMessage::Ack(ack) => {
                ack.validate()?;
                let pending = self.pending.ok_or("Unsolicited acknowledgement")?;
                if ack.request_id != pending.id {
                    return Err("Acknowledgement request ID does not match".into());
                }
                let stamp = Stamp {
                    epoch: ack.epoch,
                    seconds: ack.seconds,
                    nanoseconds: ack.nanoseconds,
                };
                if stamp < pending.stamp
                    || (ack.ok
                        && match pending.action {
                            Action::Pause => !ack.paused,
                            Action::Resume => ack.paused,
                            Action::Reset => {
                                pending.stamp.epoch.checked_add(1) != Some(ack.epoch)
                                    || !ack.paused
                                    || ack.seconds != 0
                                    || ack.nanoseconds != 0
                            }
                        })
                {
                    return Err("Acknowledgement contradicts the committed command".into());
                }
                self.pending = None;
                if self.current.is_none_or(|c| c.stamp <= stamp) {
                    self.fence = Some((stamp, ack.paused));
                }
                if self.current.is_some_and(|c| stamp.epoch > c.stamp.epoch) {
                    self.clear_scene = true;
                    self.latest = None;
                    self.current = None;
                }
                self.notice = Some(match ack.error {
                    NullableError::Text(error) => format!("Server rejected control: {error}"),
                    NullableError::None => format!(
                        "Server acknowledged {:?}: epoch {}, {}.{:09}s, paused={}",
                        pending.action, ack.epoch, ack.seconds, ack.nanoseconds, ack.paused
                    ),
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;

impl Scene {
    /// Flat authoritative world poses remain raw Z-up below GzWorldRoot.
    pub fn description(&self) -> Result<crate::scene::SceneDesc, String> {
        self.validate()?;
        use crate::scene::*;
        Ok(SceneDesc {
            name: self.world.clone(),
            ambient: Some([0.7; 3]),
            background: Some([0.12, 0.14, 0.18, 1.0]),
            sky: None,
            fog: None,
            grid: true,
            shadows: false,
            roots: self
                .visuals
                .iter()
                .map(|v| {
                    let mut material = MaterialDesc::absent();
                    material.diffuse = Some(v.color.map(|x| x as f32));
                    material.present = true;
                    Node {
                        name: v.name.clone(),
                        id: v.id as u32,
                        kind: NodeKind::Visual,
                        path: format!("primitive/{}", v.id),
                        pose: Pose {
                            pos: v.translation,
                            quat: v.rotation,
                        },
                        scale: [1.0; 3],
                        visible: true,
                        light: None,
                        sensors: Vec::new(),
                        children: Vec::new(),
                        visual: Some(VisualDesc {
                            proto_type: "primitive".into(),
                            geometry: match v.shape {
                                Shape::Box { size } => GeometryDesc::Box { size },
                                Shape::Sphere { radius } => GeometryDesc::Sphere { radius },
                            },
                            material,
                            cast_shadows: false,
                            transparency: 0.0,
                        }),
                    }
                })
                .collect(),
        })
    }
    pub fn same_layout(&self, other: &Self) -> bool {
        self.world_id == other.world_id
            && self.epoch == other.epoch
            && self.visuals.len() == other.visuals.len()
            && self.visuals.iter().zip(&other.visuals).all(|(a, b)| {
                a.id == b.id && a.name == b.name && a.shape == b.shape && a.color == b.color
            })
    }
}
