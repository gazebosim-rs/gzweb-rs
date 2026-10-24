//! The Sensors panel (spec §3.3, §3.3.1; DECISIONS.md D-103): the selected
//! model's sensors, and the chosen one's output: a camera's frames, a
//! lidar's scan from above, or any other sensor's message as text.
//!
//! A sensor's topic comes from the world's SDFormat (`<topic>`) and the
//! server's topic list, else the one Gazebo derives. The preview holds it
//! in [`GzSubscriptions`](gz_bevy::topics::GzSubscriptions) only while the
//! panel shows (PR #30 second review, item 6), at
//! [`model::PREVIEW_RATE`]; a camera's PNG frames decode off the main
//! thread.
//!
//! The panel contract (DECISIONS.md D-58): the chosen sensor is
//! [`model::SensorsState`], its data [`model::Preview`]; the body is
//! built from them and follows in place.

pub(crate) mod model;
mod systems;
mod view;

use bevy::prelude::*;

use super::{ActionId, HeaderAction, PanelAction, PanelDef, PanelId, RegisterPanel, DATA_GROUP};
use crate::assets::IconId;
use crate::shell::{panel_shown, ShellSystems};

/// The Sensors panel's id.
pub const SENSORS: PanelId = PanelId::new("sensors");
/// Its header's Pop out (M7e).
pub(crate) const POP_OUT: ActionId = ActionId::new("pop_out");
/// Its footer's Show in viewport and Record.
pub(crate) const SHOW: ActionId = ActionId::new("show_in_viewport");
pub(crate) const RECORD: ActionId = ActionId::new("record");

/// The Sensors panel's content marker: its ready body.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct SensorsBody;

/// How the panel starts, for a capture (`gzweb --sensors [NAME]`, with
/// `--select MODEL`): it opens in the primary slot and shows the sensor
/// called `sensor` once the model's list has it; then the resource is
/// removed.
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct SensorsStart {
    pub sensor: Option<String>,
}

/// Show this sensor (its link and name): the inspector's View in
/// Sensors.
#[derive(Message, Clone, Debug, PartialEq, Eq)]
pub struct ViewSensor(pub Entity, pub String);

/// Registers the Sensors panel.
pub struct SensorsPanelPlugin;

impl Plugin for SensorsPanelPlugin {
    fn build(&self, app: &mut App) {
        gz_bevy::topics::TopicsPlugin::ensure(app);
        super::topics::refresh_while_shown(app, SENSORS);
        app.register_panel(
            PanelDef::new(SENSORS, "Sensors", IconId::ActivitySensors)
                .place(DATA_GROUP, 40)
                .view(systems::view(false, false, false))
                .header_action(HeaderAction::new(POP_OUT, IconId::PopOut, "Pop out").disabled())
                .content::<SensorsBody>()
                .footer_action(PanelAction::new(SHOW, "Show in viewport").disabled())
                .footer_action(PanelAction::new(RECORD, "Record").disabled()),
        )
        .add_message::<ViewSensor>()
        .init_resource::<model::SensorsState>()
        .init_resource::<model::Preview>()
        .init_resource::<systems::SensorList>()
        .init_resource::<systems::Decoding>()
        .init_resource::<systems::BuiltRows>()
        .add_systems(
            Update,
            (
                (
                    systems::collect,
                    systems::apply_start,
                    systems::view_sensor,
                    systems::choose,
                    systems::sensors_status,
                    systems::want_sdf,
                    systems::start_preview,
                    systems::hold_preview.before(gz_bevy::topics::sync_subscriptions),
                    systems::read_preview,
                    systems::finish_decode,
                )
                    .chain(),
                (
                    systems::sync_rows,
                    systems::row_states,
                    systems::preview_parts,
                    systems::draw_scan,
                    systems::preview_status,
                )
                    .chain()
                    .after(systems::finish_decode)
                    .run_if(panel_shown(SENSORS)),
            )
                .in_set(ShellSystems::Panels),
        )
        .add_observer(view::build_sensors)
        .add_observer(systems::on_row);
    }
}

#[cfg(test)]
mod tests;
