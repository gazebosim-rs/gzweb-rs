//! The Scene panel (spec §3.3, §5.7): the world's entity tree, which M4
//! fills. Until then its body shows the connection: skeleton rows while
//! the first scene arrives, the error with Reconnect once it's lost, and
//! "No world loaded" before; its header counts the entities and offers
//! Expand all and Collapse all (disabled until the tree exists). It opens
//! at start.

mod systems;

use bevy::prelude::*;

use super::{ActionId, HeaderAction, PanelDef, PanelId, RegisterPanel, WORLD_GROUP};
use crate::assets::IconId;
use crate::shell::{Phase, ShellSystems};

/// The Scene panel's id.
pub const SCENE: PanelId = PanelId::new("scene");
/// Its header's Expand all (M4).
pub(crate) const EXPAND_ALL: ActionId = ActionId::new("expand_all");
/// Its header's Collapse all (M4).
pub(crate) const COLLAPSE_ALL: ActionId = ActionId::new("collapse_all");
/// Its empty state's Open world… (the World menu, M8).
pub(crate) const OPEN_WORLD: ActionId = ActionId::new("open_world");
/// Its empty state's New world (M8).
pub(crate) const NEW_WORLD: ActionId = ActionId::new("new_world");

/// Registers the Scene panel, its view and its entity count.
pub struct ScenePanelPlugin;

impl Plugin for ScenePanelPlugin {
    fn build(&self, app: &mut App) {
        app.register_panel(
            PanelDef::new(SCENE, "Scene", IconId::ActivityScene)
                .place(WORLD_GROUP, 0)
                .binding("panel.scene")
                .default_open()
                .view(systems::view(Phase::Idle))
                .header_action(
                    HeaderAction::new(EXPAND_ALL, IconId::ExpandAll, "Expand all").disabled(),
                )
                .header_action(
                    HeaderAction::new(COLLAPSE_ALL, IconId::CollapseAll, "Collapse all").disabled(),
                ),
        )
        .add_systems(
            Update,
            (systems::scene_view, systems::entity_count).in_set(ShellSystems::Panels),
        );
    }
}

#[cfg(test)]
mod tests;
