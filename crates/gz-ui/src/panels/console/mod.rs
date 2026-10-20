//! The Console panel (spec §3.3; M7; UI_PLAN §3 item 3): client-side
//! messages and log topics. Its activity button counts the unread
//! warnings and errors.

mod view;

use bevy::prelude::*;

use super::{Panel, PanelDef, RegisterPanel};
use crate::assets::icons::IconId;

/// Registers the Console panel.
pub struct ConsolePanelPlugin;

impl Plugin for ConsolePanelPlugin {
    fn build(&self, app: &mut App) {
        app.register_panel(
            PanelDef::new(Panel::Console, "Console", IconId::ActivityConsole, 60)
                .binding("panel.console")
                .view(view::state)
                .badge(view::badge),
        );
    }
}

#[cfg(test)]
mod tests;
