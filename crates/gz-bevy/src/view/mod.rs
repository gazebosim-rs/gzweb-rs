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

pub mod align;
pub mod controls;
pub mod editing;
pub mod gizmo;
pub mod input;
pub mod model_edits;
pub mod modes;
pub mod outline;
pub mod place;
pub mod select;
#[cfg(target_arch = "wasm32")]
pub mod web;

use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::RenderLayers;
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
pub use select::{
    select, select_parts, BoxSelectTool, Marquee, MarqueeRect, SelectInViews, SelectMode,
};

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
/// How much of the world an orthographic camera backs off for, at most:
/// `f32` still places points to about a millimetre this far out.
pub const MAX_ORTHO_EXTENT: f32 = 10_000.0;
/// A mesh lying wholly this far from the world origin has fallen or flown
/// away: the world's bounds leave it out.
pub const RUNAWAY_DISTANCE: f32 = MAX_ORTHO_EXTENT;
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

impl std::str::FromStr for SnapView {
    type Err = String;

    /// `top`, `bottom`, `front`, `back`, `right` or `left` (any case).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.trim().to_ascii_lowercase().as_str() {
            "top" => SnapView::Top,
            "bottom" => SnapView::Bottom,
            "front" => SnapView::Front,
            "back" => SnapView::Back,
            "right" => SnapView::Right,
            "left" => SnapView::Left,
            _ => {
                return Err(format!(
                    "unknown view '{s}' (top, bottom, front, back, right or left)"
                ))
            }
        })
    }
}

/// How the view starts once the first world loads: an axis view,
/// overlays turned on and top-level models selected (`gzweb --view`,
/// `--overlays`, `--select`; DECISIONS.md D-64), so a capture can show
/// them. Applied once.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct StartView {
    pub view: Option<SnapView>,
    pub overlays: Vec<crate::overlays::Overlay>,
    /// Top-level model names.
    pub select: Vec<String>,
    /// Edit the first of them (`gzweb --edit`; §6.6).
    pub edit: bool,
}

/// Applies [`StartView`] when the first scene has arrived, then removes
/// it.
#[allow(clippy::too_many_arguments)]
pub fn apply_start_view(
    start: Option<Res<StartView>>,
    index: Option<Res<crate::spawn::GzIndex>>,
    views: Query<(Entity, Option<&ViewId>), With<ViewCamera>>,
    parents: Query<&ChildOf>,
    mut selection: ResMut<Selection>,
    mut view_out: MessageWriter<ViewCommand>,
    mut overlay_out: MessageWriter<crate::overlays::OverlayCommand>,
    mut edit_out: MessageWriter<editing::EditCommand>,
    mut commands: Commands,
) {
    let Some(start) = start else { return };
    if index.as_ref().is_none_or(|i| i.scene.is_none()) {
        return;
    }
    let Some((view, _)) = views.iter().min_by_key(|(_, id)| id.copied()) else {
        return;
    };
    if let Some(face) = start.view {
        view_out.write(ViewCommand {
            view,
            op: ViewOp::Snap(face),
        });
    }
    for o in &start.overlays {
        overlay_out.write(crate::overlays::OverlayCommand::set(*o, true));
    }
    if let Some(index) = index.filter(|_| !start.select.is_empty()) {
        let top = |e: &&Entity| {
            parents
                .get(**e)
                .is_ok_and(|p| Some(p.parent()) == index.root)
        };
        let picked: Vec<Entity> = start
            .select
            .iter()
            .filter_map(|name| index.by_name.get(name))
            .flat_map(|es| es.iter().filter(top).copied())
            .collect();
        select(&mut selection.reborrow(), SelectMode::Replace, &picked);
        if let Some(model) = picked.first().filter(|_| start.edit) {
            edit_out.write(editing::EditCommand::Enter(*model));
        }
    }
    commands.remove_resource::<StartView>();
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
    /// A click at `at`: picks the top-level model under it (nothing, on
    /// empty space) into the selection by `mode`.
    Select {
        at: Vec2,
        mode: SelectMode,
    },
    /// A click while placing ([`place::Placing`]): place at the surface
    /// under `at`; `keep` to place more.
    Place {
        at: Vec2,
        keep: bool,
    },
    /// A marquee from `from` to `to`: picks the top-level models it
    /// overlaps.
    SelectRect {
        from: Vec2,
        to: Vec2,
        mode: SelectMode,
    },
    /// A right click at `at` (no drag): the model under it is the
    /// subject of a context menu ([`EntityContext`]), selected first
    /// unless it already is.
    Context {
        at: Vec2,
    },
    /// Put the camera where a workspace saved it (§8): its pose, pivot,
    /// projection and axis view; free.
    Restore(ViewPose),
    /// A double click at `at` with Translate or Rotate (spec §6.6): edit
    /// the model under it ([`editing::EditCommand::Enter`]).
    EditAt {
        at: Vec2,
    },
}

/// Where a view's camera is, as a workspace saves it (spec §8): its pose
/// and pivot (Bevy's world frame), the orthographic view height when
/// orthographic, and the axis view it sits in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewPose {
    pub translation: Vec3,
    pub rotation: Quat,
    pub pivot: Vec3,
    pub ortho: Option<f32>,
    pub snapped: Option<SnapView>,
}

impl ViewPose {
    /// Where the camera is going: a running animation's end, else its
    /// pose now.
    pub fn of(view: &ViewCamera, t: &Transform, p: &Projection) -> Self {
        let (mut view, mut t, mut p) = (view.clone(), *t, p.clone());
        finish(&mut view, &mut t, &mut p);
        Self {
            translation: t.translation,
            rotation: t.rotation,
            pivot: view.pivot,
            ortho: ortho_height(&p),
            snapped: view.snapped,
        }
    }
}

/// Puts the camera at `pose` ([`ViewOp::Restore`]).
fn restore(view: &mut ViewCamera, t: &mut Transform, p: &mut Projection, pose: ViewPose) {
    view.snapped = pose.snapped;
    view.mode = CameraMode::Free;
    view.pivot = pose.pivot;
    t.translation = pose.translation;
    t.rotation = pose.rotation.normalize();
    match pose.ortho {
        Some(height) => {
            let depth = depth_of(t, view.pivot).max(MIN_DISTANCE);
            *p = Projection::Orthographic(OrthographicProjection {
                near: depth,
                far: view.perspective.far,
                ..OrthographicProjection::default_3d()
            });
            set_ortho_height(p, height);
        }
        None => *p = Projection::Perspective(view.perspective.clone()),
    }
}

/// A right click on a model in a view, or on an entity's row in the
/// Scene tree (§7.9): the full UI opens the entity context menu for it at
/// the pointer.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntityContext {
    pub entity: Entity,
    /// The view's camera, when a view asked (not the Scene tree).
    pub view: Option<Entity>,
}

/// A [`ViewOp`] for the view on camera entity `view`.
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct ViewCommand {
    pub view: Entity,
    pub op: ViewOp,
}

/// The selection: entities (top-level models picked in a view, spec §6.4,
/// and the models, links and lights the full UI's Scene tree selects) and
/// parts, the joints and sensors the tree selects, which have no entity of
/// their own. One selection for every view; the orbit pivot fallback,
/// focus, snaps, Follow and Track use its entities.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct Selection {
    pub entities: Vec<Entity>,
    pub parts: Vec<Part>,
}

impl Selection {
    /// How many things are selected, entities and parts.
    pub fn len(&self) -> usize {
        self.entities.len() + self.parts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// What Follow and Track take from the selection: its first entity,
    /// else its first joint's or sensor's model or link.
    pub fn first_target(&self) -> Option<Entity> {
        self.entities
            .first()
            .copied()
            .or_else(|| self.parts.first().map(|p| p.owner))
    }
}

/// The Teleop panel is driving (armed): the drive keys and X are its, so
/// the gizmo takes no drags (spec decision Q5; DECISIONS.md D-104).
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DriveArmed(pub bool);

/// A selected joint (on its model) or sensor (on its link).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Part {
    pub kind: PartKind,
    /// The model (a joint) or link (a sensor) it belongs to.
    pub owner: Entity,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PartKind {
    Joint,
    Sensor,
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

    /// A new view's start from this one: the same pivot, drag action and
    /// projection settings, free and still (no target, no animation).
    pub fn copied(&self) -> Self {
        Self {
            mode: CameraMode::Free,
            follow_last: None,
            animation: None,
            ..self.clone()
        }
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

/// Which view a camera is (spec §3.4: up to four, DECISIONS.md D-90):
/// the full UI's view nodes name their camera by it, since a new
/// environment respawns the cameras. The first is 0.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ViewId(pub u8);

impl ViewId {
    /// The most views a layout has (Quad).
    pub const MAX: u8 = 4;

    /// The render layer only this view's camera sees, besides the scene's
    /// (the transform gizmo of the view it applies to).
    pub fn layer(self) -> usize {
        VIEW_LAYER + usize::from(self.0)
    }

    /// The camera's layers: the scene's and its own.
    pub fn layers(self) -> RenderLayers {
        RenderLayers::from_layers(&[0, self.layer()])
    }
}

/// The first view's own render layer ([`ViewId::layer`]).
pub const VIEW_LAYER: usize = 20;

/// How many views the layout shows: the full UI sets it, and
/// `render::spawn_views` gives each a camera.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewSlots(pub u8);

impl Default for ViewSlots {
    fn default() -> Self {
        Self(1)
    }
}

/// Where the full UI shows a view (spec §9.3), on its camera. The shell
/// writes `rect` after UI layout; [`apply_view_placement`] then renders the
/// camera into [`ViewPlacement::image`], sized to the rect in physical
/// pixels, which the shell shows in the view's node. A view without a
/// rect isn't shown (its camera is off: the layout has fewer views); a
/// camera without a placement (the minimal mode) renders to the window as
/// before.
///
/// Rendering to a texture rather than to a `Camera::viewport` of the
/// window: Bevy 0.20's TAA samples the whole window-sized main texture and
/// draws it into the viewport, squeezing the scene (DECISIONS.md D-31).
#[derive(Component, Clone, Debug, Default, PartialEq)]
pub struct ViewPlacement {
    /// The view in the window, in logical pixels.
    pub rect: Option<Rect>,
    /// The window's physical pixels per logical pixel.
    pub scale: f32,
    /// The texture the camera renders into while placed.
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

/// Renders each placed camera into its rect's texture: creates or resizes
/// it, points the camera at it, and updates the camera's target size and
/// projection in the same frame, so nothing renders at the old size after
/// a resize or a splitter drag (spec §9.3); a camera placed nowhere is
/// off. Runs after UI layout and before frustum culling.
#[allow(clippy::type_complexity)]
pub fn apply_view_placement(
    images: Option<ResMut<Assets<Image>>>,
    mut cameras: Query<
        (
            Entity,
            &mut ViewPlacement,
            &mut Camera,
            &mut Projection,
            &mut RenderTarget,
            Option<&ViewOrigin>,
        ),
        With<crate::camera::MainCamera>,
    >,
    mut commands: Commands,
) {
    let Some(mut images) = images else {
        return;
    };
    for (e, mut placement, mut camera, mut projection, mut target, origin) in &mut cameras {
        let (Some(rect), Some(size)) = (placement.rect, placement.physical_size()) else {
            if camera.is_active {
                camera.is_active = false;
            }
            continue;
        };
        if !camera.is_active {
            camera.is_active = true;
        }
        let scale = placement.scale.max(f32::EPSILON);
        let handle = match &placement.image {
            Some(h) => {
                if images.get(h).is_some_and(|i| i.size() != size) {
                    if let Some(mut image) = images.get_mut(h) {
                        image.resize(bevy::render::render_resource::Extent3d {
                            width: size.x,
                            height: size.y,
                            depth_or_array_layers: 1,
                        });
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
    /// A focus or home that linked views follow ([`LinkedViews`]).
    link: bool,
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

    /// The eight corners.
    pub fn corners(&self) -> [Vec3; 8] {
        std::array::from_fn(|i| {
            Vec3::new(
                if i & 1 == 0 { self.min.x } else { self.max.x },
                if i & 2 == 0 { self.min.y } else { self.max.y },
                if i & 4 == 0 { self.min.z } else { self.max.z },
            )
        })
    }

    /// The distance from `p` to the nearest point of the bounds.
    pub fn distance_to(&self, p: Vec3) -> f32 {
        (p.clamp(self.min, self.max) - p).length()
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

/// What [`ViewScene::bounds_of`] leaves out besides hidden subtrees.
#[derive(Debug, Clone, Copy)]
struct Skip {
    planes: bool,
    runaway: bool,
}

impl Skip {
    const NOTHING: Skip = Skip {
        planes: false,
        runaway: false,
    };
    const RUNAWAY: Skip = Skip {
        planes: false,
        runaway: true,
    };
    const RUNAWAY_AND_PLANES: Skip = Skip {
        planes: true,
        runaway: true,
    };
}

/// What views look at: ray casts against the scene, the selection and
/// entity bounds.
#[derive(SystemParam)]
pub struct ViewScene<'w, 's> {
    ray_cast: MeshRayCast<'w, 's>,
    selection: ResMut<'w, Selection>,
    nodes: Query<'w, 's, NodeData>,
    parents: Query<'w, 's, &'static ChildOf>,
    roots: Query<'w, 's, Entity, With<GzWorldRoot>>,
    skip: Query<'w, 's, (), With<NoViewPick>>,
    editing: Option<Res<'w, editing::EditingModel>>,
}

impl ViewScene<'_, '_> {
    /// The model being edited, if one is.
    fn editing(&self) -> Option<Entity> {
        self.editing
            .as_deref()
            .and_then(editing::EditingModel::model)
    }

    /// The nearest visible mesh on the ray: what the camera pivots on and
    /// focus frames, dimmed models too.
    fn hit(&mut self, ray: Ray3d) -> Option<(Entity, Vec3)> {
        let skip = &self.skip;
        let filter = |e: Entity| !skip.contains(e);
        let settings = MeshRayCastSettings::default().with_filter(&filter);
        self.ray_cast
            .cast_ray(ray, &settings)
            .first()
            .map(|(e, hit)| (*e, hit.point))
    }

    /// The nearest visible mesh on the ray that can be picked: in the
    /// model being edited, if one is (the others are see-through).
    fn pick(&mut self, ray: Ray3d) -> Option<(Entity, Vec3)> {
        let skip = &self.skip;
        let parents = &self.parents;
        let editing = self.editing();
        let filter = |e: Entity| {
            !skip.contains(e) && editing.is_none_or(|m| editing::is_under(parents, e, m))
        };
        let settings = MeshRayCastSettings::default().with_filter(&filter);
        self.ray_cast
            .cast_ray(ray, &settings)
            .first()
            .map(|(e, hit)| (*e, hit.point))
    }

    /// The nearest mesh on the ray, else where it meets the ground, within
    /// `far`, the surface's normal there, facing the ray's origin, and the
    /// mesh (none for the ground). While a model is edited, its own meshes
    /// come first: the others are see-through, so a click reaches its
    /// links through them.
    fn surface_hit(&mut self, ray: Ray3d, far: f32) -> Option<(Vec3, Vec3, Option<Entity>)> {
        let skip = &self.skip;
        let parents = &self.parents;
        let editing = self.editing();
        let mut cast = |inside: Option<Entity>| {
            let filter = |e: Entity| {
                !skip.contains(e) && inside.is_none_or(|m| editing::is_under(parents, e, m))
            };
            let settings = MeshRayCastSettings::default().with_filter(&filter);
            self.ray_cast
                .cast_ray(ray, &settings)
                .first()
                .map(|(e, hit)| (*e, hit.point, hit.normal))
        };
        let hit = editing.and_then(|m| cast(Some(m))).or_else(|| cast(None));
        if let Some((e, point, normal)) = hit {
            let n = normal.normalize_or(Vec3::Y);
            let n = if n.dot(*ray.direction) > 0.0 { -n } else { n };
            return Some((point, n, Some(e)));
        }
        ground_point(ray, far).map(|p| (p, Vec3::Y, None))
    }

    /// What ⇧F and ⇧T follow: the first selected entity, else the
    /// top-level model on `ray`.
    fn target(&mut self, ray: Option<Ray3d>) -> Option<Entity> {
        if let Some(e) = self.selection.first_target() {
            return Some(e);
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
    /// out, with `skip.planes` plane visuals too (a ground plane would
    /// swamp the world), and with `skip.runaway` meshes lying wholly
    /// [`RUNAWAY_DISTANCE`] from the world origin: a box falling with no
    /// floor under it, or a link that fell off its model, would otherwise
    /// have focus frame it, the zoom-out limit follow it and the shadow
    /// cascades span it.
    fn bounds_of(&self, entity: Entity, skip: Skip) -> Option<Bounds> {
        let (aabb, global, children, visual, visibility) = self.nodes.get(entity).ok()?;
        if visibility == Some(&Visibility::Hidden)
            || skip.planes
                && visual.is_some_and(|v| matches!(v.0.geometry, GeometryDesc::Plane { .. }))
        {
            return None;
        }
        let mut out = aabb.and_then(|aabb| {
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
                .filter(|b| !skip.runaway || b.distance_to(Vec3::ZERO) <= RUNAWAY_DISTANCE)
        });
        for child in children.into_iter().flatten() {
            if let Some(b) = self.bounds_of(*child, skip) {
                out = Some(out.map_or(b, |o| o.union(b)));
            }
        }
        out
    }

    /// The selection's bounds; an entity with no meshes (a light, say)
    /// counts as the point it is at, and a joint or sensor as its model or
    /// link.
    fn selection_bounds(&self) -> Option<Bounds> {
        self.selection
            .entities
            .iter()
            .chain(self.selection.parts.iter().map(|p| &p.owner))
            .filter_map(|e| {
                self.bounds_of(*e, Skip::NOTHING).or_else(|| {
                    let (_, global, ..) = self.nodes.get(*e).ok()?;
                    Some(Bounds::point(global.translation()))
                })
            })
            .reduce(Bounds::union)
    }

    /// Every mesh under the world root but those that have run away,
    /// ground planes left out unless there is nothing else.
    fn world_bounds(&self) -> Option<Bounds> {
        let root = self.roots.iter().next()?;
        self.bounds_of(root, Skip::RUNAWAY_AND_PLANES)
            .or_else(|| self.bounds_of(root, Skip::RUNAWAY))
    }

    /// Everything a view must keep in range: the world with its ground
    /// planes but not what has run away, and the grid.
    fn extent(&self) -> Option<Bounds> {
        let root = self.roots.iter().next()?;
        let grid = Bounds {
            min: Vec3::new(-GRID_HALF, 0.0, -GRID_HALF),
            max: Vec3::new(GRID_HALF, 0.0, GRID_HALF),
        };
        Some(
            self.bounds_of(root, Skip::RUNAWAY)
                .map_or(grid, |b| b.union(grid)),
        )
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

/// Link cameras (spec §7.4, DECISIONS.md D-94): a pan, zoom, fly, focus
/// or home in one view moves every other showing view's pivot the same
/// way and scales its reach (distance, or orthographic height) the same,
/// each keeping its own direction and projection. Orbits, snaps and
/// projection changes stay with their view.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LinkedViews(pub bool);

/// How one view's move carries to the views linked to it.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Link {
    shift: Vec3,
    scale: f32,
}

impl Link {
    /// From the view's pivot and reach before and after.
    fn between(pivot: (Vec3, Vec3), reach: (f32, f32)) -> Self {
        let scale = if reach.0 > f32::EPSILON && reach.1.is_finite() {
            reach.1 / reach.0
        } else {
            1.0
        };
        Self {
            shift: pivot.1 - pivot.0,
            scale,
        }
    }

    /// Moves a linked view: its pivot and camera by the shift, then its
    /// reach by the scale about the pivot. A view animating towards a pose
    /// of its own (a snap) has that pose moved too, so the animation keeps
    /// the carry; one animating as a linked move itself (a focus every
    /// view got) is left to land where it was sent.
    fn carry(self, view: &mut ViewCamera, t: &mut Transform, p: &mut Projection) {
        if let Some(anim) = view.animation.as_mut() {
            if anim.link {
                return;
            }
            for pose in [&mut anim.from, &mut anim.to] {
                pose.pivot += self.shift;
                pose.offset *= self.scale;
                pose.ortho_height *= self.scale;
            }
        }
        t.translation += self.shift;
        view.pivot += self.shift;
        if (self.scale - 1.0).abs() > f32::EPSILON {
            match ortho_height(p) {
                Some(h) => set_ortho_height(p, (h * self.scale).max(MIN_ORTHO_HEIGHT)),
                None => {
                    let d = t.translation - view.pivot;
                    if d.length() * self.scale >= MIN_DISTANCE {
                        t.translation = view.pivot + d * self.scale;
                    }
                }
            }
        }
    }
}

/// [`reach`] for a camera at `at` (orthographic `height`, if it is) about
/// `pivot`.
fn reach_from(at: Vec3, height: Option<f32>, half_tan: f32, pivot: Vec3) -> f32 {
    match height {
        Some(h) => h / (2.0 * half_tan),
        None => (at - pivot).length(),
    }
}

/// The views [`apply_view_commands`] moves.
type ViewMoves<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static mut ViewCamera,
        &'static mut Transform,
        &'static mut Projection,
        &'static Camera,
        Option<&'static ViewId>,
    ),
>;

/// Applies [`ViewCommand`]s, then advances animations, carries linked
/// moves to the other views, and publishes the first view's pivot as
/// [`GzCameraTarget`]. Idle frames change nothing.
#[allow(clippy::too_many_arguments)]
pub fn apply_view_commands(
    mut commands: MessageReader<ViewCommand>,
    mut views: ViewMoves,
    mut scene: ViewScene,
    mut placed: MessageWriter<place::PlaceAt>,
    mut contexts: MessageWriter<EntityContext>,
    mut edits: MessageWriter<editing::EditCommand>,
    time: Res<Time<Real>>,
    linked: Option<Res<LinkedViews>>,
    mut target: ResMut<GzCameraTarget>,
) {
    let linked = linked.is_some_and(|l| l.0);
    let busy = !commands.is_empty() || views.iter().any(|(_, v, ..)| v.animating());
    if busy {
        let world = scene.extent();
        let frame = |camera: &Camera| Frame {
            size: camera.logical_viewport_size().unwrap_or(Vec2::ZERO),
            world,
        };
        let mut links: Vec<(Entity, Link)> = Vec::new();
        for cmd in commands.read() {
            let Ok((_, mut view, mut t, mut p, camera, _)) = views.get_mut(cmd.view) else {
                continue;
            };
            let f = frame(camera);
            if let ViewOp::Place { at, keep } = cmd.op {
                if let Some((p, n, hit)) = place::surface_at(&mut scene, (&t, &p), f.size, at) {
                    placed.write(place::PlaceAt {
                        point: place::to_gazebo(p),
                        normal: place::to_gazebo(n),
                        keep,
                        hit,
                    });
                }
                continue;
            }
            if let ViewOp::EditAt { at } = cmd.op {
                // In a model already, the pick is a link of it.
                if scene.editing().is_none() {
                    if let Some(e) = select::pick_at(&mut scene, (&t, &p), f.size, at) {
                        edits.write(editing::EditCommand::Enter(e));
                    }
                }
                continue;
            }
            if let ViewOp::Context { at } = cmd.op {
                if let Some(e) = select::pick_at(&mut scene, (&t, &p), f.size, at) {
                    if !scene.selection.entities.contains(&e) {
                        select(&mut scene.selection.reborrow(), SelectMode::Replace, &[e]);
                    }
                    contexts.write(EntityContext {
                        entity: e,
                        view: Some(cmd.view),
                    });
                }
                continue;
            }
            let carried = matches!(
                cmd.op,
                ViewOp::Pan(_) | ViewOp::Fly { .. } | ViewOp::Zoom(_) | ViewOp::ZoomAt { .. }
            );
            // These land a running animation first (`apply`): measure from
            // where it lands, so its jump isn't carried.
            if linked && carried {
                finish(&mut view, &mut t, &mut p);
            }
            let before = (view.pivot, t.translation, ortho_height(&p));
            apply(cmd.op, &mut view, &mut t, &mut p, &f, &mut scene);
            fit_clip(&mut view, &mut t, &mut p, &f);
            if !linked {
                continue;
            }
            match cmd.op {
                ViewOp::Pan(_) | ViewOp::Fly { .. } => {
                    let r0 = reach_from(before.1, before.2, view.half_tan(), before.0);
                    links.push((
                        cmd.view,
                        Link::between((before.0, view.pivot), (r0, reach(&view, &t, &p))),
                    ));
                }
                // A zoom about a point (the wheel's): the pivot moving to
                // the point is no move of the view, so only the zoom
                // carries, measured about the new pivot.
                ViewOp::Zoom(_) | ViewOp::ZoomAt { .. } => {
                    let r0 = reach_from(before.1, before.2, view.half_tan(), view.pivot);
                    let r1 = reach(&view, &t, &p);
                    links.push((cmd.view, Link::between((Vec3::ZERO, Vec3::ZERO), (r0, r1))));
                }
                ViewOp::Focus { .. } | ViewOp::Home => {
                    if let Some(a) = view.animation.as_mut() {
                        a.link = true;
                    }
                }
                _ => {}
            }
        }
        let dt = time.delta_secs();
        for (e, mut view, mut t, mut p, camera, _) in &mut views {
            if view.animating() {
                let carries = view.animation.as_ref().is_some_and(|a| a.link) && linked;
                let before = (view.pivot, reach(&view, &t, &p));
                animate(&mut view, &mut t, &mut p, dt);
                fit_clip(&mut view, &mut t, &mut p, &frame(camera));
                if carries {
                    let after = (view.pivot, reach(&view, &t, &p));
                    links.push((e, Link::between((before.0, after.0), (before.1, after.1))));
                }
            }
        }
        for (lead, link) in links {
            for (e, mut view, mut t, mut p, camera, _) in &mut views {
                if e == lead || !camera.is_active {
                    continue;
                }
                link.carry(&mut view, &mut t, &mut p);
                fit_clip(&mut view, &mut t, &mut p, &frame(camera));
            }
        }
    }
    if let Some((_, view, ..)) = views.iter().min_by_key(|v| v.5.copied()) {
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
            | ViewOp::Select { .. }
            | ViewOp::SelectRect { .. }
            | ViewOp::Context { .. }
    ) {
        finish(view, t, p);
    }
    match op {
        ViewOp::Select { at, mode } => {
            let picked = select::pick_at(scene, (t, p), f.size, at);
            select(&mut scene.selection.reborrow(), mode, picked.as_slice());
        }
        // Placed, and the context menu asked for, by `apply_view_commands`,
        // which writes the result.
        ViewOp::Place { .. } | ViewOp::Context { .. } | ViewOp::EditAt { .. } => {}
        ViewOp::SelectRect { from, to, mode } => {
            let picked = scene.models_in_rect((t, p), f.size, from, to);
            select(&mut scene.selection.reborrow(), mode, &picked);
        }
        ViewOp::SetDragAction(action) => view.drag_action = action,
        ViewOp::Restore(pose) => restore(view, t, p, pose),
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
            // ground planes (a ground model has nothing else) or any part
            // of it that has run away, else the world.
            let bounds = scene
                .selection_bounds()
                .or_else(|| {
                    let hit = at.and_then(|at| ray(p, t, at)).and_then(|r| scene.hit(r));
                    hit.and_then(|(e, _)| {
                        scene.bounds_of(scene.model_of(e), Skip::RUNAWAY_AND_PLANES)
                    })
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
/// plane is just behind the world. The world it backs off from reaches at
/// most [`MAX_ORTHO_EXTENT`] from the pivot, so a model that has fallen
/// away doesn't send the camera so far out that `f32` can't place
/// anything. A perspective far plane grows past
/// [`ViewCamera::perspective`]'s to the world's far side.
fn fit_clip(view: &mut ViewCamera, t: &mut Transform, p: &mut Projection, f: &Frame) {
    let floor = view.perspective.far;
    match p {
        Projection::Orthographic(_) => {
            let extent = f.extent_from(view.pivot).min(MAX_ORTHO_EXTENT);
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
        link: false,
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
