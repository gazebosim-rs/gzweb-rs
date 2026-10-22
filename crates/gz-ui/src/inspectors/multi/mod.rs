//! The multi-selection inspector (spec §3.5): the count, what is
//! selected, Focus (Group and Delete wait for M8 and M5c), and the pose the
//! selected entities share, live, "—" where they differ.

use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use gz_bevy::spawn::{GzEntity, GzWorldRoot};
use gz_bevy::view::{PartKind, Selection};
use gz_core::scene::NodeKind;

use super::common::{
    breadcrumb, gazebo_pose, group, links, pose_texts, quick_action, quick_action_node,
    quick_actions, title_row, EntityAction, PoseFrame,
};
use super::{InspectKind, InspectorDef, InspectorId, RegisterInspector};
use crate::format;
use crate::shell::ShellSystems;
use crate::theme::Theme;
use crate::widgets::text::set_text;

/// The multi-selection inspector's id.
pub const MULTI: InspectorId = InspectorId::new("multi");

/// Its content marker.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct MultiBody;

/// One of the shared pose's six values.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SharedPose(pub(crate) usize);

/// Registers the multi-selection inspector.
pub struct MultiInspectorPlugin;

impl Plugin for MultiInspectorPlugin {
    fn build(&self, app: &mut App) {
        app.register_inspector(InspectorDef::new(MULTI, InspectKind::Multi).content::<MultiBody>())
            .add_systems(Update, shared_pose.in_set(ShellSystems::Readouts))
            .add_observer(build_multi);
    }
}

/// "2 models · 1 link · 1 joint": what is selected, by kind.
pub(crate) fn summary(kinds: &[&str]) -> String {
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for k in kinds {
        match counts.iter_mut().find(|(n, _)| n == k) {
            Some((_, c)) => *c += 1,
            None => counts.push((k, 1)),
        }
    }
    counts
        .into_iter()
        .map(|(k, c)| format!("{c} {k}{}", if c == 1 { "" } else { "s" }))
        .collect::<Vec<_>>()
        .join(" · ")
}

fn build_multi(
    ev: On<Add<MultiBody>>,
    selection: Res<Selection>,
    theme: Res<Theme>,
    entities: Query<&GzEntity>,
    mut commands: Commands,
) {
    let mut kinds: Vec<&str> = selection
        .entities
        .iter()
        .filter_map(|e| entities.get(*e).ok())
        .map(|g| match g.kind {
            NodeKind::Model => "model",
            NodeKind::Link => "link",
            NodeKind::Light => "light",
            NodeKind::Visual => "visual",
        })
        .collect();
    kinds.extend(selection.parts.iter().map(|p| match p.kind {
        PartKind::Joint => "joint",
        PartKind::Sensor => "sensor",
    }));
    let title = format::selection(selection.len(), None);
    let crumb = summary(&kinds);
    let posed = !selection.entities.is_empty();
    let t = theme.clone();
    commands
        .entity(ev.entity)
        .insert(Children::spawn(SpawnWith(move |b: &mut ChildSpawner| {
            b.spawn(title_row(&t, InspectKind::Multi, &title));
            b.spawn(breadcrumb(crumb));
            b.spawn(quick_actions()).with_children(|r| {
                // Joints and sensors frame their owners (D-79).
                r.spawn(quick_action(&t, "Focus", None))
                    .insert(quick_action_node(&t))
                    .insert(EntityAction::FocusSelection);
                r.spawn(quick_action(&t, "Group", Some("Not available yet")))
                    .insert(quick_action_node(&t));
                r.spawn(quick_action(&t, "Delete", Some("Not available yet")))
                    .insert(quick_action_node(&t));
            });
            if posed {
                group(
                    b,
                    "Pose (shared)",
                    true,
                    &super::common::POSE_LABELS,
                    SharedPose,
                );
            }
            links(b, &["All components"]);
        })));
}

/// Each of the six values the selected entities' world poses share, or
/// "—" where they differ (§3.5).
fn shared_pose(
    selection: Res<Selection>,
    poses: Query<(Ref<Transform>, Ref<GlobalTransform>)>,
    roots: Query<&GlobalTransform, With<GzWorldRoot>>,
    added: Query<(), Added<SharedPose>>,
    mut values: Query<(&SharedPose, &mut Text)>,
) {
    if values.is_empty() {
        return;
    }
    let found: Vec<_> = selection
        .entities
        .iter()
        .filter_map(|e| poses.get(*e).ok())
        .collect();
    let moved = found.iter().any(|(l, g)| l.is_changed() || g.is_changed());
    if !moved && added.is_empty() && !selection.is_changed() {
        return;
    }
    let root = roots.iter().next();
    let all: Vec<[String; 6]> = found
        .iter()
        .map(|(l, g)| pose_texts(&gazebo_pose(PoseFrame::World, l, g, root)))
        .collect();
    for (v, mut text) in &mut values {
        let mut each = all.iter().map(|p| &p[v.0]);
        let want = match each.next() {
            Some(first) if each.all(|o| o == first) => first.clone(),
            _ => "—".into(),
        };
        set_text(&mut text, &want);
    }
}

#[cfg(test)]
mod tests;
