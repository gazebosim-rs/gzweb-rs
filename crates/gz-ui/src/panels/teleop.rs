//! The Teleop panel (spec §3.3; M7): drive the selected robot from the
//! keyboard, sliders or a gamepad. Until then its body asks for a robot.

use bevy::prelude::*;

use super::{PanelDef, PanelId, PanelView, RegisterPanel, DATA_GROUP};
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
                .view(PanelView::empty("Select a robot to drive")),
        );
    }
}
