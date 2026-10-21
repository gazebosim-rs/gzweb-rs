//! The Console panel (spec §3.3; M7; UI_PLAN §3 item 3): client-side
//! messages and log topics. Its activity button counts the unread
//! warnings and errors.

mod systems;

use bevy::prelude::*;

use super::{PanelDef, PanelId, PanelView, RegisterPanel, DATA_GROUP};
use crate::assets::IconId;
use crate::shell::ShellSystems;

/// The Console panel's id.
pub const CONSOLE: PanelId = PanelId::new("console");

/// Unread messages (M7 keeps them); the activity badge counts them.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ConsoleUnread {
    pub warnings: u32,
    pub errors: u32,
}

/// Registers the Console panel and its badge.
pub struct ConsolePanelPlugin;

impl Plugin for ConsolePanelPlugin {
    fn build(&self, app: &mut App) {
        app.register_panel(
            PanelDef::new(CONSOLE, "Console", IconId::ActivityConsole)
                .place(DATA_GROUP, 60)
                .binding("panel.console")
                .view(PanelView::empty("No messages")),
        )
        .init_resource::<ConsoleUnread>()
        .add_systems(Update, systems::unread_badge.in_set(ShellSystems::Panels));
    }
}

#[cfg(test)]
mod tests;
