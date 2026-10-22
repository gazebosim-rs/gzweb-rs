//! The Translate and Rotate tools' gizmo (spec §6.1): handles on the
//! selected top-level models and lights, in Gazebo's world axes (X, Y, Z in
//! `axis_x`, `axis_y`, `axis_z`) and a constant size on screen. Translate
//! has an arrow per axis and a square per plane, Rotate a ring per axis.
//! A press on a handle drags it (the views' own drag doesn't start); Shift
//! snaps to [`Snap`], X, Y and Z switch the drag to that axis, Esc puts
//! everything back. The edit goes out as [`SetPose`]s while dragging (at
//! most [`SEND_SECS`] apart) and on release, and the sim's poses for the
//! entities are [`Held`] meanwhile.

use bevy::prelude::*;

use super::controls::{target, Views};
use super::{viewport_ray, Selection};
use crate::connection::GzStatus;
use crate::edit::{edits_allowed, movable, HasPlane, Held, SetPose};
use crate::spawn::{GzEntity, GzWorldRoot};

/// The arrows' length on screen (logical pixels).
pub const GIZMO_PX: f32 = 90.0;
/// How close the pointer must be to a handle to grab it.
pub const GRAB_PX: f32 = 8.0;
/// The plane squares, as a share of the arrows' length.
const PLANE_FROM: f32 = 0.25;
const PLANE_TO: f32 = 0.45;
/// The rings' radius, as a share of the arrows' length.
pub const RING: f32 = 0.8;
const RING_SEGMENTS: usize = 64;
/// At most this often an edit goes out while dragging.
pub const SEND_SECS: f64 = 0.1;

/// The active transform tool (the full UI sets it from its tool).
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TransformTool {
    #[default]
    None,
    Translate,
    Rotate,
}

/// What Shift snaps to: a length (m) and an angle (rad). The Snap popover
/// (§7.6) will set them.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct Snap {
    pub length: f32,
    pub angle: f32,
}

impl Default for Snap {
    fn default() -> Self {
        Self {
            length: 0.25,
            angle: 15f32.to_radians(),
        }
    }
}

/// The handles' colours: Gazebo X, Y, Z, and a hovered or dragged handle.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct GizmoStyle {
    pub axes: [Color; 3],
    pub hover: Color,
}

impl Default for GizmoStyle {
    fn default() -> Self {
        Self {
            axes: [
                Color::srgb(0.9, 0.25, 0.25),
                Color::srgb(0.3, 0.75, 0.3),
                Color::srgb(0.25, 0.45, 0.95),
            ],
            hover: Color::WHITE,
        }
    }
}

/// A handle: an axis arrow, the plane square normal to an axis, or the ring
/// about an axis (Gazebo X, Y, Z: 0, 1, 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handle {
    Axis(usize),
    Plane(usize),
    Ring(usize),
}

/// The gizmo's state: the hovered handle, the drag, and the snapping
/// readout (view logical pixels and text) for the UI.
#[derive(Resource, Debug, Clone, Default)]
pub struct GizmoState {
    pub hovered: Option<Handle>,
    pub drag: Option<GizmoDrag>,
    pub readout: Option<(Vec2, String)>,
    /// This frame's Esc cancelled a drag.
    pub escaped: bool,
}

impl GizmoState {
    /// A gizmo drag owns the pointer: the views' controls leave it alone.
    pub fn captures(&self) -> bool {
        self.drag.is_some()
    }

    /// Whether Esc is the gizmo's this frame (a drag to cancel, or one it
    /// just cancelled), whichever side of [`gizmo_input`] the reader runs:
    /// one step per Esc (spec §13).
    pub fn takes_escape(&self) -> bool {
        self.captures() || self.escaped
    }
}

#[derive(Debug, Clone)]
pub struct GizmoDrag {
    pub handle: Handle,
    /// Where the press went down.
    press: Vec2,
    /// The gizmo's centre and Gazebo's axes in the world when it began.
    center: Vec3,
    axes: [Vec3; 3],
    /// The entities and their poses (in their parents' frames) when it
    /// began.
    targets: Vec<(Entity, Transform)>,
    /// The poses last sent, and when.
    sent: Vec<Transform>,
    sent_at: f64,
}

#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct TransformGizmos;

pub struct GizmoPlugin;

impl Plugin for GizmoPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TransformTool>()
            .init_resource::<Snap>()
            .init_resource::<GizmoStyle>()
            .init_resource::<GizmoState>()
            .init_gizmo_group::<TransformGizmos>()
            .add_systems(Startup, setup_gizmos)
            .add_systems(
                Update,
                (gizmo_input, draw_gizmo)
                    .chain()
                    .before(super::controls::mouse_input),
            );
    }
}

/// Drawn over the scene, thick enough to grab.
fn setup_gizmos(mut store: ResMut<GizmoConfigStore>) {
    let (config, _) = store.config_mut::<TransformGizmos>();
    config.depth_bias = -1.0;
    config.line.width = 3.0;
}

/// The selection's entities the tools move ([`movable`]).
fn selected_movable(
    selection: &Selection,
    nodes: &Query<(&GzEntity, &ChildOf, &Transform, &GlobalTransform)>,
    planes: &Query<(), With<HasPlane>>,
    roots: &Query<(Entity, &GlobalTransform), With<GzWorldRoot>>,
) -> Vec<Entity> {
    let Some((root, _)) = roots.iter().next() else {
        return Vec::new();
    };
    selection
        .entities
        .iter()
        .copied()
        .filter(|e| {
            nodes
                .get(*e)
                .is_ok_and(|(g, parent, ..)| movable(g, parent, root, planes.contains(*e)))
        })
        .collect()
}

/// Where the gizmo sits on a movable (top-level) entity: its transform
/// under the world root, this frame's, not the propagated one a frame
/// behind a drag.
fn center_of(
    e: Entity,
    nodes: &Query<(&GzEntity, &ChildOf, &Transform, &GlobalTransform)>,
    roots: &Query<(Entity, &GlobalTransform), With<GzWorldRoot>>,
) -> Option<Vec3> {
    let (_, root) = roots.iter().next()?;
    let (_, _, t, _) = nodes.get(e).ok()?;
    Some(root.transform_point(t.translation))
}

/// The screen point of `p` (view logical pixels), if in front.
fn screen(t: &Transform, p: &Projection, size: Vec2, point: Vec3) -> Option<Vec2> {
    let mut p = p.clone();
    p.update(size.x, size.y);
    let clip = p.get_clip_from_view() * t.to_matrix().inverse() * point.extend(1.0);
    (clip.w > f32::EPSILON).then(|| {
        let ndc = clip.truncate() / clip.w;
        Vec2::new((ndc.x + 1.0) / 2.0 * size.x, (1.0 - ndc.y) / 2.0 * size.y)
    })
}

/// World metres per logical pixel at `point`.
pub(super) fn metres_per_px(t: &Transform, p: &Projection, size: Vec2, point: Vec3) -> f32 {
    let h = size.y.max(1.0);
    match p {
        Projection::Orthographic(_) => super::ortho_height(p).unwrap_or(1.0) / h,
        Projection::Perspective(pp) => {
            let d = (point - t.translation).dot(*t.forward()).max(0.01);
            2.0 * d * (pp.fov / 2.0).tan() / h
        }
        Projection::Custom(_) => 0.01,
    }
}

/// The distance from `q` to the segment `a`–`b`.
fn to_segment(q: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let k = ((q - a).dot(ab) / ab.length_squared().max(f32::EPSILON)).clamp(0.0, 1.0);
    q.distance(a + ab * k)
}

/// A polygon's area on screen (square pixels).
fn area(poly: &[Vec2]) -> f32 {
    let n = poly.len();
    (0..n)
        .map(|i| poly[i].perp_dot(poly[(i + 1) % n]))
        .sum::<f32>()
        .abs()
        / 2.0
}

/// A plane square seen this edge-on (square pixels) can't be grabbed: it
/// would take clicks meant for the arrow it lies along.
const MIN_PLANE_AREA: f32 = 40.0;

/// Whether `q` is inside the convex polygon `poly`.
fn inside(q: Vec2, poly: &[Vec2]) -> bool {
    let n = poly.len();
    let sides = (0..n).map(|i| (poly[(i + 1) % n] - poly[i]).perp_dot(q - poly[i]));
    let (mut pos, mut neg) = (false, false);
    for s in sides {
        pos |= s > 0.0;
        neg |= s < 0.0;
    }
    !(pos && neg)
}

/// The handle's world points: an arrow's ends, a square's corners, a ring.
fn handle_points(handle: Handle, center: Vec3, axes: &[Vec3; 3], len: f32) -> Vec<Vec3> {
    match handle {
        Handle::Axis(i) => vec![center, center + axes[i] * len],
        Handle::Plane(i) => {
            let (j, k) = ((i + 1) % 3, (i + 2) % 3);
            let (a, b) = (PLANE_FROM * len, PLANE_TO * len);
            vec![
                center + axes[j] * a + axes[k] * a,
                center + axes[j] * b + axes[k] * a,
                center + axes[j] * b + axes[k] * b,
                center + axes[j] * a + axes[k] * b,
            ]
        }
        Handle::Ring(i) => {
            let (j, k) = ((i + 1) % 3, (i + 2) % 3);
            (0..=RING_SEGMENTS)
                .map(|s| {
                    let a = s as f32 / RING_SEGMENTS as f32 * std::f32::consts::TAU;
                    center + (axes[j] * a.cos() + axes[k] * a.sin()) * RING * len
                })
                .collect()
        }
    }
}

/// The tool's handles.
fn handles(tool: TransformTool) -> Vec<Handle> {
    match tool {
        TransformTool::None => Vec::new(),
        TransformTool::Translate => (0..3)
            .map(Handle::Plane)
            .chain((0..3).map(Handle::Axis))
            .collect(),
        TransformTool::Rotate => (0..3).map(Handle::Ring).collect(),
    }
}

/// The handle under `cursor`: a plane square it is inside, else the
/// nearest arrow or ring within [`GRAB_PX`]; between handles as near (to
/// the pixel), the one that shows more of itself (a ring facing the
/// camera over one seen edge-on).
fn pick(
    tool: TransformTool,
    cursor: Vec2,
    view: (&Transform, &Projection, Vec2),
    center: Vec3,
    axes: &[Vec3; 3],
) -> Option<Handle> {
    let (t, p, size) = view;
    let len = GIZMO_PX * metres_per_px(t, p, size, center);
    let mut best: Option<((f32, f32), Handle)> = None;
    for h in handles(tool) {
        let pts: Option<Vec<Vec2>> = handle_points(h, center, axes, len)
            .into_iter()
            .map(|w| screen(t, p, size, w))
            .collect();
        let Some(pts) = pts else { continue };
        let shown = if matches!(h, Handle::Axis(_)) {
            0.0
        } else {
            area(&pts)
        };
        let d = match h {
            Handle::Plane(_) if shown >= MIN_PLANE_AREA && inside(cursor, &pts) => 0.0,
            Handle::Plane(_) => continue,
            _ => pts
                .windows(2)
                .map(|w| to_segment(cursor, w[0], w[1]))
                .fold(f32::INFINITY, f32::min),
        };
        let key = (d.round(), -shown);
        if d <= GRAB_PX && best.is_none_or(|(b, _)| key < b) {
            best = Some((key, h));
        }
    }
    best.map(|(_, h)| h)
}

/// Where the ray meets the plane through `center` with normal `n`.
fn on_plane(ray: Ray3d, center: Vec3, n: Vec3) -> Option<Vec3> {
    let denom = ray.direction.dot(n);
    if denom.abs() < 1e-4 {
        return None;
    }
    let s = (center - ray.origin).dot(n) / denom;
    (s > 0.0).then(|| ray.origin + *ray.direction * s)
}

/// The parameter along the line `center + dir·s` closest to the ray.
fn along(ray: Ray3d, center: Vec3, dir: Vec3) -> Option<f32> {
    let d = *ray.direction;
    let w = center - ray.origin;
    let (a, b, c) = (dir.dot(dir), dir.dot(d), d.dot(d));
    let (dd, e) = (dir.dot(w), d.dot(w));
    let denom = a * c - b * b;
    (denom.abs() > 1e-6).then(|| (b * e - c * dd) / denom)
}

fn snap_to(v: f32, step: f32) -> f32 {
    if step > 0.0 {
        (v / step).round() * step
    } else {
        v
    }
}

/// The drag's move: a translation or a rotation (in Gazebo's frame, about
/// each entity's own origin), and the readout's text when snapping.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Move {
    Translate(Vec3),
    Rotate(Quat),
}

/// What the pointer at `now_ray` makes of the drag that pressed at
/// `press_ray`.
fn drag_move(
    drag: &GizmoDrag,
    press_ray: Ray3d,
    now_ray: Ray3d,
    root: Quat,
    snap: Option<Snap>,
) -> Option<(Move, String)> {
    let (c, axes) = (drag.center, drag.axes);
    match drag.handle {
        Handle::Axis(i) => {
            let s0 = along(press_ray, c, axes[i])?;
            let s = along(now_ray, c, axes[i])?;
            let ds = snap.map_or(s - s0, |sn| snap_to(s - s0, sn.length));
            let gz = root.inverse() * (axes[i] * ds);
            Some((Move::Translate(gz), format!("{ds:.2} m")))
        }
        Handle::Plane(i) => {
            let p0 = on_plane(press_ray, c, axes[i])?;
            let p = on_plane(now_ray, c, axes[i])?;
            let mut gz = root.inverse() * (p - p0);
            if let Some(sn) = snap {
                gz = Vec3::new(
                    snap_to(gz.x, sn.length),
                    snap_to(gz.y, sn.length),
                    snap_to(gz.z, sn.length),
                );
                gz[i] = 0.0;
            }
            let (j, k) = ((i + 1) % 3, (i + 2) % 3);
            Some((Move::Translate(gz), format!("{:.2}, {:.2} m", gz[j], gz[k])))
        }
        Handle::Ring(i) => {
            let v0 = on_plane(press_ray, c, axes[i])? - c;
            let v = on_plane(now_ray, c, axes[i])? - c;
            let mut angle = v0.cross(v).dot(axes[i]).atan2(v0.dot(v));
            if let Some(sn) = snap {
                angle = snap_to(angle, sn.angle);
            }
            let mut unit = Vec3::ZERO;
            unit[i] = 1.0;
            let q = Quat::from_axis_angle(unit, angle);
            Some((Move::Rotate(q), format!("{:.0}°", angle.to_degrees())))
        }
    }
}

fn apply_move(m: Move, start: &Transform) -> Transform {
    match m {
        Move::Translate(d) => Transform {
            translation: start.translation + d,
            ..*start
        },
        Move::Rotate(q) => Transform {
            rotation: (q * start.rotation).normalize(),
            ..*start
        },
    }
}

/// A node's identity, parent and placement.
type Node = (
    &'static GzEntity,
    &'static ChildOf,
    &'static Transform,
    &'static GlobalTransform,
);

/// The movable nodes: read to pick and drag, then written to apply a move.
type GizmoNodes<'w, 's> = ParamSet<
    'w,
    's,
    (
        Query<'static, 'static, Node>,
        Query<'static, 'static, &'static mut Transform, With<GzEntity>>,
    ),
>;

/// Hover, press, drag, constrain, cancel and release.
#[allow(clippy::too_many_arguments)]
pub fn gizmo_input(
    tool: Res<TransformTool>,
    status: Option<Res<GzStatus>>,
    selection: Res<Selection>,
    snap: Res<Snap>,
    windows: Query<&Window>,
    views: Views,
    cameras: Query<(&Transform, &Projection), Without<GzEntity>>,
    routing: Res<super::controls::ViewRouting>,
    (buttons, keys): (Res<ButtonInput<MouseButton>>, Res<ButtonInput<KeyCode>>),
    time: Res<Time<Real>>,
    roots: Query<(Entity, &GlobalTransform), With<GzWorldRoot>>,
    planes: Query<(), With<HasPlane>>,
    mut nodes: GizmoNodes,
    mut state: ResMut<GizmoState>,
    mut held: ResMut<Held>,
    mut edits: MessageWriter<SetPose>,
) {
    let now = time.elapsed_secs_f64();
    if state.escaped {
        state.escaped = false;
    }
    let active = *tool != TransformTool::None && edits_allowed(status.as_deref());
    let Some(target) = target(&windows, &views).filter(|_| active) else {
        if let Some(drag) = take_drag(&mut state) {
            finish(&drag, &mut nodes.p1(), &mut held, &mut edits, now, true);
        }
        if state.hovered.is_some() || state.readout.is_some() {
            state.hovered = None;
            state.readout = None;
        }
        return;
    };
    let Ok((ct, cp)) = cameras.get(target.view) else {
        return;
    };
    let (size, cursor) = (target.size, target.cursor);
    let root = roots
        .iter()
        .next()
        .map_or(Quat::IDENTITY, |(_, g)| g.rotation());
    let axes = [root * Vec3::X, root * Vec3::Y, root * Vec3::Z];

    if let Some(mut drag) = take_drag(&mut state) {
        // Esc: back where it was.
        if keys.just_pressed(KeyCode::Escape) {
            finish(&drag, &mut nodes.p1(), &mut held, &mut edits, now, true);
            state.readout = None;
            state.escaped = true;
            return;
        }
        // Plain X, Y, Z (not ⌘/Ctrl+Z, undo).
        let chord = keys.any_pressed([
            KeyCode::ControlLeft,
            KeyCode::ControlRight,
            KeyCode::SuperLeft,
            KeyCode::SuperRight,
            KeyCode::AltLeft,
            KeyCode::AltRight,
        ]);
        for (k, i) in [(KeyCode::KeyX, 0), (KeyCode::KeyY, 1), (KeyCode::KeyZ, 2)] {
            if !chord && keys.just_pressed(k) {
                drag.handle = match drag.handle {
                    Handle::Ring(_) => Handle::Ring(i),
                    _ => Handle::Axis(i),
                };
            }
        }
        let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
        let snapping = shift.then_some(*snap);
        let rays = (
            viewport_ray(cp, ct, size, drag.press),
            cursor.and_then(|c| viewport_ray(cp, ct, size, c)),
        );
        let moved = match rays {
            (Some(a), Some(b)) => drag_move(&drag, a, b, root, snapping),
            _ => None,
        };
        if let Some((m, text)) = moved {
            let mut q = nodes.p1();
            let poses: Vec<Transform> = drag
                .targets
                .iter()
                .map(|(_, start)| apply_move(m, start))
                .collect();
            for ((e, _), pose) in drag.targets.iter().zip(&poses) {
                if let Ok(mut t) = q.get_mut(*e) {
                    t.set_if_neq(*pose);
                }
            }
            if poses != drag.sent && now - drag.sent_at >= SEND_SECS {
                for ((e, _), pose) in drag.targets.iter().zip(&poses) {
                    edits.write(SetPose {
                        entity: *e,
                        pose: *pose,
                    });
                }
                drag.sent = poses;
                drag.sent_at = now;
            }
            state.readout = match (shift, cursor) {
                (true, Some(c)) => Some((c, text)),
                _ => None,
            };
        }
        if buttons.pressed(MouseButton::Left) {
            state.drag = Some(drag);
        } else {
            // Released: the last pose goes out.
            finish(&drag, &mut nodes.p1(), &mut held, &mut edits, now, false);
            state.readout = None;
        }
        return;
    }

    let targets = selected_movable(&selection, &nodes.p0(), &planes, &roots);
    let center = targets
        .first()
        .and_then(|e| center_of(*e, &nodes.p0(), &roots));
    let hovered = match (center, cursor) {
        (Some(c), Some(at)) if routing.pointer => pick(*tool, at, (ct, cp, size), c, &axes),
        _ => None,
    };
    if state.hovered != hovered {
        state.hovered = hovered;
    }
    let (Some(handle), Some(center), Some(at)) = (hovered, center, cursor) else {
        return;
    };
    if buttons.just_pressed(MouseButton::Left) {
        let starts: Vec<(Entity, Transform)> = targets
            .iter()
            .filter_map(|e| nodes.p0().get(*e).ok().map(|(_, _, t, _)| (*e, *t)))
            .collect();
        for (e, _) in &starts {
            held.hold(*e);
        }
        state.drag = Some(GizmoDrag {
            handle,
            press: at,
            center,
            axes,
            sent: starts.iter().map(|(_, t)| *t).collect(),
            targets: starts,
            sent_at: f64::NEG_INFINITY,
        });
    }
}

/// The drag, if any, taken out; without one the state isn't touched (so
/// it isn't marked changed every frame).
fn take_drag(state: &mut ResMut<GizmoState>) -> Option<GizmoDrag> {
    if state.drag.is_some() {
        state.drag.take()
    } else {
        None
    }
}

/// Ends a drag: sends the entities' last poses (or, cancelled, puts them
/// back), unless the sim already has them (a press released without a
/// move sends nothing, so a moving model isn't snapped back), and lets the
/// sim's poses through a moment later.
fn finish(
    drag: &GizmoDrag,
    transforms: &mut Query<&mut Transform, With<GzEntity>>,
    held: &mut Held,
    edits: &mut MessageWriter<SetPose>,
    now: f64,
    cancel: bool,
) {
    for (i, (e, start)) in drag.targets.iter().enumerate() {
        let Ok(mut t) = transforms.get_mut(*e) else {
            continue;
        };
        if cancel {
            t.set_if_neq(*start);
        }
        // `sent` starts as the press's poses.
        let pose = *t;
        if pose != drag.sent[i] {
            edits.write(SetPose { entity: *e, pose });
        }
        held.release(*e, now);
    }
}

/// The handles, the hovered or dragged one in the hover colour.
#[allow(clippy::too_many_arguments)]
fn draw_gizmo(
    tool: Res<TransformTool>,
    status: Option<Res<GzStatus>>,
    selection: Res<Selection>,
    style: Res<GizmoStyle>,
    state: Res<GizmoState>,
    windows: Query<&Window>,
    views: Views,
    cameras: Query<(&Transform, &Projection), Without<GzEntity>>,
    roots: Query<(Entity, &GlobalTransform), With<GzWorldRoot>>,
    nodes: Query<(&GzEntity, &ChildOf, &Transform, &GlobalTransform)>,
    planes: Query<(), With<HasPlane>>,
    mut gizmos: Gizmos<TransformGizmos>,
) {
    if *tool == TransformTool::None || !edits_allowed(status.as_deref()) {
        return;
    }
    let Some(target) = target(&windows, &views) else {
        return;
    };
    let Ok((ct, cp)) = cameras.get(target.view) else {
        return;
    };
    let root = roots
        .iter()
        .next()
        .map_or(Quat::IDENTITY, |(_, g)| g.rotation());
    let axes = [root * Vec3::X, root * Vec3::Y, root * Vec3::Z];
    let targets = selected_movable(&selection, &nodes, &planes, &roots);
    let Some(center) = targets.first().and_then(|e| center_of(*e, &nodes, &roots)) else {
        return;
    };
    let len = GIZMO_PX * metres_per_px(ct, cp, target.size, center);
    let lit = state.drag.as_ref().map(|d| d.handle).or(state.hovered);
    for h in handles(*tool) {
        let i = match h {
            Handle::Axis(i) | Handle::Plane(i) | Handle::Ring(i) => i,
        };
        let colour = if lit == Some(h) {
            style.hover
        } else {
            style.axes[i]
        };
        let pts = handle_points(h, center, &axes, len);
        match h {
            Handle::Axis(_) => {
                gizmos.arrow(pts[0], pts[1], colour);
            }
            Handle::Plane(_) => {
                let mut square = pts.clone();
                square.push(pts[0]);
                gizmos.linestrip(square, colour.with_alpha(0.8));
            }
            Handle::Ring(_) => {
                gizmos.linestrip(pts, colour);
            }
        }
    }
}

#[cfg(test)]
mod tests;
