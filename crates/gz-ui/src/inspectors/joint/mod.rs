//! The Joint inspector (spec §3.5): the JOINT tag and the joint's name, its
//! path, Select parent, Select child and Show axis, the live position,
//! velocity and effort (from the model's joint states, when the world
//! publishes them), a command slider for a joint with a
//! JointPositionController on a server that takes publications
//! (DECISIONS.md D-88, D-104), and its definition from the world's
//! SDFormat.

use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use bevy::ui_widgets::SliderValue;
use gz_bevy::connection::GzStatus;
use gz_bevy::joint_state::{JointStateWanted, JointStateWants, JointStates};
use gz_bevy::overlays::extras::SdfExtras;
use gz_bevy::spawn::{GzEntity, GzIndex};

use super::common::{
    breadcrumb, crumb, definition_group, group, links, quick_action, quick_action_node,
    quick_actions, title_row, EntityAction, SelectPath,
};
use super::{InspectKind, Inspected, InspectorDef, InspectorId, RegisterInspector};
use crate::format;
use crate::shell::{world_name, ShellSystems};
use crate::theme::Theme;
use crate::widgets::text::set_text;
use crate::widgets::{slider_row, SliderProps};

/// The Joint inspector's id.
pub const JOINT: InspectorId = InspectorId::new("joint");

/// The Joint inspector's content marker.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct JointBody;

/// A live value: position, velocity or effort; `rotates` picks the units.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub(crate) struct JointValue {
    pub(crate) index: usize,
    pub(crate) rotates: bool,
}

/// The command slider's row: the topic it publishes positions on.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub(crate) struct JointCommand(pub(crate) String);

/// The command slider was moved: it no longer follows the joint.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct Commanded;

/// Registers the Joint inspector, the joint-state subscription it uses and
/// its live values.
pub struct JointInspectorPlugin;

impl Plugin for JointInspectorPlugin {
    fn build(&self, app: &mut App) {
        app.register_inspector(InspectorDef::new(JOINT, InspectKind::Joint).content::<JointBody>())
            .add_plugins(gz_bevy::joint_state::JointStatePlugin)
            .add_systems(
                Update,
                (want_joint_states, joint_values, follow_joint).in_set(ShellSystems::Readouts),
            )
            .add_observer(build_joint)
            .add_observer(on_command);
    }
}

/// The inspected joint's model's states are wanted while a joint shows,
/// on the topic its JointStatePublisher sets, if the SDFormat says.
fn want_joint_states(
    inspected: Res<Inspected>,
    sdf: Option<Res<SdfExtras>>,
    mut wants: ResMut<JointStateWants>,
) {
    if !inspected.is_changed() && !sdf.as_ref().is_some_and(|s| s.is_changed()) {
        return;
    }
    let model = match inspected.kind {
        InspectKind::Joint if !inspected.removed => inspected.path.clone(),
        _ => None,
    };
    let topic = model.as_ref().and_then(|m| {
        sdf.as_deref()
            .and_then(|s| s.extras.as_ref())
            .and_then(|w| w.model(m))
            .and_then(|m| m.joint_state_topic.clone())
    });
    // Before Teleop's (D-104): the joint shown is the one to read.
    let want = model.map(|m| JointStateWanted {
        model: Some(m),
        topic,
    });
    wants.set(0, "joint_inspector", want);
}

/// A link's scene path from its SDFormat name (`world` has none).
fn link_path(model: &str, link: &str) -> Option<String> {
    (link != "world" && !link.is_empty()).then(|| format!("{model}/{}", link.replace("::", "/")))
}

/// A limit as the inspector shows it: "—" for SDFormat's unlimited.
fn limit(v: f64, rotates: bool, unlimited: bool) -> String {
    match (unlimited, rotates) {
        (true, _) => "—".into(),
        (false, true) => format::degrees(v as f32),
        (false, false) => format!("{} m", format::number(v, 3)),
    }
}

#[allow(clippy::too_many_arguments)]
fn build_joint(
    ev: On<Add<JointBody>>,
    inspected: Res<Inspected>,
    theme: Res<Theme>,
    status: Option<Res<GzStatus>>,
    index: Option<Res<GzIndex>>,
    sdf: Option<Res<SdfExtras>>,
    models: Query<&GzEntity>,
    mut commands: Commands,
) {
    let (Some(model), Some(part)) = (inspected.entity, inspected.part.clone()) else {
        return;
    };
    let Ok(g) = models.get(model) else {
        return;
    };
    let path = g.path.clone();
    let world = world_name(status.as_deref(), index.as_deref());
    let crumb = crumb(world.as_deref(), &format!("{path}/{}", part.name));
    let joint = sdf
        .as_deref()
        .and_then(|s| s.extras.as_ref())
        .and_then(|w| w.joint(&path, &part.name).cloned());
    // Only ends that are links in the scene (not `world`, a frame or a
    // model).
    let in_scene = |p: Option<String>| p.filter(|p| models.iter().any(|g| &g.path == p));
    let ends = joint.as_ref().map(|j| {
        (
            in_scene(link_path(&path, &j.parent)),
            in_scene(link_path(&path, &j.child)),
        )
    });
    // A ball joint turns too (the overlays draw it without an axis ring).
    let rotates = joint
        .as_ref()
        .is_none_or(|j| j.kind.rotates() || j.kind == gz_core::sdf::JointKind::Ball);
    // Its JointPositionController's topic, where the server publishes.
    let publishes = status.as_deref().and_then(|s| s.publishes) == Some(true);
    let controller = sdf
        .as_deref()
        .and_then(|s| s.extras.as_ref())
        .and_then(|w| w.model(&path))
        .and_then(|m| m.joint_controllers.iter().find(|(j, _)| *j == part.name))
        .map(|(_, topic)| topic.clone())
        .filter(|_| publishes);
    let range = joint
        .as_ref()
        .and_then(|j| j.limits.first())
        .map(|l| (l.lower, l.upper))
        .filter(|(lo, hi)| lo.is_finite() && hi.is_finite() && hi - lo < 1e6 && hi > lo)
        .unwrap_or((-std::f64::consts::PI, std::f64::consts::PI));
    let t = theme.clone();
    commands
        .entity(ev.entity)
        .insert(Children::spawn(SpawnWith(move |b: &mut ChildSpawner| {
            b.spawn(title_row(&t, InspectKind::Joint, &part.name));
            b.spawn(breadcrumb(crumb));
            b.spawn(quick_actions()).with_children(|r| {
                for (label, end) in [
                    ("Select parent", ends.as_ref().and_then(|e| e.0.clone())),
                    ("Select child", ends.as_ref().and_then(|e| e.1.clone())),
                ] {
                    let why = end.is_none().then_some("No link to select");
                    let mut e = r.spawn(quick_action(&t, label, why));
                    e.insert(quick_action_node(&t));
                    if let Some(p) = end {
                        e.insert(SelectPath(p));
                    }
                }
                r.spawn(quick_action(&t, "Show axis", None))
                    .insert(quick_action_node(&t))
                    .insert(EntityAction::ShowJoints);
            });
            group(
                b,
                "Live",
                true,
                &["Position", "Velocity", "Effort"],
                |index| JointValue { index, rotates },
            );
            if let Some(topic) = controller {
                #[allow(clippy::cast_possible_truncation)]
                let (lo, hi) = (range.0 as f32, range.1 as f32);
                let props = SliderProps::new("Command", lo, hi, 0.0f32.clamp(lo, hi));
                b.spawn(Node {
                    padding: UiRect::horizontal(px(crate::theme::SPACE[5])),
                    ..default()
                })
                .with_child((slider_row(&t, props), JointCommand(topic)));
            }
            let Some(j) = joint else {
                definition_group(b, "Definition", vec![("—".into(), String::new())]);
                links(b, &["Plot position/velocity", "All components"]);
                return;
            };
            let n = |v: f64| format::number(v, 3);
            let l = j.limits.first().copied().unwrap_or_default();
            let mut rows = vec![
                ("Type".into(), j.kind.as_str().to_string()),
                ("Parent".into(), j.parent.clone()),
                ("Child".into(), j.child.clone()),
            ];
            // As the SDFormat writes it, with the frame it is in.
            if let Some(([x, y, z], frame)) = j.written_axes.first() {
                let xyz = format!("{}  {}  {}", n(*x), n(*y), n(*z));
                rows.push((
                    "Axis".into(),
                    match frame.as_deref() {
                        None => xyz,
                        Some("__model__") => format!("{xyz} (model)"),
                        Some(f) => format!("{xyz} ({f})"),
                    },
                ));
            }
            rows.extend([
                ("Lower".into(), limit(l.lower, rotates, l.lower <= -1e16)),
                ("Upper".into(), limit(l.upper, rotates, l.upper >= 1e16)),
                (
                    "Effort limit".into(),
                    match (l.effort < 0.0, rotates) {
                        (true, _) => "—".into(),
                        (false, true) => format!("{} N·m", n(l.effort)),
                        (false, false) => format!("{} N", n(l.effort)),
                    },
                ),
                (
                    "Velocity limit".into(),
                    match (l.velocity < 0.0, rotates) {
                        (true, _) => "—".into(),
                        (false, true) => format!("{}/s", format::degrees(l.velocity as f32)),
                        (false, false) => format!("{} m/s", n(l.velocity)),
                    },
                ),
                ("Damping".into(), n(l.damping)),
                ("Friction".into(), n(l.friction)),
            ]);
            definition_group(b, "Definition", rows);
            links(b, &["Plot position/velocity", "All components"]);
        })));
}

/// The live values follow the joint states (degrees and degrees a second
/// for a joint that turns, metres and metres a second for one that
/// slides).
fn joint_values(
    inspected: Res<Inspected>,
    states: Res<JointStates>,
    added: Query<(), Added<JointValue>>,
    mut values: Query<(&JointValue, &mut Text)>,
) {
    if !states.is_changed() && added.is_empty() && !inspected.is_changed() {
        return;
    }
    let state = inspected
        .part
        .as_ref()
        .and_then(|p| states.joints.get(&p.name));
    for (v, mut text) in &mut values {
        let want = match (state, v.rotates) {
            (None, _) => "—".to_string(),
            (Some(s), true) => match v.index {
                0 => format::degrees(s.position as f32),
                1 => format!("{}/s", format::degrees(s.velocity as f32)),
                _ => format!("{} N·m", format::number(s.effort, 3)),
            },
            (Some(s), false) => match v.index {
                0 => format::metres(s.position as f32),
                1 => format!("{} m/s", format::number(s.velocity, 3)),
                _ => format!("{} N", format::number(s.effort, 3)),
            },
        };
        set_text(&mut text, &want);
    }
}

#[cfg(test)]
mod tests;

/// The command slider publishes the joint's position command.
fn on_command(
    ev: On<bevy::ui_widgets::ValueChange<f32>>,
    parents: Query<&ChildOf>,
    rows: Query<&JointCommand>,
    mut out: MessageWriter<gz_bevy::connection::GzCommand>,
    mut commands: Commands,
) {
    let Some((e, row)) = parents
        .get(ev.source)
        .ok()
        .and_then(|p| rows.get(p.parent()).ok().map(|r| (p.parent(), r)))
    else {
        return;
    };
    commands.entity(e).insert(Commanded);
    out.write(gz_bevy::connection::GzCommand::Publish {
        topic: row.0.clone(),
        msg_type: "gz.msgs.Double".into(),
        payload: gz_core::teleop::double(f64::from(ev.value)),
    });
}

/// Until it is moved, the command slider sits at the joint's live
/// position, so taking the thumb doesn't jump the joint.
fn follow_joint(
    inspected: Res<Inspected>,
    states: Res<JointStates>,
    rows: Query<(), (With<JointCommand>, Without<Commanded>)>,
    sliders: Query<(Entity, &ChildOf, &SliderValue)>,
    mut commands: Commands,
) {
    let Some(position) = inspected
        .part
        .as_ref()
        .and_then(|p| states.joints.get(&p.name))
        .map(|s| s.position)
    else {
        return;
    };
    #[allow(clippy::cast_possible_truncation)]
    let v = position as f32;
    for (e, row, value) in &sliders {
        if rows.contains(row.parent()) && value.0 != v {
            commands.entity(e).insert(SliderValue(v));
        }
    }
}
