//! The Sensors panel (spec §3.3; M7): camera images and lidar scans of
//! the selected model's sensors.

mod view;

use bevy::prelude::*;

use super::{Panel, PanelDef, RegisterPanel};
use crate::assets::icons::IconId;

/// Registers the Sensors panel.
pub struct SensorsPanelPlugin;

impl Plugin for SensorsPanelPlugin {
    fn build(&self, app: &mut App) {
        app.register_panel(
            PanelDef::new(Panel::Sensors, "Sensors", IconId::ActivitySensors, 40).view(view::state),
        );
    }
}

#[cfg(test)]
mod tests;
