//! The Workspaces panel (spec §3.3, §8; DECISIONS.md D-111): a card for
//! each workspace (a thumbnail of its layout, its name, what it shows,
//! Alt+N; the current one marked), the current layout's summary with Save
//! to …, Save as new… and Reset, and a footer saying where the layouts are
//! kept, with Import / export layouts…. It has no activity button
//! (decision Q2): the switcher's Manage… and ⌘K open it.
//!
//! The panel contract (DECISIONS.md D-58): the workspaces live in the
//! shell ([`Workspaces`](crate::shell::Workspaces),
//! [`CurrentLayout`](crate::shell::CurrentLayout)); the panel shows them
//! and asks for changes through
//! [`WorkspaceCommand`](crate::shell::WorkspaceCommand). Showing it in a
//! slot doesn't change the layout it shows (`Beneath`).

mod systems;
mod view;

use bevy::prelude::*;

use super::{ActionId, HeaderAction, PanelDef, RegisterPanel};
use crate::assets::IconId;
use crate::shell::{panel_shown, ShellSystems, WORKSPACES_PANEL};

/// Its header's + (Save as new…).
pub(crate) const NEW: ActionId = ActionId::new("new");

/// Opens the panel at start, for a capture (`gzweb --workspaces`); then
/// the resource is removed.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WorkspacesStart;

/// What the user set in the panel: naming a new workspace.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PanelState {
    pub(crate) naming: bool,
}

/// The panel's content marker: its ready body.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct WorkspacesBody;

/// Its footer's content: the note and Import / export layouts….
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct WorkspacesFooter;

/// Registers the Workspaces panel.
pub struct WorkspacesPanelPlugin;

impl Plugin for WorkspacesPanelPlugin {
    fn build(&self, app: &mut App) {
        app.register_panel(
            PanelDef::new(WORKSPACES_PANEL, "Workspaces", IconId::ActivityWorkspaces)
                .no_activity_button()
                .header_action(HeaderAction::new(
                    NEW,
                    IconId::Plus,
                    "Save as new workspace…",
                ))
                .content::<WorkspacesBody>()
                .footer_content::<WorkspacesFooter>(),
        )
        .init_resource::<PanelState>()
        .add_systems(
            Update,
            (
                systems::apply_start,
                systems::start_naming,
                (
                    systems::sync_cards,
                    systems::sync_summary,
                    systems::sync_naming,
                    systems::focus_name,
                )
                    .chain()
                    .run_if(panel_shown(WORKSPACES_PANEL)),
            )
                .chain()
                .in_set(ShellSystems::Panels),
        )
        .add_observer(view::build_body)
        .add_observer(view::build_footer)
        .add_observer(systems::on_action)
        .add_observer(systems::on_card)
        .add_observer(systems::on_layout_button)
        .add_observer(systems::on_name_button)
        .add_observer(systems::on_name_submit)
        .add_observer(systems::on_name_key)
        .add_observer(systems::on_io_item);
    }
}

#[cfg(test)]
mod tests;
