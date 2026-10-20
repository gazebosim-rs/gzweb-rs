//! The Scene panel (spec §3.3, §5.7): the world's entity tree, which M4
//! fills. Until then its body shows the connection: skeleton rows while
//! the first scene arrives, the error with Reconnect once it's lost, and
//! "No world loaded" before; its header counts the entities. It opens at
//! start.

mod view;

use bevy::prelude::*;

use super::{PanelDef, PanelId, RegisterPanel, WORLD_GROUP};
use crate::assets::icons::IconId;
use crate::shell::ShellSystems;

/// The Scene panel's id.
pub const SCENE: PanelId = PanelId::new("scene");

/// Registers the Scene panel and its entity count.
pub struct ScenePanelPlugin;

impl Plugin for ScenePanelPlugin {
    fn build(&self, app: &mut App) {
        app.register_panel(
            PanelDef::new(SCENE, "Scene", IconId::ActivityScene, WORLD_GROUP, 0)
                .binding("panel.scene")
                .default_open()
                .view(view::state),
        )
        .add_systems(Update, view::entity_count.in_set(ShellSystems::PanelStatus));
    }
}

#[cfg(test)]
mod tests;
