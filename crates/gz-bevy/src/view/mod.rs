//! Views: the camera controller and key map shared by the minimal and full
//! UI modes (`.agent/UI_PLAN.md` §2.2, spec §6.2).
//!
//! A view is a camera with a [`ViewCamera`] state machine: pivot, plain
//! left-drag action, projection, the snapped face and a running animation.
//! Everything that moves it is a [`ViewCommand`] message, so the minimal
//! mode's raw mouse and keys ([`controls::ViewerControls`]) and, later, the
//! full UI's rail and view cube drive the same code.
//!
//! The camera works in Bevy space (Y up). Gazebo's Z-up world sits under
//! the rotated `GzWorldRoot`; the Gazebo z = 0 plane is Bevy's y = 0.
//!
//! Following gzweb's patched three.js `OrbitControls`, the camera is never
//! re-aimed at its pivot: orbiting rotates the camera about the pivot, which
//! stays where it is on screen.

pub mod controls;
pub mod input;

use bevy::anti_alias::taa::TemporalAntiAliasing;
use bevy::camera::primitives::Aabb;
use bevy::camera::ScalingMode;
use bevy::ecs::system::SystemParam;
use bevy::picking::mesh_picking::ray_cast::{MeshRayCast, MeshRayCastSettings};
use bevy::prelude::*;
use gz_core::scene::GeometryDesc;
use std::f32::consts::{PI, TAU};

use crate::camera::GzCameraTarget;
use crate::spawn::{gz_to_bevy, GzVisual, GzWorldRoot};

pub use controls::ViewerControls;
pub use input::InputMap;

/// How long snaps, focus and home animate (spec §12: cube snaps in 250 ms).
pub const ANIMATION_SECS: f32 = 0.25;
/// gzweb's default camera position (`Scene.ts`), in Gazebo coordinates,
/// looking at the origin.
pub const HOME_GZ: Vec3 = Vec3::new(-6.0, 0.0, 6.0);
/// Zoom factor per wheel step (three.js `dollyIn`, zoom speed 1).
pub const ZOOM_STEP: f32 = 0.95;
/// How far an orthographic camera stays behind its pivot, at least, so the
/// near plane doesn't cut through the scene when zoomed in.
pub const ORTHO_STANDOFF: f32 = 50.0;
/// Space around focused bounds.
const FOCUS_MARGIN: f32 = 1.1;
/// The smallest radius focus frames, so a point still gets a view.
const MIN_FOCUS_RADIUS: f32 = 0.05;
/// Fly speed floor, as a distance to the pivot.
const MIN_FLY_REACH: f32 = 0.5;
const MIN_ORTHO_HEIGHT: f32 = 1e-3;
/// three.js `Spherical.makeSafe` epsilon.
const POLAR_EPS: f32 = 1e-6;

/// What a plain left drag does in a view (the rail's Orbit/Pan/Zoom).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DragAction {
    #[default]
    Orbit,
    Pan,
    Zoom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectionKind {
    Perspective,
    Orthographic,
}

/// The six axis views (keys 7/1/3, Shift for the opposite side).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapView {
    Top,
    Bottom,
    Front,
    Back,
    Right,
    Left,
}

impl SnapView {
    pub const ALL: [SnapView; 6] = [
        SnapView::Top,
        SnapView::Bottom,
        SnapView::Front,
        SnapView::Back,
        SnapView::Right,
        SnapView::Left,
    ];

    /// Look direction and screen up, in Gazebo axes. As in the Gazebo GUI
    /// (gz-sim `ViewAngle` with gz-math `LookAt`): front looks along -X,
    /// right along -Y, and the top view has +X up the screen.
    pub fn gz_axes(self) -> (Vec3, Vec3) {
        match self {
            SnapView::Top => (Vec3::NEG_Z, Vec3::X),
            SnapView::Bottom => (Vec3::Z, Vec3::NEG_X),
            SnapView::Front => (Vec3::NEG_X, Vec3::Z),
            SnapView::Back => (Vec3::X, Vec3::Z),
            SnapView::Right => (Vec3::NEG_Y, Vec3::Z),
            SnapView::Left => (Vec3::Y, Vec3::Z),
        }
    }

    /// The camera rotation for this view, in Bevy space.
    pub fn rotation(self) -> Quat {
        let (dir, up) = self.gz_axes();
        Transform::default()
            .looking_to(gz_to_bevy() * dir, gz_to_bevy() * up)
            .rotation
    }
}

/// Fly-key directions (minimal mode's W A S D / arrows and Q / E).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlyDir {
    Forward,
    Back,
    Left,
    Right,
    Up,
    Down,
}

impl FlyDir {
    /// x right, y up, z forward.
    pub fn vector(self) -> Vec3 {
        match self {
            FlyDir::Forward => Vec3::Z,
            FlyDir::Back => Vec3::NEG_Z,
            FlyDir::Left => Vec3::NEG_X,
            FlyDir::Right => Vec3::X,
            FlyDir::Up => Vec3::Y,
            FlyDir::Down => Vec3::NEG_Y,
        }
    }
}

/// One change to a view. Positions are logical pixels in the view's
/// viewport; deltas are pointer movement in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ViewOp {
    /// A drag starts here: the pivot becomes the point under it (a mesh,
    /// else the ground), falling back to the selection's centre, then the
    /// world origin.
    PivotAt(Vec2),
    /// Rotate about the pivot: 2π per viewport height. Leaves a snapped
    /// view, back to perspective.
    Orbit(Vec2),
    /// Move camera and pivot in the view plane.
    Pan(Vec2),
    /// Dolly towards the pivot (orthographic: shrink the view about it), by
    /// [`ZOOM_STEP`] per step. Positive steps zoom in.
    Zoom(f32),
    /// Zoom about the point under `at`, which becomes the pivot.
    ZoomAt {
        at: Vec2,
        steps: f32,
    },
    SetProjection(ProjectionKind),
    ToggleProjection,
    /// Animate to an axis view, then switch to orthographic.
    Snap(SnapView),
    /// Animate to frame the selection, else the entity under `at`, else the
    /// whole world.
    Focus {
        at: Option<Vec2>,
    },
    /// Animate back to gzweb's default view.
    Home,
    SetDragAction(DragAction),
    /// Move camera and pivot: `dir` is x right and z forward in the ground
    /// plane, y world up; `secs` of travel at the view's fly speed.
    Fly {
        dir: Vec3,
        secs: f32,
    },
}

/// A [`ViewOp`] for the view on camera entity `view`.
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct ViewCommand {
    pub view: Entity,
    pub op: ViewOp,
}

/// The selected entities. Nothing selects yet; the full UI will (M5).
/// Views use it for the pivot fallback, focus and snaps.
#[derive(Resource, Debug, Clone, Default)]
pub struct Selection {
    pub entities: Vec<Entity>,
}

/// A view's camera state. Lives on the camera entity next to its
/// `Transform` and `Projection`.
#[derive(Component, Debug, Clone)]
pub struct ViewCamera {
    /// Orbit and zoom centre, in Bevy world coordinates.
    pub pivot: Vec3,
    pub drag_action: DragAction,
    /// The axis view the camera sits in, until it orbits away.
    pub snapped: Option<SnapView>,
    /// What perspective mode looks like; orthographic framing is matched to
    /// its field of view.
    pub perspective: PerspectiveProjection,
    pub rotate_speed: f32,
    pub zoom_speed: f32,
    pub pan_speed: f32,
    /// Fly keys travel this many pivot distances per second.
    pub fly_speed: f32,
    animation: Option<Animation>,
    /// The projection last frame, to spot a switch.
    was_ortho: bool,
}

impl ViewCamera {
    pub fn new(pivot: Vec3, perspective: PerspectiveProjection) -> Self {
        Self {
            pivot,
            drag_action: DragAction::Orbit,
            snapped: None,
            perspective,
            rotate_speed: 1.0,
            zoom_speed: 1.0,
            pan_speed: 1.0,
            fly_speed: 0.5,
            animation: None,
            was_ortho: false,
        }
    }

    pub fn animating(&self) -> bool {
        self.animation.is_some()
    }

    fn half_tan(&self) -> f32 {
        (self.perspective.fov / 2.0).tan()
    }
}

/// gzweb's default camera: [`HOME_GZ`] looking at the origin.
pub fn home_transform() -> Transform {
    Transform::from_translation(gz_to_bevy() * HOME_GZ).looking_at(Vec3::ZERO, Vec3::Y)
}

/// The orthographic view height, if the projection is orthographic.
pub fn ortho_height(projection: &Projection) -> Option<f32> {
    match projection {
        Projection::Orthographic(o) => Some(match o.scaling_mode {
            ScalingMode::FixedVertical { viewport_height } => viewport_height * o.scale,
            _ => o.area.height(),
        }),
        _ => None,
    }
}

fn orthographic(view: &ViewCamera, height: f32) -> Projection {
    Projection::Orthographic(OrthographicProjection {
        near: 0.0,
        far: view.perspective.far,
        scaling_mode: ScalingMode::FixedVertical {
            viewport_height: height.max(MIN_ORTHO_HEIGHT),
        },
        ..OrthographicProjection::default_3d()
    })
}

/// The pivot in the camera's own frame.
fn offset(view: &ViewCamera, t: &Transform) -> Vec3 {
    t.rotation.inverse() * (view.pivot - t.translation)
}

/// Switches to orthographic, keeping the pivot where it is on screen and
/// the scale in the plane through it. The camera backs off to
/// [`ORTHO_STANDOFF`] if it was closer.
fn to_ortho(view: &ViewCamera, t: &mut Transform, p: &mut Projection) {
    if ortho_height(p).is_some() {
        return;
    }
    let mut off = offset(view, t);
    let depth = (-off.z).max(MIN_ORTHO_HEIGHT);
    *p = orthographic(view, 2.0 * depth * view.half_tan());
    off.z = -depth.max(ORTHO_STANDOFF);
    t.translation = view.pivot - t.rotation * off;
}

/// Switches to perspective, keeping the pivot on screen and the scale in
/// the plane through it: the camera moves to the matching depth.
fn to_perspective(view: &ViewCamera, t: &mut Transform, p: &mut Projection) {
    let Some(height) = ortho_height(p) else {
        return;
    };
    let mut off = offset(view, t);
    off.z = -height / (2.0 * view.half_tan());
    t.translation = view.pivot - t.rotation * off;
    *p = Projection::Perspective(view.perspective.clone());
}

/// How far the view reaches: the distance to the pivot, or for an
/// orthographic view the perspective distance that frames it the same way.
pub fn reach(view: &ViewCamera, t: &Transform, p: &Projection) -> f32 {
    match ortho_height(p) {
        Some(h) => h / (2.0 * view.half_tan()),
        None => (t.translation - view.pivot).length(),
    }
}

/// The world ray through viewport position `at`, from the projection itself
/// (not `Camera`'s cached matrices, which lag a frame behind changes).
pub fn viewport_ray(projection: &Projection, t: &Transform, size: Vec2, at: Vec2) -> Option<Ray3d> {
    if size.x <= 0.0 || size.y <= 0.0 {
        return None;
    }
    let mut p = projection.clone();
    p.update(size.x, size.y);
    let ndc = Vec2::new(at.x / size.x * 2.0 - 1.0, 1.0 - at.y / size.y * 2.0);
    let view_from_clip = p.get_clip_from_view().inverse();
    // Reversed Z: near is 1; the far plane may be at infinity (0).
    let near = view_from_clip.project_point3(ndc.extend(1.0));
    let far = view_from_clip.project_point3(ndc.extend(f32::EPSILON));
    let dir = Dir3::new(t.rotation * (far - near)).ok()?;
    Some(Ray3d::new(t.transform_point(near), dir))
}

/// A camera pose that animates well: the pivot, the rotation, and the
/// pivot's position in the camera's frame, plus the orthographic height.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Pose {
    pivot: Vec3,
    rotation: Quat,
    offset: Vec3,
    ortho_height: f32,
}

impl Pose {
    fn of(view: &ViewCamera, t: &Transform, p: &Projection) -> Pose {
        Pose {
            pivot: view.pivot,
            rotation: t.rotation,
            offset: offset(view, t),
            ortho_height: ortho_height(p).unwrap_or(0.0),
        }
    }

    fn lerp(&self, to: &Pose, s: f32) -> Pose {
        Pose {
            pivot: self.pivot.lerp(to.pivot, s),
            rotation: self.rotation.slerp(to.rotation, s),
            offset: self.offset.lerp(to.offset, s),
            ortho_height: self.ortho_height + (to.ortho_height - self.ortho_height) * s,
        }
    }

    fn apply(&self, view: &mut ViewCamera, t: &mut Transform, p: &mut Projection) {
        view.pivot = self.pivot;
        t.rotation = self.rotation;
        t.translation = self.pivot - self.rotation * self.offset;
        if ortho_height(p).is_some() {
            *p = orthographic(view, self.ortho_height);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Animation {
    from: Pose,
    to: Pose,
    elapsed: f32,
    /// Switch to orthographic and mark the view snapped at the end.
    snap: Option<SnapView>,
}

/// Bounds in Bevy world space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub min: Vec3,
    pub max: Vec3,
}

impl Bounds {
    fn point(p: Vec3) -> Bounds {
        Bounds { min: p, max: p }
    }

    fn union(self, other: Bounds) -> Bounds {
        Bounds {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }

    pub fn center(&self) -> Vec3 {
        (self.min + self.max) / 2.0
    }

    pub fn radius(&self) -> f32 {
        (self.max - self.min).length() / 2.0
    }
}

/// A scene node's bounds, placement, children and visual, for framing.
type NodeData = (
    Option<&'static Aabb>,
    &'static GlobalTransform,
    Option<&'static Children>,
    Option<&'static GzVisual>,
);

/// What views look at: ray casts against the scene, the selection and
/// entity bounds.
#[derive(SystemParam)]
pub struct ViewScene<'w, 's> {
    ray_cast: MeshRayCast<'w, 's>,
    selection: Res<'w, Selection>,
    nodes: Query<'w, 's, NodeData>,
    parents: Query<'w, 's, &'static ChildOf>,
    roots: Query<'w, 's, Entity, With<GzWorldRoot>>,
}

impl ViewScene<'_, '_> {
    /// The nearest visible mesh on the ray.
    fn hit(&mut self, ray: Ray3d) -> Option<(Entity, Vec3)> {
        self.ray_cast
            .cast_ray(ray, &MeshRayCastSettings::default())
            .first()
            .map(|(e, hit)| (*e, hit.point))
    }

    /// The nearest mesh on the ray, else where it meets the ground.
    fn point(&mut self, ray: Ray3d) -> Option<Vec3> {
        if let Some((_, p)) = self.hit(ray) {
            return Some(p);
        }
        let t = -ray.origin.y / ray.direction.y;
        (t.is_finite() && t > 0.0).then(|| ray.origin + ray.direction * t)
    }

    /// Bounds of every mesh in `entity`'s subtree. With `skip_planes`,
    /// plane visuals are left out (a ground plane would swamp the world).
    fn bounds_of(&self, entity: Entity, skip_planes: bool) -> Option<Bounds> {
        let (aabb, global, children, visual) = self.nodes.get(entity).ok()?;
        if skip_planes && visual.is_some_and(|v| matches!(v.0.geometry, GeometryDesc::Plane { .. }))
        {
            return None;
        }
        let mut out = aabb.map(|aabb| {
            let (c, h) = (Vec3::from(aabb.center), Vec3::from(aabb.half_extents));
            (0..8)
                .map(|i| {
                    let sign = Vec3::new(
                        if i & 1 == 0 { -1.0 } else { 1.0 },
                        if i & 2 == 0 { -1.0 } else { 1.0 },
                        if i & 4 == 0 { -1.0 } else { 1.0 },
                    );
                    Bounds::point(global.transform_point(c + h * sign))
                })
                .reduce(Bounds::union)
                .unwrap_or(Bounds::point(global.translation()))
        });
        for child in children.into_iter().flatten() {
            if let Some(b) = self.bounds_of(*child, skip_planes) {
                out = Some(out.map_or(b, |o| o.union(b)));
            }
        }
        out
    }

    fn selection_bounds(&self) -> Option<Bounds> {
        self.selection
            .entities
            .iter()
            .filter_map(|e| self.bounds_of(*e, false))
            .reduce(Bounds::union)
    }

    /// Every mesh under the world root, ground planes left out unless
    /// there is nothing else.
    fn world_bounds(&self) -> Option<Bounds> {
        let root = self.roots.iter().next()?;
        self.bounds_of(root, true)
            .or_else(|| self.bounds_of(root, false))
    }

    /// The top-level model (child of the world root) that `entity` is part
    /// of; `entity` itself outside the Gazebo tree.
    fn model_of(&self, entity: Entity) -> Entity {
        let mut e = entity;
        while let Ok(parent) = self.parents.get(e) {
            if self.roots.contains(parent.parent()) {
                return e;
            }
            e = parent.parent();
        }
        entity
    }

    /// The drag pivot under `ray`, with the spec's fallbacks.
    fn pivot(&mut self, ray: Option<Ray3d>) -> Vec3 {
        ray.and_then(|r| self.point(r))
            .or_else(|| self.selection_bounds().map(|b| b.center()))
            .unwrap_or(Vec3::ZERO)
    }
}

/// Applies [`ViewCommand`]s, then advances animations, and publishes the
/// first view's pivot as [`GzCameraTarget`].
pub fn apply_view_commands(
    mut commands: MessageReader<ViewCommand>,
    mut views: Query<(
        &mut ViewCamera,
        &mut Transform,
        &mut Projection,
        &Camera,
        Option<&mut TemporalAntiAliasing>,
    )>,
    mut scene: ViewScene,
    time: Res<Time<Real>>,
    mut target: ResMut<GzCameraTarget>,
) {
    for cmd in commands.read() {
        let Ok((mut view, mut t, mut p, camera, _)) = views.get_mut(cmd.view) else {
            continue;
        };
        let size = camera.logical_viewport_size().unwrap_or(Vec2::ZERO);
        apply(cmd.op, &mut view, &mut t, &mut p, size, &mut scene);
    }
    let dt = time.delta_secs();
    for (mut view, mut t, mut p, _, taa) in &mut views {
        animate(&mut view, &mut t, &mut p, dt);
        // A projection switch is a camera cut: TAA's history no longer
        // matches the frame.
        let ortho = ortho_height(&p).is_some();
        if ortho != view.was_ortho {
            view.was_ortho = ortho;
            if let Some(mut taa) = taa {
                taa.reset = true;
            }
        }
    }
    if let Some((view, ..)) = views.iter().next() {
        let gz = (gz_to_bevy().inverse() * view.pivot).to_array();
        if target.0 != gz {
            target.0 = gz;
        }
    }
}

fn apply(
    op: ViewOp,
    view: &mut ViewCamera,
    t: &mut Transform,
    p: &mut Projection,
    size: Vec2,
    scene: &mut ViewScene,
) {
    let ray = |p: &Projection, t: &Transform, at: Vec2| viewport_ray(p, t, size, at);
    let centre = size / 2.0;
    match op {
        ViewOp::Snap(face) => {
            let pivot = scene
                .selection_bounds()
                .map(|b| b.center())
                .or_else(|| ray(p, t, centre).and_then(|r| scene.point(r)))
                .unwrap_or(view.pivot);
            snap(view, t, p, face, pivot);
            return;
        }
        ViewOp::Focus { at } => {
            let bounds = scene.selection_bounds().or_else(|| {
                let hit = at.and_then(|at| ray(p, t, at)).and_then(|r| scene.hit(r));
                hit.and_then(|(e, _)| scene.bounds_of(scene.model_of(e), false))
            });
            match bounds.or_else(|| scene.world_bounds()) {
                Some(b) => focus(view, t, p, b, size),
                None => home(view, t, p),
            }
            return;
        }
        ViewOp::Home => {
            home(view, t, p);
            return;
        }
        ViewOp::SetDragAction(action) => {
            view.drag_action = action;
            return;
        }
        _ => {}
    }
    // Anything else stops an animation where it is.
    view.animation = None;
    match op {
        ViewOp::PivotAt(at) => view.pivot = scene.pivot(ray(p, t, at)),
        ViewOp::Orbit(delta) => {
            if view.snapped.take().is_some() {
                to_perspective(view, t, p);
            }
            orbit(view, t, delta, size.y.max(1.0));
        }
        ViewOp::Pan(delta) => pan(view, t, p, delta, size.y.max(1.0)),
        ViewOp::Zoom(steps) => zoom(view, t, p, steps),
        ViewOp::ZoomAt { at, steps } => {
            view.pivot = scene.pivot(ray(p, t, at));
            zoom(view, t, p, steps);
        }
        ViewOp::SetProjection(ProjectionKind::Orthographic) => to_ortho(view, t, p),
        ViewOp::SetProjection(ProjectionKind::Perspective) => to_perspective(view, t, p),
        ViewOp::ToggleProjection => {
            if ortho_height(p).is_some() {
                to_perspective(view, t, p);
            } else {
                to_ortho(view, t, p);
            }
        }
        ViewOp::Fly { dir, secs } => fly(view, t, p, dir, secs),
        ViewOp::Snap(_) | ViewOp::Focus { .. } | ViewOp::Home | ViewOp::SetDragAction(_) => {}
    }
}

/// gzweb's orbit: yaw about world up, then pitch about the camera's right
/// axis, keeping the offset's polar angle in (0, π).
fn orbit(view: &ViewCamera, t: &mut Transform, delta: Vec2, height: f32) {
    let d = delta * view.rotate_speed;
    let d_theta = -TAU * d.x / height;
    let d_phi = -TAU * d.y / height;
    let pivot = view.pivot;
    let offset = t.translation - pivot;
    let phi = (offset.y / offset.length().max(f32::EPSILON))
        .clamp(-1.0, 1.0)
        .acos();
    let new_phi = (phi + d_phi).clamp(POLAR_EPS, PI - POLAR_EPS);
    t.rotate_around(pivot, Quat::from_axis_angle(Vec3::Y, d_theta));
    let right = t.rotation * Vec3::X;
    t.rotate_around(pivot, Quat::from_axis_angle(right, new_phi - phi));
}

/// Screen-space pan: the plane through the pivot follows the pointer
/// (perspective uses the distance to the pivot, as gzweb does).
fn pan(view: &mut ViewCamera, t: &mut Transform, p: &Projection, delta: Vec2, height: f32) {
    let per_px = match ortho_height(p) {
        Some(h) => h / height,
        None => 2.0 * (t.translation - view.pivot).length() * view.half_tan() / height,
    };
    let d = delta * view.pan_speed * per_px;
    let pan = -(t.rotation * Vec3::X) * d.x + (t.rotation * Vec3::Y) * d.y;
    t.translation += pan;
    view.pivot += pan;
}

/// Dolly about the pivot; an orthographic view shrinks about it instead,
/// keeping the pivot fixed on screen.
fn zoom(view: &ViewCamera, t: &mut Transform, p: &mut Projection, steps: f32) {
    let s = ZOOM_STEP.powf(steps * view.zoom_speed);
    match ortho_height(p) {
        Some(h) => {
            *p = orthographic(view, h * s);
            let mut off = offset(view, t);
            off.x *= s;
            off.y *= s;
            t.translation = view.pivot - t.rotation * off;
        }
        None => t.translation = view.pivot + (t.translation - view.pivot) * s,
    }
}

/// Moves camera and pivot together in the ground plane (W/S along the view
/// direction flattened, A/D sideways) or straight up and down, at
/// `fly_speed` pivot distances per second.
fn fly(view: &mut ViewCamera, t: &mut Transform, p: &Projection, dir: Vec3, secs: f32) {
    let step = view.fly_speed * reach(view, t, p).max(MIN_FLY_REACH) * secs;
    let flat = |v: Vec3| Vec3::new(v.x, 0.0, v.z).try_normalize();
    // Looking straight down or up, "forward" is up the screen.
    let forward = flat(t.rotation * Vec3::NEG_Z)
        .or_else(|| flat(t.rotation * Vec3::Y))
        .unwrap_or(Vec3::NEG_Z);
    let right = forward.cross(Vec3::Y);
    let delta = (right * dir.x + Vec3::Y * dir.y + forward * dir.z) * step;
    t.translation += delta;
    view.pivot += delta;
}

fn start(view: &mut ViewCamera, from: Pose, to: Pose, snap: Option<SnapView>) {
    view.animation = Some(Animation {
        from,
        to,
        elapsed: 0.0,
        snap,
    });
}

/// Animates to `face` about `pivot`, keeping the distance; orthographic at
/// the end, with the same framing.
fn snap(view: &mut ViewCamera, t: &Transform, p: &Projection, face: SnapView, pivot: Vec3) {
    let from = Pose::of(view, t, p);
    let ortho = ortho_height(p).is_some();
    let distance = if ortho {
        reach(view, t, p)
    } else {
        (t.translation - pivot).length()
    };
    let to = Pose {
        pivot,
        rotation: face.rotation(),
        offset: Vec3::new(
            0.0,
            0.0,
            -if ortho {
                distance.max(ORTHO_STANDOFF)
            } else {
                distance
            },
        ),
        ortho_height: from.ortho_height,
    };
    start(view, from, to, Some(face));
}

/// Animates to frame `bounds` from the current direction.
fn focus(view: &mut ViewCamera, t: &Transform, p: &Projection, bounds: Bounds, size: Vec2) {
    let from = Pose::of(view, t, p);
    let r = bounds.radius().max(MIN_FOCUS_RADIUS) * FOCUS_MARGIN;
    let aspect = if size.y > 0.0 { size.x / size.y } else { 1.0 };
    let half = view.half_tan();
    let fit = half.atan().min((half * aspect).atan());
    let ortho = ortho_height(p).is_some();
    let distance = r / fit.sin();
    let to = Pose {
        pivot: bounds.center(),
        rotation: t.rotation,
        offset: Vec3::new(
            0.0,
            0.0,
            -if ortho {
                distance.max(ORTHO_STANDOFF)
            } else {
                distance
            },
        ),
        ortho_height: if ortho {
            2.0 * r / aspect.min(1.0)
        } else {
            from.ortho_height
        },
    };
    start(view, from, to, None);
}

/// Animates back to gzweb's default view, in perspective.
fn home(view: &mut ViewCamera, t: &mut Transform, p: &mut Projection) {
    to_perspective(view, t, p);
    view.snapped = None;
    let from = Pose::of(view, t, p);
    let home = home_transform();
    let to = Pose {
        pivot: Vec3::ZERO,
        rotation: home.rotation,
        offset: home.rotation.inverse() * -home.translation,
        ortho_height: 0.0,
    };
    start(view, from, to, None);
}

fn animate(view: &mut ViewCamera, t: &mut Transform, p: &mut Projection, dt: f32) {
    let Some(anim) = view.animation.as_mut() else {
        return;
    };
    anim.elapsed += dt;
    let k = (anim.elapsed / ANIMATION_SECS).min(1.0);
    let eased = k * k * (3.0 - 2.0 * k);
    let pose = anim.from.lerp(&anim.to, eased);
    let snap = anim.snap;
    pose.apply(view, t, p);
    if k >= 1.0 {
        view.animation = None;
        if let Some(face) = snap {
            to_ortho(view, t, p);
            view.snapped = Some(face);
        }
    }
}

#[cfg(test)]
mod tests;
