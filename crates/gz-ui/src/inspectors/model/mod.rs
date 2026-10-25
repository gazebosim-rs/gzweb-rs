//! The Model inspector (spec §3.5): the MODEL tag and the model's name,
//! its path, Focus (Duplicate and Save as… wait for M5c and M8), the live
//! world pose, the Physics flags and the plugins from the world's
//! SDFormat, and the links to its links, joints and sensors.

use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use gz_bevy::connection::GzStatus;
use gz_bevy::edit::HasPlane;
use gz_bevy::overlays::extras::SdfExtras;
use gz_bevy::spawn::{GzEntity, GzIndex, GzSensors, GzWorldRoot};
use gz_core::scene::NodeKind;

use super::common::{
    breadcrumb, crumb, definition_group, links_with, pose_editable, pose_group, quick_action,
    quick_action_node, quick_actions, title_row, EntityAction, PoseFrame,
};
use super::{InspectKind, Inspected, InspectorDef, InspectorId, RegisterInspector};
use crate::format;
use crate::shell::world_name;
use crate::theme::Theme;

/// The Model inspector's id.
pub const MODEL: InspectorId = InspectorId::new("model");

/// The Model inspector's content marker.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct ModelBody;

/// Registers the Model inspector.
pub struct ModelInspectorPlugin;

impl Plugin for ModelInspectorPlugin {
    fn build(&self, app: &mut App) {
        app.register_inspector(InspectorDef::new(MODEL, InspectKind::Model).content::<ModelBody>())
            .add_observer(build_model);
    }
}

/// What the model holds: its links, and their sensors and joints.
pub(crate) struct Parts {
    pub(crate) links: usize,
    pub(crate) sensors: usize,
    pub(crate) joints: usize,
}

/// Counts the links (its own, not nested models'), their sensors, and the
/// joints the SDFormat gives the model.
fn parts(
    model: Entity,
    path: &str,
    children: &Query<&Children>,
    entities: &Query<(&GzEntity, Option<&GzSensors>)>,
    sdf: Option<&SdfExtras>,
) -> Parts {
    let links: Vec<Entity> = children
        .get(model)
        .into_iter()
        .flatten()
        .copied()
        .filter(|c| {
            entities
                .get(*c)
                .is_ok_and(|(g, _)| g.kind == NodeKind::Link)
        })
        .collect();
    let sensors = links
        .iter()
        .filter_map(|l| entities.get(*l).ok().and_then(|(_, s)| s))
        .map(|s| s.0.len())
        .sum();
    let joints = sdf
        .and_then(|s| s.extras.as_ref())
        .map_or(0, |w| w.joints_of(path).count());
    Parts {
        links: links.len(),
        sensors,
        joints,
    }
}

#[allow(clippy::too_many_arguments)]
fn build_model(
    ev: On<Add<ModelBody>>,
    inspected: Res<Inspected>,
    theme: Res<Theme>,
    status: Option<Res<GzStatus>>,
    index: Option<Res<GzIndex>>,
    sdf: Option<Res<SdfExtras>>,
    children: Query<&Children>,
    entities: Query<(&GzEntity, Option<&GzSensors>)>,
    nodes: Query<(&GzEntity, &ChildOf, Has<HasPlane>)>,
    roots: Query<Entity, With<GzWorldRoot>>,
    mut commands: Commands,
) {
    let Some(model) = inspected.entity else {
        return;
    };
    let Ok((g, _)) = entities.get(model) else {
        return;
    };
    let (name, path) = (g.name.clone(), g.path.clone());
    let world = world_name(status.as_deref(), index.as_deref());
    let crumb = crumb(world.as_deref(), &path);
    let extras = sdf
        .as_deref()
        .and_then(|s| s.extras.as_ref())
        .and_then(|w| w.model(&path).cloned());
    let gravity = sdf.as_deref().and_then(|s| s.extras.as_ref()).map(|w| {
        // Its own links, as Links counts them (not nested models').
        let prefix = format!("{path}/");
        let mut flags = w
            .links
            .iter()
            .filter(|(p, _)| p.starts_with(&prefix) && !p[prefix.len()..].contains('/'))
            .map(|(_, l)| l.flags.gravity);
        match flags.next() {
            None => "—",
            Some(first) if flags.all(|f| f == first) => format::yes_no(first),
            Some(_) => "Mixed",
        }
    });
    let counts = parts(model, &path, &children, &entities, sdf.as_deref());
    let editable = pose_editable(status.as_deref(), model, &nodes, &roots);
    // Only a top-level model is edited in place (§6.6).
    let top = nodes
        .get(model)
        .is_ok_and(|(_, p, _)| roots.contains(p.parent()));
    let t = theme.clone();
    commands
        .entity(ev.entity)
        .insert(Children::spawn(SpawnWith(move |b: &mut ChildSpawner| {
            b.spawn(title_row(&t, InspectKind::Model, &name));
            b.spawn(breadcrumb(crumb));
            b.spawn(quick_actions()).with_children(|r| {
                r.spawn(quick_action(&t, "Focus", None))
                    .insert(quick_action_node(&t))
                    .insert(EntityAction::Focus);
                r.spawn(quick_action(&t, "Duplicate", Some("Not available yet")))
                    .insert(quick_action_node(&t));
                r.spawn(quick_action(&t, "Save as…", Some("Not available yet")))
                    .insert(quick_action_node(&t));
            });
            pose_group(b, &t, "Pose", PoseFrame::World, editable);
            let flag = |f: Option<bool>| f.map_or("—", format::yes_no).to_string();
            definition_group(
                b,
                "Physics",
                vec![
                    ("Static".into(), flag(extras.as_ref().map(|e| e.is_static))),
                    ("Gravity".into(), gravity.unwrap_or("—").to_string()),
                    (
                        "Self collide".into(),
                        flag(extras.as_ref().map(|e| e.self_collide)),
                    ),
                ],
            );
            let plugins: Vec<(String, String)> = match &extras {
                Some(e) if !e.plugins.is_empty() => e
                    .plugins
                    .iter()
                    .map(|p| {
                        (
                            p.rsplit("::").next().unwrap_or(p).to_string(),
                            String::new(),
                        )
                    })
                    .collect(),
                Some(_) => vec![("None".into(), String::new())],
                None => vec![("—".into(), String::new())],
            };
            definition_group(b, "Plugins", plugins);
            links_with(
                b,
                &[
                    &format!("Links {}", counts.links),
                    &format!("Joints {}", counts.joints),
                    &format!("Sensors {}", counts.sensors),
                    "Edit model",
                    "All components",
                ],
                if top {
                    &[("Edit model", EntityAction::EditModel)]
                } else {
                    &[]
                },
            );
        })));
}

#[cfg(test)]
pub(crate) mod tests;
