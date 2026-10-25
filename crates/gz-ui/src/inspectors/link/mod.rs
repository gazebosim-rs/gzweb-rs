//! The Link inspector (spec §3.5): the LINK tag and the link's name, its
//! path, Select model, Show inertia and Show collision, Add visual, Add
//! collision and Add sensor while its model is edited (§6.6), the live
//! pose in its model's frame, and the inertial, flags and geometry from the
//! scene and the world's SDFormat.

use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use gz_bevy::connection::GzStatus;
use gz_bevy::overlays::extras::SdfExtras;
use gz_bevy::spawn::{GzEntity, GzIndex, GzSensors, GzVisual};

use super::common::{
    breadcrumb, crumb, definition_group, links, pose_group, quick_action, quick_action_node,
    quick_actions, title_row, EntityAction, PoseFrame,
};
use super::{InspectKind, Inspected, InspectorDef, InspectorId, RegisterInspector};
use crate::format;
use crate::panels::PartAction;
use crate::shell::world_name;
use crate::theme::Theme;

/// The Link inspector's id.
pub const LINK: InspectorId = InspectorId::new("link");

/// The Link inspector's content marker.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct LinkBody;

/// Registers the Link inspector.
pub struct LinkInspectorPlugin;

impl Plugin for LinkInspectorPlugin {
    fn build(&self, app: &mut App) {
        app.register_inspector(InspectorDef::new(LINK, InspectKind::Link).content::<LinkBody>())
            .add_observer(build_link);
    }
}

#[allow(clippy::too_many_arguments)]
fn build_link(
    ev: On<Add<LinkBody>>,
    inspected: Res<Inspected>,
    theme: Res<Theme>,
    status: Option<Res<GzStatus>>,
    index: Option<Res<GzIndex>>,
    sdf: Option<Res<SdfExtras>>,
    entities: Query<(&GzEntity, Option<&GzSensors>, Option<&Children>)>,
    visuals: Query<(&GzEntity, &GzVisual)>,
    mut commands: Commands,
) {
    let Some(link) = inspected.entity else {
        return;
    };
    let Ok((g, sensors, children)) = entities.get(link) else {
        return;
    };
    let (name, path) = (g.name.clone(), g.path.clone());
    let sensors = sensors.map_or(0, |s| s.0.len());
    let world = world_name(status.as_deref(), index.as_deref());
    let crumb = crumb(world.as_deref(), &path);
    let extras = sdf
        .as_deref()
        .and_then(|s| s.extras.as_ref())
        .and_then(|w| w.link(&path).cloned());
    let mut geometry: Vec<(String, String)> = children
        .into_iter()
        .flatten()
        .filter_map(|c| visuals.get(*c).ok())
        .map(|(v, d)| {
            (
                v.name.clone(),
                format!("visual · {}", d.0.geometry.kind_name()),
            )
        })
        .collect();
    if let Some(e) = &extras {
        geometry.extend(e.collisions.iter().map(|c| {
            (
                c.name.clone(),
                format!("collision · {}", c.geometry.kind_name()),
            )
        }));
    }
    if geometry.is_empty() {
        geometry.push(("None".into(), String::new()));
    }
    let t = theme.clone();
    commands
        .entity(ev.entity)
        .insert(Children::spawn(SpawnWith(move |b: &mut ChildSpawner| {
            b.spawn(title_row(&t, InspectKind::Link, &name));
            b.spawn(breadcrumb(crumb));
            b.spawn(quick_actions()).with_children(|r| {
                r.spawn(quick_action(&t, "Select model", None))
                    .insert(quick_action_node(&t))
                    .insert(EntityAction::SelectModel);
                r.spawn(quick_action(&t, "Show inertia", None))
                    .insert(quick_action_node(&t))
                    .insert(EntityAction::ShowInertia);
                r.spawn(quick_action(&t, "Show collision", None))
                    .insert(quick_action_node(&t))
                    .insert(EntityAction::ShowCollision);
            });
            b.spawn(quick_actions()).with_children(|r| {
                for action in [
                    PartAction::Visual,
                    PartAction::Collision,
                    PartAction::Sensor,
                ] {
                    r.spawn(quick_action(&t, action.label(), Some(action.unavailable())))
                        .insert(quick_action_node(&t))
                        .insert(action);
                }
            });
            pose_group(b, &t, "Pose (in model)", PoseFrame::Parent, false);
            let inertial = extras.as_ref().and_then(|e| e.inertial);
            let n = |v: Option<f64>| v.map_or("—".into(), |v| format::number(v, 4));
            definition_group(
                b,
                "Inertial",
                vec![
                    (
                        "Mass".into(),
                        inertial.map_or("—".into(), |i| {
                            format!("{} kg", format::number(i.mass, 3))
                        }),
                    ),
                    ("Ixx".into(), n(inertial.map(|i| i.inertia[0]))),
                    ("Iyy".into(), n(inertial.map(|i| i.inertia[3]))),
                    ("Izz".into(), n(inertial.map(|i| i.inertia[5]))),
                ],
            );
            let flag = |f: Option<bool>| f.map_or("—", format::yes_no).to_string();
            let flags = extras.as_ref().map(|e| e.flags);
            definition_group(
                b,
                "Flags",
                vec![
                    ("Gravity".into(), flag(flags.map(|f| f.gravity))),
                    ("Kinematic".into(), flag(flags.map(|f| f.kinematic))),
                    ("Wind".into(), flag(flags.map(|f| f.wind))),
                ],
            );
            definition_group(b, "Geometry", geometry);
            links(b, &[&format!("Sensors {sensors}"), "All components"]);
        })));
}

#[cfg(test)]
mod tests;
