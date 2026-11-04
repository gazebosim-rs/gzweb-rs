// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
//! Opt-in bounded world edits; all values retain the server's world Z-up basis.
use super::{Scene, MAX_VISUALS};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Template {
    pub template_id: u64,
    pub label: String,
}
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Model {
    pub model_id: u64,
    pub name: String,
    pub visual_ids: Vec<u64>,
    pub can_remove: bool,
    pub can_set_state: bool,
}
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub version: u32,
    pub world_id: u64,
    pub epoch: u64,
    pub scene_sequence: u64,
    pub templates: Vec<Template>,
    pub models: Vec<Model>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub translation: [f64; 3],
    pub rotation: [f64; 4],
    pub linear_velocity: [f64; 3],
    pub angular_velocity: [f64; 3],
}
impl Default for State {
    fn default() -> Self {
        Self {
            translation: [0.; 3],
            rotation: [0., 0., 0., 1.],
            linear_velocity: [0.; 3],
            angular_velocity: [0.; 3],
        }
    }
}
impl State {
    pub fn validate(&self) -> Result<(), String> {
        let norm = self.rotation.iter().map(|x| x * x).sum::<f64>();
        if !self
            .translation
            .iter()
            .chain(&self.linear_velocity)
            .chain(&self.angular_velocity)
            .chain(&self.rotation)
            .all(|v| v.is_finite())
            || !norm.is_finite()
            || (norm - 1.).abs() > 1e-12
            || self.translation.iter().any(|v| !(*v as f32).is_finite())
        {
            return Err("Invalid finite world pose/velocity or unit quaternion".into());
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Spawn,
    Remove,
    GetState,
    SetState,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Command {
    Spawn {
        template_id: u64,
        translation: [f64; 3],
        rotation: [f64; 4],
    },
    Remove {
        model_id: u64,
    },
    GetState {
        model_id: u64,
    },
    SetState {
        model_id: u64,
        #[serde(flatten)]
        state: State,
    },
}
impl Command {
    pub fn action(&self) -> Action {
        match self {
            Self::Spawn { .. } => Action::Spawn,
            Self::Remove { .. } => Action::Remove,
            Self::GetState { .. } => Action::GetState,
            Self::SetState { .. } => Action::SetState,
        }
    }
    pub fn model(&self) -> Option<u64> {
        match self {
            Self::Spawn { .. } => None,
            Self::Remove { model_id }
            | Self::GetState { model_id }
            | Self::SetState { model_id, .. } => Some(*model_id),
        }
    }
    pub fn validate(&self, catalog: &Catalog) -> Result<(), String> {
        match self {
            Self::Spawn {
                template_id,
                translation,
                rotation,
            } => {
                if !catalog
                    .templates
                    .iter()
                    .any(|t| t.template_id == *template_id)
                {
                    return Err("Unknown declared template".into());
                }
                State {
                    translation: *translation,
                    rotation: *rotation,
                    ..State::default()
                }
                .validate()?;
            }
            Self::Remove { model_id }
            | Self::GetState { model_id }
            | Self::SetState { model_id, .. } => {
                let model = catalog
                    .models
                    .iter()
                    .find(|m| m.model_id == *model_id)
                    .ok_or("Unknown catalog model")?;
                if matches!(self, Self::Remove { .. }) && !model.can_remove
                    || matches!(self, Self::SetState { .. }) && !model.can_set_state
                {
                    return Err("Model does not admit this operation".into());
                }
                if let Self::SetState { state, .. } = self {
                    state.validate()?;
                }
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub version: u32,
    pub request_id: u64,
    pub action: Action,
    pub world_id: u64,
    pub epoch: u64,
    pub scene_sequence: u64,
    pub seconds: u64,
    pub nanoseconds: u32,
    pub paused: bool,
    pub ok: bool,
    #[serde(deserialize_with = "required_nullable")]
    pub error: Option<String>,
    #[serde(deserialize_with = "required_nullable")]
    pub model_id: Option<u64>,
    #[serde(deserialize_with = "required_nullable")]
    pub state: Option<State>,
}
impl Catalog {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1
            || self.world_id == 0
            || self.world_id > u32::MAX as u64
            || self.templates.len() > MAX_VISUALS
            || self.models.len() > MAX_VISUALS
        {
            return Err("Invalid lifecycle catalog envelope or bounds".into());
        }
        let mut ids = BTreeSet::new();
        for t in &self.templates {
            if t.template_id == 0 || !ids.insert(t.template_id) || t.label.len() > 256 {
                return Err("Invalid template identity or label".into());
            }
        }
        ids.clear();
        let mut visuals = BTreeSet::new();
        for m in &self.models {
            if m.model_id == 0
                || m.model_id == self.world_id
                || !ids.insert(m.model_id)
                || m.name.len() > 256
                || m.visual_ids.is_empty()
                || m.visual_ids.len() > MAX_VISUALS
            {
                return Err("Invalid model identity or name".into());
            }
            for v in &m.visual_ids {
                if *v == 0 || *v > u32::MAX as u64 || !visuals.insert(*v) {
                    return Err("Invalid or shared visual owner".into());
                }
            }
        }
        if ids.iter().any(|id| visuals.contains(id))
            || visuals.contains(&self.world_id)
            || visuals.len() > MAX_VISUALS
        {
            return Err("Too many lifecycle visuals".into());
        }
        Ok(())
    }
    pub fn matches_scene(&self, scene: &Scene) -> bool {
        self.world_id == scene.world_id
            && self.epoch == scene.epoch
            && self.scene_sequence == scene.sequence
            && self
                .models
                .iter()
                .all(|m| !scene.visuals.iter().any(|v| v.id == m.model_id))
            && self
                .models
                .iter()
                .flat_map(|m| &m.visual_ids)
                .all(|id| scene.visuals.iter().any(|v| v.id == *id))
    }
}
impl Reply {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1
            || self.request_id == 0
            || self.world_id == 0
            || self.world_id > u32::MAX as u64
            || self.nanoseconds >= 1_000_000_000
            || self.ok != self.error.is_none()
            || self
                .error
                .as_ref()
                .is_some_and(|s| s.is_empty() || s.len() > 256)
            || self.model_id == Some(0)
            || self.model_id == Some(self.world_id)
        {
            return Err("Invalid lifecycle acknowledgement".into());
        }
        if let Some(state) = &self.state {
            state.validate()?;
        }
        Ok(())
    }
}

fn required_nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}
