//! The Scene panel (spec §3.3, §5.7): the world's entity tree, which M4
//! fills. Until then its body shows the connection: skeleton rows while
//! the first scene arrives, the error with Reconnect once it's lost, and
//! "No world loaded" before; its header counts the entities.

mod view;

use bevy::prelude::*;

use super::{Panel, PanelDef, RegisterPanel};
use crate::assets::icons::IconId;

/// Registers the Scene panel.
pub struct ScenePanelPlugin;

impl Plugin for ScenePanelPlugin {
    fn build(&self, app: &mut App) {
        app.register_panel(
            PanelDef::new(Panel::Scene, "Scene", IconId::ActivityScene, 0)
                .binding("panel.scene")
                .view(view::state)
                .count(view::count),
        );
    }
}

#[cfg(test)]
mod tests;
