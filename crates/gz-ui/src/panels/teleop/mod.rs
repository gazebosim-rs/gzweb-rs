//! The Teleop panel (spec §3.3; M7): drive the selected robot from the
//! keyboard, sliders or a gamepad.

mod view;

use bevy::prelude::*;

use super::{Panel, PanelDef, RegisterPanel};
use crate::assets::icons::IconId;

/// Registers the Teleop panel.
pub struct TeleopPanelPlugin;

impl Plugin for TeleopPanelPlugin {
    fn build(&self, app: &mut App) {
        app.register_panel(
            PanelDef::new(Panel::Teleop, "Teleop", IconId::ActivityTeleop, 50).view(view::state),
        );
    }
}

#[cfg(test)]
mod tests;
