//! [`ViewerControls`]: the minimal mode's mouse and keyboard, turned into
//! [`ViewCommand`]s and sim commands (`.agent/UI_PLAN.md` §2.3).
//!
//! Mouse (spec §6.2): left drag runs the view's drag action, right drag
//! pans, wheel or middle drag zooms, Ctrl+wheel and pinch zoom to the
//! cursor, and Space held with a left drag pans. Keys come from the
//! [`InputMap`] in the scopes the mode enables. In minimal mode there are
//! no UI nodes, so the whole window is the view. In the full UI,
//! [`ViewRouting`] (written by the shell) says whether the pointer is over
//! the view and whether the view has the keys.
//!
//! Trackpads (DECISIONS.md D-18): a native pinch (`PinchGesture`, macOS)
//! zooms to the cursor, and a native two-finger scroll, which arrives as
//! pixel-unit wheel deltas, pans (`PanGesture` too, on iOS). Browsers report
//! a two-finger scroll and a mouse wheel alike, so there both zoom; a pinch
//! arrives as a wheel event with `ctrlKey` set and no Control key down,
//! which only a page listener sees ([`BrowserWheel`], `view::web`).

use bevy::ecs::system::SystemParam;
use bevy::input::gestures::{PanGesture, PinchGesture};
use bevy::input::keyboard::KeyboardFocusLost;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::input::touch::TouchPhase;
use bevy::prelude::*;

use super::input::{Action, InputMap, KeyMods, Platform, Scope};
use super::{
    apply_view_commands, apply_view_placement, view_rect, DragAction, Selection, ViewCamera,
    ViewCommand, ViewOp, ViewOrigin, ViewPlacement, ZOOM_STEP,
};
use crate::connection::{GzCommand, GzStatus, StepsPerClick};
use crate::overlays::{apply_overlay_commands, OverlayCommand, Overlays};
use crate::render::GzGrid;
use crate::UiMode;

/// A Space press shorter than this, with no pointer drag, is a tap
/// (play/pause, decision Q4).
pub const TAP_SECS: f64 = 0.2;
/// Middle (or Zoom-action) drags zoom one [`super::ZOOM_STEP`] per this many
/// pixels of vertical movement.
pub const ZOOM_DRAG_PX: f32 = 5.0;
/// Pixel-unit wheel deltas per zoom step, in logical pixels: one notch of a
/// mouse wheel in Chrome, so a notch is a step as it is natively.
pub const WHEEL_PX_PER_STEP: f32 = 100.0;
/// Line-unit wheel deltas per zoom step in the browser: Firefox reports
/// three lines a notch (native line deltas are one a notch).
pub const WEB_LINES_PER_STEP: f32 = 3.0;
/// A browser pinch reports about this many logical pixels of wheel delta
/// per e-fold of scale (`deltaY ≈ -100·ln(scale)`).
pub const PINCH_PX_PER_E: f32 = 100.0;
/// Wheel zoom per frame stops at this many steps, so a momentum flick
/// doesn't throw the view.
pub const MAX_WHEEL_STEPS: f32 = 5.0;
/// Fly keys move at most this long per frame, so a stalled frame doesn't
/// jump the camera.
pub const MAX_FLY_SECS: f32 = 0.1;

/// Mouse and keyboard camera controls and the viewport/sim keys.
pub struct ViewerControls {
    pub mode: UiMode,
}

impl Default for ViewerControls {
    fn default() -> Self {
        Self {
            mode: UiMode::Minimal,
        }
    }
}

impl ViewerControls {
    /// The key scopes `mode` enables.
    pub fn scopes(mode: UiMode) -> Vec<Scope> {
        match mode {
            UiMode::None => Vec::new(),
            // The full UI routes viewport keys by hover and focus
            // (`ViewRouting`); W A S D stay the fly keys until Teleop (M7)
            // can take them while armed.
            UiMode::Minimal | UiMode::Full => vec![Scope::Global, Scope::Viewport, Scope::Minimal],
        }
    }
}

/// Where [`apply_view_placement`] runs: after UI layout, before culling.
/// The full UI writes [`ViewPlacement`] before it.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ViewPlacementSystems;

/// The key scopes live right now.
#[derive(Resource, Debug, Clone, Default)]
pub struct ActiveScopes(pub Vec<Scope>);

/// Where the camera's input may come from (spec §3.4, `.agent/UI_PLAN.md`
/// §2.2 pointer routing). Minimal mode has no UI, so both stay true; the
/// full UI's shell sets them every frame.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewRouting {
    /// The pointer is over the view and not over any UI node: presses,
    /// the wheel and gestures reach the camera. A drag that started in the
    /// view keeps going over the UI.
    pub pointer: bool,
    /// Viewport-scope keys (and the minimal fly keys) apply: the pointer is
    /// over the view, or the view has focus.
    pub keys: bool,
}

impl Default for ViewRouting {
    fn default() -> Self {
        Self {
            pointer: true,
            keys: true,
        }
    }
}

impl ViewRouting {
    /// The scopes that apply to keys right now: `active` without the
    /// view's own scopes when the view doesn't have the keys. Several
    /// systems ask every frame, so this doesn't allocate.
    pub fn key_scopes(&self, active: &[Scope]) -> KeyScopes {
        let mut out = KeyScopes::default();
        for &s in active {
            let live = self.keys || !matches!(s, Scope::Viewport | Scope::Minimal);
            if live && !out.contains(&s) {
                out.scopes[out.len] = s;
                out.len += 1;
            }
        }
        out
    }
}

/// A set of [`Scope`]s on the stack (there are five), as a slice.
#[derive(Clone, Copy, Debug)]
pub struct KeyScopes {
    scopes: [Scope; 5],
    len: usize,
}

impl Default for KeyScopes {
    fn default() -> Self {
        Self {
            scopes: [Scope::Global; 5],
            len: 0,
        }
    }
}

impl std::ops::Deref for KeyScopes {
    type Target = [Scope];

    fn deref(&self) -> &[Scope] {
        &self.scopes[..self.len]
    }
}

/// What the page saw of this frame's wheel and gesture events that Bevy's
/// don't carry. Filled by `view::web` in the browser.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq)]
pub struct BrowserWheel {
    /// A wheel event had `ctrlKey`: with no Control key down, that is a
    /// trackpad pinch (Chrome, Firefox).
    pub ctrl: bool,
    /// Safari's pinch (`gesturechange`): the change in log scale.
    pub pinch: f32,
}

/// Pointer gesture and Space-tap state.
#[derive(Resource, Debug, Clone, Default)]
pub struct ControlState {
    drag: Option<Drag>,
    tap: Option<Tap>,
    /// Keys that went down while ⌘ was held (see [`drop_command_chord_keys`]).
    under_command: Vec<KeyCode>,
}

#[derive(Debug, Clone, Copy)]
struct Drag {
    button: MouseButton,
    gesture: DragAction,
    last: Option<Vec2>,
}

/// A tap chord that went down at `since` (`Time<Real>` seconds).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Tap {
    key: KeyCode,
    action: Option<Action>,
    since: f64,
    dragged: bool,
}

impl Plugin for ViewerControls {
    fn build(&self, app: &mut App) {
        app.add_message::<ViewCommand>()
            .add_message::<GzCommand>()
            .add_message::<OverlayCommand>()
            .init_resource::<Overlays>()
            .init_resource::<Selection>()
            .init_resource::<InputMap>()
            .init_resource::<ControlState>()
            .init_resource::<BrowserWheel>()
            .init_resource::<ViewRouting>()
            .init_resource::<ViewPlacement>()
            .init_resource::<StepsPerClick>()
            .init_resource::<GzGrid>()
            .init_resource::<crate::camera::GzCameraTarget>()
            .insert_resource(ActiveScopes(Self::scopes(self.mode)))
            .add_systems(
                Update,
                (
                    drop_command_chord_keys,
                    super::apply_start_view,
                    mouse_input,
                    key_input,
                    fly_input,
                    apply_view_commands,
                    apply_overlay_commands,
                )
                    .chain()
                    // A new environment respawns the camera from its state:
                    // this frame's commands go in first.
                    .before(crate::render::apply_environment),
            )
            // Followed targets move with this frame's poses.
            .add_systems(
                Update,
                super::modes::follow_targets
                    .after(apply_view_commands)
                    .after(crate::spawn::apply_poses),
            );
        app.configure_sets(
            PostUpdate,
            ViewPlacementSystems
                .after(bevy::ui::UiSystems::Layout)
                .before(bevy::camera::visibility::VisibilitySystems::UpdateFrusta),
        )
        .add_systems(
            PostUpdate,
            // Only a placed view (the full UI) touches `Assets<Image>`.
            apply_view_placement
                .run_if(|p: Res<ViewPlacement>| p.rect.is_some())
                .in_set(ViewPlacementSystems),
        );
        #[cfg(target_arch = "wasm32")]
        super::web::install(app);
    }
}

/// The view under the pointer: its camera entity and state, the cursor in
/// its viewport, the viewport's size and the window's scale factor.
#[derive(Clone, Copy)]
struct Target<'a> {
    view: Entity,
    camera: &'a ViewCamera,
    cursor: Option<Vec2>,
    size: Vec2,
    scale: f32,
}

/// The views the controls drive.
pub type Views<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static ViewCamera,
        &'static Camera,
        Option<&'static ViewOrigin>,
    ),
>;

fn target<'a>(windows: &Query<&Window>, views: &'a Views) -> Option<Target<'a>> {
    let window = windows.single().ok()?;
    let (view, camera, cam, placed) = views.single().ok()?;
    let rect = view_rect(cam, placed);
    let origin = rect.map_or(Vec2::ZERO, |r| r.min);
    let size = rect.map_or(Vec2::new(window.width(), window.height()), |r| r.size());
    Some(Target {
        view,
        camera,
        cursor: window.cursor_position().map(|c| c - origin),
        size,
        scale: window.scale_factor(),
    })
}

/// macOS browsers send no key-up for a key released while ⌘ is held, so
/// after ⌘→ (step) the arrow would stay down and fly the camera. When ⌘
/// goes up, the keys that went down under it are released too; one still
/// held comes back with its next key repeat.
pub fn drop_command_chord_keys(
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut state: ResMut<ControlState>,
) {
    const SUPER: [KeyCode; 2] = [KeyCode::SuperLeft, KeyCode::SuperRight];
    if keys.any_pressed(SUPER) {
        let new: Vec<KeyCode> = keys
            .get_just_pressed()
            .copied()
            .filter(|k| !is_modifier(*k))
            .collect();
        state.under_command.extend(new);
    } else if keys.any_just_released(SUPER) {
        for key in std::mem::take(&mut state.under_command) {
            keys.release(key);
        }
    }
}

fn is_modifier(key: KeyCode) -> bool {
    matches!(
        key,
        KeyCode::ShiftLeft
            | KeyCode::ShiftRight
            | KeyCode::ControlLeft
            | KeyCode::ControlRight
            | KeyCode::AltLeft
            | KeyCode::AltRight
            | KeyCode::SuperLeft
            | KeyCode::SuperRight
    )
}

/// Wheel and trackpad input for [`mouse_input`].
#[derive(SystemParam)]
pub struct WheelInput<'w, 's> {
    wheel: MessageReader<'w, 's, MouseWheel>,
    pinch: MessageReader<'w, 's, PinchGesture>,
    pan: MessageReader<'w, 's, PanGesture>,
    browser: Res<'w, BrowserWheel>,
}

impl WheelInput<'_, '_> {
    fn clear(&mut self) {
        self.wheel.clear();
        self.pinch.clear();
        self.pan.clear();
    }
}

/// One frame of wheel and trackpad input, summed.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct WheelFrame {
    /// Zoom steps about the view centre (plain wheel).
    centre: f32,
    /// Zoom steps about the cursor (Ctrl+wheel, pinch).
    cursor: f32,
    /// Two-finger pan, in logical pixels.
    pan: Vec2,
    /// A two-finger scroll started: pick the pan pivot under the cursor.
    pan_started: bool,
}

/// Zoom steps for a pinch that scales the view by `e^log_scale`.
fn pinch_steps(log_scale: f32) -> f32 {
    log_scale / -ZOOM_STEP.ln()
}

/// Sums this frame's wheel and trackpad input. `ctrl` is a Control key
/// held; `web` normalises browser deltas.
fn read_wheel(input: &mut WheelInput, scale: f32, ctrl: bool, web: bool) -> WheelFrame {
    let mut out = WheelFrame::default();
    let pinch = input.browser.ctrl && !ctrl;
    for e in input.wheel.read() {
        let (x, y) = match e.unit {
            MouseScrollUnit::Line => (e.x, e.y),
            MouseScrollUnit::Pixel => (e.x / scale, e.y / scale),
        };
        match e.unit {
            MouseScrollUnit::Pixel if pinch => out.cursor += pinch_steps(y / PINCH_PX_PER_E),
            // A native two-finger scroll pans; with Ctrl it zooms.
            MouseScrollUnit::Pixel if !web && !ctrl => {
                out.pan += Vec2::new(x, y);
                out.pan_started |= e.phase == TouchPhase::Started;
            }
            MouseScrollUnit::Pixel => {
                *if ctrl {
                    &mut out.cursor
                } else {
                    &mut out.centre
                } += y / WHEEL_PX_PER_STEP;
            }
            MouseScrollUnit::Line => {
                let steps = if web { y / WEB_LINES_PER_STEP } else { y };
                *if ctrl || pinch {
                    &mut out.cursor
                } else {
                    &mut out.centre
                } += steps;
            }
        }
    }
    out.cursor += pinch_steps(input.browser.pinch);
    for PinchGesture(d) in input.pinch.read() {
        if *d > -1.0 {
            out.cursor += pinch_steps(d.ln_1p());
        }
    }
    for PanGesture(d) in input.pan.read() {
        out.pan += *d / scale;
    }
    out.centre = out.centre.clamp(-MAX_WHEEL_STEPS, MAX_WHEEL_STEPS);
    out.cursor = out.cursor.clamp(-MAX_WHEEL_STEPS, MAX_WHEEL_STEPS);
    out
}

/// Mouse buttons, wheel and trackpad → [`ViewCommand`]s.
#[allow(clippy::too_many_arguments)]
pub fn mouse_input(
    windows: Query<&Window>,
    views: Views,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    map: Res<InputMap>,
    scopes: Res<ActiveScopes>,
    routing: Res<ViewRouting>,
    mut wheel: WheelInput,
    mut state: ResMut<ControlState>,
    mut out: MessageWriter<ViewCommand>,
) {
    let Some(target) = target(&windows, &views) else {
        wheel.clear();
        state.drag = None;
        return;
    };
    let scopes = routing.key_scopes(&scopes.0);
    let view = target.view;
    let cursor = target.cursor;
    let send = |out: &mut MessageWriter<ViewCommand>, op| {
        out.write(ViewCommand { view, op });
    };

    if state.drag.is_some_and(|d| !buttons.pressed(d.button)) {
        state.drag = None;
    }
    if state.drag.is_none() && routing.pointer {
        let pressed = [MouseButton::Left, MouseButton::Right, MouseButton::Middle]
            .into_iter()
            .find(|b| buttons.just_pressed(*b));
        if let (Some(button), Some(at)) = (pressed, cursor) {
            let gesture = match button {
                MouseButton::Left if map.holding(&keys, &scopes, Action::PanHold) => {
                    DragAction::Pan
                }
                MouseButton::Left => target.camera.drag_action,
                MouseButton::Right => DragAction::Pan,
                _ => DragAction::Zoom,
            };
            let orbit = gesture == DragAction::Orbit;
            send(&mut out, ViewOp::PivotAt { at, orbit });
            state.drag = Some(Drag {
                button,
                gesture,
                last: Some(at),
            });
        }
    }
    if let Some(drag) = state.drag.as_mut() {
        if let (Some(now), Some(last)) = (cursor, drag.last) {
            let d = now - last;
            match drag.gesture {
                _ if d == Vec2::ZERO => {}
                DragAction::Orbit => send(&mut out, ViewOp::Orbit(d)),
                DragAction::Pan => send(&mut out, ViewOp::Pan(d)),
                // Down zooms out, as three.js's middle-drag dolly does.
                DragAction::Zoom if d.y != 0.0 => send(&mut out, ViewOp::Zoom(-d.y / ZOOM_DRAG_PX)),
                DragAction::Zoom => {}
            }
        }
        drag.last = cursor;
    }

    if !routing.pointer {
        // The wheel and gestures belong to the UI under the pointer.
        wheel.clear();
        return;
    }
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let input = read_wheel(
        &mut wheel,
        target.scale,
        ctrl,
        map.platform == Platform::Web,
    );
    let Some(cursor) = cursor else { return };
    if input.pan_started {
        send(
            &mut out,
            ViewOp::PivotAt {
                at: cursor,
                orbit: false,
            },
        );
    }
    if input.pan != Vec2::ZERO {
        send(&mut out, ViewOp::Pan(input.pan));
    }
    // Plain wheel zooms about the view centre; Ctrl+wheel and pinch about
    // the point under the cursor (spec §6.2, DECISIONS.md D-11).
    for (at, steps) in [(target.size / 2.0, input.centre), (cursor, input.cursor)] {
        if steps != 0.0 {
            send(&mut out, ViewOp::ZoomAt { at, steps });
        }
    }
}

/// Viewport and sim keys → [`ViewCommand`]s, [`OverlayCommand`]s and
/// [`GzCommand`]s, including the Space tap.
#[allow(clippy::too_many_arguments)]
pub fn key_input(
    windows: Query<&Window>,
    views: Views,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    map: Res<InputMap>,
    scopes: Res<ActiveScopes>,
    routing: Res<ViewRouting>,
    time: Res<Time<Real>>,
    status: Option<Res<GzStatus>>,
    steps: Option<Res<StepsPerClick>>,
    mut focus_lost: MessageReader<KeyboardFocusLost>,
    mut state: ResMut<ControlState>,
    mut overlay_out: MessageWriter<OverlayCommand>,
    mut view_out: MessageWriter<ViewCommand>,
    mut sim_out: MessageWriter<GzCommand>,
) {
    let now = time.elapsed_secs_f64();
    let scopes = routing.key_scopes(&scopes.0);
    let mut fired: Vec<Action> = map
        .just_pressed(&keys, &scopes)
        .filter_map(|b| b.action)
        .collect();

    // Losing focus releases every key; that release is no tap (alt-tab
    // right after pressing Space mustn't toggle the sim).
    if focus_lost.read().count() > 0 {
        state.tap = None;
        state.under_command.clear();
    } else {
        tap(&keys, &buttons, &map, &scopes, now, &mut state, &mut fired);
    }

    let target = target(&windows, &views);
    let sim = status.and_then(|s| s.sim);
    for action in fired {
        let op = match action {
            Action::Focus => ViewOp::Focus {
                at: target.and_then(|t| t.cursor),
            },
            Action::Home => ViewOp::Home,
            Action::ToggleProjection => ViewOp::ToggleProjection,
            Action::Snap(face) => ViewOp::Snap(face),
            Action::Follow => ViewOp::Follow {
                at: target.and_then(|t| t.cursor),
            },
            Action::Track => ViewOp::Track {
                at: target.and_then(|t| t.cursor),
            },
            Action::ToggleOverlay(o) => {
                overlay_out.write(OverlayCommand::toggle(o));
                continue;
            }
            // The stats say which way to toggle; without them, nothing.
            Action::PlayPause => {
                if let Some(sim) = sim {
                    sim_out.write(if sim.paused {
                        GzCommand::Play
                    } else {
                        GzCommand::Pause
                    });
                }
                continue;
            }
            Action::Step => {
                if sim.is_some_and(|s| s.paused) {
                    let n = steps.as_deref().copied().unwrap_or_default().0;
                    sim_out.write(GzCommand::Step(n));
                }
                continue;
            }
            Action::PanHold | Action::Fly(_) => continue,
        };
        if let Some(t) = target {
            view_out.write(ViewCommand { view: t.view, op });
        }
    }
}

/// A tap: down and up within [`TAP_SECS`], no mouse button in between. Down
/// and up in one frame is a tap too.
fn tap(
    keys: &ButtonInput<KeyCode>,
    buttons: &ButtonInput<MouseButton>,
    map: &InputMap,
    scopes: &[Scope],
    now: f64,
    state: &mut ControlState,
    fired: &mut Vec<Action>,
) {
    let mouse_down = buttons.get_pressed().next().is_some();
    if let Some(tap) = state.tap.as_mut() {
        tap.dragged |= mouse_down;
        if keys.just_released(tap.key) {
            if now - tap.since < TAP_SECS && !tap.dragged {
                fired.extend(tap.action);
            }
            state.tap = None;
        }
    }
    if state.tap.is_some() {
        return;
    }
    let held = KeyMods::held(keys);
    let Some((binding, chord)) = map
        .taps(scopes)
        .find(|(_, c)| keys.just_pressed(c.key) && c.matches(held, map.platform))
    else {
        return;
    };
    if keys.just_released(chord.key) {
        if !mouse_down {
            fired.extend(binding.action);
        }
        return;
    }
    state.tap = Some(Tap {
        key: chord.key,
        action: binding.action,
        since: now,
        dragged: mouse_down,
    });
}

/// Held fly keys → one [`ViewOp::Fly`] per frame, for the frame's duration
/// (at most [`MAX_FLY_SECS`]).
#[allow(clippy::too_many_arguments)]
pub fn fly_input(
    windows: Query<&Window>,
    views: Views,
    keys: Res<ButtonInput<KeyCode>>,
    map: Res<InputMap>,
    scopes: Res<ActiveScopes>,
    routing: Res<ViewRouting>,
    time: Res<Time<Real>>,
    mut out: MessageWriter<ViewCommand>,
) {
    let scopes = routing.key_scopes(&scopes.0);
    let dir: Vec3 = map
        .held(&keys, &scopes)
        .filter_map(|b| match b.action {
            Some(Action::Fly(d)) => Some(d.vector()),
            _ => None,
        })
        .sum();
    let (Some(dir), Some(t)) = (dir.try_normalize(), target(&windows, &views)) else {
        return;
    };
    out.write(ViewCommand {
        view: t.view,
        op: ViewOp::Fly {
            dir,
            secs: time.delta_secs().min(MAX_FLY_SECS),
        },
    });
}
