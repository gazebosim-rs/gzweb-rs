//! The Console panel (spec §3.3; UI_PLAN §3 item 3; DECISIONS.md D-105):
//! the client's own log (the websocket server forwards none) and a `gz`
//! command line. Its activity button counts the warnings and errors not
//! seen yet.
//!
//! The panel contract (DECISIONS.md D-58): what the user set is
//! [`model::ConsoleState`]; the log keeps recording while the panel is
//! closed, so only the rows wait for it to show.

mod model;
mod systems;
mod view;

use bevy::prelude::*;

use super::{ActionId, HeaderAction, PanelDef, PanelId, RegisterPanel, DATA_GROUP};
use crate::assets::IconId;
use crate::shell::{panel_shown, ShellSystems};

/// The Console panel's id.
pub const CONSOLE: PanelId = PanelId::new("console");
/// Its header's Clear.
pub(crate) const CLEAR: ActionId = ActionId::new("clear");
/// Its header's Pause, a toggle.
pub(crate) const PAUSE: ActionId = ActionId::new("pause");

/// Opens the panel at start, for a capture (`gzweb --console`); then the
/// resource is removed.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ConsoleStart;

/// The log's warnings and errors not seen yet; the activity badge counts
/// them.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ConsoleUnread {
    pub warnings: u32,
    pub errors: u32,
}

/// The Console panel's content marker: its ready body.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct ConsoleBody;

/// Its footer's content: the command line.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct ConsoleFooter;

/// Registers the Console panel and its badge.
pub struct ConsolePanelPlugin;

impl Plugin for ConsolePanelPlugin {
    fn build(&self, app: &mut App) {
        app.register_panel(
            PanelDef::new(CONSOLE, "Console", IconId::ActivityConsole)
                .place(DATA_GROUP, 60)
                .binding("panel.console")
                .view(super::PanelView::empty(systems::NO_MESSAGES))
                .header_action(HeaderAction::new(CLEAR, IconId::Trash, "Clear messages").disabled())
                .header_action(
                    HeaderAction::new(PAUSE, IconId::Pause, "Pause messages")
                        .toggle(IconId::Play, "Resume messages"),
                )
                .content::<ConsoleBody>()
                .footer_content::<ConsoleFooter>(),
        )
        .init_resource::<gz_bevy::messages::SystemMessages>()
        .init_resource::<ConsoleUnread>()
        .init_resource::<model::ConsoleState>()
        .init_resource::<model::ConsoleOutput>()
        .init_resource::<systems::BuiltRows>()
        .init_resource::<systems::CommandState>()
        .add_systems(
            Update,
            (
                (
                    systems::apply_start,
                    systems::service_replies,
                    systems::unread,
                    systems::unread_badge,
                    systems::console_status,
                )
                    .chain(),
                (
                    systems::apply_recall,
                    systems::filter_text,
                    systems::chip_states,
                    systems::sync_rows,
                )
                    .chain()
                    .after(systems::console_status)
                    .run_if(panel_shown(CONSOLE)),
            )
                .in_set(ShellSystems::Panels),
        )
        .add_observer(view::build_console)
        .add_observer(view::build_footer)
        .add_observer(systems::on_chip)
        .add_observer(systems::on_action)
        .add_observer(systems::on_submit)
        .add_observer(systems::on_history_key);
    }
}

#[cfg(test)]
mod tests;
