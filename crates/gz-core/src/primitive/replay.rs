// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
//! Bounded playback provenance, distinct from transport clock and live commands.
use super::Scene;
use serde::Deserialize;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RecordedAction {
    Pause,
    Resume,
    Reset,
    Spawn,
    Remove,
    GetState,
    SetState,
}
impl RecordedAction {
    pub fn name(self) -> &'static str {
        match self {
            Self::Pause => "pause",
            Self::Resume => "resume",
            Self::Reset => "reset",
            Self::Spawn => "spawn",
            Self::Remove => "remove",
            Self::GetState => "get_state",
            Self::SetState => "set_state",
        }
    }
}
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Outcome {
    pub action: RecordedAction,
    #[serde(deserialize_with = "required_nullable")]
    pub model_id: Option<u64>,
    pub ok: bool,
}
fn required_nullable<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
    Option::deserialize(d)
}
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReplayStatus {
    pub version: u32,
    pub world_id: u64,
    pub epoch: u64,
    pub scene_sequence: u64,
    pub frame: u32,
    pub frames: u32,
    pub recorded_epoch: u64,
    pub recorded_sequence: u64,
    pub recorded_seconds: u64,
    pub recorded_nanoseconds: u32,
    pub recorded_paused: bool,
    pub playing: bool,
    pub finished: bool,
    pub outcomes: Vec<Outcome>,
}
impl ReplayStatus {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1
            || self.world_id == 0
            || self.world_id > u64::from(u32::MAX)
            || self.frame == 0
            || self.frames == 0
            || self.frame > self.frames
            || self.frames > 4096
            || self.recorded_nanoseconds >= 1_000_000_000
            || self.outcomes.len() > 8
            || (self.finished && (self.playing || self.frame != self.frames))
        {
            return Err("Invalid bounded replay status".into());
        }
        for outcome in &self.outcomes {
            let world = matches!(
                outcome.action,
                RecordedAction::Pause | RecordedAction::Resume | RecordedAction::Reset
            );
            if (world && outcome.model_id.is_some())
                || (!world && outcome.model_id == Some(0))
                || (!world
                    && outcome.ok
                    && outcome.model_id.is_none_or(|id| id > u64::from(u32::MAX)))
            {
                return Err("Invalid recorded outcome identity".into());
            }
        }
        Ok(())
    }
    pub fn matches_scene(&self, scene: &Scene) -> bool {
        self.world_id == scene.world_id
            && self.epoch == scene.epoch
            && self.scene_sequence == scene.sequence
            && self.recorded_seconds == scene.seconds
            && self.recorded_nanoseconds == scene.nanoseconds
            && self.playing != scene.paused
    }
}
