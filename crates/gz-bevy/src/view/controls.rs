//! [`ViewerControls`]: the minimal mode's mouse and keyboard, turned into
//! [`ViewCommand`]s and sim commands (`.agent/UI_PLAN.md` §2.3).
//!
//! Mouse (spec §6.2): left drag runs the view's drag action, right drag
//! pans, wheel or middle drag zooms, Ctrl+wheel zooms to the cursor, and
//! Space held with a left drag pans. Keys come from the [`InputMap`] in the
//! scopes the mode enables. In minimal mode there are no UI nodes, so the
//! whole window is the view.

use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;

use super::input::{Action, InputMap, KeyMods, Scope};
use super::{apply_view_commands, DragAction, Selection, ViewCamera, ViewCommand, ViewOp};
use crate::connection::{GzCommand, GzStatus};
use crate::render::GzGrid;
use crate::UiMode;

/// A Space press shorter than this, with no pointer drag, is a tap
/// (play/pause, decision Q4).
pub const TAP_SECS: f64 = 0.2;
/// Middle (or Zoom-action) drags zoom one [`super::ZOOM_STEP`] per this many
/// pixels of vertical movement.
pub const ZOOM_DRAG_PX: f32 = 5.0;
/// Pixel-unit wheel deltas (trackpads, browsers) per wheel step.
pub const WHEEL_PX_PER_STEP: f32 = 50.0;

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
            // TODO(M1): the full UI enables every scope, routes keys by focus
            // and hover, and gives W A S D to an armed Teleop panel. Until
            // then it is the minimal mode.
            UiMode::Minimal | UiMode::Full => vec![Scope::Global, Scope::Viewport, Scope::Minimal],
        }
    }
}

/// The key scopes live right now.
#[derive(Resource, Debug, Clone, Default)]
pub struct ActiveScopes(pub Vec<Scope>);

/// Pointer gesture and Space-tap state.
#[derive(Resource, Debug, Clone, Default)]
pub struct ControlState {
    drag: Option<Drag>,
    tap: Option<Tap>,
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
            .init_resource::<Selection>()
            .init_resource::<InputMap>()
            .init_resource::<ControlState>()
            .init_resource::<GzGrid>()
            .init_resource::<crate::camera::GzCameraTarget>()
            .insert_resource(ActiveScopes(Self::scopes(self.mode)))
            .add_systems(
                Update,
                (mouse_input, key_input, fly_input, apply_view_commands).chain(),
            );
    }
}

/// The window, the view's camera entity and state, and the cursor in its
/// viewport, if all exist.
fn target<'a>(
    windows: &Query<&Window>,
    views: &'a Query<(Entity, &ViewCamera, &Camera)>,
) -> Option<(Entity, &'a ViewCamera, Option<Vec2>, Vec2)> {
    let window = windows.single().ok()?;
    let (entity, view, camera) = views.single().ok()?;
    let origin = camera.logical_viewport_rect().map_or(Vec2::ZERO, |r| r.min);
    let size = camera
        .logical_viewport_size()
        .unwrap_or(Vec2::new(window.width(), window.height()));
    let cursor = window.cursor_position().map(|c| c - origin);
    Some((entity, view, cursor, size))
}

/// Mouse buttons and wheel → [`ViewCommand`]s.
#[allow(clippy::too_many_arguments)]
pub fn mouse_input(
    windows: Query<&Window>,
    views: Query<(Entity, &ViewCamera, &Camera)>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    map: Res<InputMap>,
    scopes: Res<ActiveScopes>,
    mut wheel: MessageReader<MouseWheel>,
    mut state: ResMut<ControlState>,
    mut out: MessageWriter<ViewCommand>,
) {
    let Some((view, camera, cursor, size)) = target(&windows, &views) else {
        wheel.clear();
        state.drag = None;
        return;
    };
    let send = |out: &mut MessageWriter<ViewCommand>, op| {
        out.write(ViewCommand { view, op });
    };

    if state.drag.is_some_and(|d| !buttons.pressed(d.button)) {
        state.drag = None;
    }
    if state.drag.is_none() {
        let pressed = [MouseButton::Left, MouseButton::Right, MouseButton::Middle]
            .into_iter()
            .find(|b| buttons.just_pressed(*b));
        if let (Some(button), Some(at)) = (pressed, cursor) {
            let gesture = match button {
                MouseButton::Left if map.holding(&keys, &scopes.0, Action::PanHold) => {
                    DragAction::Pan
                }
                MouseButton::Left => camera.drag_action,
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

    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    for event in wheel.read() {
        let steps = match event.unit {
            MouseScrollUnit::Line => event.y,
            MouseScrollUnit::Pixel => event.y / WHEEL_PX_PER_STEP,
        };
        let Some(cursor) = cursor else { continue };
        if steps == 0.0 {
            continue;
        }
        // Plain wheel zooms about the view centre; Ctrl+wheel about the
        // point under the cursor (spec §6.2, DECISIONS.md D-11).
        let at = if ctrl { cursor } else { size / 2.0 };
        send(&mut out, ViewOp::ZoomAt { at, steps });
    }
}

/// Viewport and sim keys → [`ViewCommand`]s, the grid and [`GzCommand`]s,
/// including the Space tap.
#[allow(clippy::too_many_arguments)]
pub fn key_input(
    windows: Query<&Window>,
    views: Query<(Entity, &ViewCamera, &Camera)>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    map: Res<InputMap>,
    scopes: Res<ActiveScopes>,
    time: Res<Time<Real>>,
    status: Option<Res<GzStatus>>,
    mut state: ResMut<ControlState>,
    mut grid: ResMut<GzGrid>,
    mut view_out: MessageWriter<ViewCommand>,
    mut sim_out: MessageWriter<GzCommand>,
) {
    let now = time.elapsed_secs_f64();
    let mut fired: Vec<Action> = map
        .just_pressed(&keys, &scopes.0)
        .filter_map(|b| b.action)
        .collect();

    // A tap: down and up within TAP_SECS, no mouse button in between.
    let held = KeyMods::held(&keys);
    if let Some(tap) = state.tap.as_mut() {
        if buttons.get_pressed().next().is_some() {
            tap.dragged = true;
        }
        if keys.just_released(tap.key) {
            if now - tap.since < TAP_SECS && !tap.dragged {
                fired.extend(tap.action);
            }
            state.tap = None;
        }
    }
    if state.tap.is_none() {
        if let Some((binding, chord)) = map
            .taps(&scopes.0)
            .find(|(_, c)| keys.just_pressed(c.key) && c.matches(held, map.platform))
        {
            state.tap = Some(Tap {
                key: chord.key,
                action: binding.action,
                since: now,
                dragged: buttons.get_pressed().next().is_some(),
            });
        }
    }

    let target = target(&windows, &views);
    let sim = status.and_then(|s| s.sim);
    for action in fired {
        let op = match action {
            Action::Focus => ViewOp::Focus {
                at: target.and_then(|t| t.2),
            },
            Action::Home => ViewOp::Home,
            Action::ToggleProjection => ViewOp::ToggleProjection,
            Action::Snap(face) => ViewOp::Snap(face),
            Action::ToggleGrid => {
                grid.visible = !grid.visible;
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
                    sim_out.write(GzCommand::Step(1));
                }
                continue;
            }
            Action::PanHold | Action::Fly(_) => continue,
        };
        if let Some((view, ..)) = target {
            view_out.write(ViewCommand { view, op });
        }
    }
}

/// Held fly keys → one [`ViewOp::Fly`] per frame, for the frame's duration.
pub fn fly_input(
    windows: Query<&Window>,
    views: Query<(Entity, &ViewCamera, &Camera)>,
    keys: Res<ButtonInput<KeyCode>>,
    map: Res<InputMap>,
    scopes: Res<ActiveScopes>,
    time: Res<Time<Real>>,
    mut out: MessageWriter<ViewCommand>,
) {
    let dir: Vec3 = map
        .held(&keys, &scopes.0)
        .filter_map(|b| match b.action {
            Some(Action::Fly(d)) => Some(d.vector()),
            _ => None,
        })
        .sum();
    let (Some(dir), Some((view, ..))) = (dir.try_normalize(), target(&windows, &views)) else {
        return;
    };
    out.write(ViewCommand {
        view,
        op: ViewOp::Fly {
            dir,
            secs: time.delta_secs(),
        },
    });
}
