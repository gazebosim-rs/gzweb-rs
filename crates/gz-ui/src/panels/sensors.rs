//! The Sensors panel (spec §3.3; M7): camera images and lidar scans of
//! the selected model's sensors. Until then its body asks for a selection.
//! (§3.3.1's second message, "No sensors in this world", needs the
//! world's sensors, which the scene model doesn't carry yet, and the
//! selection, M5: its systems will choose between the two.)

use bevy::prelude::*;

use super::{PanelDef, PanelId, PanelView, RegisterPanel, DATA_GROUP};
use crate::assets::IconId;

/// The Sensors panel's id.
pub const SENSORS: PanelId = PanelId::new("sensors");

/// Registers the Sensors panel.
pub struct SensorsPanelPlugin;

impl Plugin for SensorsPanelPlugin {
    fn build(&self, app: &mut App) {
        app.register_panel(
            PanelDef::new(SENSORS, "Sensors", IconId::ActivitySensors)
                .place(DATA_GROUP, 40)
                .view(PanelView::empty("Select a model with sensors")),
        );
    }
}
