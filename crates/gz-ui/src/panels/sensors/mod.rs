//! The Sensors panel (spec §3.3; M7): camera images and lidar scans of
//! the selected model's sensors.

mod view;

use bevy::prelude::*;

use super::{PanelDef, PanelId, RegisterPanel, DATA_GROUP};
use crate::assets::IconId;

/// The Sensors panel's id.
pub const SENSORS: PanelId = PanelId::new("sensors");

/// Registers the Sensors panel.
pub struct SensorsPanelPlugin;

impl Plugin for SensorsPanelPlugin {
    fn build(&self, app: &mut App) {
        app.register_panel(
            PanelDef::new(SENSORS, "Sensors", IconId::ActivitySensors, DATA_GROUP, 40)
                .view(view::state),
        );
    }
}

#[cfg(test)]
mod tests;
