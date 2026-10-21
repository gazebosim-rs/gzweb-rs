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
//!
//! A view's [`CameraMode`] (spec §7.2, gzweb's and the Gazebo GUI's camera
//! tracking) can follow an entity, moving with it, or track it, turning to
//! keep it in view ([`modes`], DECISIONS.md D-60).

pub mod controls;
pub mod input;
pub mod modes;
#[cfg(target_arch = "wasm32")]
pub mod web;

use bevy::camera::primitives::Aabb;
use bevy::camera::{RenderTarget, ScalingMode};
use bevy::ecs::system::SystemParam;
use bevy::picking::mesh_picking::ray_cast::{MeshRayCast, MeshRayCastSettings};
use bevy::prelude::*;
use gz_core::scene::GeometryDesc;
use std::f32::consts::{FRAC_PI_2, TAU};

use crate::camera::GzCameraTarget;
use crate::spawn::{gz_to_bevy, GzVisual, GzWorldRoot};

pub use controls::ViewerControls;
pub use input::InputMap;
pub use modes::CameraMode;

/// How long snaps, focus and home animate (spec §12: cube snaps in 250 ms).
pub const ANIMATION_SECS: f32 = 0.25;
/// gzweb's default camera position (`Scene.ts`), in Gazebo coordinates,
/// looking at the origin.
pub const HOME_GZ: Vec3 = Vec3::new(-6.0, 0.0, 6.0);
/// Zoom factor per wheel step (three.js `dollyIn`, zoom speed 1).
pub const ZOOM_STEP: f32 = 0.95;
/// How much closer than the nearest world point an orthographic camera's
/// near plane stays, at least.
pub const ORTHO_MARGIN: f32 = 1.0;
/// How far the world reaches from the pivot when there is no world to
/// measure (nothing under `GzWorldRoot`).
pub const DEFAULT_EXTENT: f32 = 50.0;
/// The distance that frames the whole world, when there is no world.
pub const DEFAULT_WORLD_REACH: f32 = 100.0;
/// Zooming out stops this many whole-world framings away.
pub const ZOOM_OUT_LIMIT: f32 = 4.0;
/// Zooming in stops this close to the pivot (the near plane is 0.01).
pub const MIN_DISTANCE: f32 = 0.02;
/// Half the size of Gazebo's 20 × 20 m grid, which views keep in range.
const GRID_HALF: f32 = 10.0;
/// Space around focused bounds.
const FOCUS_MARGIN: f32 = 1.1;
/// The smallest radius focus frames, so a point still gets a view.
const MIN_FOCUS_RADIUS: f32 = 0.05;
/// Fly speed floor, as a distance to what the view looks at.
const MIN_FLY_REACH: f32 = 0.5;
const MIN_ORTHO_HEIGHT: f32 = 1e-3;
/// How close to straight up or down the camera may pitch (radians).
const PITCH_EPS: f32 = 1e-4;

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
    /// else the ground). With nothing there, an `orbit` drag falls back to
    /// the selection's centre, then the world origin (spec §6.2), if that
    /// is in front of the camera; otherwise the pivot moves onto the
    /// pointer's ray at its old depth.
    PivotAt {
        at: Vec2,
        orbit: bool,
    },
    /// Rotate about the pivot: 2π per viewport height. The camera stays
    /// upright, from straight down to straight up. Leaves a snapped view,
    /// back to perspective.
    Orbit(Vec2),
    /// Move camera and pivot in the view plane; the plane through the pivot
    /// follows the pointer.
    Pan(Vec2),
    /// Dolly towards the pivot (orthographic: shrink the view about it), by
    /// [`ZOOM_STEP`] per step. Positive steps zoom in.
    Zoom(f32),
    /// Zoom about the point under `at`, which becomes the pivot (on a miss,
    /// the point on the ray at the pivot's old depth).
    ZoomAt {
        at: Vec2,
        steps: f32,
    },
    /// Orthographic re-centres the pivot on what is at the view centre.
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
    /// Animate to look at the pivot from `from` (a direction in Gazebo
    /// axes, pivot to camera), keeping the distance and the projection:
    /// the view cube's edges and corners. Leaves a snapped view.
    LookFrom(Vec3),
    /// Free the camera, or follow or track an entity: the view turns to it
    /// (animated), then moves with it (follow) or keeps turning to it
    /// (track). See [`CameraMode`].
    SetMode(CameraMode),
    /// ⇧F: release the follow if following; else follow the selection,
    /// else the model under `at`.
    Follow {
        at: Option<Vec2>,
    },
    /// ⇧T: as [`ViewOp::Follow`], tracking.
    Track {
        at: Option<Vec2>,
    },
    /// Move camera and pivot: `dir` is x right and z forward in the ground
    /// plane, y world up (orthographic: the screen plane, and y zooms);
    /// `secs` of travel at the view's fly speed.
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

/// The selected entities. Nothing selects yet: the full UI's selection
/// (M5) fills it. Views already use it for the orbit pivot fallback, focus
/// and snaps.
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
    /// its field of view. Its `far` is a floor: views push the far plane out
    /// to keep the world inside it.
    pub perspective: PerspectiveProjection,
    pub rotate_speed: f32,
    pub zoom_speed: f32,
    pub pan_speed: f32,
    /// Fly keys travel this many view distances per second.
    pub fly_speed: f32,
    /// Free, or following or tracking an entity.
    pub mode: CameraMode,
    /// Where the followed entity was last frame.
    follow_last: Option<Vec3>,
    animation: Option<Animation>,
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
            mode: CameraMode::Free,
            follow_last: None,
            animation: None,
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

/// Sets an orthographic projection's view height.
fn set_ortho_height(p: &mut Projection, height: f32) {
    if let Projection::Orthographic(o) = p {
        o.scale = 1.0;
        o.scaling_mode = ScalingMode::FixedVertical {
            viewport_height: height.max(MIN_ORTHO_HEIGHT),
        };
    }
}

fn near(p: &Projection) -> f32 {
    match p {
        Projection::Perspective(p) => p.near,
        Projection::Orthographic(o) => o.near,
        Projection::Custom(_) => 0.0,
    }
}

/// The pivot in the camera's own frame.
fn offset(view: &ViewCamera, t: &Transform) -> Vec3 {
    t.rotation.inverse() * (view.pivot - t.translation)
}

/// How far in front of the camera `point` is, along its view axis.
fn depth_of(t: &Transform, point: Vec3) -> f32 {
    (point - t.translation).dot(*t.forward())
}

/// The point on `ray` at view depth `depth`.
fn on_ray_at_depth(ray: Ray3d, t: &Transform, depth: f32) -> Vec3 {
    let along = ray.direction.dot(*t.forward()).max(f32::EPSILON);
    ray.origin + *ray.direction * ((depth - depth_of(t, ray.origin)) / along)
}

/// Switches to orthographic, keeping the pivot where it is on screen and
/// the scale in the plane through it. A pivot at or behind the near plane
/// has no such plane: the distance to it stands in.
fn to_ortho(view: &ViewCamera, t: &mut Transform, p: &mut Projection) {
    if ortho_height(p).is_some() {
        return;
    }
    let mut off = offset(view, t);
    let depth = if -off.z > near(p) {
        -off.z
    } else {
        off.length().max(MIN_DISTANCE)
    };
    *p = Projection::Orthographic(OrthographicProjection {
        near: depth,
        far: view.perspective.far,
        ..OrthographicProjection::default_3d()
    });
    set_ortho_height(p, 2.0 * depth * view.half_tan());
    off.z = -depth;
    t.translation = view.pivot - t.rotation * off;
}

/// Switches to perspective, keeping the pivot on screen and the scale in
/// the plane through it: the camera moves to the matching depth. The view
/// is no longer snapped.
fn to_perspective(view: &mut ViewCamera, t: &mut Transform, p: &mut Projection) {
    view.snapped = None;
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

/// Where `ray` meets the ground (Gazebo z = 0, Bevy y = 0) in front of
/// its origin and within `far` of it.
pub fn ground_point(ray: Ray3d, far: f32) -> Option<Vec3> {
    let t = -ray.origin.y / ray.direction.y;
    (t.is_finite() && t > 0.0 && t <= far).then(|| ray.origin + ray.direction * t)
}

/// The Gazebo x, y on the ground plane under `cursor` (logical pixels in
/// the window), as the view picks it: the status bar's cursor readout.
pub fn cursor_on_ground(
    camera: &Camera,
    origin: Option<&ViewOrigin>,
    projection: &Projection,
    t: &Transform,
    cursor: Vec2,
) -> Option<Vec2> {
    let rect = view_rect(camera, origin)?;
    let at = cursor - rect.min;
    if at.x < 0.0 || at.y < 0.0 || at.x > rect.width() || at.y > rect.height() {
        return None;
    }
    let far = match projection {
        Projection::Perspective(p) => p.far,
        Projection::Orthographic(o) => o.far,
        Projection::Custom(_) => f32::MAX,
    };
    let p = ground_point(viewport_ray(projection, t, rect.size(), at)?, far)?;
    let gz = gz_to_bevy().inverse() * p;
    Some(Vec2::new(gz.x, gz.y))
}

/// Where the full UI shows the 3D view (spec §9.3). The shell writes
/// `rect` after UI layout; [`apply_view_placement`] then renders the main
/// camera into [`ViewPlacement::image`], sized to the rect in physical
/// pixels, which the shell shows in the view's node. Without a rect (the
/// minimal mode) the camera renders to the window as before.
///
/// Rendering to a texture rather than to a `Camera::viewport` of the
/// window: Bevy 0.20's TAA samples the whole window-sized main texture and
/// draws it into the viewport, squeezing the scene (DECISIONS.md D-31).
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct ViewPlacement {
    /// The view in the window, in logical pixels.
    pub rect: Option<Rect>,
    /// The window's physical pixels per logical pixel.
    pub scale: f32,
    /// The texture the main camera renders into while placed.
    pub image: Option<Handle<Image>>,
}

impl ViewPlacement {
    /// The texture size for the rect: its physical size, at least 1×1.
    pub fn physical_size(&self) -> Option<UVec2> {
        let r = self.rect?;
        Some(
            (r.size() * self.scale.max(f32::EPSILON))
                .round()
                .as_uvec2()
                .max(UVec2::ONE),
        )
    }
}

/// Where a placed camera's image sits in the window (logical pixels), for
/// pointer maths ([`view_rect`]).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct ViewOrigin(pub Vec2);

/// A camera's view in window logical pixels: its image's place when it is
/// placed, else its viewport (or the whole target).
pub fn view_rect(camera: &Camera, origin: Option<&ViewOrigin>) -> Option<Rect> {
    let size = camera.logical_viewport_size()?;
    let min = origin.map_or_else(
        || camera.logical_viewport_rect().map_or(Vec2::ZERO, |r| r.min),
        |o| o.0,
    );
    Some(Rect::from_corners(min, min + size))
}

/// A render-target texture of `size` (no CPU copy: it is only drawn into).
pub fn view_texture(size: UVec2) -> Image {
    let mut image = Image::new_target_texture(
        size.x,
        size.y,
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        None,
    );
    image.data = None;
    // Bevy's target textures copy their old contents into the resized
    // texture, which needs COPY_SRC; a view redraws every frame anyway.
    image.copy_on_resize = false;
    image
}

/// Renders the main camera into the placed rect's texture: creates or
/// resizes it, points the camera at it, and updates the camera's target
/// size and projection in the same frame, so nothing renders at the old
/// size after a resize or a splitter drag (spec §9.3). Runs after UI
/// layout and before frustum culling.
#[allow(clippy::type_complexity)]
pub fn apply_view_placement(
    mut placement: ResMut<ViewPlacement>,
    images: Option<ResMut<Assets<Image>>>,
    mut cameras: Query<
        (
            Entity,
            &mut Camera,
            &mut Projection,
            &mut RenderTarget,
            Option<&ViewOrigin>,
        ),
        With<crate::camera::MainCamera>,
    >,
    mut commands: Commands,
) {
    let (Some(rect), Some(size), Some(mut images)) =
        (placement.rect, placement.physical_size(), images)
    else {
        return;
    };
    let scale = placement.scale.max(f32::EPSILON);
    let handle = match &placement.image {
        Some(h) => {
            if let Some(image) = images.get(h) {
                if image.size() != size {
                    if let Some(mut image) = images.get_mut(h) {
                        image.resize(bevy::render::render_resource::Extent3d {
                            width: size.x,
                            height: size.y,
                            depth_or_array_layers: 1,
                        });
                    }
                }
            }
            h.clone()
        }
        None => {
            let h = images.add(view_texture(size));
            placement.image = Some(h.clone());
            h
        }
    };
    for (e, mut camera, mut projection, mut target, origin) in &mut cameras {
        let placed = matches!(&*target, RenderTarget::Image(t) if t.handle == handle && t.scale_factor == scale);
        if !placed {
            *target = RenderTarget::Image(bevy::camera::ImageRenderTarget {
                handle: handle.clone(),
                scale_factor: scale,
            });
        }
        let info = bevy::camera::RenderTargetInfo {
            physical_size: size,
            scale_factor: scale,
        };
        let stale = camera
            .computed
            .target_info
            .as_ref()
            .is_none_or(|t| t.physical_size != size || t.scale_factor != scale);
        if stale || camera.viewport.is_some() {
            camera.viewport = None;
            camera.computed.target_info = Some(info);
            let logical = size.as_vec2() / scale;
            projection.update(logical.x, logical.y);
            camera.computed.clip_from_view = projection.get_clip_from_view();
        }
        if origin.map(|o| o.0) != Some(rect.min) {
            commands.entity(e).insert(ViewOrigin(rect.min));
        }
    }
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
        set_ortho_height(p, self.ortho_height);
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

    /// The distance from `p` to the farthest point of the bounds, at most.
    pub fn reach_from(&self, p: Vec3) -> f32 {
        (self.center() - p).length() + self.radius()
    }
}

/// What an op needs besides the view: the viewport size and the world's
/// bounds (ground planes and the grid included), for the view's limits.
#[derive(Debug, Clone, Copy)]
struct Frame {
    size: Vec2,
    world: Option<Bounds>,
}

impl Frame {
    fn height(&self) -> f32 {
        self.size.y.max(1.0)
    }

    fn aspect(&self) -> f32 {
        if self.size.y > 0.0 {
            self.size.x / self.size.y
        } else {
            1.0
        }
    }

    /// How far the world reaches from `p`.
    fn extent_from(&self, p: Vec3) -> f32 {
        self.world.map_or(DEFAULT_EXTENT, |b| b.reach_from(p))
    }

    /// The distance that frames the whole world.
    fn world_reach(&self, view: &ViewCamera) -> f32 {
        self.world.map_or(DEFAULT_WORLD_REACH, |b| {
            focus_distance(view, b.radius(), self.aspect())
        })
    }

    /// How far zooming out may go.
    fn max_distance(&self, view: &ViewCamera) -> f32 {
        ZOOM_OUT_LIMIT * self.world_reach(view)
    }
}

/// How far from a sphere of radius `r` the camera sits to frame it.
fn focus_distance(view: &ViewCamera, r: f32, aspect: f32) -> f32 {
    let half = view.half_tan();
    let fit = half.atan().min((half * aspect).atan());
    r.max(MIN_FOCUS_RADIUS) * FOCUS_MARGIN / fit.sin()
}

/// A mesh that view picks pass through: the full UI's view cube, which
/// renders on its own layer but lives in the same world.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct NoViewPick;

/// A scene node's bounds, placement, children, visual and visibility, for
/// framing.
type NodeData = (
    Option<&'static Aabb>,
    &'static GlobalTransform,
    Option<&'static Children>,
    Option<&'static GzVisual>,
    Option<&'static Visibility>,
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
    skip: Query<'w, 's, (), With<NoViewPick>>,
}

impl ViewScene<'_, '_> {
    /// The nearest visible mesh on the ray.
    fn hit(&mut self, ray: Ray3d) -> Option<(Entity, Vec3)> {
        let skip = &self.skip;
        let filter = |e: Entity| !skip.contains(e);
        let settings = MeshRayCastSettings::default().with_filter(&filter);
        self.ray_cast
            .cast_ray(ray, &settings)
            .first()
            .map(|(e, hit)| (*e, hit.point))
    }

    /// What ⇧F and ⇧T follow: the first selected entity, else the
    /// top-level model on `ray`.
    fn target(&mut self, ray: Option<Ray3d>) -> Option<Entity> {
        if let Some(e) = self.selection.entities.first() {
            return Some(*e);
        }
        let (hit, _) = self.hit(ray?)?;
        Some(self.model_of(hit))
    }

    /// Where `e` is in the world (last frame's placement).
    fn position(&self, e: Entity) -> Option<Vec3> {
        self.nodes.get(e).ok().map(|(_, g, ..)| g.translation())
    }

    /// The nearest mesh on the ray, else where it meets the ground, within
    /// `far` of its origin.
    fn surface(&mut self, ray: Ray3d, far: f32) -> Option<Vec3> {
        if let Some((_, p)) = self.hit(ray) {
            return Some(p);
        }
        ground_point(ray, far)
    }

    /// Bounds of every mesh in `entity`'s subtree. Hidden subtrees are left
    /// out, and with `skip_planes` plane visuals too (a ground plane would
    /// swamp the world).
    fn bounds_of(&self, entity: Entity, skip_planes: bool) -> Option<Bounds> {
        let (aabb, global, children, visual, visibility) = self.nodes.get(entity).ok()?;
        if visibility == Some(&Visibility::Hidden)
            || skip_planes
                && visual.is_some_and(|v| matches!(v.0.geometry, GeometryDesc::Plane { .. }))
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

    /// Everything a view must keep in range: the world with its ground
    /// planes, and the grid.
    fn extent(&self) -> Option<Bounds> {
        let root = self.roots.iter().next()?;
        let grid = Bounds {
            min: Vec3::new(-GRID_HALF, 0.0, -GRID_HALF),
            max: Vec3::new(GRID_HALF, 0.0, GRID_HALF),
        };
        Some(self.bounds_of(root, false).map_or(grid, |b| b.union(grid)))
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

    /// The drag pivot under `ray` (spec §6.2). On a miss, an orbit falls
    /// back to the selection's centre, then the origin, if in front of the
    /// camera; otherwise the point on the ray at the pivot's depth.
    fn pivot(
        &mut self,
        ray: Option<Ray3d>,
        orbit: bool,
        view: &ViewCamera,
        t: &Transform,
        p: &Projection,
    ) -> Vec3 {
        let far = p.far();
        if let Some(hit) = ray.and_then(|r| self.surface(r, far)) {
            return hit;
        }
        if orbit {
            let fallback = self.selection_bounds().map_or(Vec3::ZERO, |b| b.center());
            if ray.is_none() || depth_of(t, fallback) > near(p) {
                return fallback;
            }
        }
        match ray {
            Some(r) => on_ray_at_depth(r, t, pivot_depth(view, t, p)),
            None => view.pivot,
        }
    }

    /// The depth of what is at the view centre (as a snap picks it), else
    /// of the pivot, else the view's reach, kept between [`MIN_DISTANCE`]
    /// and a framing of the whole world: what fly speed and the
    /// orthographic framing scale with.
    fn centre_depth(&mut self, view: &ViewCamera, t: &Transform, p: &Projection, f: &Frame) -> f32 {
        let near = near(p);
        let far = p.far();
        let depth = viewport_ray(p, t, f.size, f.size / 2.0)
            .and_then(|r| self.surface(r, far))
            .map(|q| depth_of(t, q))
            .filter(|d| *d > near)
            .unwrap_or_else(|| pivot_depth(view, t, p));
        depth.clamp(MIN_DISTANCE, f.world_reach(view).max(MIN_DISTANCE))
    }
}

/// The pivot's depth in front of the camera, or the view's reach when the
/// pivot is at or behind the near plane.
fn pivot_depth(view: &ViewCamera, t: &Transform, p: &Projection) -> f32 {
    let d = depth_of(t, view.pivot);
    if d > near(p) {
        d
    } else {
        reach(view, t, p).max(MIN_DISTANCE)
    }
}

/// Applies [`ViewCommand`]s, then advances animations, and publishes the
/// first view's pivot as [`GzCameraTarget`]. Idle frames change nothing.
pub fn apply_view_commands(
    mut commands: MessageReader<ViewCommand>,
    mut views: Query<(&mut ViewCamera, &mut Transform, &mut Projection, &Camera)>,
    mut scene: ViewScene,
    time: Res<Time<Real>>,
    mut target: ResMut<GzCameraTarget>,
) {
    let busy = !commands.is_empty() || views.iter().any(|(v, ..)| v.animating());
    if busy {
        let world = scene.extent();
        let frame = |camera: &Camera| Frame {
            size: camera.logical_viewport_size().unwrap_or(Vec2::ZERO),
            world,
        };
        for cmd in commands.read() {
            let Ok((mut view, mut t, mut p, camera)) = views.get_mut(cmd.view) else {
                continue;
            };
            let f = frame(camera);
            apply(cmd.op, &mut view, &mut t, &mut p, &f, &mut scene);
            fit_clip(&mut view, &mut t, &mut p, &f);
        }
        let dt = time.delta_secs();
        for (mut view, mut t, mut p, camera) in &mut views {
            if view.animating() {
                animate(&mut view, &mut t, &mut p, dt);
                fit_clip(&mut view, &mut t, &mut p, &frame(camera));
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
    f: &Frame,
    scene: &mut ViewScene,
) {
    let ray = |p: &Projection, t: &Transform, at: Vec2| viewport_ray(p, t, f.size, at);
    // An op that moves the view right away lands a running animation first,
    // so a click mid-snap doesn't leave the view half-turned. Snap, focus
    // and home start their own from where the view is.
    if !matches!(
        op,
        ViewOp::Snap(_)
            | ViewOp::Focus { .. }
            | ViewOp::Home
            | ViewOp::SetDragAction(_)
            | ViewOp::LookFrom(_)
            | ViewOp::SetMode(_)
            | ViewOp::Follow { .. }
            | ViewOp::Track { .. }
    ) {
        finish(view, t, p);
    }
    match op {
        ViewOp::SetDragAction(action) => view.drag_action = action,
        ViewOp::PivotAt { at, orbit } => {
            view.pivot = scene.pivot(ray(p, t, at), orbit, view, t, p);
        }
        ViewOp::Orbit(delta) => {
            if view.snapped.is_some() {
                to_perspective(view, t, p);
            }
            orbit(view, t, delta, f.height());
        }
        ViewOp::Pan(delta) => pan(view, t, p, delta, f.height()),
        ViewOp::Zoom(steps) => zoom(view, t, p, steps, f),
        ViewOp::ZoomAt { at, steps } => {
            view.pivot = scene.pivot(ray(p, t, at), false, view, t, p);
            zoom(view, t, p, steps, f);
        }
        ViewOp::SetProjection(ProjectionKind::Orthographic) => {
            to_ortho_centred(view, t, p, f, scene);
        }
        ViewOp::SetProjection(ProjectionKind::Perspective) => to_perspective(view, t, p),
        ViewOp::ToggleProjection => {
            if ortho_height(p).is_some() {
                to_perspective(view, t, p);
            } else {
                to_ortho_centred(view, t, p, f, scene);
            }
        }
        ViewOp::Snap(face) => {
            let far = p.far();
            let pivot = scene
                .selection_bounds()
                .map(|b| b.center())
                .or_else(|| ray(p, t, f.size / 2.0).and_then(|r| scene.surface(r, far)))
                .unwrap_or(view.pivot);
            snap(view, t, p, face, pivot, f);
        }
        ViewOp::Focus { at } => {
            // The selection, else the model under the cursor without its
            // ground planes (a ground model has nothing else), else the world.
            let bounds = scene
                .selection_bounds()
                .or_else(|| {
                    let hit = at.and_then(|at| ray(p, t, at)).and_then(|r| scene.hit(r));
                    hit.and_then(|(e, _)| scene.bounds_of(scene.model_of(e), true))
                })
                .or_else(|| scene.world_bounds());
            match bounds {
                Some(b) => focus(view, t, p, b, f),
                None => home(view, t, p),
            }
        }
        ViewOp::Home => {
            view.mode = CameraMode::Free;
            home(view, t, p);
        }
        ViewOp::LookFrom(from) => look_from(view, t, p, from, f),
        ViewOp::SetMode(mode) => modes::set_mode(view, t, p, mode, scene),
        ViewOp::Follow { at } | ViewOp::Track { at } => {
            let follow = matches!(op, ViewOp::Follow { .. });
            let same = matches!(
                (view.mode, follow),
                (CameraMode::Follow(_), true) | (CameraMode::Track(_), false)
            );
            if same {
                modes::set_mode(view, t, p, CameraMode::Free, scene);
            } else if let Some(e) = scene.target(at.and_then(|at| ray(p, t, at))) {
                let mode = if follow {
                    CameraMode::Follow(e)
                } else {
                    CameraMode::Track(e)
                };
                modes::set_mode(view, t, p, mode, scene);
            }
        }
        ViewOp::Fly { dir, secs } => {
            let reach = match ortho_height(p) {
                Some(_) => reach(view, t, p),
                None => scene.centre_depth(view, t, p, f),
            };
            fly(view, t, p, dir, secs, reach.max(MIN_FLY_REACH));
        }
    }
}

/// Re-centres the pivot on what is at the view centre (at
/// [`ViewScene::centre_depth`]), then switches to orthographic about it.
fn to_ortho_centred(
    view: &mut ViewCamera,
    t: &mut Transform,
    p: &mut Projection,
    f: &Frame,
    scene: &mut ViewScene,
) {
    if ortho_height(p).is_some() {
        return;
    }
    let depth = scene.centre_depth(view, t, p, f);
    view.pivot = match viewport_ray(p, t, f.size, f.size / 2.0) {
        Some(r) => on_ray_at_depth(r, t, depth),
        None => t.translation + *t.forward() * depth,
    };
    to_ortho(view, t, p);
}

/// Keeps the world in the view's clip range after every change. An
/// orthographic camera backs off until the whole world is in front of its
/// near plane, which sits at the view's reach, so rays the atmosphere
/// traces through it spread like the matching perspective view's; its far
/// plane is just behind the world. A perspective far plane grows past
/// [`ViewCamera::perspective`]'s to the world's far side.
fn fit_clip(view: &mut ViewCamera, t: &mut Transform, p: &mut Projection, f: &Frame) {
    let floor = view.perspective.far;
    match p {
        Projection::Orthographic(_) => {
            let extent = f.extent_from(view.pivot);
            let reach = reach(view, t, p);
            let mut off = offset(view, t);
            let standoff = extent + reach.max(ORTHO_MARGIN);
            if -off.z < standoff {
                off.z = -standoff;
                t.translation = view.pivot - t.rotation * off;
            }
            let (near, far) = (reach, -off.z + extent + ORTHO_MARGIN);
            if let Projection::Orthographic(o) = p {
                if (o.near, o.far) != (near, far) {
                    (o.near, o.far) = (near, far);
                }
            }
        }
        Projection::Perspective(pp) => {
            let far = f.world.map_or(floor, |b| {
                (b.reach_from(t.translation) + ORTHO_MARGIN).max(floor)
            });
            if pp.far != far {
                pp.far = far;
            }
        }
        Projection::Custom(_) => {}
    }
}

/// gzweb's orbit: yaw about world up, then pitch about the camera's right
/// axis. The pitch stops with the camera looking straight down or up, so
/// it never turns upside down, wherever the pivot is on screen.
fn orbit(view: &ViewCamera, t: &mut Transform, delta: Vec2, height: f32) {
    let d = delta * view.rotate_speed;
    let d_yaw = -TAU * d.x / height;
    let d_pitch = -TAU * d.y / height;
    let pivot = view.pivot;
    t.rotate_around(pivot, Quat::from_axis_angle(Vec3::Y, d_yaw));
    let f = *t.forward();
    let pitch = f.y.atan2(Vec2::new(f.x, f.z).length());
    let limit = FRAC_PI_2 - PITCH_EPS;
    let turn = (pitch + d_pitch).clamp(-limit, limit) - pitch;
    let right = t.rotation * Vec3::X;
    t.rotate_around(pivot, Quat::from_axis_angle(right, turn));
}

/// Screen-space pan: the plane through the pivot, facing the camera,
/// follows the pointer.
fn pan(view: &mut ViewCamera, t: &mut Transform, p: &Projection, delta: Vec2, height: f32) {
    let per_px = match ortho_height(p) {
        Some(h) => h / height,
        None => 2.0 * pivot_depth(view, t, p) * view.half_tan() / height,
    };
    let d = delta * view.pan_speed * per_px;
    let pan = -(t.rotation * Vec3::X) * d.x + (t.rotation * Vec3::Y) * d.y;
    t.translation += pan;
    view.pivot += pan;
}

/// Dolly about the pivot; an orthographic view shrinks about it instead,
/// keeping the pivot fixed on screen. Between [`MIN_DISTANCE`] and
/// [`ZOOM_OUT_LIMIT`] world framings (or wherever the view already is).
fn zoom(view: &ViewCamera, t: &mut Transform, p: &mut Projection, steps: f32, f: &Frame) {
    let s = ZOOM_STEP.powf(steps * view.zoom_speed);
    let max = f.max_distance(view);
    let limit = |d: f32| (d * s).clamp(MIN_DISTANCE.min(d), max.max(d)) / d.max(f32::EPSILON);
    match ortho_height(p) {
        Some(h) => {
            let d = h / (2.0 * view.half_tan());
            let s = limit(d).max(MIN_ORTHO_HEIGHT / h);
            set_ortho_height(p, h * s);
            let mut off = offset(view, t);
            off.x *= s;
            off.y *= s;
            t.translation = view.pivot - t.rotation * off;
        }
        None => {
            let s = limit((t.translation - view.pivot).length());
            t.translation = view.pivot + (t.translation - view.pivot) * s;
        }
    }
}

/// Moves camera and pivot at `fly_speed × reach` per second. Perspective:
/// in the ground plane (W/S along the view direction flattened, A/D
/// sideways) or straight up and down. Orthographic has no depth to fly
/// along: W/S and A/D move in the screen plane, and Q/E zoom (E out, as
/// rising does over a top view).
fn fly(
    view: &mut ViewCamera,
    t: &mut Transform,
    p: &mut Projection,
    dir: Vec3,
    secs: f32,
    reach: f32,
) {
    let step = view.fly_speed * reach * secs;
    if let Some(h) = ortho_height(p) {
        let delta = (t.rotation * Vec3::X * dir.x + t.rotation * Vec3::Y * dir.z) * step;
        t.translation += delta;
        view.pivot += delta;
        if dir.y != 0.0 {
            let s = (view.fly_speed * dir.y * secs).exp();
            set_ortho_height(p, h * s);
            let mut off = offset(view, t);
            off.x *= s;
            off.y *= s;
            t.translation = view.pivot - t.rotation * off;
        }
        return;
    }
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

/// Animates to `face` about `pivot`, keeping the distance (the reach, if
/// orthographic); orthographic at the end, with the same framing.
fn snap(
    view: &mut ViewCamera,
    t: &Transform,
    p: &Projection,
    face: SnapView,
    pivot: Vec3,
    f: &Frame,
) {
    let from = Pose::of(view, t, p);
    let distance = match ortho_height(p) {
        Some(_) => reach(view, t, p),
        None => (t.translation - pivot).length(),
    };
    let to = Pose {
        pivot,
        rotation: face.rotation(),
        offset: Vec3::new(
            0.0,
            0.0,
            -distance.clamp(MIN_DISTANCE, f.max_distance(view)),
        ),
        ortho_height: from.ortho_height,
    };
    start(view, from, to, Some(face));
}

/// Animates to look at the pivot from `from` (Gazebo axes), keeping the
/// distance (the reach, if orthographic) and the projection; up is
/// Gazebo +Z, or +X looking straight down or up.
fn look_from(view: &mut ViewCamera, t: &Transform, p: &Projection, from: Vec3, f: &Frame) {
    let Some(dir) = (gz_to_bevy() * from).try_normalize() else {
        return;
    };
    view.snapped = None;
    let start_pose = Pose::of(view, t, p);
    let distance = match ortho_height(p) {
        Some(_) => reach(view, t, p),
        None => (t.translation - view.pivot).length(),
    };
    let up = if dir.y.abs() > 1.0 - 1e-4 {
        gz_to_bevy() * Vec3::X
    } else {
        Vec3::Y
    };
    let to = Pose {
        pivot: view.pivot,
        rotation: Transform::default().looking_to(-dir, up).rotation,
        offset: Vec3::new(
            0.0,
            0.0,
            -distance.clamp(MIN_DISTANCE, f.max_distance(view)),
        ),
        ortho_height: start_pose.ortho_height,
    };
    start(view, start_pose, to, None);
}

/// Animates to frame `bounds` from the current direction.
fn focus(view: &mut ViewCamera, t: &Transform, p: &Projection, bounds: Bounds, f: &Frame) {
    let from = Pose::of(view, t, p);
    let r = bounds.radius().max(MIN_FOCUS_RADIUS) * FOCUS_MARGIN;
    let to = Pose {
        pivot: bounds.center(),
        rotation: t.rotation,
        offset: Vec3::new(0.0, 0.0, -focus_distance(view, bounds.radius(), f.aspect())),
        ortho_height: match ortho_height(p) {
            Some(_) => 2.0 * r / f.aspect().min(1.0),
            None => 0.0,
        },
    };
    start(view, from, to, None);
}

/// Animates back to gzweb's default view, in perspective.
fn home(view: &mut ViewCamera, t: &mut Transform, p: &mut Projection) {
    to_perspective(view, t, p);
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
    if k >= 1.0 {
        finish(view, t, p);
        return;
    }
    let eased = ease(k);
    let pose = anim.from.lerp(&anim.to, eased);
    pose.apply(view, t, p);
}

/// The motion tokens' easing (`motion.easing`: cubic-out), for `k` in
/// 0..=1.
pub fn ease(k: f32) -> f32 {
    1.0 - (1.0 - k.clamp(0.0, 1.0)).powi(3)
}

/// Jumps a running animation to its end: the final pose, and for a snap
/// the orthographic axis view.
fn finish(view: &mut ViewCamera, t: &mut Transform, p: &mut Projection) {
    let Some(anim) = view.animation.take() else {
        return;
    };
    anim.to.apply(view, t, p);
    if let Some(face) = anim.snap {
        to_ortho(view, t, p);
        view.snapped = Some(face);
    }
}

#[cfg(test)]
mod tests;
