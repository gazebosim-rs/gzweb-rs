//! The Console panel (spec §3.3; M7; UI_PLAN §3 item 3): client-side
//! messages and log topics. Its activity button counts the unread
//! warnings and errors.

mod view;

use bevy::prelude::*;

use super::{PanelDef, PanelId, RegisterPanel, DATA_GROUP};
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
            PanelDef::new(CONSOLE, "Console", IconId::ActivityConsole, DATA_GROUP, 60)
                .binding("panel.console")
                .view(view::state),
        )
        .init_resource::<ConsoleUnread>()
        .add_systems(Update, view::unread_badge.in_set(ShellSystems::PanelStatus));
    }
}

#[cfg(test)]
mod tests;
