//! The Teleop panel (spec §3.3; M7): drive the selected robot from the
//! keyboard, sliders or a gamepad.

mod view;

use bevy::prelude::*;

use super::{PanelDef, PanelId, RegisterPanel, DATA_GROUP};
use crate::assets::IconId;

/// The Teleop panel's id.
pub const TELEOP: PanelId = PanelId::new("teleop");

/// Registers the Teleop panel.
pub struct TeleopPanelPlugin;

impl Plugin for TeleopPanelPlugin {
    fn build(&self, app: &mut App) {
        app.register_panel(
            PanelDef::new(TELEOP, "Teleop", IconId::ActivityTeleop)
                .place(DATA_GROUP, 50)
                .view(view::state),
        );
    }
}

#[cfg(test)]
mod tests;
