//! View tests: every [`ViewOp`] on its own, then the minimal mode's mouse
//! and keys end to end through [`ViewerControls`] in a headless app driven
//! by synthetic input messages.

use super::controls::{ViewerControls, TAP_SECS, WHEEL_PX_PER_STEP, ZOOM_DRAG_PX};
use super::*;
use crate::camera::perspective as gz_perspective;
use crate::connection::{GzCommand, GzStatus, SimStats};
use crate::render::GzGrid;
use crate::UiMode;
use bevy::camera::visibility::SetViewVisibility;
use bevy::camera::RenderTargetInfo;
use bevy::ecs::system::RunSystemOnce;
use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};
use bevy::input::mouse::{MouseButtonInput, MouseScrollUnit, MouseWheel};
use bevy::input::ButtonState;
use bevy::time::TimeUpdateStrategy;
use bevy::window::WindowResolution;
use gz_core::scene::{MaterialDesc, VisualDesc};
use std::time::Duration;

const W: f32 = 800.0;
const H: f32 = 600.0;
const CENTER: Vec2 = Vec2::new(W / 2.0, H / 2.0);
/// Each `update` advances real time by this much.
const FRAME_MS: u64 = 10;

/// GzCommands sent so far.
#[derive(Resource, Default)]
struct Sent(Vec<GzCommand>);

fn collect(mut reader: MessageReader<GzCommand>, mut sent: ResMut<Sent>) {
    sent.0.extend(reader.read().cloned());
}

struct Rig {
    app: App,
    window: Entity,
    camera: Entity,
}

/// gzweb's default view: gz (-6, 0, 6) looking at the origin.
fn default_view() -> Transform {
    home_transform()
}

fn rig(view: Transform) -> Rig {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::input::InputPlugin, TransformPlugin))
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            FRAME_MS,
        )))
        .insert_resource(Assets::<Mesh>::default())
        .insert_resource(GzStatus::new(""))
        .init_resource::<Sent>()
        .add_plugins(ViewerControls {
            mode: UiMode::Minimal,
        })
        .add_systems(Last, collect);
    let window = app
        .world_mut()
        .spawn(Window {
            resolution: WindowResolution::new(W as u32, H as u32),
            ..default()
        })
        .id();
    // What the render camera system would fill in.
    let mut camera = Camera::default();
    camera.computed.target_info = Some(RenderTargetInfo {
        physical_size: UVec2::new(W as u32, H as u32),
        scale_factor: 1.0,
    });
    let camera = app
        .world_mut()
        .spawn((
            camera,
            Projection::Perspective(gz_perspective()),
            view,
            ViewCamera::new(Vec3::ZERO, gz_perspective()),
        ))
        .id();
    app.update();
    Rig {
        app,
        window,
        camera,
    }
}

impl Rig {
    fn op(&mut self, op: ViewOp) {
        let view = self.camera;
        self.app.world_mut().write_message(ViewCommand { view, op });
        self.app.update();
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.app.update();
        }
    }

    /// Long enough for any animation to finish.
    fn settle(&mut self) {
        self.frames((ANIMATION_SECS * 1000.0) as usize / FRAME_MS as usize + 2);
    }

    fn transform(&self) -> Transform {
        *self.app.world().get::<Transform>(self.camera).unwrap()
    }

    fn view(&self) -> ViewCamera {
        self.app
            .world()
            .get::<ViewCamera>(self.camera)
            .unwrap()
            .clone()
    }

    fn view_mut(&mut self) -> Mut<'_, ViewCamera> {
        self.app
            .world_mut()
            .get_mut::<ViewCamera>(self.camera)
            .unwrap()
    }

    fn projection(&self) -> Projection {
        self.app
            .world()
            .get::<Projection>(self.camera)
            .unwrap()
            .clone()
    }

    fn pivot(&self) -> Vec3 {
        self.view().pivot
    }

    fn ortho(&self) -> Option<f32> {
        ortho_height(&self.projection())
    }

    fn ray(&self, at: Vec2) -> Ray3d {
        viewport_ray(&self.projection(), &self.transform(), Vec2::new(W, H), at).unwrap()
    }

    /// Where world point `p` lands in the viewport.
    fn screen(&self, p: Vec3) -> Vec2 {
        let mut proj = self.projection();
        proj.update(W, H);
        let t = self.transform();
        let view = t.compute_affine().inverse().transform_point3(p);
        let ndc = proj.get_clip_from_view().project_point3(view);
        Vec2::new((ndc.x + 1.0) / 2.0 * W, (1.0 - ndc.y) / 2.0 * H)
    }

    fn sent(&mut self) -> Vec<GzCommand> {
        std::mem::take(&mut self.app.world_mut().resource_mut::<Sent>().0)
    }

    fn set_sim(&mut self, paused: Option<bool>) {
        self.app.world_mut().resource_mut::<GzStatus>().sim = paused.map(|paused| SimStats {
            paused,
            sim_time: 0.0,
            real_time_factor: 1.0,
            iterations: 0,
        });
    }

    // Synthetic input: each call queues messages for the next frame.

    fn cursor(&mut self, at: Option<Vec2>) {
        self.app
            .world_mut()
            .get_mut::<Window>(self.window)
            .unwrap()
            .set_cursor_position(at);
    }

    fn key(&mut self, key_code: KeyCode, state: ButtonState) {
        let window = self.window;
        self.app.world_mut().write_message(KeyboardInput {
            key_code,
            logical_key: Key::Unidentified(NativeKey::Unidentified),
            state,
            text: None,
            repeat: false,
            window,
        });
    }

    fn button(&mut self, button: MouseButton, state: ButtonState) {
        let window = self.window;
        self.app.world_mut().write_message(MouseButtonInput {
            button,
            state,
            window,
        });
    }

    fn wheel(&mut self, y: f32, unit: MouseScrollUnit) {
        let window = self.window;
        self.app.world_mut().write_message(MouseWheel {
            unit,
            x: 0.0,
            y,
            window,
            phase: bevy::input::touch::TouchPhase::Moved,
        });
    }

    /// Presses `keys` (modifiers first) in one frame, releases them in the
    /// next.
    fn chord(&mut self, keys: &[KeyCode]) {
        for k in keys {
            self.key(*k, ButtonState::Pressed);
        }
        self.app.update();
        for k in keys.iter().rev() {
            self.key(*k, ButtonState::Released);
        }
        self.app.update();
    }

    /// A drag with `button` from `from` through `to`, one frame per point.
    fn drag(&mut self, button: MouseButton, from: Vec2, to: &[Vec2]) {
        self.cursor(Some(from));
        self.button(button, ButtonState::Pressed);
        self.app.update();
        for p in to {
            self.cursor(Some(*p));
            self.app.update();
        }
        self.button(button, ButtonState::Released);
        self.app.update();
    }

    /// Adds a cube mesh of half size `half` at `at` under `parent`, visible
    /// to ray casts.
    fn cube(&mut self, parent: Option<Entity>, at: Vec3, half: f32) -> Entity {
        let world = self.app.world_mut();
        let mesh = world.resource_mut::<Assets<Mesh>>().add(Cuboid::new(
            2.0 * half,
            2.0 * half,
            2.0 * half,
        ));
        let mut e = world.spawn((
            Mesh3d(mesh),
            Transform::from_translation(at),
            Aabb::from_min_max(Vec3::splat(-half), Vec3::splat(half)),
            InheritedVisibility::VISIBLE,
            ViewVisibility::default(),
        ));
        if let Some(p) = parent {
            e.insert(ChildOf(p));
        }
        let id = e.id();
        self.make_visible();
        id
    }

    fn make_visible(&mut self) {
        self.app
            .world_mut()
            .run_system_once(|mut q: Query<&mut ViewVisibility>| {
                for mut v in &mut q {
                    v.set_visible();
                }
            })
            .unwrap();
        self.app.update();
    }

    /// A Gazebo world root with one model, returning (root, model).
    fn world_root(&mut self) -> (Entity, Entity) {
        let world = self.app.world_mut();
        let root = world
            .spawn((
                GzWorldRoot,
                Transform::from_rotation(gz_to_bevy()),
                Visibility::default(),
            ))
            .id();
        let model = world
            .spawn((Transform::default(), Visibility::default(), ChildOf(root)))
            .id();
        (root, model)
    }
}

fn plane_visual() -> GzVisual {
    GzVisual(VisualDesc {
        proto_type: "plane".into(),
        geometry: GeometryDesc::Plane {
            normal: [0.0, 0.0, 1.0],
            size: [100.0, 100.0],
        },
        material: MaterialDesc::absent(),
        cast_shadows: false,
        transparency: 0.0,
    })
}

fn assert_near(a: Vec3, b: Vec3, eps: f32) {
    assert!((a - b).length() < eps, "{a} vs {b}");
}

fn assert_near2(a: Vec2, b: Vec2, eps: f32) {
    assert!((a - b).length() < eps, "{a} vs {b}");
}

/// The pivot's direction in the camera's own frame: what fixes its
/// position on screen in perspective.
fn view_dir(t: &Transform, pivot: Vec3) -> Vec3 {
    (t.rotation.inverse() * (pivot - t.translation)).normalize()
}

fn polar(t: &Transform, pivot: Vec3) -> f32 {
    let o = t.translation - pivot;
    (o.y / o.length()).clamp(-1.0, 1.0).acos()
}

fn half_tan() -> f32 {
    30f32.to_radians().tan()
}

/// Distance from the default view to the origin.
fn home_distance() -> f32 {
    HOME_GZ.length()
}

fn horizon() -> Transform {
    Transform::from_xyz(0.0, 1.0, 0.0).looking_at(Vec3::new(0.0, 1.0, -10.0), Vec3::Y)
}

// ---------------------------------------------------------------- ViewOps

#[test]
fn pivot_at_picks_the_point_under_the_cursor_without_moving_the_camera() {
    let mut r = rig(default_view());
    let before = r.transform();
    let cursor = Vec2::new(520.0, 380.0);
    r.op(ViewOp::PivotAt(cursor));
    assert_eq!(r.transform(), before);
    let pivot = r.pivot();
    assert!(pivot.y.abs() < 1e-4, "on the ground: {pivot}");
    assert_near(
        (pivot - before.translation).normalize(),
        *r.ray(cursor).direction,
        1e-4,
    );
    // Published in Gazebo coordinates.
    let gz = gz_to_bevy().inverse() * pivot;
    let published = r.app.world().resource::<GzCameraTarget>().0;
    assert_near(Vec3::from_array(published), gz, 1e-5);
}

#[test]
fn a_pivot_on_nothing_falls_back_to_the_selection_then_the_origin() {
    // The top of the screen sees only sky.
    let mut r = rig(horizon());
    r.view_mut().pivot = Vec3::new(5.0, 5.0, 5.0);
    r.op(ViewOp::PivotAt(Vec2::new(400.0, 10.0)));
    assert_eq!(r.pivot(), Vec3::ZERO);
    assert_eq!(r.transform(), horizon());

    // Behind the camera, out of the ray's way.
    let selected = r.cube(None, Vec3::new(3.0, 2.0, 8.0), 0.5);
    r.app.world_mut().resource_mut::<Selection>().entities = vec![selected];
    r.op(ViewOp::PivotAt(Vec2::new(400.0, 10.0)));
    assert_near(r.pivot(), Vec3::new(3.0, 2.0, 8.0), 1e-4);
}

#[test]
fn meshes_are_picked_before_the_ground() {
    let mut r = rig(default_view());
    r.cube(None, Vec3::new(0.0, 1.0, 0.0), 1.0);
    r.op(ViewOp::PivotAt(CENTER));
    // The centre ray from (-6, 6, 0) towards the origin meets the cube's
    // -X face at (-1, 1, 0).
    assert_near(r.pivot(), Vec3::new(-1.0, 1.0, 0.0), 1e-3);
}

#[test]
fn orbit_rotates_about_the_pivot_keeping_it_fixed_on_screen() {
    let mut r = rig(default_view());
    r.op(ViewOp::PivotAt(Vec2::new(520.0, 380.0)));
    let pivot = r.pivot();
    let before = r.transform();
    r.op(ViewOp::Orbit(Vec2::new(40.0, 25.0)));
    let after = r.transform();
    assert!((after.translation - before.translation).length() > 0.1);
    assert!(
        ((after.translation - pivot).length() - (before.translation - pivot).length()).abs() < 1e-4
    );
    assert_near(view_dir(&after, pivot), view_dir(&before, pivot), 1e-4);
    assert_eq!(r.pivot(), pivot);

    // Horizontal moves only yaw: the height above the pivot is kept.
    let h = r.transform().translation.y - pivot.y;
    r.op(ViewOp::Orbit(Vec2::new(60.0, 0.0)));
    assert!((r.transform().translation.y - pivot.y - h).abs() < 1e-4);
}

#[test]
fn orbiting_stops_at_the_poles_instead_of_flipping() {
    // 300 px is a half turn (2π per viewport height), past either pole.
    let mut r = rig(default_view());
    r.op(ViewOp::PivotAt(Vec2::new(400.0, 100.0)));
    let pivot = r.pivot();
    r.op(ViewOp::Orbit(Vec2::new(0.0, 300.0)));
    assert!(polar(&r.transform(), pivot) < 1e-2);
    let top = r.transform();
    r.op(ViewOp::Orbit(Vec2::new(0.0, 190.0)));
    assert_near(r.transform().translation, top.translation, 1e-3);
    r.op(ViewOp::Orbit(Vec2::new(0.0, -580.0)));
    assert!(polar(&r.transform(), pivot) > PI - 1e-2);
    assert_near(view_dir(&r.transform(), pivot), view_dir(&top, pivot), 1e-3);
}

#[test]
fn pan_moves_camera_and_pivot_together() {
    let mut r = rig(default_view());
    r.op(ViewOp::PivotAt(CENTER));
    let pivot = r.pivot();
    let before = r.transform();
    r.op(ViewOp::Pan(Vec2::new(60.0, -30.0)));
    let after = r.transform();
    let d = (before.translation - pivot).length() * half_tan();
    let right = before.rotation * Vec3::X;
    let up = before.rotation * Vec3::Y;
    let want = -right * (2.0 * 60.0 * d / H) + up * (2.0 * -30.0 * d / H);
    assert_near(after.translation - before.translation, want, 1e-4);
    assert_near(r.pivot() - pivot, want, 1e-4);
    assert_eq!(after.rotation, before.rotation);
}

#[test]
fn orthographic_pan_follows_the_view_height() {
    let mut r = rig(default_view());
    r.op(ViewOp::PivotAt(CENTER));
    r.op(ViewOp::SetProjection(ProjectionKind::Orthographic));
    let h = r.ortho().unwrap();
    let before = r.transform();
    r.op(ViewOp::Pan(Vec2::new(60.0, 0.0)));
    let moved = (r.transform().translation - before.translation).length();
    assert!((moved - 60.0 * h / H).abs() < 1e-4);
    assert_eq!(r.ortho(), Some(h));
}

#[test]
fn zoom_dollies_along_the_pivot_line() {
    let mut r = rig(default_view());
    let d0 = home_distance();
    r.op(ViewOp::Zoom(1.0));
    let t = r.transform();
    assert!(((t.translation - r.pivot()).length() - ZOOM_STEP * d0).abs() < 1e-4);
    assert_eq!(t.rotation, default_view().rotation);
    r.op(ViewOp::Zoom(-2.0));
    let d = (r.transform().translation - r.pivot()).length();
    assert!((d - d0 / ZOOM_STEP).abs() < 1e-3, "{d}");
}

#[test]
fn zoom_at_dollies_toward_the_point_under_the_cursor() {
    let mut r = rig(default_view());
    let cursor = Vec2::new(300.0, 350.0);
    r.op(ViewOp::ZoomAt {
        at: cursor,
        steps: 1.0,
    });
    let pivot = r.pivot();
    assert!(pivot.y.abs() < 1e-4);
    let start = default_view();
    let d0 = (start.translation - pivot).length();
    let t = r.transform();
    assert!(((t.translation - pivot).length() - ZOOM_STEP * d0).abs() < 1e-3);
    assert_near(
        (t.translation - pivot).normalize(),
        (start.translation - pivot).normalize(),
        1e-4,
    );
    assert_eq!(t.rotation, start.rotation);
    // The point stays under the cursor.
    assert_near2(r.screen(pivot), cursor, 1e-2);
}

#[test]
fn orthographic_zoom_scales_the_view_about_the_pivot() {
    let mut r = rig(default_view());
    let cursor = Vec2::new(300.0, 350.0);
    r.op(ViewOp::SetProjection(ProjectionKind::Orthographic));
    let h = r.ortho().unwrap();
    r.op(ViewOp::ZoomAt {
        at: cursor,
        steps: 2.0,
    });
    assert!((r.ortho().unwrap() - h * ZOOM_STEP * ZOOM_STEP).abs() < 1e-4);
    // The point under the cursor stays there.
    assert_near2(r.screen(r.pivot()), cursor, 1e-2);
    let pivot = r.pivot();
    r.op(ViewOp::Zoom(-3.0));
    assert_near2(r.screen(pivot), cursor, 1e-2);
    // It never collapses.
    r.op(ViewOp::Zoom(1e6));
    assert!(r.ortho().unwrap() >= 1e-3);
}

#[test]
fn projection_toggle_keeps_the_framing() {
    let mut r = rig(default_view());
    let at = Vec2::new(520.0, 380.0);
    r.op(ViewOp::PivotAt(at));
    let pivot = r.pivot();
    let before = r.transform();
    // A point beside the pivot, in the plane facing the camera.
    let beside = pivot + before.rotation * Vec3::X;
    let span = r.screen(beside) - r.screen(pivot);

    r.op(ViewOp::ToggleProjection);
    let depth = -(before.rotation.inverse() * (pivot - before.translation)).z;
    assert!((r.ortho().unwrap() - 2.0 * depth * half_tan()).abs() < 1e-4);
    assert_near2(r.screen(pivot), at, 1e-2);
    assert_near2(r.screen(beside) - r.screen(pivot), span, 1e-2);
    // Backed off so the near plane doesn't clip the scene.
    let off = r.transform().rotation.inverse() * (pivot - r.transform().translation);
    assert!((off.z + ORTHO_STANDOFF).abs() < 1e-3);
    assert!((reach(&r.view(), &r.transform(), &r.projection()) - depth).abs() < 1e-3);

    // Setting the current projection changes nothing.
    let ortho = r.transform();
    r.op(ViewOp::SetProjection(ProjectionKind::Orthographic));
    assert_eq!(r.transform(), ortho);

    r.op(ViewOp::ToggleProjection);
    assert!(matches!(r.projection(), Projection::Perspective(_)));
    assert_near(r.transform().translation, before.translation, 1e-3);
    r.op(ViewOp::SetProjection(ProjectionKind::Perspective));
    assert_near(r.transform().translation, before.translation, 1e-3);
}

#[test]
fn a_projection_switch_resets_temporal_anti_aliasing() {
    use bevy::anti_alias::taa::TemporalAntiAliasing;
    let mut r = rig(default_view());
    let reset = |r: &Rig| {
        r.app
            .world()
            .get::<TemporalAntiAliasing>(r.camera)
            .unwrap()
            .reset
    };
    let clear = |r: &mut Rig| {
        r.app
            .world_mut()
            .entity_mut(r.camera)
            .insert(TemporalAntiAliasing { reset: false });
    };
    clear(&mut r);
    r.op(ViewOp::Orbit(Vec2::new(5.0, 0.0)));
    assert!(!reset(&r));
    r.op(ViewOp::ToggleProjection);
    assert!(reset(&r));
    clear(&mut r);
    r.op(ViewOp::Pan(Vec2::new(5.0, 0.0)));
    assert!(!reset(&r));
    // A snap from perspective switches at the end of its animation.
    r.op(ViewOp::ToggleProjection);
    clear(&mut r);
    r.op(ViewOp::Snap(SnapView::Top));
    assert!(!reset(&r));
    r.settle();
    assert!(reset(&r));
}

#[test]
fn each_snap_animates_into_its_orthographic_axis_view() {
    for face in SnapView::ALL {
        let mut r = rig(default_view());
        r.op(ViewOp::Snap(face));
        // Under way, still in perspective.
        assert!(r.view().animating(), "{face:?}");
        assert_eq!(r.view().snapped, None);
        assert!(r.ortho().is_none());
        let (dir, up) = face.gz_axes();
        let start_angle = r.transform().rotation.angle_between(face.rotation());
        r.frames(10);
        let mid_angle = r.transform().rotation.angle_between(face.rotation());
        assert!(mid_angle > 0.0 && mid_angle < start_angle, "{face:?}");

        r.settle();
        let view = r.view();
        let t = r.transform();
        assert!(!view.animating());
        assert_eq!(view.snapped, Some(face));
        let to_gz = gz_to_bevy().inverse();
        assert_near(to_gz * *t.forward(), dir, 1e-4);
        assert_near(to_gz * *t.up(), up, 1e-4);
        // About the point at the view centre (the origin here), with the
        // same framing: the distance became the orthographic height.
        assert_near(r.pivot(), Vec3::ZERO, 1e-3);
        let h = r.ortho().expect("orthographic at the end");
        assert!(
            (h - 2.0 * home_distance() * half_tan()).abs() < 1e-3,
            "{face:?} {h}"
        );
        assert_near2(r.screen(r.pivot()), CENTER, 1e-2);

        // Orbiting away returns to perspective at the same distance.
        r.op(ViewOp::Orbit(Vec2::new(10.0, 0.0)));
        assert_eq!(r.view().snapped, None);
        assert!(r.ortho().is_none());
        let d = (r.transform().translation - r.pivot()).length();
        assert!((d - home_distance()).abs() < 1e-3, "{face:?} {d}");
    }
}

#[test]
fn snaps_centre_on_the_selection_and_keep_an_orthographic_height() {
    let mut r = rig(default_view());
    let cube = r.cube(None, Vec3::new(2.0, 0.5, -1.0), 0.5);
    r.app.world_mut().resource_mut::<Selection>().entities = vec![cube];
    r.op(ViewOp::SetProjection(ProjectionKind::Orthographic));
    let h = r.ortho().unwrap();
    r.op(ViewOp::Snap(SnapView::Top));
    // Already orthographic: it animates in orthographic.
    assert!(r.ortho().is_some());
    r.settle();
    assert_near(r.pivot(), Vec3::new(2.0, 0.5, -1.0), 1e-4);
    assert!((r.ortho().unwrap() - h).abs() < 1e-4);
    assert_eq!(r.view().snapped, Some(SnapView::Top));
    // Pan and zoom keep the snap; the top view's screen up is gz +X.
    r.op(ViewOp::Pan(Vec2::new(0.0, 10.0)));
    r.op(ViewOp::Zoom(1.0));
    assert_eq!(r.view().snapped, Some(SnapView::Top));
    assert_near(gz_to_bevy().inverse() * *r.transform().up(), Vec3::X, 1e-4);
}

#[test]
fn a_snap_looking_at_the_sky_keeps_the_pivot() {
    let mut r = rig(horizon());
    r.view_mut().pivot = Vec3::new(0.0, 1.0, -4.0);
    r.op(ViewOp::Snap(SnapView::Front));
    r.settle();
    assert_near(r.pivot(), Vec3::new(0.0, 1.0, -4.0), 1e-4);
}

#[test]
fn other_ops_stop_an_animation_where_it_is() {
    let mut r = rig(default_view());
    r.op(ViewOp::Snap(SnapView::Top));
    r.frames(5);
    // Changing the drag action doesn't interrupt it.
    r.op(ViewOp::SetDragAction(DragAction::Pan));
    assert!(r.view().animating());
    assert_eq!(r.view().drag_action, DragAction::Pan);
    r.op(ViewOp::Pan(Vec2::new(5.0, 0.0)));
    assert!(!r.view().animating());
    let stopped = r.transform();
    r.settle();
    assert_eq!(r.transform(), stopped);
    assert_eq!(r.view().snapped, None);
    assert!(r.ortho().is_none());
}

#[test]
fn focus_frames_the_model_under_the_cursor() {
    let mut r = rig(default_view());
    let (_, model) = r.world_root();
    // A 1 m cube in gz at (0, 0, 0.5): Bevy (0, 0.5, 0).
    r.cube(Some(model), Vec3::new(0.0, 0.0, 0.5), 0.5);
    let before = r.transform();
    let at = r.screen(Vec3::new(0.0, 0.5, 0.0));
    r.op(ViewOp::Focus { at: Some(at) });
    r.settle();
    assert_near(r.pivot(), Vec3::new(0.0, 0.5, 0.0), 1e-3);
    assert!(r.transform().rotation.angle_between(before.rotation) < 1e-4);
    let radius = 3f32.sqrt() / 2.0 * 1.1;
    let d = radius / 30f32.to_radians().sin();
    assert!(((r.transform().translation - r.pivot()).length() - d).abs() < 1e-3);
    assert_near2(r.screen(r.pivot()), CENTER, 1e-2);
}

#[test]
fn focus_without_a_target_frames_the_world_but_not_its_ground() {
    let mut r = rig(default_view());
    let (root, model) = r.world_root();
    r.cube(Some(model), Vec3::new(4.0, 0.0, 1.0), 1.0);
    let ground = r.cube(Some(root), Vec3::ZERO, 50.0);
    r.app.world_mut().entity_mut(ground).insert(plane_visual());
    r.op(ViewOp::Focus { at: None });
    r.settle();
    // gz (4, 0, 1) is Bevy (4, 1, 0).
    assert_near(r.pivot(), Vec3::new(4.0, 1.0, 0.0), 1e-3);
}

#[test]
fn focus_prefers_the_selection_and_frames_a_ground_only_world() {
    let mut r = rig(default_view());
    let (root, _) = r.world_root();
    let ground = r.cube(Some(root), Vec3::new(1.0, 0.0, 0.0), 10.0);
    r.app.world_mut().entity_mut(ground).insert(plane_visual());
    // Aimed at the sky, so nothing is under the cursor.
    r.op(ViewOp::Focus {
        at: Some(Vec2::new(400.0, 0.0)),
    });
    r.settle();
    // gz (1, 0, 0) is Bevy (1, 0, 0).
    assert_near(r.pivot(), Vec3::new(1.0, 0.0, 0.0), 1e-3);

    let other = r.cube(None, Vec3::new(-3.0, 2.0, 2.0), 0.25);
    r.app.world_mut().resource_mut::<Selection>().entities = vec![other];
    let at = r.screen(Vec3::new(1.0, 0.0, 0.0));
    r.op(ViewOp::Focus { at: Some(at) });
    r.settle();
    assert_near(r.pivot(), Vec3::new(-3.0, 2.0, 2.0), 1e-3);
}

#[test]
fn orthographic_focus_sets_the_height() {
    let mut r = rig(default_view());
    let cube = r.cube(None, Vec3::new(0.0, 1.0, 0.0), 1.0);
    r.app.world_mut().resource_mut::<Selection>().entities = vec![cube];
    r.op(ViewOp::SetProjection(ProjectionKind::Orthographic));
    r.op(ViewOp::Focus { at: None });
    r.settle();
    // Wider than tall: the height fits the diameter.
    let want = 2.0 * 3f32.sqrt() * 1.1;
    assert!((r.ortho().unwrap() - want).abs() < 1e-3);
    assert_near2(r.screen(Vec3::new(0.0, 1.0, 0.0)), CENTER, 1e-2);
}

#[test]
fn focus_on_an_empty_world_goes_home() {
    let view = Transform::from_xyz(3.0, 2.0, 1.0).looking_at(Vec3::new(5.0, 0.0, 0.0), Vec3::Y);
    let mut r = rig(view);
    r.op(ViewOp::Focus { at: None });
    r.settle();
    assert_near(
        r.transform().translation,
        home_transform().translation,
        1e-4,
    );
}

#[test]
fn home_animates_back_to_gzwebs_default_view_in_perspective() {
    let view = Transform::from_xyz(3.0, 2.0, 1.0).looking_at(Vec3::new(5.0, 0.0, 0.0), Vec3::Y);
    let mut r = rig(view);
    r.view_mut().pivot = Vec3::new(5.0, 0.0, 0.0);
    r.op(ViewOp::Snap(SnapView::Left));
    r.settle();
    assert!(r.ortho().is_some());
    r.op(ViewOp::Home);
    assert!(r.ortho().is_none());
    assert_eq!(r.view().snapped, None);
    assert!(r.view().animating());
    r.settle();
    let home = home_transform();
    let t = r.transform();
    assert_near(t.translation, home.translation, 1e-4);
    assert!(t.rotation.angle_between(home.rotation) < 1e-3);
    assert_near(r.pivot(), Vec3::ZERO, 1e-5);
    let published = Vec3::from_array(r.app.world().resource::<GzCameraTarget>().0);
    assert_near(published, Vec3::ZERO, 1e-5);
}

#[test]
fn fly_moves_in_the_ground_plane_at_a_frame_rate_independent_speed() {
    let mut r = rig(default_view());
    let start = r.transform().translation;
    r.op(ViewOp::Fly {
        dir: Vec3::Z,
        secs: 0.1,
    });
    // Looking down at 45° along +X: forward is +X, flattened.
    let step = 0.5 * home_distance() * 0.1;
    assert_near(r.transform().translation - start, Vec3::X * step, 1e-4);
    assert_near(r.pivot(), Vec3::X * step, 1e-4);
    // Two half frames go as far as one whole one.
    for _ in 0..2 {
        r.op(ViewOp::Fly {
            dir: Vec3::Z,
            secs: 0.05,
        });
    }
    assert_near(
        r.transform().translation - start,
        Vec3::X * 2.0 * step,
        1e-4,
    );
    // Right is gz -Y (Bevy +Z); up is straight up.
    let before = r.transform().translation;
    r.op(ViewOp::Fly {
        dir: Vec3::new(1.0, 1.0, 0.0),
        secs: 0.1,
    });
    assert_near(
        r.transform().translation - before,
        Vec3::new(0.0, step, step),
        1e-4,
    );
    // Closer means slower, down to a floor.
    let here = r.transform().translation;
    r.view_mut().pivot = here;
    r.op(ViewOp::Fly {
        dir: Vec3::Y,
        secs: 1.0,
    });
    assert_near(r.transform().translation - here, Vec3::Y * 0.25, 1e-4);
}

#[test]
fn looking_straight_down_fly_forward_is_up_the_screen() {
    let mut r = rig(default_view());
    r.op(ViewOp::Snap(SnapView::Top));
    r.settle();
    let before = r.transform().translation;
    r.op(ViewOp::Fly {
        dir: Vec3::Z,
        secs: 0.1,
    });
    let moved = r.transform().translation - before;
    // Top view: gz +X is up the screen.
    assert_near((gz_to_bevy().inverse() * moved).normalize(), Vec3::X, 1e-4);
    // Orthographic speed follows the framing, not the standoff.
    let step = 0.5 * home_distance() * 0.1;
    assert!((moved.length() - step).abs() < 1e-3);
}

#[test]
fn commands_for_other_entities_and_empty_viewports_are_harmless() {
    let mut r = rig(default_view());
    let stranger = r.app.world_mut().spawn_empty().id();
    r.app.world_mut().write_message(ViewCommand {
        view: stranger,
        op: ViewOp::Home,
    });
    r.app.update();
    assert_eq!(r.transform(), default_view());
    assert!(viewport_ray(&r.projection(), &r.transform(), Vec2::ZERO, CENTER).is_none());
    // A viewport of no size can't pick: the selection/origin fallback.
    r.app
        .world_mut()
        .get_mut::<Camera>(r.camera)
        .unwrap()
        .computed
        .target_info = None;
    r.view_mut().pivot = Vec3::ONE;
    r.op(ViewOp::PivotAt(CENTER));
    assert_eq!(r.pivot(), Vec3::ZERO);
}

#[test]
fn snap_axes_follow_the_gazebo_gui() {
    let to_gz = gz_to_bevy().inverse();
    for face in SnapView::ALL {
        let (dir, up) = face.gz_axes();
        let rot = face.rotation();
        assert_near(to_gz * (rot * Vec3::NEG_Z), dir, 1e-5);
        assert_near(to_gz * (rot * Vec3::Y), up, 1e-5);
    }
    // Front looks along -X with +Y to the right, as in gz-sim's ViewAngle.
    let right = to_gz * (SnapView::Front.rotation() * Vec3::X);
    assert_near(right, Vec3::Y, 1e-5);
    assert_eq!(FlyDir::Down.vector(), Vec3::NEG_Y);
    assert_eq!(FlyDir::Back.vector(), Vec3::NEG_Z);
}

#[test]
fn bounds_helpers() {
    let b = Bounds::point(Vec3::ONE).union(Bounds::point(Vec3::new(3.0, 1.0, 1.0)));
    assert_eq!(b.center(), Vec3::new(2.0, 1.0, 1.0));
    assert_eq!(b.radius(), 1.0);
    // Orthographic projections with another scaling mode report their area.
    let o = OrthographicProjection {
        area: Rect::new(0.0, 0.0, 4.0, 3.0),
        ..OrthographicProjection::default_3d()
    };
    assert_eq!(ortho_height(&Projection::Orthographic(o)), Some(3.0));
}

// ------------------------------------------- minimal mode, input end to end

#[test]
fn left_drag_runs_the_drag_action() {
    let mut r = rig(default_view());
    let from = Vec2::new(520.0, 380.0);
    r.drag(MouseButton::Left, from, &[Vec2::new(560.0, 405.0)]);
    let pivot = r.pivot();
    assert!(pivot.y.abs() < 1e-4);
    let orbited = r.transform();
    assert_ne!(orbited.rotation, default_view().rotation);
    assert!(
        ((orbited.translation - pivot).length() - (default_view().translation - pivot).length())
            .abs()
            < 1e-3
    );
    // Moving with no button held does nothing.
    r.cursor(Some(Vec2::new(700.0, 500.0)));
    r.frames(2);
    assert_eq!(r.transform(), orbited);

    r.view_mut().drag_action = DragAction::Pan;
    r.drag(MouseButton::Left, CENTER, &[CENTER + Vec2::new(30.0, 0.0)]);
    assert_eq!(r.transform().rotation, orbited.rotation);
    assert_ne!(r.transform().translation, orbited.translation);

    let mut r = rig(default_view());
    r.view_mut().drag_action = DragAction::Zoom;
    r.drag(
        MouseButton::Left,
        CENTER,
        &[CENTER + Vec2::new(0.0, 2.0 * ZOOM_DRAG_PX)],
    );
    let d = (r.transform().translation - r.pivot()).length();
    assert!(
        (d - home_distance() / ZOOM_STEP / ZOOM_STEP).abs() < 1e-3,
        "{d}"
    );
}

#[test]
fn right_drag_pans_and_middle_drag_zooms() {
    let mut r = rig(default_view());
    r.drag(
        MouseButton::Right,
        CENTER,
        &[CENTER + Vec2::new(60.0, -30.0)],
    );
    assert_eq!(r.transform().rotation, default_view().rotation);
    let moved = r.transform().translation - default_view().translation;
    assert!(moved.length() > 0.1);
    assert_near(r.pivot(), moved, 1e-4);

    let mut r = rig(default_view());
    r.drag(
        MouseButton::Middle,
        CENTER,
        &[
            CENTER + Vec2::new(0.0, ZOOM_DRAG_PX),
            // Sideways middle drags don't zoom.
            CENTER + Vec2::new(40.0, ZOOM_DRAG_PX),
        ],
    );
    let d = (r.transform().translation - r.pivot()).length();
    assert!((d - home_distance() / ZOOM_STEP).abs() < 1e-3, "{d}");
}

#[test]
fn the_wheel_zooms_about_the_centre_and_ctrl_wheel_about_the_cursor() {
    let cursor = Vec2::new(300.0, 350.0);
    let mut r = rig(default_view());
    r.cursor(Some(cursor));
    r.wheel(1.0, MouseScrollUnit::Line);
    r.app.update();
    // The view centre sees the origin.
    assert_near(r.pivot(), Vec3::ZERO, 1e-3);
    let d = (r.transform().translation - r.pivot()).length();
    assert!((d - home_distance() * ZOOM_STEP).abs() < 1e-3);

    let mut r = rig(default_view());
    r.cursor(Some(cursor));
    r.key(KeyCode::ControlLeft, ButtonState::Pressed);
    r.wheel(WHEEL_PX_PER_STEP, MouseScrollUnit::Pixel);
    r.app.update();
    let under = r.pivot();
    assert!(under.y.abs() < 1e-4 && under.length() > 0.5, "{under}");
    assert_near2(r.screen(under), cursor, 1e-2);
    let d0 = (default_view().translation - under).length();
    let d = (r.transform().translation - under).length();
    assert!((d - d0 * ZOOM_STEP).abs() < 1e-3);
    // No zoom without a cursor or without a step.
    let t = r.transform();
    r.cursor(None);
    r.wheel(1.0, MouseScrollUnit::Line);
    r.app.update();
    r.cursor(Some(cursor));
    r.wheel(0.0, MouseScrollUnit::Line);
    r.app.update();
    assert_eq!(r.transform(), t);
}

#[test]
fn space_tap_plays_and_pauses_and_space_hold_pans() {
    let mut r = rig(default_view());
    // Without stats it doesn't know which way to toggle.
    r.chord(&[KeyCode::Space]);
    assert!(r.sent().is_empty());

    r.set_sim(Some(false));
    r.chord(&[KeyCode::Space]);
    assert_eq!(r.sent(), [GzCommand::Pause]);
    r.set_sim(Some(true));
    r.chord(&[KeyCode::Space]);
    assert_eq!(r.sent(), [GzCommand::Play]);

    // Held longer than a tap: nothing.
    r.key(KeyCode::Space, ButtonState::Pressed);
    r.frames((TAP_SECS * 1000.0) as usize / FRAME_MS as usize + 1);
    r.key(KeyCode::Space, ButtonState::Released);
    r.app.update();
    assert!(r.sent().is_empty());

    // Space + left drag pans, however quick, and never toggles.
    r.key(KeyCode::Space, ButtonState::Pressed);
    r.drag(MouseButton::Left, CENTER, &[CENTER + Vec2::new(40.0, 0.0)]);
    r.key(KeyCode::Space, ButtonState::Released);
    r.app.update();
    assert!(r.sent().is_empty());
    assert_eq!(r.transform().rotation, default_view().rotation);
    assert_ne!(r.transform().translation, default_view().translation);

    // Shift+Space is not a tap.
    r.chord(&[KeyCode::ShiftLeft, KeyCode::Space]);
    assert!(r.sent().is_empty());
}

#[test]
fn ctrl_right_steps_a_paused_sim() {
    let mut r = rig(default_view());
    r.chord(&[KeyCode::ControlLeft, KeyCode::ArrowRight]);
    r.set_sim(Some(false));
    r.chord(&[KeyCode::ControlLeft, KeyCode::ArrowRight]);
    assert!(r.sent().is_empty());
    r.set_sim(Some(true));
    let t = r.transform();
    r.chord(&[KeyCode::ControlRight, KeyCode::ArrowRight]);
    assert_eq!(r.sent(), [GzCommand::Step(1)]);
    // The arrow didn't fly the camera.
    assert_eq!(r.transform(), t);
    // N no longer steps.
    r.chord(&[KeyCode::KeyN]);
    assert!(r.sent().is_empty());
}

#[test]
fn viewport_keys_drive_the_view_and_the_grid() {
    let mut r = rig(default_view());
    assert!(!r.app.world().resource::<GzGrid>().visible);
    r.chord(&[KeyCode::KeyG]);
    assert!(r.app.world().resource::<GzGrid>().visible);

    r.chord(&[KeyCode::Digit5]);
    assert!(r.ortho().is_some());
    r.chord(&[KeyCode::Digit5]);
    assert!(r.ortho().is_none());

    for (keys, face) in [
        (&[KeyCode::Digit7][..], SnapView::Top),
        (&[KeyCode::ShiftLeft, KeyCode::Digit7], SnapView::Bottom),
        (&[KeyCode::Digit1], SnapView::Front),
        (&[KeyCode::ShiftRight, KeyCode::Digit1], SnapView::Back),
        (&[KeyCode::Numpad3], SnapView::Right),
        (&[KeyCode::ShiftLeft, KeyCode::Digit3], SnapView::Left),
    ] {
        r.chord(keys);
        r.settle();
        assert_eq!(r.view().snapped, Some(face), "{keys:?}");
    }

    r.chord(&[KeyCode::KeyH]);
    r.settle();
    assert_near(
        r.transform().translation,
        home_transform().translation,
        1e-4,
    );
    assert!(r.ortho().is_none());

    // F frames the model under the cursor.
    let (_, model) = r.world_root();
    r.cube(Some(model), Vec3::new(2.0, 0.0, 0.5), 0.5);
    let at = r.screen(Vec3::new(2.0, 0.5, 0.0));
    r.cursor(Some(at));
    r.chord(&[KeyCode::KeyF]);
    r.settle();
    assert_near(r.pivot(), Vec3::new(2.0, 0.5, 0.0), 1e-3);
    assert!(r.sent().is_empty());
}

#[test]
fn fly_keys_move_the_camera_unless_a_command_chord_is_held() {
    let mut r = rig(default_view());
    r.key(KeyCode::KeyW, ButtonState::Pressed);
    r.frames(10);
    let moved = r.transform().translation - default_view().translation;
    // Forward in the ground plane, about 0.1 s at half the pivot distance
    // per second.
    assert!(moved.y.abs() < 1e-5 && moved.z.abs() < 1e-5, "{moved}");
    let per_frame = 0.5 * home_distance() * FRAME_MS as f32 / 1000.0;
    assert!(
        moved.x > 8.5 * per_frame && moved.x < 10.5 * per_frame,
        "{moved}"
    );
    // S cancels W.
    r.key(KeyCode::KeyS, ButtonState::Pressed);
    r.app.update();
    let t = r.transform();
    r.frames(3);
    assert_eq!(r.transform(), t);
    r.key(KeyCode::KeyS, ButtonState::Released);
    r.key(KeyCode::KeyW, ButtonState::Released);
    r.app.update();

    // Ctrl+E is no fly key.
    r.key(KeyCode::ControlLeft, ButtonState::Pressed);
    r.key(KeyCode::KeyE, ButtonState::Pressed);
    r.app.update();
    let t = r.transform();
    r.frames(3);
    assert_eq!(r.transform(), t);
    r.key(KeyCode::ControlLeft, ButtonState::Released);
    r.frames(3);
    assert!(r.transform().translation.y > t.translation.y);
}

#[test]
fn controls_wait_for_a_window_and_a_camera() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::input::InputPlugin))
        .insert_resource(Assets::<Mesh>::default())
        .add_plugins(ViewerControls::default());
    app.update();
    app.world_mut().write_message(MouseWheel {
        unit: MouseScrollUnit::Line,
        x: 0.0,
        y: 1.0,
        window: Entity::PLACEHOLDER,
        phase: bevy::input::touch::TouchPhase::Moved,
    });
    app.update();
    app.world_mut().spawn(Window::default());
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyH);
    app.update();
    assert!(app.world().resource::<Messages<ViewCommand>>().is_empty());
}
