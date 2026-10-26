//! The Scene panel (spec §3.3, §3.3.1, §5.7): the world's entity tree.
//!
//! Its body follows the connection (skeleton rows while the first scene
//! arrives, the error with Reconnect once it's lost, "No world loaded"
//! before); its header counts the entities and has Expand all and
//! Collapse all. Once ready, the body is a filter bar (filter field and
//! the button that shows the type chips), the type chips (All, Models,
//! Lights, Sensors, Joints) and the tree; the footer, under every state,
//! has New group, Hide others and Isolate.
//!
//! The panel contract (DECISIONS.md D-58, D-66): the tree's data is
//! [`model::TreeModel`], rebuilt when entities come or go; what the user
//! set is [`model::TreeState`] and [`model::TreeScroll`]; the body is
//! built from them by [`view::build_tree`] (an observer on
//! `Add<SceneTree>`) and can be respawned at any time. The tree is
//! virtualized: a pool of recycled rows covers what is in view
//! ([`systems::place_rows`]), so thousands of entities cost what a
//! screenful does. Rows are controlled: their carets and eyes ask, the
//! panel answers (expansion here, visibility on the scene entity). The
//! tree is one tab stop inside the sidebar's tab group; arrow keys move
//! its cursor (§6.5). Systems that only feed the shown tree run under
//! `panel_shown(SCENE)`.

mod footer;
pub(crate) mod model;
mod systems;
mod view;

use bevy::prelude::*;

use super::{ActionId, HeaderAction, PanelDef, PanelId, RegisterPanel, WORLD_GROUP};
use crate::assets::IconId;
use crate::shell::{panel_shown, Phase, ShellSystems};

/// The Scene panel's id.
pub const SCENE: PanelId = PanelId::new("scene");
/// Its header's Expand all.
pub(crate) const EXPAND_ALL: ActionId = ActionId::new("expand_all");
/// Its header's Collapse all.
pub(crate) const COLLAPSE_ALL: ActionId = ActionId::new("collapse_all");
/// Its empty state's Open world… (the World menu, M8).
pub(crate) const OPEN_WORLD: ActionId = ActionId::new("open_world");
/// Its empty state's New world (M8).
pub(crate) const NEW_WORLD: ActionId = ActionId::new("new_world");

/// How the tree starts, for a capture (`gzweb --scene-filter`,
/// `--scene-expand`; DECISIONS.md D-70): applied once the first tree is
/// built, then removed.
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct SceneTreeStart {
    /// The filter text.
    pub filter: String,
    /// Every row open.
    pub expand_all: bool,
}

/// The Scene panel's content marker: its ready body.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct SceneTree;

/// The Scene panel's footer content marker.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct SceneFooter;

/// Registers the Scene panel: its view, count, header actions, tree.
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
                )
                .content::<SceneTree>()
                .footer_content::<SceneFooter>(),
        )
        .init_resource::<model::TreeModel>()
        .init_resource::<model::TreeState>()
        .init_resource::<model::TreeScroll>()
        .init_resource::<systems::TreeRows>()
        .init_resource::<systems::TreeWork>()
        .init_resource::<gz_bevy::undo::Isolated>()
        .init_resource::<gz_bevy::view::outline::Hovered>()
        .add_systems(
            Update,
            (
                systems::scene_view,
                (
                    // After the scene's entities spawn, in the same frame.
                    systems::rebuild_model.after(gz_bevy::spawn::apply_scene),
                    (
                        systems::entity_count,
                        systems::header_actions,
                        systems::apply_start,
                        systems::expand_edited,
                    ),
                )
                    .chain(),
                systems::want_sdf,
                systems::hover_outline,
                (
                    systems::filter_text,
                    systems::compute_rows,
                    systems::place_rows,
                    systems::chip_states,
                    systems::cursor_ring,
                    footer::footer_states,
                )
                    .chain()
                    .after(systems::rebuild_model)
                    .run_if(panel_shown(SCENE)),
            )
                .in_set(ShellSystems::Panels),
        )
        .add_observer(view::build_tree)
        .add_observer(systems::on_expand)
        .add_observer(systems::on_visibility)
        .add_observer(systems::on_row_activate)
        .add_observer(systems::on_row_context)
        .add_message::<gz_bevy::view::EntityContext>()
        .add_observer(footer::build_footer)
        .add_observer(footer::on_footer)
        .add_observer(systems::on_chip)
        .add_observer(systems::on_filter_button)
        .add_observer(systems::on_header_action)
        .add_observer(systems::on_tree_key);
    }
}

#[cfg(test)]
mod stress;
#[cfg(test)]
mod tests;
