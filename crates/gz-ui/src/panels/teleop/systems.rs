//! The Teleop panel's systems: the targets (from the world's SDFormat and
//! the server's topics), arming, the command from the keys, the pad, the
//! sliders or a gamepad, published while armed, and the joints' commands
//! (DECISIONS.md D-104).

use bevy::ecs::system::SystemParam;
use bevy::input::gamepad::{Gamepad, GamepadAxis};
use bevy::input_focus::InputFocus;
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::PointerId;
use bevy::prelude::*;
use bevy::ui::{Checked, Pressed};
use bevy::ui_widgets::{SliderValue, ValueChange};
use gz_bevy::connection::{GzCommand, GzStatus};
use gz_bevy::joint_state::{JointStateWanted, JointStates};
use gz_bevy::overlays::extras::{want_sdf as want, SdfExtras, SdfWanted};
use gz_bevy::spawn::{GzEntity, GzWorldRoot};
use gz_bevy::topics::GzTopics;
use gz_bevy::view::controls::ViewRouting;
use gz_bevy::view::input::KeyMods;
use gz_bevy::view::{DriveArmed, Selection};
use gz_core::teleop::{double, twist};

use super::model::{
    moved, speed_label, stick, topic_model, DriveMode, DriveTarget, Held, JointTarget, TeleopState,
    PUBLISH_HZ,
};
use super::view::{
    spawn_joints, spawn_sliders, spawn_target, DriveSliders, GamepadStatus, JointsSection,
    LimitField, ModeChoice, ModePane, PadKey, TargetRow, TargetSelect, TeleopSlider,
};
use super::{TeleopBody, TeleopStart, APPLY_FORCE, STOP, TELEOP};
use crate::panels::{PanelActionActivated, PanelView};
use crate::shell::{
    CurrentPhase, PanelStatus, Phase, SetPanelStatus, ShownPanels, Slot, UiCommand,
};
use crate::theme::visuals::Active;
use crate::theme::Theme;
use crate::widgets::focus::TextFocus;
use crate::widgets::layout::set_shown;
use crate::widgets::text::set_text;
use crate::widgets::{NumberCommit, SelectChanged};

/// The empty states (§3.3.1).
pub(crate) const SELECT: &str = "Select a robot to drive";
pub(crate) const NO_PUBLISH: &str =
    "This server doesn't take publications, so Teleop can't drive (it needs the patched \
     websocket server)";

/// What there is to drive.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct Targets(pub Vec<DriveTarget>);

impl Targets {
    pub(crate) fn get(&self, topic: &str) -> Option<&DriveTarget> {
        self.0.iter().find(|t| t.topic == topic)
    }
}

/// What the target-dependent parts were built for.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct Built {
    targets: Option<(Vec<String>, Option<String>)>,
    joints: Option<Option<String>>,
    limits: Option<(u64, u64)>,
}

/// The view: "Not connected"; until the server says it publishes, the
/// loading state; why it can't drive; "Select a robot to drive" with
/// nothing to drive; else the controls.
pub(super) fn view(live: bool, publishes: Option<bool>, targets: bool) -> PanelView {
    match (live, publishes, targets) {
        (false, ..) => PanelView::empty("Not connected"),
        (true, None, _) => PanelView::Loading,
        (true, Some(false), _) => PanelView::empty(NO_PUBLISH),
        (true, Some(true), false) => PanelView::empty(SELECT),
        (true, Some(true), true) => PanelView::Ready,
    }
}

/// A [`TeleopStart`]: opens the panel.
pub(super) fn apply_start(
    start: Option<Res<TeleopStart>>,
    mut ui: MessageWriter<UiCommand>,
    mut commands: Commands,
) {
    if start.is_some() {
        ui.write(UiCommand::OpenPanel {
            panel: TELEOP,
            slot: Slot::Primary,
        });
        commands.remove_resource::<TeleopStart>();
    }
}

/// The world's SDFormat (its drives and joint controllers) is wanted
/// while the panel shows.
pub(super) fn want_sdf(hosts: Res<ShownPanels>, wanted: Option<ResMut<SdfWanted>>) {
    if let Some(mut wanted) = wanted {
        want(&mut wanted, "teleop", hosts.is_shown(TELEOP));
    }
}

/// The drives the world's SDFormat names (with their joints), and any
/// other `gz.msgs.Twist` topic the server lists.
pub(super) fn collect_targets(
    sdf: Option<Res<SdfExtras>>,
    topics: Res<GzTopics>,
    mut targets: ResMut<Targets>,
) {
    if !topics.is_changed() && !sdf.as_ref().is_some_and(Res::is_changed) && !targets.is_added() {
        return;
    }
    let mut out: Vec<DriveTarget> = vec![];
    if let Some(world) = sdf.as_deref().and_then(|s| s.extras.as_ref()) {
        let limits = |model: &str, joint: &str| {
            world
                .links
                .values()
                .flat_map(|l| &l.joints)
                .find(|j| j.name == joint && j.model == model)
                .and_then(|j| j.limits.first())
                .map(|l| (l.lower, l.upper))
                .filter(|(lo, hi)| lo.is_finite() && hi.is_finite() && hi - lo < 1e6 && hi > lo)
                .unwrap_or((-std::f64::consts::PI, std::f64::consts::PI))
        };
        for (path, m) in &world.models {
            let Some(topic) = &m.drive_topic else {
                continue;
            };
            // A plugin's joints share a topic: one slider moves them.
            let mut seen = std::collections::HashSet::new();
            let joints = m
                .joint_controllers
                .iter()
                .filter(|(_, topic)| seen.insert(topic))
                .map(|(joint, topic)| {
                    let (lower, upper) = limits(path, joint);
                    JointTarget {
                        joint: joint.clone(),
                        topic: topic.clone(),
                        lower,
                        upper,
                    }
                })
                .collect();
            out.push(DriveTarget {
                model: path.clone(),
                topic: topic.clone(),
                joints,
            });
        }
    }
    for t in topics
        .list
        .iter()
        .filter(|t| t.msg_type.ends_with(".msgs.Twist"))
    {
        if !out.iter().any(|d| d.topic == t.name) {
            out.push(DriveTarget {
                model: topic_model(&t.name).unwrap_or_default().to_string(),
                topic: t.name.clone(),
                joints: vec![],
            });
        }
    }
    out.sort_by(|a, b| (&a.model, &a.topic).cmp(&(&b.model, &b.topic)));
    targets.set_if_neq(Targets(out));
}

/// The selected model's name, if it is a top-level model.
#[derive(SystemParam)]
pub(super) struct SelectedModel<'w, 's> {
    selection: Option<Res<'w, Selection>>,
    nodes: Query<'w, 's, (&'static GzEntity, &'static ChildOf)>,
    roots: Query<'w, 's, (), With<GzWorldRoot>>,
}

impl SelectedModel<'_, '_> {
    fn changed(&self) -> bool {
        self.selection.as_ref().is_some_and(Res::is_changed)
    }

    fn name(&self) -> Option<String> {
        let mut e = self.selection.as_deref()?.first_target()?;
        for _ in 0..64 {
            let (g, parent) = self.nodes.get(e).ok()?;
            if self.roots.contains(parent.parent()) {
                return Some(g.name.clone());
            }
            e = parent.parent();
        }
        None
    }
}

/// The target: the selected model's drive, else the one chosen before if
/// it is still there, else the first. A new target disarms.
pub(super) fn choose_target(
    targets: Res<Targets>,
    selected: SelectedModel,
    mut state: ResMut<TeleopState>,
) {
    if !targets.is_changed() && !selected.changed() && !state.is_added() {
        return;
    }
    let of_selection = selected
        .name()
        .and_then(|m| targets.0.iter().find(|t| t.model == m))
        .map(|t| t.topic.clone());
    let kept = state.target.clone().filter(|t| targets.get(t).is_some());
    let want = of_selection
        .filter(|_| selected.changed())
        .or(kept)
        .or_else(|| targets.0.first().map(|t| t.topic.clone()));
    if state.target != want {
        state.target = want;
        state.armed = false;
    }
}

/// The view, "publishing" in the header while armed, and Stop while
/// armed.
pub(super) fn teleop_status(
    phase: Res<CurrentPhase>,
    status: Option<Res<GzStatus>>,
    targets: Res<Targets>,
    state: Res<TeleopState>,
    mut panel: ResMut<PanelStatus>,
) {
    let changed = phase.is_changed()
        || status.as_ref().is_some_and(Res::is_changed)
        || targets.is_changed()
        || state.is_changed();
    if !changed {
        return;
    }
    let live = phase.0 == Phase::Live;
    let publishes = status.as_deref().and_then(|s| s.publishes);
    panel.set_view(TELEOP, view(live, publishes, !targets.0.is_empty()));
    panel.set_count(TELEOP, state.armed.then(|| "publishing".to_string()));
    panel.set_action_enabled(TELEOP, STOP, state.armed);
    panel.set_action_enabled(TELEOP, APPLY_FORCE, false);
}

/// The panel has the drive keys: the last press, or keyboard focus, was
/// in it (a press or focus elsewhere, the Scene tree's, takes them).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TeleopKeys(pub bool);

/// Who has the drive keys: a press, or keyboard focus, in the panel or
/// elsewhere.
pub(super) fn route_keys(
    buttons: Res<ButtonInput<MouseButton>>,
    hover: Option<Res<HoverMap>>,
    focus: Option<Res<InputFocus>>,
    bodies: Query<(), With<TeleopBody>>,
    parents: Query<&ChildOf>,
    mut keys: ResMut<TeleopKeys>,
) {
    let in_panel = |e: Entity| {
        std::iter::once(e)
            .chain(parents.iter_ancestors(e))
            .any(|a| bodies.contains(a))
    };
    if buttons.get_just_pressed().next().is_some() {
        let over = hover
            .as_ref()
            .and_then(|h| h.get(&PointerId::Mouse))
            .is_some_and(|hits| hits.keys().any(|e| in_panel(*e)));
        keys.set_if_neq(TeleopKeys(over));
    }
    if let Some(e) = focus.filter(|f| f.is_changed()).and_then(|f| f.get()) {
        keys.set_if_neq(TeleopKeys(in_panel(e)));
    }
}

/// What drives: the keys, the pad, a gamepad.
#[derive(SystemParam)]
pub(super) struct DriveInput<'w, 's> {
    keys: Res<'w, ButtonInput<KeyCode>>,
    typing: Option<Res<'w, TextFocus>>,
    routing: Res<'w, ViewRouting>,
    panel_keys: Res<'w, TeleopKeys>,
    hosts: Res<'w, ShownPanels>,
    pads: Query<'w, 's, (&'static PadKey, Has<Pressed>)>,
    gamepads: Query<'w, 's, &'static Gamepad>,
}

impl DriveInput<'_, '_> {
    /// The keys may drive: not while typing; from the views or the panel
    /// (shortcuts.json), not from another panel's list.
    fn keys_live(&self) -> bool {
        !self.typing() && (self.routing.keys || self.panel_keys.0)
    }

    /// A text field has the keys.
    fn typing(&self) -> bool {
        self.typing.as_ref().is_some_and(|t| t.0)
    }

    /// The drive keys held (W A S D and the arrows). With Ctrl, Alt or ⌘
    /// down a command chord is being typed (⌘S saves), so nothing is.
    fn keys_held(&self) -> Held {
        if KeyMods::held(&self.keys).command() {
            return Held::default();
        }
        let any = |k: [KeyCode; 2]| self.keys.any_pressed(k);
        Held {
            forward: any([KeyCode::KeyW, KeyCode::ArrowUp]),
            back: any([KeyCode::KeyS, KeyCode::ArrowDown]),
            left: any([KeyCode::KeyA, KeyCode::ArrowLeft]),
            right: any([KeyCode::KeyD, KeyCode::ArrowRight]),
        }
    }

    /// The pad keys pressed.
    fn pad_held(&self) -> Held {
        let mut h = Held::default();
        for (k, pressed) in &self.pads {
            if pressed {
                match k {
                    PadKey::Forward => h.forward = true,
                    PadKey::Back => h.back = true,
                    PadKey::Left => h.left = true,
                    PadKey::Right => h.right = true,
                }
            }
        }
        h
    }

    /// The first gamepad's left stick: forward and turn (left is +).
    fn stick(&self) -> Option<(f64, f64)> {
        let g = self.gamepads.iter().next()?;
        let y = g.get(GamepadAxis::LeftStickY).unwrap_or(0.0);
        let x = g.get(GamepadAxis::LeftStickX).unwrap_or(0.0);
        Some((stick(y), -stick(x)))
    }
}

/// The command going out while armed (the gamepad's state reads it).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Driving(pub Option<(f64, f64)>);

/// What [`drive`] keeps between frames.
#[derive(Default)]
pub(super) struct DriveMemory {
    /// The last command sent, and when.
    last: Option<(String, (f64, f64))>,
    sent_at: f64,
    was_armed: bool,
    /// The mode and target seen last.
    seen: Option<(DriveMode, Option<String>)>,
    /// Since a disarm, a new mode or a new target: nothing arms until the
    /// pad and the stick let go.
    latched: bool,
    /// Zero commands not yet sent (the server stopped taking them), with
    /// the world they are for.
    zeros: Vec<(String, Option<String>)>,
}

/// The command, and arming: in Keyboard mode the pad arms, and once armed
/// the drive keys drive too; a slider arms in Sliders mode; the stick in
/// Gamepad mode. X, Stop, a hidden panel, a new mode or target, or a world
/// that can't take it disarms, and nothing arms again until the mode's
/// input lets go. The robot driven last gets a zero command when the drive leaves it,
/// once the server takes it (after a reconnect, if the connection
/// dropped). While armed, the command goes out on each change and
/// [`PUBLISH_HZ`] times a second.
#[allow(clippy::too_many_arguments)]
pub(super) fn drive(
    time: Res<Time<Real>>,
    input: DriveInput,
    phase: Res<CurrentPhase>,
    status: Option<Res<GzStatus>>,
    mut state: ResMut<TeleopState>,
    mut armed: ResMut<DriveArmed>,
    mut driving: ResMut<Driving>,
    mut out: MessageWriter<GzCommand>,
    mut mem: Local<DriveMemory>,
) {
    let now = time.elapsed_secs_f64();
    let publishes =
        phase.0 == Phase::Live && status.as_deref().and_then(|s| s.publishes) == Some(true);
    let world = status.as_deref().and_then(|s| s.world.clone());
    let can = publishes && input.hosts.is_shown(TELEOP) && state.target.is_some();
    // X stops wherever the keys are (shortcuts.json `teleop.stop`), only
    // not while typing; like Stop, it zeroes the sliders.
    let stop = state.armed && !input.typing() && input.keys.just_pressed(KeyCode::KeyX);
    if stop {
        state.lin = 0.0;
        state.ang = 0.0;
    }
    let seen = (state.mode, state.target.clone());
    let new_drive = mem.seen.as_ref() != Some(&seen);
    if state.armed && (!can || stop || new_drive) {
        state.armed = false;
    }
    if (mem.was_armed && !state.armed) || new_drive {
        mem.latched = true;
    }
    mem.seen = Some(seen);
    let pad = input.pad_held();
    let stick = input.stick().unwrap_or((0.0, 0.0));
    // The mode's own input lets go: a resting stick doesn't hold the pad
    // off, nor the pad the stick.
    let idle = match state.mode {
        DriveMode::Keyboard => !pad.any(),
        DriveMode::Sliders => true,
        DriveMode::Gamepad => stick == (0.0, 0.0),
    };
    if mem.latched && idle {
        mem.latched = false;
    }
    let may_arm = can && !state.armed && !mem.latched;
    let command = match state.mode {
        DriveMode::Keyboard => {
            if pad.any() && may_arm {
                state.armed = true;
            }
            let keys = if state.armed && input.keys_live() {
                input.keys_held()
            } else {
                Held::default()
            };
            let held = Held {
                forward: keys.forward || pad.forward,
                back: keys.back || pad.back,
                left: keys.left || pad.left,
                right: keys.right || pad.right,
            };
            held.command(state.max_lin, state.max_ang)
        }
        DriveMode::Sliders => (state.lin, state.ang),
        DriveMode::Gamepad => {
            if stick != (0.0, 0.0) && may_arm {
                state.armed = true;
            }
            (stick.0 * state.max_lin, stick.1 * state.max_ang)
        }
    };
    // The drive left the robot it drove: it stops.
    let target = state.target.clone();
    let armed_on = |t: &String| state.armed && target.as_ref() == Some(t);
    if let Some((topic, _)) = mem.last.take_if(|(t, _)| !armed_on(t)) {
        if !mem.zeros.iter().any(|(t, _)| *t == topic) {
            mem.zeros.push((topic, world.clone()));
        }
    }
    if publishes {
        for (topic, w) in std::mem::take(&mut mem.zeros) {
            if w == world {
                out.write(GzCommand::Publish {
                    topic,
                    msg_type: "gz.msgs.Twist".into(),
                    payload: twist(0.0, 0.0),
                });
            }
        }
    }
    if let (true, Some(topic)) = (state.armed, target) {
        let max = (state.max_lin, state.max_ang);
        let fresh = mem
            .last
            .as_ref()
            .is_none_or(|(_, c)| moved(*c, command, max));
        if fresh || now - mem.sent_at >= 1.0 / PUBLISH_HZ {
            out.write(GzCommand::Publish {
                topic: topic.clone(),
                msg_type: "gz.msgs.Twist".into(),
                payload: twist(command.0, command.1),
            });
            mem.sent_at = now;
            mem.last = Some((topic, command));
        }
    }
    mem.was_armed = state.armed;
    armed.set_if_neq(DriveArmed(state.armed));
    driving.set_if_neq(Driving(state.armed.then_some(command)));
}

/// The target row, rebuilt when the targets or the choice change.
pub(super) fn sync_target(
    theme: Res<Theme>,
    targets: Res<Targets>,
    state: Res<TeleopState>,
    rows: Query<(Entity, Ref<TargetRow>)>,
    mut built: ResMut<Built>,
    mut commands: Commands,
) {
    let Ok((row, added)) = rows.single() else {
        return;
    };
    let key = (
        targets.0.iter().map(DriveTarget::label).collect(),
        state.target.clone(),
    );
    if !added.is_added() && built.targets.as_ref() == Some(&key) {
        return;
    }
    commands.entity(row).despawn_children();
    let t = theme.clone();
    let list = targets.0.clone();
    let chosen = state.target.clone();
    commands
        .entity(row)
        .with_children(|r| spawn_target(r, &t, &list, chosen.as_deref()));
    built.targets = Some(key);
}

/// The sliders, rebuilt when the limits change; the joints, when the
/// target does.
pub(super) fn sync_sliders(
    theme: Res<Theme>,
    targets: Res<Targets>,
    state: Res<TeleopState>,
    sliders: Query<(Entity, Ref<DriveSliders>)>,
    mut joints: Query<(Entity, Ref<JointsSection>, &mut Node)>,
    mut built: ResMut<Built>,
    mut commands: Commands,
) {
    if let Ok((e, added)) = sliders.single() {
        let key = (state.max_lin.to_bits(), state.max_ang.to_bits());
        if added.is_added() || built.limits != Some(key) {
            commands.entity(e).despawn_children();
            let (t, s) = (theme.clone(), state.clone());
            commands
                .entity(e)
                .with_children(|p| spawn_sliders(p, &t, &s));
            built.limits = Some(key);
        }
    }
    if let Ok((e, added, mut node)) = joints.single_mut() {
        let key = state.target.clone();
        if added.is_added() || built.joints.as_ref() != Some(&key) {
            commands.entity(e).despawn_children();
            let target = key
                .as_deref()
                .and_then(|t| targets.get(t))
                .filter(|t| !t.joints.is_empty())
                .cloned();
            set_shown(&mut node, target.is_some());
            if let Some(target) = target {
                let (t, s) = (theme.clone(), state.clone());
                commands
                    .entity(e)
                    .with_children(|p| spawn_joints(p, &t, &target, &s));
            }
            built.joints = Some(key);
        }
    }
}

/// While the panel shows a target with joints, its model's joint states
/// are wanted, for the sliders' live positions; the Joint inspector keeps
/// the want while it shows another model's joint.
pub(super) fn want_joint_states(
    hosts: Res<ShownPanels>,
    state: Res<TeleopState>,
    targets: Res<Targets>,
    sdf: Option<Res<SdfExtras>>,
    wanted: Option<ResMut<JointStateWanted>>,
    mut mine: Local<Option<String>>,
) {
    let Some(mut wanted) = wanted else {
        return;
    };
    if wanted.model.is_some() && wanted.model != *mine {
        *mine = None;
        return;
    }
    let want = state
        .target
        .as_deref()
        .and_then(|t| targets.get(t))
        .filter(|t| hosts.is_shown(TELEOP) && !t.joints.is_empty())
        .map(|t| t.model.clone());
    if wanted.model != want {
        let topic = want.as_deref().and_then(|m| {
            sdf.as_deref()
                .and_then(|s| s.extras.as_ref())
                .and_then(|w| w.model(m))
                .and_then(|m| m.joint_state_topic.clone())
        });
        wanted.set_if_neq(JointStateWanted {
            model: want.clone(),
            topic,
        });
    }
    *mine = want;
}

/// The joints' live positions, where the joint states held are the
/// target's model's.
#[derive(SystemParam)]
pub(super) struct LivePositions<'w> {
    states: Option<Res<'w, JointStates>>,
    wanted: Option<Res<'w, JointStateWanted>>,
}

impl LivePositions<'_> {
    fn of(&self, model: &str, joint: &str) -> Option<f64> {
        let wanted = self.wanted.as_deref()?.model.as_deref()?;
        let states = self.states.as_deref()?;
        (wanted == model)
            .then(|| states.joints.get(joint))
            .flatten()
            .map(|s| s.position)
    }
}

/// The sliders show what goes out: the drive's command (Stop zeroes it),
/// and a joint's last command, else its position, so taking the thumb
/// doesn't jump it.
pub(super) fn follow_sliders(
    state: Res<TeleopState>,
    targets: Res<Targets>,
    live: LivePositions,
    rows: Query<&TeleopSlider>,
    sliders: Query<(Entity, &ChildOf, &SliderValue)>,
    mut commands: Commands,
) {
    let target = state.target.as_deref().and_then(|t| targets.get(t));
    for (e, row, value) in &sliders {
        let Ok(slider) = rows.get(row.parent()) else {
            continue;
        };
        let want = match slider {
            TeleopSlider::Linear => Some(state.lin),
            TeleopSlider::Angular => Some(state.ang),
            TeleopSlider::Joint(topic) => state.joints.get(topic).copied().or_else(|| {
                let t = target?;
                let j = t.joints.iter().find(|j| j.topic == *topic)?;
                live.of(&t.model, &j.joint)
            }),
        };
        #[allow(clippy::cast_possible_truncation)]
        if let Some(v) = want.map(|v| v as f32).filter(|v| *v != value.0) {
            commands.entity(e).insert(SliderValue(v));
        }
    }
}

/// The chosen mode's tab is checked and its pane shows; the pad's keys
/// show pressed while they drive; the gamepad's state.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(super) fn mode_states(
    state: Res<TeleopState>,
    driving: Res<Driving>,
    input: DriveInput,
    tabs: Query<(Entity, &ModeChoice, Has<Checked>)>,
    mut panes: Query<(&ModePane, &mut Node)>,
    pad: Query<(Entity, &PadKey, Has<Active>)>,
    mut gamepad: Query<&mut Text, With<GamepadStatus>>,
    mut commands: Commands,
) {
    for (e, tab, checked) in &tabs {
        let want = tab.0 == state.mode;
        if want != checked {
            if want {
                commands.entity(e).insert(Checked);
            } else {
                commands.entity(e).remove::<Checked>();
            }
        }
    }
    for (pane, mut node) in &mut panes {
        set_shown(&mut node, pane.0 == state.mode);
    }
    let keys = if state.armed && input.keys_live() {
        input.keys_held()
    } else {
        Held::default()
    };
    let pads = input.pad_held();
    for (e, key, active) in &pad {
        let want = match key {
            PadKey::Forward => keys.forward || pads.forward,
            PadKey::Back => keys.back || pads.back,
            PadKey::Left => keys.left || pads.left,
            PadKey::Right => keys.right || pads.right,
        };
        if want != active {
            if want {
                commands.entity(e).insert(Active);
            } else {
                commands.entity(e).remove::<Active>();
            }
        }
    }
    let say = match (input.gamepads.iter().next(), driving.0) {
        (Some(_), Some((lin, ang))) if state.mode == DriveMode::Gamepad => format!(
            "Driving: {} · {}",
            speed_label(lin, "m/s"),
            speed_label(ang, "rad/s")
        ),
        (Some(_), _) => "Gamepad connected".to_string(),
        (None, _) => "No gamepad connected".to_string(),
    };
    for mut t in &mut gamepad {
        set_text(&mut t, &say);
    }
}

/// A mode tab picks the mode (and disarms).
pub(super) fn on_mode(
    ev: On<ValueChange<Entity>>,
    tabs: Query<&ModeChoice>,
    mut state: ResMut<TeleopState>,
) {
    if let Ok(tab) = tabs.get(ev.value) {
        if state.mode != tab.0 {
            state.mode = tab.0;
            state.armed = false;
        }
    }
}

/// The target select picks the target.
pub(super) fn on_target(
    ev: On<SelectChanged>,
    selects: Query<&TargetSelect>,
    mut state: ResMut<TeleopState>,
) {
    let Ok(select) = selects.get(ev.entity) else {
        return;
    };
    if let Some(topic) = select.0.get(ev.index) {
        if state.target.as_ref() != Some(topic) {
            state.target = Some(topic.clone());
            state.armed = false;
        }
    }
}

/// A slider: the drive sliders set the command and arm; a joint's
/// publishes its position.
pub(super) fn on_slider(
    ev: On<ValueChange<f32>>,
    parents: Query<&ChildOf>,
    sliders: Query<&TeleopSlider>,
    phase: Res<CurrentPhase>,
    status: Option<Res<GzStatus>>,
    mut state: ResMut<TeleopState>,
    mut out: MessageWriter<GzCommand>,
) {
    let Some(slider) = parents
        .get(ev.source)
        .ok()
        .and_then(|p| sliders.get(p.parent()).ok())
    else {
        return;
    };
    let publishes =
        phase.0 == Phase::Live && status.as_deref().and_then(|s| s.publishes) == Some(true);
    let v = f64::from(ev.value);
    match slider {
        TeleopSlider::Linear => state.lin = v,
        TeleopSlider::Angular => state.ang = v,
        TeleopSlider::Joint(topic) => {
            state.joints.insert(topic.clone(), v);
            if publishes {
                out.write(GzCommand::Publish {
                    topic: topic.clone(),
                    msg_type: "gz.msgs.Double".into(),
                    payload: double(v),
                });
            }
            return;
        }
    }
    if publishes && state.mode == DriveMode::Sliders && !state.armed {
        state.armed = true;
    }
}

/// The limits' fields set the limits.
pub(super) fn on_limit(
    ev: On<NumberCommit>,
    fields: Query<&LimitField>,
    mut state: ResMut<TeleopState>,
) {
    let Ok(field) = fields.get(ev.entity) else {
        return;
    };
    let v = ev.value.max(0.0);
    match field {
        LimitField::Linear => {
            state.max_lin = v;
            state.lin = state.lin.clamp(-v, v);
        }
        LimitField::Angular => {
            state.max_ang = v;
            state.ang = state.ang.clamp(-v, v);
        }
    }
}

/// Stop disarms (and the drive's last command is zero).
pub(super) fn on_action(ev: On<PanelActionActivated>, mut state: ResMut<TeleopState>) {
    if ev.panel == TELEOP && ev.action == STOP && state.armed {
        state.armed = false;
        state.lin = 0.0;
        state.ang = 0.0;
    }
}
