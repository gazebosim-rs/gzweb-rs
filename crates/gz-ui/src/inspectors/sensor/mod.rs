//! The Sensor inspector (spec §3.5): the SENSOR tag and the sensor's name,
//! its path, View in Sensors (Echo topic and Plot wait for the Topics and
//! Plot panels, M7), its pose on the link, and its output from the world's
//! SDFormat.

use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use gz_bevy::connection::GzStatus;
use gz_bevy::overlays::extras::SdfExtras;
use gz_bevy::spawn::{GzEntity, GzIndex, GzSensors};

use super::common::{
    breadcrumb, crumb, definition_group, links, pose_texts, quick_action, quick_action_node,
    quick_actions, title_row,
};
use super::{InspectKind, Inspected, InspectorDef, InspectorId, RegisterInspector};
use crate::format;
use crate::panels::SENSORS;
use crate::shell::{world_name, Slot, UiCommand};
use crate::theme::Theme;

/// The Sensor inspector's id.
pub const SENSOR: InspectorId = InspectorId::new("sensor");

/// The Sensor inspector's content marker.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct SensorBody;

/// View in Sensors.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct ViewInSensors;

/// Registers the Sensor inspector.
pub struct SensorInspectorPlugin;

impl Plugin for SensorInspectorPlugin {
    fn build(&self, app: &mut App) {
        app.register_inspector(
            InspectorDef::new(SENSOR, InspectKind::Sensor).content::<SensorBody>(),
        )
        .add_observer(build_sensor)
        .add_observer(on_view_in_sensors);
    }
}

/// The message a sensor of this type publishes (Gazebo's sensors).
pub(crate) fn message_type(ty: &str) -> Option<&'static str> {
    Some(match ty {
        "camera"
        | "depth_camera"
        | "depth"
        | "rgbd_camera"
        | "rgbd"
        | "thermal_camera"
        | "thermal"
        | "segmentation_camera"
        | "boundingbox_camera"
        | "wideanglecamera" => "gz.msgs.Image",
        "gpu_lidar" | "gpu_ray" | "lidar" | "ray" => "gz.msgs.LaserScan",
        "imu" => "gz.msgs.IMU",
        "altimeter" => "gz.msgs.Altimeter",
        "magnetometer" => "gz.msgs.Magnetometer",
        "air_pressure" => "gz.msgs.FluidPressure",
        "air_speed" => "gz.msgs.AirSpeed",
        "contact" => "gz.msgs.Contacts",
        "logical_camera" => "gz.msgs.LogicalCameraImage",
        "navsat" | "gps" => "gz.msgs.NavSat",
        "force_torque" => "gz.msgs.Wrench",
        _ => return None,
    })
}

#[allow(clippy::too_many_arguments)]
fn build_sensor(
    ev: On<Add<SensorBody>>,
    inspected: Res<Inspected>,
    theme: Res<Theme>,
    status: Option<Res<GzStatus>>,
    index: Option<Res<GzIndex>>,
    sdf: Option<Res<SdfExtras>>,
    links_q: Query<(&GzEntity, Option<&GzSensors>)>,
    mut commands: Commands,
) {
    let (Some(link), Some(part)) = (inspected.entity, inspected.part.clone()) else {
        return;
    };
    let Ok((g, sensors)) = links_q.get(link) else {
        return;
    };
    let path = g.path.clone();
    let world = world_name(status.as_deref(), index.as_deref());
    let crumb = crumb(world.as_deref(), &format!("{path}/{}", part.name));
    let desc = sensors.and_then(|s| s.0.iter().find(|d| d.name == part.name).cloned());
    let output = sdf
        .as_deref()
        .and_then(|s| s.extras.as_ref())
        .and_then(|w| w.link(&path))
        .and_then(|l| l.sensors.iter().find(|s| s.name == part.name).cloned());
    let t = theme.clone();
    commands
        .entity(ev.entity)
        .insert(Children::spawn(SpawnWith(move |b: &mut ChildSpawner| {
            b.spawn(title_row(&t, InspectKind::Sensor, &part.name));
            b.spawn(breadcrumb(crumb));
            b.spawn(quick_actions()).with_children(|r| {
                r.spawn(quick_action(&t, "View in Sensors", None))
                    .insert(quick_action_node(&t))
                    .insert(ViewInSensors);
                r.spawn(quick_action(&t, "Echo topic", Some("Not available yet")))
                    .insert(quick_action_node(&t));
                r.spawn(quick_action(&t, "Plot", Some("Not available yet")))
                    .insert(quick_action_node(&t));
            });
            let ty = desc
                .as_ref()
                .map(|d| d.ty.clone())
                .or_else(|| output.as_ref().map(|o| o.ty.clone()))
                .unwrap_or_default();
            if let Some(d) = &desc {
                let pose = Transform::from_xyz(
                    d.pose.pos[0] as f32,
                    d.pose.pos[1] as f32,
                    d.pose.pos[2] as f32,
                )
                .with_rotation(Quat::from_xyzw(
                    d.pose.quat[0] as f32,
                    d.pose.quat[1] as f32,
                    d.pose.quat[2] as f32,
                    d.pose.quat[3] as f32,
                ));
                let v = pose_texts(&pose);
                let labels = super::common::POSE_LABELS;
                definition_group(
                    b,
                    "Pose (in link)",
                    labels.iter().map(|l| l.to_string()).zip(v).collect(),
                );
            }
            let dash = || "—".to_string();
            let flag = |f: Option<bool>| f.map_or("—", format::yes_no).to_string();
            definition_group(
                b,
                "Output",
                vec![
                    (
                        "Type".into(),
                        if ty.is_empty() { dash() } else { ty.clone() },
                    ),
                    (
                        "Topic".into(),
                        output
                            .as_ref()
                            .and_then(|o| o.topic.clone())
                            .unwrap_or_else(|| "Default".into()),
                    ),
                    (
                        "Message type".into(),
                        message_type(&ty).map_or_else(dash, str::to_string),
                    ),
                    (
                        "Rate".into(),
                        output
                            .as_ref()
                            .filter(|o| o.update_rate > 0.0)
                            .map_or_else(dash, |o| {
                                format!("{} Hz", format::number(o.update_rate, 2))
                            }),
                    ),
                    (
                        "Always on".into(),
                        flag(output.as_ref().map(|o| o.always_on)),
                    ),
                    (
                        "Visualize".into(),
                        flag(output.as_ref().map(|o| o.visualize)),
                    ),
                ],
            );
            let lidar = message_type(&ty) == Some("gz.msgs.LaserScan");
            let names: Vec<&str> = if lidar {
                vec!["Vertical scan", "All components"]
            } else {
                vec!["All components"]
            };
            links(b, &names);
        })));
}

fn on_view_in_sensors(
    ev: On<Activate>,
    buttons: Query<(), With<ViewInSensors>>,
    mut out: MessageWriter<UiCommand>,
) {
    if buttons.contains(ev.entity) {
        out.write(UiCommand::OpenPanel {
            panel: SENSORS,
            slot: Slot::Primary,
        });
    }
}

#[cfg(test)]
mod tests;
