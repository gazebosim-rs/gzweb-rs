//! View tests: every [`ViewOp`] on its own, then the minimal mode's mouse
//! and keys end to end through [`ViewerControls`] in a headless app driven
//! by synthetic input messages.

use super::controls::{ViewerControls, TAP_SECS, WHEEL_PX_PER_STEP, ZOOM_DRAG_PX};
use super::input::{Platform, Scope};
use super::*;
use crate::camera::perspective as gz_perspective;
use crate::connection::{GzCommand, GzStatus, SimStats};
use crate::UiMode;
use bevy::camera::visibility::SetViewVisibility;
use bevy::camera::{RenderTarget, RenderTargetInfo};
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
            real_time: 0.0,
            real_time_factor: 1.0,
            iterations: 0,
            step_size: 0.001,
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
    r.op(ViewOp::PivotAt {
        at: cursor,
        orbit: true,
    });
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

/// A level camera at (0, 1, 5) looking along -Z: the origin is 5 m ahead.
fn horizon_back() -> Transform {
    Transform::from_xyz(0.0, 1.0, 5.0).looking_to(Vec3::NEG_Z, Vec3::Y)
}

#[test]
fn an_orbit_on_nothing_falls_back_to_the_selection_then_the_origin() {
    // The top of the screen sees only sky.
    let sky = Vec2::new(400.0, 10.0);
    let mut r = rig(horizon_back());
    r.view_mut().pivot = Vec3::new(5.0, 5.0, 5.0);
    r.op(ViewOp::PivotAt {
        at: sky,
        orbit: true,
    });
    assert_eq!(r.pivot(), Vec3::ZERO);
    assert_eq!(r.transform(), horizon_back());

    // In front of the camera, out of the ray's way.
    let selected = r.cube(None, Vec3::new(3.0, 2.0, -8.0), 0.5);
    r.app.world_mut().resource_mut::<Selection>().entities = vec![selected];
    r.op(ViewOp::PivotAt {
        at: sky,
        orbit: true,
    });
    assert_near(r.pivot(), Vec3::new(3.0, 2.0, -8.0), 1e-4);
}

/// A fallback behind the camera, or a pan or zoom on nothing, keeps the
/// pivot's depth along the pointer's ray instead.
#[test]
fn a_miss_keeps_the_pivot_depth_on_the_ray() {
    let sky = Vec2::new(400.0, 10.0);
    // The origin and the selection are behind (or level with) the camera.
    let mut r = rig(horizon());
    let behind = r.cube(None, Vec3::new(3.0, 2.0, 8.0), 0.5);
    r.app.world_mut().resource_mut::<Selection>().entities = vec![behind];
    r.view_mut().pivot = Vec3::new(1.0, 1.0, -6.0);
    r.op(ViewOp::PivotAt {
        at: sky,
        orbit: true,
    });
    let pivot = r.pivot();
    assert!((depth_of(&horizon(), pivot) - 6.0).abs() < 1e-4, "{pivot}");
    assert_near(
        (pivot - horizon().translation).normalize(),
        *r.ray(sky).direction,
        1e-4,
    );

    // Pan and zoom never jump to the origin, even in front of the camera.
    let mut r = rig(horizon_back());
    r.view_mut().pivot = Vec3::new(0.0, 3.0, -2.0);
    r.op(ViewOp::PivotAt {
        at: sky,
        orbit: false,
    });
    assert!((depth_of(&horizon_back(), r.pivot()) - 7.0).abs() < 1e-4);
    // A pivot at or behind the camera: its distance stands in for depth.
    r.view_mut().pivot = Vec3::new(0.0, 4.0, 5.0);
    r.op(ViewOp::ZoomAt {
        at: sky,
        steps: 0.0,
    });
    assert!((depth_of(&horizon_back(), r.pivot()) - 3.0).abs() < 1e-4);
}

#[test]
fn the_wheel_on_sky_zooms_forward_not_toward_the_origin() {
    // Camera at (10, 5, 0) looking along -Z and a little up: the origin is
    // off to the side and nothing is at the view centre.
    let view = Transform::from_xyz(10.0, 5.0, 0.0).looking_to(Vec3::new(0.0, 0.3, -1.0), Vec3::Y);
    let mut r = rig(view);
    r.op(ViewOp::ZoomAt {
        at: CENTER,
        steps: 5.0,
    });
    let moved = r.transform().translation - view.translation;
    assert!(moved.length() > 0.1);
    assert_near(moved.normalize(), *view.forward(), 1e-4);

    // With the pivot behind the camera, zooming in still goes forward.
    let view = Transform::from_xyz(0.0, 1.0, 5.0).looking_at(Vec3::new(0.0, 1.0, 20.0), Vec3::Y);
    let mut r = rig(view);
    r.op(ViewOp::PivotAt {
        at: Vec2::new(400.0, 10.0),
        orbit: false,
    });
    r.op(ViewOp::Zoom(5.0));
    let moved = r.transform().translation - view.translation;
    assert!(moved.dot(*view.forward()) > 0.1, "{moved}");
}

#[test]
fn orthographic_after_a_click_on_the_sky_keeps_a_sensible_scale() {
    let mut r = rig(horizon());
    r.op(ViewOp::PivotAt {
        at: Vec2::new(400.0, 10.0),
        orbit: true,
    });
    r.op(ViewOp::ToggleProjection);
    // The view centre sees nothing, the pivot was level with the camera:
    // its distance (1 m from the origin) frames the view.
    let h = r.ortho().unwrap();
    assert!((h - 2.0 * half_tan()).abs() < 1e-3, "{h}");
}

#[test]
fn meshes_are_picked_before_the_ground() {
    let mut r = rig(default_view());
    r.cube(None, Vec3::new(0.0, 1.0, 0.0), 1.0);
    r.op(ViewOp::PivotAt {
        at: CENTER,
        orbit: true,
    });
    // The centre ray from (-6, 6, 0) towards the origin meets the cube's
    // -X face at (-1, 1, 0).
    assert_near(r.pivot(), Vec3::new(-1.0, 1.0, 0.0), 1e-3);
}

#[test]
fn orbit_rotates_about_the_pivot_keeping_it_fixed_on_screen() {
    let mut r = rig(default_view());
    r.op(ViewOp::PivotAt {
        at: Vec2::new(520.0, 380.0),
        orbit: true,
    });
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
    // 300 px is a half turn (2π per viewport height), past either pole. The
    // pivot is above the screen centre, so the camera passes straight down
    // before it would be over the pivot.
    let mut r = rig(default_view());
    r.op(ViewOp::PivotAt {
        at: Vec2::new(400.0, 100.0),
        orbit: true,
    });
    let pivot = r.pivot();
    let start = view_dir(&r.transform(), pivot);
    r.op(ViewOp::Orbit(Vec2::new(0.0, 300.0)));
    let t = r.transform();
    assert!(t.up().y >= 0.0, "upright: {}", t.up());
    assert!(t.forward().y < -0.9999, "straight down: {}", t.forward());
    let top = r.transform();
    r.op(ViewOp::Orbit(Vec2::new(0.0, 190.0)));
    assert_near(r.transform().translation, top.translation, 1e-3);
    r.op(ViewOp::Orbit(Vec2::new(0.0, -580.0)));
    let t = r.transform();
    assert!(t.up().y >= 0.0, "upright: {}", t.up());
    assert!(t.forward().y > 0.9999, "straight up: {}", t.forward());
    // The pivot kept its place on screen throughout.
    assert_near(view_dir(&t, pivot), start, 1e-3);
}

#[test]
fn a_pivot_low_on_screen_can_still_look_straight_down() {
    let mut r = rig(default_view());
    r.op(ViewOp::PivotAt {
        at: Vec2::new(400.0, 550.0),
        orbit: true,
    });
    r.op(ViewOp::Orbit(Vec2::new(0.0, 300.0)));
    let t = r.transform();
    assert!(t.forward().y < -0.9999, "{}", t.forward());
    assert!(t.up().y >= 0.0);
    // Yawing while looking straight down stays upright.
    r.op(ViewOp::Orbit(Vec2::new(50.0, 0.0)));
    assert!(r.transform().up().y >= 0.0);
    assert!(r.transform().forward().y < -0.9999);
}

#[test]
fn pan_moves_camera_and_pivot_together() {
    let mut r = rig(default_view());
    r.op(ViewOp::PivotAt {
        at: CENTER,
        orbit: true,
    });
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

/// The plane through an off-centre pivot follows the pointer exactly.
#[test]
fn pan_keeps_an_off_centre_pivot_under_the_pointer() {
    let mut r = rig(default_view());
    let at = Vec2::new(750.0, 560.0);
    r.op(ViewOp::PivotAt { at, orbit: false });
    let pivot = r.pivot();
    for (delta, total) in [
        (Vec2::new(40.0, 0.0), Vec2::new(40.0, 0.0)),
        (Vec2::new(-15.0, 25.0), Vec2::new(25.0, 25.0)),
    ] {
        r.op(ViewOp::Pan(delta));
        assert_near2(r.screen(pivot), at + total, 1e-2);
    }
}

#[test]
fn orthographic_pan_follows_the_view_height() {
    let mut r = rig(default_view());
    r.op(ViewOp::PivotAt {
        at: CENTER,
        orbit: true,
    });
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
    // Wherever the last click was, orthographic frames what is at the view
    // centre (the origin here), keeping the scale in the plane through it.
    r.op(ViewOp::PivotAt {
        at: Vec2::new(520.0, 380.0),
        orbit: true,
    });
    let before = r.transform();
    let pivot = Vec3::ZERO;
    let beside = pivot + before.rotation * Vec3::X;
    let span = r.screen(beside) - r.screen(pivot);

    r.op(ViewOp::ToggleProjection);
    assert_near(r.pivot(), pivot, 1e-3);
    let depth = home_distance();
    assert!((r.ortho().unwrap() - 2.0 * depth * half_tan()).abs() < 1e-3);
    assert_near2(r.screen(pivot), CENTER, 1e-2);
    assert_near2(r.screen(beside) - r.screen(pivot), span, 1e-2);
    // Backed off so the near plane doesn't clip the scene: with no world
    // to measure, DEFAULT_EXTENT beyond the reach.
    let off = r.transform().rotation.inverse() * (pivot - r.transform().translation);
    assert!((off.z + DEFAULT_EXTENT + depth).abs() < 1e-3, "{off}");
    assert!((reach(&r.view(), &r.transform(), &r.projection()) - depth).abs() < 1e-3);
    let Projection::Orthographic(o) = r.projection() else {
        unreachable!()
    };
    assert!((o.near - depth).abs() < 1e-3 && o.far >= -off.z + DEFAULT_EXTENT);

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
fn other_ops_land_an_animation_first() {
    let mut r = rig(default_view());
    r.op(ViewOp::Snap(SnapView::Top));
    r.frames(5);
    // Changing the drag action doesn't interrupt it.
    r.op(ViewOp::SetDragAction(DragAction::Pan));
    assert!(r.view().animating());
    assert_eq!(r.view().drag_action, DragAction::Pan);
    // A bare click mid-snap lands it in its orthographic axis view rather
    // than freezing it half-turned.
    r.op(ViewOp::PivotAt {
        at: CENTER,
        orbit: true,
    });
    assert!(!r.view().animating());
    assert_eq!(r.view().snapped, Some(SnapView::Top));
    assert!(r.ortho().is_some());
    let landed = r.transform();
    r.settle();
    assert_eq!(r.transform(), landed);
    assert_near(
        gz_to_bevy().inverse() * *landed.forward(),
        Vec3::NEG_Z,
        1e-4,
    );
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
    // Closer means slower, down to a floor: looking at the sky with the
    // pivot at the camera.
    let mut r = rig(horizon());
    r.view_mut().pivot = horizon().translation;
    r.op(ViewOp::Fly {
        dir: Vec3::Y,
        secs: 1.0,
    });
    assert_near(
        r.transform().translation - horizon().translation,
        Vec3::Y * 0.25,
        1e-4,
    );
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
    r.op(ViewOp::PivotAt {
        at: CENTER,
        orbit: true,
    });
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
    // The steps-per-click setting applies to the key too, as to the Step
    // button whose tooltip shows it.
    r.app
        .world_mut()
        .insert_resource(crate::connection::StepsPerClick(10));
    r.chord(&[KeyCode::ControlLeft, KeyCode::ArrowRight]);
    assert_eq!(r.sent(), [GzCommand::Step(10)]);
}

#[test]
fn viewport_keys_drive_the_view_and_the_grid() {
    let mut r = rig(default_view());
    assert!(!r.app.world().resource::<crate::overlays::Overlays>().grid);
    r.chord(&[KeyCode::KeyG]);
    assert!(r.app.world().resource::<crate::overlays::Overlays>().grid);

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

// ------------------------------------------------ review regression tests

fn depth(r: &Rig) -> f32 {
    depth_of(&r.transform(), r.pivot())
}

fn ortho_near_far(r: &Rig) -> (f32, f32) {
    match r.projection() {
        Projection::Orthographic(o) => (o.near, o.far),
        _ => panic!("not orthographic"),
    }
}

/// After every op an orthographic view keeps the whole world (up to
/// [`MAX_ORTHO_EXTENT`] from the pivot) in front of its near plane, with
/// the near plane at the view's reach.
fn assert_world_in_front(r: &Rig, extent: f32) {
    let (near, far) = ortho_near_far(r);
    let reach = reach(&r.view(), &r.transform(), &r.projection());
    assert!((near - reach).abs() < 1e-3, "near {near} reach {reach}");
    assert!(near > 0.0);
    let d = depth(r);
    // A few f32 steps at the camera's distance: 10 km out a step is about
    // a millimetre, and the depth is a chain of dot products.
    let tolerance = 1e-3_f32.max((d + extent) * 1e-6);
    assert!(
        d - extent >= near - tolerance,
        "depth {d} extent {extent} near {near}"
    );
    assert!(far >= d + extent - tolerance, "far {far}");
}

#[test]
fn orthographic_views_keep_the_world_in_front_after_every_op() {
    let mut r = rig(default_view());
    let (_, model) = r.world_root();
    r.cube(Some(model), Vec3::new(0.0, 0.0, 0.5), 0.5);
    r.op(ViewOp::Snap(SnapView::Front));
    r.settle();
    let extent = |r: &Rig| {
        // The world (one cube) and the 20 m grid, from the pivot.
        let b = Bounds {
            min: Vec3::new(-10.0, -0.0, -10.0),
            max: Vec3::new(10.0, 1.0, 10.0),
        };
        b.reach_from(r.pivot())
    };
    assert_world_in_front(&r, extent(&r));
    // Fly forward in a front view moves up the screen, not into the scene.
    let probe = Vec3::new(0.0, 0.5, 0.0);
    let s0 = r.screen(probe);
    r.op(ViewOp::Fly {
        dir: Vec3::Z,
        secs: 0.2,
    });
    let s1 = r.screen(probe);
    assert!(s1.y > s0.y + 1.0 && (s1.x - s0.x).abs() < 1e-3, "{s0} {s1}");
    assert_world_in_front(&r, extent(&r));
    // E zooms out, Q in.
    let h = r.ortho().unwrap();
    r.op(ViewOp::Fly {
        dir: Vec3::Y,
        secs: 0.2,
    });
    assert!(r.ortho().unwrap() > h);
    r.op(ViewOp::Fly {
        dir: Vec3::NEG_Y,
        secs: 0.4,
    });
    assert!(r.ortho().unwrap() < h);
    assert_world_in_front(&r, extent(&r));
    // Picks and zooms can put the pivot at any depth; the camera backs off.
    r.op(ViewOp::PivotAt {
        at: r.screen(probe),
        orbit: true,
    });
    assert_world_in_front(&r, extent(&r));
    r.op(ViewOp::ZoomAt {
        at: Vec2::new(100.0, 500.0),
        steps: 3.0,
    });
    assert_world_in_front(&r, extent(&r));
    r.op(ViewOp::Pan(Vec2::new(-200.0, 80.0)));
    assert_world_in_front(&r, extent(&r));
}

#[test]
fn the_orthographic_standoff_follows_the_world_size() {
    let mut r = rig(default_view());
    let (_, model) = r.world_root();
    // A model 300 m away along gz +X.
    r.cube(Some(model), Vec3::new(300.0, 0.0, 0.5), 0.5);
    r.op(ViewOp::ToggleProjection);
    let far_corner = Vec3::new(300.5, 1.0, 10.0);
    let extent = far_corner.length();
    assert!(depth(&r) > extent, "{} vs {extent}", depth(&r));
    let (_, far) = ortho_near_far(&r);
    assert!(far > depth(&r) + extent - 1.0);
}

#[test]
fn the_orthographic_standoff_stops_at_the_cap() {
    let mut r = rig(default_view());
    let (_, model) = r.world_root();
    r.cube(Some(model), Vec3::new(1.0, 0.0, 0.5), 0.5);
    // Terrain from 9 km to 29 km down: not run away (it starts within
    // 10 km of the origin), but past the cap.
    r.cube(Some(model), Vec3::new(1.0, 0.0, -19_000.0), 10_000.0);
    // The toggle and a cube face (a snap) both end in an orthographic view
    // that backs off by the cap, not the 29 km world, and keeps the cap's
    // depth in front of the camera.
    for op in [ViewOp::ToggleProjection, ViewOp::Snap(SnapView::Front)] {
        r.op(op);
        r.settle();
        assert_world_in_front(&r, MAX_ORTHO_EXTENT);
        let (_, far) = ortho_near_far(&r);
        let reach = reach(&r.view(), &r.transform(), &r.projection());
        let standoff = MAX_ORTHO_EXTENT + reach.max(ORTHO_MARGIN);
        assert!((depth(&r) - standoff).abs() < 1.0, "{}", depth(&r));
        assert!(far <= depth(&r) + MAX_ORTHO_EXTENT + 2.0, "{far}");
    }
}

#[test]
fn a_world_a_few_kilometres_across_stays_in_the_orthographic_view() {
    // What a perspective view shows: a model 3 km away is inside the clip
    // range too.
    let mut r = rig(default_view());
    let (_, model) = r.world_root();
    r.cube(Some(model), Vec3::new(1.0, 0.0, 0.5), 0.5);
    r.cube(Some(model), Vec3::new(3000.0, 0.0, 0.5), 0.5);
    r.op(ViewOp::ToggleProjection);
    let far_corner = Vec3::new(3000.5, 1.0, 10.0);
    assert_world_in_front(&r, (far_corner - r.pivot()).length());
}

#[test]
fn f_over_the_ground_frames_the_world_not_the_ground() {
    let mut r = rig(default_view());
    let (root, model) = r.world_root();
    r.cube(Some(model), Vec3::new(2.0, 0.0, 1.0), 1.0);
    // A ground model holding a 100 m plane visual.
    let ground_model = r
        .app
        .world_mut()
        .spawn((Transform::default(), Visibility::default(), ChildOf(root)))
        .id();
    let plane = r.cube(Some(ground_model), Vec3::new(0.0, 0.0, -0.05), 50.0);
    r.app.world_mut().entity_mut(plane).insert(plane_visual());
    r.make_visible();
    let at = r.screen(Vec3::new(-3.0, 0.0, 3.0));
    r.op(ViewOp::Focus { at: Some(at) });
    r.settle();
    // gz (2, 0, 1) is Bevy (2, 1, 0): the model, not the 100 m plane.
    assert_near(r.pivot(), Vec3::new(2.0, 1.0, 0.0), 1e-3);
}

/// A world with a model at gz (2, 0, 1) and, in a second model, a box
/// that has fallen to gz z = `fallen`.
fn world_with_a_fallen_model(fallen: f32) -> Rig {
    let mut r = rig(default_view());
    let (root, model) = r.world_root();
    r.cube(Some(model), Vec3::new(2.0, 0.0, 1.0), 1.0);
    let other = r
        .app
        .world_mut()
        .spawn((Transform::default(), Visibility::default(), ChildOf(root)))
        .id();
    r.cube(Some(other), Vec3::new(1.0, 0.0, fallen), 0.5);
    r.make_visible();
    r
}

#[test]
fn f_frames_the_world_without_a_model_that_has_fallen_away() {
    let mut r = world_with_a_fallen_model(-6.0e9);
    r.op(ViewOp::Focus { at: None });
    r.settle();
    // gz (2, 0, 1) is Bevy (2, 1, 0).
    assert_near(r.pivot(), Vec3::new(2.0, 1.0, 0.0), 1e-3);
    assert!(reach(&r.view(), &r.transform(), &r.projection()) < 10.0);
    // Orthographic from there stands back for the world and the grid.
    r.op(ViewOp::ToggleProjection);
    assert!(depth(&r) < 100.0, "{}", depth(&r));
}

#[test]
fn zooming_out_stops_at_the_world_without_a_model_that_has_fallen_away() {
    let mut r = world_with_a_fallen_model(-6.0e9);
    for _ in 0..200 {
        r.op(ViewOp::Zoom(-5.0));
    }
    let d = (r.transform().translation - r.pivot()).length();
    assert!(d < 1000.0, "{d}");
    // The orthographic clip range doesn't reach for it either.
    r.op(ViewOp::ToggleProjection);
    let (_, far) = ortho_near_far(&r);
    assert!(far < depth(&r) + 1000.0, "{far}");
}

#[test]
fn a_model_a_few_kilometres_away_is_still_in_the_world() {
    // 5 km down is far, but not runaway: F frames it with the rest.
    let mut r = world_with_a_fallen_model(-5000.0);
    r.op(ViewOp::Focus { at: None });
    r.settle();
    assert!(r.pivot().y < -2000.0, "{}", r.pivot());
}

#[test]
fn f_leaves_out_a_link_that_fell_off_its_model() {
    // One model: a link at gz (2, 0, 1) and one that fell to z = -6e9.
    let mut r = rig(default_view());
    let (_, model) = r.world_root();
    r.cube(Some(model), Vec3::new(2.0, 0.0, 1.0), 1.0);
    r.cube(Some(model), Vec3::new(1.0, 0.0, -6.0e9), 0.5);
    r.make_visible();
    // Nothing selected, nothing under the pointer: the world.
    r.op(ViewOp::Focus { at: None });
    r.settle();
    assert_near(r.pivot(), Vec3::new(2.0, 1.0, 0.0), 1e-3);
    assert!(reach(&r.view(), &r.transform(), &r.projection()) < 10.0);
    // Over the model: the model, without the link that fell.
    r.op(ViewOp::Home);
    r.settle();
    let at = r.screen(Vec3::new(2.0, 1.0, 0.0));
    r.op(ViewOp::Focus { at: Some(at) });
    r.settle();
    assert_near(r.pivot(), Vec3::new(2.0, 1.0, 0.0), 1e-3);
}

#[test]
fn a_world_that_has_all_run_away_has_only_the_grid() {
    let mut r = rig(default_view());
    let (_, model) = r.world_root();
    r.cube(Some(model), Vec3::new(1.0, 0.0, -6.0e9), 0.5);
    r.make_visible();
    // F has nothing to frame and goes home.
    r.op(ViewOp::Focus { at: None });
    r.settle();
    assert_near(r.pivot(), Vec3::ZERO, 1e-3);
    assert_near(
        r.transform().translation,
        home_transform().translation,
        1e-3,
    );
    // Zooming out stops at a few framings of the 20 m grid.
    for _ in 0..200 {
        r.op(ViewOp::Zoom(-5.0));
    }
    let d = (r.transform().translation - r.pivot()).length();
    assert!(d < 200.0, "{d}");
}

#[test]
fn bounds_measure_the_distance_to_their_nearest_point() {
    let b = Bounds {
        min: Vec3::new(-1.0, -1.0, -1.0),
        max: Vec3::new(1.0, 1.0, 1.0),
    };
    assert_eq!(b.distance_to(Vec3::ZERO), 0.0);
    assert_eq!(b.distance_to(Vec3::new(4.0, 0.0, 0.0)), 3.0);
    assert!((b.distance_to(Vec3::new(2.0, 2.0, 0.0)) - 2f32.sqrt()).abs() < 1e-6);
}

#[test]
fn hidden_nodes_are_left_out_of_the_bounds() {
    let mut r = rig(default_view());
    let (_, model) = r.world_root();
    r.cube(Some(model), Vec3::new(2.0, 0.0, 1.0), 1.0);
    let hidden = r.cube(Some(model), Vec3::new(-40.0, 0.0, 1.0), 1.0);
    r.app
        .world_mut()
        .entity_mut(hidden)
        .insert(Visibility::Hidden);
    r.op(ViewOp::Focus { at: None });
    r.settle();
    assert_near(r.pivot(), Vec3::new(2.0, 1.0, 0.0), 1e-3);
}

#[test]
fn idle_frames_leave_the_camera_unchanged() {
    let mut r = rig(default_view());
    r.op(ViewOp::Zoom(1.0));
    r.frames(2);
    let ticks = |r: &mut Rig| {
        let e = r.app.world().entity(r.camera);
        (
            e.get_ref::<Transform>().unwrap().last_changed(),
            e.get_ref::<Projection>().unwrap().last_changed(),
            e.get_ref::<ViewCamera>().unwrap().last_changed(),
        )
    };
    let before = ticks(&mut r);
    r.frames(3);
    assert_eq!(ticks(&mut r), before);
}

#[test]
fn fly_speed_and_orthographic_scale_ignore_a_far_last_click() {
    // A click near the horizon put the pivot 900 m away.
    let far_pivot = Vec3::new(900.0, 0.0, 0.0);
    let mut r = rig(default_view());
    r.view_mut().pivot = far_pivot;
    let start = r.transform().translation;
    r.op(ViewOp::Fly {
        dir: Vec3::Z,
        secs: 0.1,
    });
    // Half the distance to what the view centre sees (the origin).
    let step = 0.5 * home_distance() * 0.1;
    assert!(((r.transform().translation - start).length() - step).abs() < 1e-3);

    let mut r = rig(default_view());
    r.view_mut().pivot = far_pivot;
    r.op(ViewOp::ToggleProjection);
    assert!((r.ortho().unwrap() - 2.0 * home_distance() * half_tan()).abs() < 1e-3);

    // The view centre on the ground far away: clamped to the world's size
    // (no world here: DEFAULT_WORLD_REACH).
    let low = Transform::from_xyz(0.0, 1.0, 0.0).looking_to(Vec3::new(0.0, -0.001, -1.0), Vec3::Y);
    let mut r = rig(low);
    r.op(ViewOp::ToggleProjection);
    let h = r.ortho().unwrap();
    assert!(
        (h - 2.0 * DEFAULT_WORLD_REACH * half_tan()).abs() < 1e-2,
        "{h}"
    );
}

#[test]
fn far_grows_to_hold_a_focused_world_and_zoom_has_limits() {
    let mut r = rig(default_view());
    let (_, model) = r.world_root();
    r.cube(Some(model), Vec3::ZERO, 400.0);
    r.op(ViewOp::Focus { at: None });
    r.settle();
    let d = (r.transform().translation - r.pivot()).length();
    assert!(d > 1000.0, "{d}");
    let Projection::Perspective(p) = r.projection() else {
        unreachable!()
    };
    // The far side of the world is inside the far plane.
    assert!(p.far >= d + 400.0 * 3f32.sqrt(), "{}", p.far);

    // Zooming out stops at a few world framings, zooming in short of the
    // pivot.
    let mut r = rig(default_view());
    for _ in 0..200 {
        r.op(ViewOp::Zoom(-5.0));
    }
    let d = (r.transform().translation - r.pivot()).length();
    assert!(
        (d - ZOOM_OUT_LIMIT * DEFAULT_WORLD_REACH).abs() < 1e-2,
        "{d}"
    );
    let Projection::Perspective(p) = r.projection() else {
        unreachable!()
    };
    assert_eq!(p.far, 1000.0);
    for _ in 0..200 {
        r.op(ViewOp::Zoom(5.0));
    }
    let d = (r.transform().translation - r.pivot()).length();
    assert!((d - MIN_DISTANCE).abs() < 1e-4, "{d}");
}

#[test]
fn orthographic_zoom_at_its_limit_does_not_drift() {
    let mut r = rig(default_view());
    r.op(ViewOp::ToggleProjection);
    r.op(ViewOp::ZoomAt {
        at: Vec2::new(300.0, 350.0),
        steps: 1e4,
    });
    let pivot = r.pivot();
    let at = r.screen(pivot);
    let off = r.transform().rotation.inverse() * (pivot - r.transform().translation);
    let h = r.ortho().unwrap();
    r.op(ViewOp::Zoom(50.0));
    assert_eq!(r.ortho().unwrap(), h);
    // Within f32 noise at this scale (the view is 2 cm tall, 60 m away).
    assert_near2(r.screen(pivot), at, 0.5);
    let off2 = r.transform().rotation.inverse() * (pivot - r.transform().translation);
    assert!((off2.truncate() - off.truncate()).length() < 1e-4);
}

#[test]
fn leaving_orthographic_clears_the_snap() {
    let mut r = rig(default_view());
    r.op(ViewOp::Snap(SnapView::Right));
    r.settle();
    assert_eq!(r.view().snapped, Some(SnapView::Right));
    r.op(ViewOp::ToggleProjection);
    assert!(r.ortho().is_none());
    assert_eq!(r.view().snapped, None);
}

// ------------------------------------------- review regression: input

#[test]
fn a_space_tap_within_one_frame_toggles_once_and_leaves_nothing_behind() {
    let mut r = rig(default_view());
    r.set_sim(Some(true));
    r.key(KeyCode::Space, ButtonState::Pressed);
    r.key(KeyCode::Space, ButtonState::Released);
    r.frames(3);
    assert_eq!(r.sent(), [GzCommand::Play]);
    // A later ordinary tap, long after, still works.
    r.frames((TAP_SECS * 1000.0) as usize / FRAME_MS as usize + 5);
    r.chord(&[KeyCode::Space]);
    assert_eq!(r.sent(), [GzCommand::Play]);
}

#[test]
fn losing_focus_is_no_tap() {
    use bevy::input::keyboard::KeyboardFocusLost;
    let mut r = rig(default_view());
    r.set_sim(Some(true));
    r.key(KeyCode::Space, ButtonState::Pressed);
    r.app.update();
    // Alt-tab: the window loses focus, and Bevy releases every key.
    r.app.world_mut().write_message(KeyboardFocusLost);
    r.app.update();
    r.frames(2);
    assert!(r.sent().is_empty());
}

#[test]
fn releasing_cmd_releases_the_keys_pressed_under_it() {
    let mut r = rig(default_view());
    r.set_sim(Some(true));
    // Cmd+Right on a Mac browser: Right's key-up never arrives.
    r.key(KeyCode::SuperLeft, ButtonState::Pressed);
    r.key(KeyCode::ArrowRight, ButtonState::Pressed);
    r.app.update();
    r.key(KeyCode::SuperLeft, ButtonState::Released);
    r.app.update();
    assert!(!r
        .app
        .world()
        .resource::<ButtonInput<KeyCode>>()
        .pressed(KeyCode::ArrowRight));
    let t = r.transform();
    r.frames(5);
    assert_eq!(r.transform(), t, "the camera doesn't fly on");
    // Plain keys are left alone.
    r.key(KeyCode::KeyW, ButtonState::Pressed);
    r.key(KeyCode::ShiftLeft, ButtonState::Pressed);
    r.app.update();
    assert!(r
        .app
        .world()
        .resource::<ButtonInput<KeyCode>>()
        .pressed(KeyCode::KeyW));
}

#[test]
fn a_stalled_frame_flies_at_most_a_tenth_of_a_second() {
    let mut r = rig(default_view());
    r.key(KeyCode::KeyW, ButtonState::Pressed);
    r.app.update();
    let before = r.transform().translation;
    r.app
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs(2)));
    r.app.update();
    let moved = (r.transform().translation - before).length();
    assert!(
        (moved - 0.5 * home_distance() * 0.1).abs() < 1e-3,
        "{moved}"
    );
}

fn wheel_px(r: &mut Rig, x: f32, y: f32, phase: bevy::input::touch::TouchPhase) {
    let window = r.window;
    r.app.world_mut().write_message(MouseWheel {
        unit: MouseScrollUnit::Pixel,
        x,
        y,
        window,
        phase,
    });
}

fn distance(r: &Rig) -> f32 {
    (r.transform().translation - r.pivot()).length()
}

#[test]
fn wheel_events_in_a_frame_make_one_bounded_zoom() {
    let mut r = rig(default_view());
    r.cursor(Some(CENTER));
    for _ in 0..3 {
        r.wheel(1.0, MouseScrollUnit::Line);
    }
    r.app.update();
    let zooms = |r: &Rig| r.app.world().resource::<Messages<ViewCommand>>().len();
    assert!(zooms(&r) <= 1, "one ZoomAt for the frame");
    assert!((distance(&r) - home_distance() * ZOOM_STEP.powi(3)).abs() < 1e-3);
    // A momentum flick is clamped.
    let mut r = rig(default_view());
    r.cursor(Some(CENTER));
    r.wheel(40.0, MouseScrollUnit::Line);
    r.app.update();
    let want = home_distance() * ZOOM_STEP.powf(controls::MAX_WHEEL_STEPS);
    assert!((distance(&r) - want).abs() < 1e-3);
}

#[test]
fn browser_wheel_deltas_are_normalised_to_a_step_a_notch() {
    use super::input::Platform;
    let web = |r: &mut Rig| r.app.world_mut().resource_mut::<InputMap>().platform = Platform::Web;
    // Chrome: 100 px a notch, in physical pixels at scale 2.
    let mut r = rig(default_view());
    web(&mut r);
    r.app
        .world_mut()
        .get_mut::<Window>(r.window)
        .unwrap()
        .resolution
        .set_scale_factor_and_apply_to_physical_size(2.0);
    r.cursor(Some(CENTER));
    wheel_px(&mut r, 0.0, 200.0, bevy::input::touch::TouchPhase::Moved);
    r.app.update();
    assert!((distance(&r) - home_distance() * ZOOM_STEP).abs() < 1e-3);
    // Firefox: three lines a notch.
    let mut r = rig(default_view());
    web(&mut r);
    r.cursor(Some(CENTER));
    r.wheel(3.0, MouseScrollUnit::Line);
    r.app.update();
    assert!((distance(&r) - home_distance() * ZOOM_STEP).abs() < 1e-3);
}

#[test]
fn a_pinch_zooms_to_the_cursor() {
    use super::controls::{BrowserWheel, PINCH_PX_PER_E};
    use super::input::Platform;
    let cursor = Vec2::new(300.0, 350.0);
    // The browser: a ctrlKey wheel without a Control key down.
    let mut r = rig(default_view());
    r.app.world_mut().resource_mut::<InputMap>().platform = Platform::Web;
    r.cursor(Some(cursor));
    r.app.world_mut().resource_mut::<BrowserWheel>().ctrl = true;
    // Fingers apart to 1.2× the scale.
    wheel_px(
        &mut r,
        0.0,
        PINCH_PX_PER_E * 1.2f32.ln(),
        bevy::input::touch::TouchPhase::Moved,
    );
    r.app.update();
    let under = r.pivot();
    assert_near2(r.screen(under), cursor, 1e-2);
    let d0 = (default_view().translation - under).length();
    assert!((distance(&r) - d0 / 1.2).abs() < 1e-3, "{}", distance(&r));
    // Safari's gesture: the same.
    let mut r = rig(default_view());
    r.cursor(Some(cursor));
    r.app.world_mut().resource_mut::<BrowserWheel>().pinch = 1.2f32.ln();
    r.app.update();
    assert!((distance(&r) - d0 / 1.2).abs() < 1e-3);
    // macOS: PinchGesture.
    let mut r = rig(default_view());
    r.cursor(Some(cursor));
    r.app
        .world_mut()
        .write_message(bevy::input::gestures::PinchGesture(0.2));
    r.app.update();
    assert!((distance(&r) - d0 / 1.2).abs() < 1e-3);
    // Ctrl held with a real key is Ctrl+wheel, one step a notch.
    let mut r = rig(default_view());
    r.app.world_mut().resource_mut::<InputMap>().platform = Platform::Web;
    r.cursor(Some(cursor));
    r.key(KeyCode::ControlLeft, ButtonState::Pressed);
    r.app.update();
    r.app.world_mut().resource_mut::<BrowserWheel>().ctrl = true;
    wheel_px(
        &mut r,
        0.0,
        WHEEL_PX_PER_STEP,
        bevy::input::touch::TouchPhase::Moved,
    );
    r.app.update();
    assert!((distance(&r) - d0 * ZOOM_STEP).abs() < 1e-3);
}

#[test]
fn a_native_two_finger_scroll_pans() {
    let mut r = rig(default_view());
    let at = Vec2::new(500.0, 400.0);
    r.cursor(Some(at));
    let ray = r.ray(at);
    let pivot = ray.origin + *ray.direction * (-ray.origin.y / ray.direction.y);
    wheel_px(&mut r, 30.0, -20.0, bevy::input::touch::TouchPhase::Started);
    r.app.update();
    // The point under the cursor at the start follows the fingers.
    assert_near2(r.screen(pivot), at + Vec2::new(30.0, -20.0), 1e-2);
    assert_eq!(r.transform().rotation, default_view().rotation);
    // iOS PanGesture, in physical pixels.
    r.app
        .world_mut()
        .write_message(bevy::input::gestures::PanGesture(Vec2::new(10.0, 0.0)));
    r.app.update();
    assert_near2(r.screen(pivot), at + Vec2::new(40.0, -20.0), 1e-2);
}

fn routing(r: &mut Rig, pointer: bool, keys: bool) {
    *r.app.world_mut().resource_mut::<controls::ViewRouting>() = controls::ViewRouting {
        pointer,
        keys,
        ..default()
    };
}

#[test]
fn camera_input_over_the_ui_is_ignored_and_over_the_view_applied() {
    let mut r = rig(default_view());
    // Over a UI node: a press starts no drag and the wheel does nothing.
    routing(&mut r, false, true);
    r.drag(MouseButton::Left, CENTER, &[CENTER + Vec2::new(40.0, 0.0)]);
    r.wheel(2.0, MouseScrollUnit::Line);
    r.frames(2);
    assert_eq!(r.transform(), default_view());
    // Over the view: applied.
    routing(&mut r, true, true);
    r.drag(MouseButton::Left, CENTER, &[CENTER + Vec2::new(40.0, 0.0)]);
    let orbited = r.transform();
    assert_ne!(orbited, default_view());
    // A drag that starts in the view keeps going over the UI.
    r.cursor(Some(CENTER));
    r.button(MouseButton::Left, ButtonState::Pressed);
    r.app.update();
    routing(&mut r, false, true);
    r.cursor(Some(CENTER + Vec2::new(0.0, 30.0)));
    r.app.update();
    assert_ne!(r.transform(), orbited);
    r.button(MouseButton::Left, ButtonState::Released);
    r.app.update();
}

#[test]
fn viewport_keys_need_the_view_but_global_keys_do_not() {
    let mut r = rig(default_view());
    routing(&mut r, false, false);
    r.chord(&[KeyCode::KeyG]);
    assert!(!r.app.world().resource::<crate::overlays::Overlays>().grid);
    // Fly keys are the view's too.
    r.key(KeyCode::KeyW, ButtonState::Pressed);
    r.frames(3);
    r.key(KeyCode::KeyW, ButtonState::Released);
    r.app.update();
    assert_eq!(r.transform(), default_view());
    // Space (global) still plays.
    r.set_sim(Some(true));
    r.chord(&[KeyCode::Space]);
    assert_eq!(r.sent(), [GzCommand::Play]);
    routing(&mut r, false, true);
    r.chord(&[KeyCode::KeyG]);
    assert!(r.app.world().resource::<crate::overlays::Overlays>().grid);
    let all = [Scope::Global, Scope::Viewport, Scope::Minimal];
    let off = controls::ViewRouting {
        pointer: true,
        keys: false,
        ..default()
    };
    assert_eq!(*off.key_scopes(&all), [Scope::Global]);
    // With the keys: every active scope, once each.
    let on = controls::ViewRouting { keys: true, ..off };
    assert_eq!(
        *on.key_scopes(&[Scope::Global, Scope::Viewport, Scope::Global]),
        [Scope::Global, Scope::Viewport]
    );
}

#[test]
fn a_placed_view_renders_to_its_texture_and_offsets_the_cursor() {
    let mut r = rig(default_view());
    r.app.insert_resource(Assets::<Image>::default());
    r.app
        .world_mut()
        .entity_mut(r.camera)
        .insert(crate::camera::MainCamera);
    // No rect (minimal mode): nothing changes.
    r.app.update();
    assert!(matches!(
        r.app.world().get::<RenderTarget>(r.camera),
        Some(RenderTarget::Window(_))
    ));
    let place = |r: &mut Rig, rect: Option<Rect>, scale: f32| {
        let image = r
            .app
            .world()
            .get::<ViewPlacement>(r.camera)
            .and_then(|p| p.image.clone());
        r.app
            .world_mut()
            .entity_mut(r.camera)
            .insert(ViewPlacement { rect, scale, image });
        r.app.update();
    };
    for (scale, rect) in [
        (1.0, Rect::new(100.0, 50.0, 500.0, 350.0)),
        (1.5, Rect::new(100.0, 50.0, 500.0, 350.0)),
        (2.0, Rect::new(56.0, 44.0, 840.0, 872.0)),
    ] {
        place(&mut r, Some(rect), scale);
        let placement = r
            .app
            .world()
            .get::<ViewPlacement>(r.camera)
            .unwrap()
            .clone();
        let size = (rect.size() * scale).round().as_uvec2();
        assert_eq!(placement.physical_size(), Some(size));
        let handle = placement.image.clone().expect("a texture");
        let image = r
            .app
            .world()
            .resource::<Assets<Image>>()
            .get(&handle)
            .unwrap();
        assert_eq!(image.size(), size, "{scale}");
        assert!(image.data.is_none() && !image.copy_on_resize);
        let world = r.app.world();
        let target = world.get::<RenderTarget>(r.camera).unwrap();
        assert!(
            matches!(target, RenderTarget::Image(t) if t.handle == handle && t.scale_factor == scale)
        );
        let camera = world.get::<Camera>(r.camera).unwrap();
        assert_eq!(camera.logical_viewport_size(), Some(rect.size()), "{scale}");
        let origin = world.get::<ViewOrigin>(r.camera).copied();
        assert_eq!(origin, Some(ViewOrigin(rect.min)));
        assert_eq!(view_rect(camera, origin.as_ref()), Some(rect));
        let Projection::Perspective(p) = world.get::<Projection>(r.camera).unwrap() else {
            panic!("perspective")
        };
        assert!((p.aspect_ratio - rect.width() / rect.height()).abs() < 1e-5);
    }
    // The cursor over the view's centre is the view centre: the home view
    // looks at the gz origin.
    let rect = Rect::new(100.0, 50.0, 500.0, 350.0);
    place(&mut r, Some(rect), 1.0);
    let world = r.app.world();
    let cam = world.get::<Camera>(r.camera).unwrap().clone();
    let origin = world.get::<ViewOrigin>(r.camera).copied();
    let projection = r.projection();
    let t = r.transform();
    let centre = rect.center();
    let at = cursor_on_ground(&cam, origin.as_ref(), &projection, &t, centre).unwrap();
    assert!(at.length() < 1e-3, "{at}");
    assert_eq!(
        cursor_on_ground(&cam, origin.as_ref(), &projection, &t, Vec2::ZERO),
        None
    );
    let sky = Transform::from_xyz(0.0, 1.0, 0.0).looking_at(Vec3::new(0.0, 5.0, -1.0), Vec3::Y);
    assert_eq!(
        cursor_on_ground(&cam, origin.as_ref(), &projection, &sky, centre),
        None
    );
    let ray = Ray3d::new(Vec3::new(0.0, 2.0, 0.0), Dir3::NEG_Y);
    assert_eq!(ground_point(ray, 10.0), Some(Vec3::ZERO));
    assert_eq!(ground_point(ray, 1.0), None);
    // A drag in the view orbits about the point under the cursor there.
    r.drag(MouseButton::Left, centre, &[centre + Vec2::new(30.0, 0.0)]);
    assert!(r.pivot().length() < 1e-3, "{}", r.pivot());
    // A respawned camera (new environment) is placed again.
    r.app
        .world_mut()
        .entity_mut(r.camera)
        .insert(RenderTarget::default())
        .remove::<ViewOrigin>();
    r.app.update();
    assert!(r.app.world().get::<ViewOrigin>(r.camera).is_some());
    assert!(matches!(
        r.app.world().get::<RenderTarget>(r.camera),
        Some(RenderTarget::Image(_))
    ));
    // Placed nowhere (the layout has fewer views): off.
    place(&mut r, None, 1.0);
    assert!(!r.app.world().get::<Camera>(r.camera).unwrap().is_active);
    place(&mut r, Some(rect), 1.0);
    assert!(r.app.world().get::<Camera>(r.camera).unwrap().is_active);
}

// Camera modes (follow, track), the cube's look-from views and the
// overlay keys (DECISIONS.md D-60, D-61, D-62).

fn mode(r: &Rig) -> CameraMode {
    r.view().mode
}

/// Moves `e` (a world-root child, so `at` is in Gazebo axes).
fn move_to(r: &mut Rig, e: Entity, at: Vec3) {
    r.app
        .world_mut()
        .get_mut::<Transform>(e)
        .unwrap()
        .translation = at;
}

#[test]
fn follow_turns_to_the_target_then_moves_with_it() {
    let mut r = rig(default_view());
    let (_, model) = r.world_root();
    r.frames(2);
    r.op(ViewOp::SetMode(CameraMode::Follow(model)));
    assert_eq!(mode(&r), CameraMode::Follow(model));
    r.settle();
    assert_near(r.pivot(), Vec3::ZERO, 1e-4);
    let before = r.transform().translation;
    // gz (1, 2, 0) is Bevy (1, 0, -2).
    move_to(&mut r, model, Vec3::new(1.0, 2.0, 0.0));
    r.frames(2);
    assert_near(r.pivot(), Vec3::new(1.0, 0.0, -2.0), 1e-4);
    assert_near(
        r.transform().translation - before,
        Vec3::new(1.0, 0.0, -2.0),
        1e-4,
    );
    // Orbiting still works about the target.
    r.op(ViewOp::Orbit(Vec2::new(40.0, 0.0)));
    assert_near(r.pivot(), Vec3::new(1.0, 0.0, -2.0), 1e-4);
    assert_eq!(mode(&r), CameraMode::Follow(model));
    // Home frees the camera.
    r.op(ViewOp::Home);
    assert_eq!(mode(&r), CameraMode::Free);
}

#[test]
fn a_target_moving_during_the_turn_is_followed_too() {
    let mut r = rig(default_view());
    let (_, model) = r.world_root();
    r.frames(2);
    r.op(ViewOp::SetMode(CameraMode::Follow(model)));
    move_to(&mut r, model, Vec3::new(0.0, 0.0, 3.0));
    r.settle();
    assert_near(r.pivot(), Vec3::new(0.0, 3.0, 0.0), 1e-4);
}

#[test]
fn track_stays_put_and_turns_to_the_target() {
    let mut r = rig(default_view());
    let (_, model) = r.world_root();
    r.frames(2);
    r.op(ViewOp::SetMode(CameraMode::Track(model)));
    r.settle();
    let at = r.transform().translation;
    move_to(&mut r, model, Vec3::new(0.0, 4.0, 0.0));
    r.frames(2);
    let target = Vec3::new(0.0, 0.0, -4.0);
    assert_near(r.transform().translation, at, 1e-4);
    assert_near(*r.transform().forward(), (target - at).normalize(), 1e-4);
    assert_near(r.pivot(), target, 1e-4);
    // A target that goes away frees the camera.
    r.app.world_mut().despawn(model);
    r.frames(2);
    assert_eq!(mode(&r), CameraMode::Free);
}

#[test]
fn a_mode_without_a_target_in_the_scene_stays_free() {
    let mut r = rig(default_view());
    let ghost = r.app.world_mut().spawn_empty().id();
    r.app.world_mut().despawn(ghost);
    r.op(ViewOp::SetMode(CameraMode::Track(ghost)));
    assert_eq!(mode(&r), CameraMode::Free);
    assert_eq!(CameraMode::Free.target(), None);
    assert_eq!(CameraMode::Follow(ghost).target(), Some(ghost));
    // A target straight below keeps the camera's own up.
    let (_, model) = r.world_root();
    r.frames(2);
    let start = Transform::from_xyz(0.0, 5.0, 0.0).looking_at(Vec3::ZERO, Vec3::Z);
    *r.app.world_mut().get_mut::<Transform>(r.camera).unwrap() = start;
    r.op(ViewOp::SetMode(CameraMode::Track(model)));
    r.settle();
    assert_near(*r.transform().forward(), Vec3::NEG_Y, 1e-4);
}

#[test]
fn shift_f_and_shift_t_pick_the_model_under_the_cursor_and_release() {
    let mut r = rig(default_view());
    let (_, model) = r.world_root();
    r.cube(Some(model), Vec3::new(2.0, 0.0, 0.5), 0.5);
    r.cursor(Some(r.screen(Vec3::new(2.0, 0.5, 0.0))));
    r.chord(&[KeyCode::ShiftLeft, KeyCode::KeyF]);
    assert_eq!(mode(&r), CameraMode::Follow(model));
    // ⇧T switches to tracking it; ⇧T again releases.
    r.chord(&[KeyCode::ShiftLeft, KeyCode::KeyT]);
    assert_eq!(mode(&r), CameraMode::Track(model));
    r.chord(&[KeyCode::ShiftLeft, KeyCode::KeyT]);
    assert_eq!(mode(&r), CameraMode::Free);
    r.chord(&[KeyCode::ShiftLeft, KeyCode::KeyF]);
    r.chord(&[KeyCode::ShiftLeft, KeyCode::KeyF]);
    assert_eq!(mode(&r), CameraMode::Free);
    // Esc never releases (spec §6.3).
    r.chord(&[KeyCode::ShiftLeft, KeyCode::KeyF]);
    r.chord(&[KeyCode::Escape]);
    assert_eq!(mode(&r), CameraMode::Follow(model));
    // Over the sky with no selection there is nothing to follow.
    r.chord(&[KeyCode::ShiftLeft, KeyCode::KeyF]);
    r.cursor(Some(Vec2::new(W / 2.0, 5.0)));
    r.chord(&[KeyCode::ShiftLeft, KeyCode::KeyF]);
    assert_eq!(mode(&r), CameraMode::Free);
    // The selection comes first.
    let other = r.app.world_mut().spawn(Transform::default()).id();
    r.app.world_mut().resource_mut::<Selection>().entities = vec![other];
    r.frames(1);
    r.chord(&[KeyCode::ShiftLeft, KeyCode::KeyT]);
    assert_eq!(mode(&r), CameraMode::Track(other));
}

#[test]
fn look_from_animates_to_an_edge_or_corner_view_keeping_the_projection() {
    let mut r = rig(default_view());
    let d0 = distance(&r);
    // The top-east edge: from +X and +Z, looking down 45°.
    r.op(ViewOp::LookFrom(Vec3::new(1.0, 0.0, 1.0)));
    assert!(r.view().animating());
    r.settle();
    let want = -(gz_to_bevy() * Vec3::new(1.0, 0.0, 1.0)).normalize();
    assert_near(*r.transform().forward(), want, 1e-4);
    assert!((distance(&r) - d0).abs() < 1e-3);
    assert!(r.ortho().is_none());
    // Snapped orthographic: stays orthographic, no longer snapped.
    r.op(ViewOp::Snap(SnapView::Top));
    r.settle();
    r.op(ViewOp::LookFrom(Vec3::new(-1.0, -1.0, 1.0)));
    r.settle();
    assert!(r.ortho().is_some());
    assert_eq!(r.view().snapped, None);
    // Straight down: +X up the screen, like the top view.
    r.op(ViewOp::LookFrom(Vec3::Z));
    r.settle();
    assert_near(
        r.transform().rotation * Vec3::Y,
        gz_to_bevy() * Vec3::X,
        1e-4,
    );
    // No direction: nothing.
    let t = r.transform();
    r.op(ViewOp::LookFrom(Vec3::ZERO));
    assert_eq!(r.transform(), t);
}

#[test]
fn animations_ease_out_cubically() {
    assert_eq!(ease(0.0), 0.0);
    assert_eq!(ease(1.0), 1.0);
    assert!((ease(0.5) - 0.875).abs() < 1e-6);
    assert_eq!(ease(2.0), 1.0);
}

#[test]
fn view_picks_pass_through_no_view_pick_meshes() {
    let mut r = rig(default_view());
    let (_, model) = r.world_root();
    let cube = r.cube(Some(model), Vec3::new(2.0, 0.0, 0.5), 0.5);
    r.app.world_mut().entity_mut(cube).insert(NoViewPick);
    r.frames(1);
    let at = r.screen(Vec3::new(2.0, 0.5, 0.0));
    r.op(ViewOp::PivotAt { at, orbit: false });
    // The ground behind it, not the cube's top.
    assert!(r.pivot().y.abs() < 1e-3, "{}", r.pivot());
}

#[test]
fn overlay_keys_toggle_the_overlays() {
    use crate::overlays::Overlays;
    let mut r = rig(default_view());
    for (key, get) in [
        (
            KeyCode::KeyC,
            (|o: &Overlays| o.collisions) as fn(&Overlays) -> bool,
        ),
        (KeyCode::KeyI, |o: &Overlays| o.inertia),
        (KeyCode::KeyJ, |o: &Overlays| o.joints),
        (KeyCode::KeyG, |o: &Overlays| o.grid),
    ] {
        r.chord(&[key]);
        assert!(get(r.app.world().resource::<Overlays>()), "{key:?}");
        r.chord(&[key]);
        assert!(!get(r.app.world().resource::<Overlays>()), "{key:?}");
    }
}

#[test]
fn the_start_view_applies_once_the_world_has_loaded() {
    use crate::overlays::{Overlay, Overlays};
    let mut r = rig(default_view());
    r.app.insert_resource(StartView {
        view: Some(SnapView::Top),
        overlays: vec![Overlay::Frames, Overlay::Grid],
        ..default()
    });
    r.app.init_resource::<crate::spawn::GzIndex>();
    r.frames(2);
    assert!(
        r.app.world().contains_resource::<StartView>(),
        "waits for a world"
    );
    r.app
        .world_mut()
        .resource_mut::<crate::spawn::GzIndex>()
        .scene = Some(gz_core::scene::SceneDesc::from_msg(
        &Default::default(),
        Default::default(),
    ));
    r.frames(1);
    r.settle();
    assert!(!r.app.world().contains_resource::<StartView>());
    assert_eq!(r.view().snapped, Some(SnapView::Top));
    let o = *r.app.world().resource::<Overlays>();
    assert!(o.frames && o.grid);
    for (s, v) in [
        ("TOP", SnapView::Top),
        (" left", SnapView::Left),
        ("back", SnapView::Back),
        ("front", SnapView::Front),
        ("right", SnapView::Right),
        ("bottom", SnapView::Bottom),
    ] {
        assert_eq!(s.parse::<SnapView>(), Ok(v));
    }
    assert!("iso".parse::<SnapView>().is_err());
}

// ------------------------------------------------------- selection (M5a)

/// Two models at gz (0, 0, 0.5) and gz (0, 3, 0.5), each a link holding
/// a cube: (rig, near model, far model). Ctrl toggles (Linux); the views
/// select, as in the full UI.
fn two_models() -> (Rig, Entity, Entity) {
    let mut r = rig(default_view());
    r.app.insert_resource(SelectInViews(true));
    r.app.world_mut().resource_mut::<InputMap>().platform = Platform::Linux;
    let (root, a) = r.world_root();
    let b = r
        .app
        .world_mut()
        .spawn((Transform::default(), Visibility::default(), ChildOf(root)))
        .id();
    for (model, y) in [(a, 0.0), (b, 3.0)] {
        let link = r
            .app
            .world_mut()
            .spawn((
                Transform::from_xyz(0.0, y, 0.5),
                Visibility::default(),
                ChildOf(model),
            ))
            .id();
        r.cube(Some(link), Vec3::ZERO, 0.5);
    }
    r.make_visible();
    (r, a, b)
}

fn on_screen(r: &Rig, gz: Vec3) -> Vec2 {
    r.screen(gz_to_bevy() * gz)
}

fn selected(r: &Rig) -> Vec<Entity> {
    r.app.world().resource::<Selection>().entities.clone()
}

fn click(r: &mut Rig, at: Vec2, modifiers: &[KeyCode]) {
    for k in modifiers {
        r.key(*k, ButtonState::Pressed);
    }
    r.drag(MouseButton::Left, at, &[]);
    for k in modifiers {
        r.key(*k, ButtonState::Released);
    }
    r.app.update();
}

#[test]
fn a_click_selects_the_model_under_it_and_empty_space_clears() {
    let (mut r, a, b) = two_models();
    let pa = on_screen(&r, Vec3::new(0.0, 0.0, 0.5));
    let pb = on_screen(&r, Vec3::new(0.0, 3.0, 0.5));
    // The mesh is a link's child: the click picks the top-level model.
    click(&mut r, pa, &[]);
    assert_eq!(selected(&r), [a]);
    click(&mut r, pb, &[]);
    assert_eq!(selected(&r), [b]);
    click(&mut r, Vec2::new(400.0, 5.0), &[]);
    assert_eq!(selected(&r), []);
}

#[test]
fn shift_adds_and_ctrl_toggles() {
    let (mut r, a, b) = two_models();
    let pa = on_screen(&r, Vec3::new(0.0, 0.0, 0.5));
    let pb = on_screen(&r, Vec3::new(0.0, 3.0, 0.5));
    click(&mut r, pa, &[]);
    click(&mut r, pb, &[KeyCode::ShiftLeft]);
    assert_eq!(selected(&r), [a, b]);
    click(&mut r, pa, &[KeyCode::ShiftLeft]);
    assert_eq!(selected(&r), [a, b], "Shift never removes");
    click(&mut r, pa, &[KeyCode::ControlLeft]);
    assert_eq!(selected(&r), [b]);
    click(&mut r, pa, &[KeyCode::ControlLeft]);
    assert_eq!(selected(&r), [b, a]);
    // Neither changes anything on empty space.
    click(&mut r, Vec2::new(400.0, 5.0), &[KeyCode::ShiftLeft]);
    click(&mut r, Vec2::new(400.0, 5.0), &[KeyCode::ControlLeft]);
    assert_eq!(selected(&r), [b, a]);
}

#[test]
fn a_drag_past_the_slop_orbits_and_selects_nothing() {
    let (mut r, a, _) = two_models();
    let pa = on_screen(&r, Vec3::new(0.0, 0.0, 0.5));
    // Within the slop: a click.
    r.drag(MouseButton::Left, pa, &[pa + Vec2::new(2.0, 1.0)]);
    assert_eq!(selected(&r), [a]);
    click(&mut r, Vec2::new(400.0, 5.0), &[]);
    let before = r.transform();
    r.drag(MouseButton::Left, pa, &[pa + Vec2::new(20.0, 0.0)]);
    assert_eq!(selected(&r), []);
    assert_ne!(r.transform(), before, "it orbited");
    // A right click never selects.
    r.drag(MouseButton::Right, pa, &[]);
    assert_eq!(selected(&r), []);
}

#[test]
fn ctrl_drag_draws_a_marquee_and_selects_what_it_overlaps() {
    let (mut r, a, b) = two_models();
    let pa = on_screen(&r, Vec3::new(0.0, 0.0, 0.5));
    let pb = on_screen(&r, Vec3::new(0.0, 3.0, 0.5));
    let before = r.transform();
    // From beside the near model to the far one's middle.
    let from = pa + Vec2::new(0.0, 60.0);
    r.key(KeyCode::ControlLeft, ButtonState::Pressed);
    r.cursor(Some(from));
    r.button(MouseButton::Left, ButtonState::Pressed);
    r.app.update();
    r.cursor(Some(pb));
    r.app.update();
    let m = r.app.world().resource::<Marquee>().0.expect("drawing");
    assert_eq!((m.from, m.to), (from, pb));
    r.button(MouseButton::Left, ButtonState::Released);
    r.app.update();
    r.key(KeyCode::ControlLeft, ButtonState::Released);
    r.app.update();
    assert_eq!(r.app.world().resource::<Marquee>().0, None);
    // The ⌘/Ctrl that started it doesn't toggle: it replaces.
    let mut got = selected(&r);
    got.sort();
    let mut want = vec![a, b];
    want.sort();
    assert_eq!(got, want);
    assert_eq!(r.transform(), before, "the camera stayed put");
}

#[test]
fn the_box_select_tool_draws_a_marquee_with_a_plain_drag() {
    let (mut r, a, b) = two_models();
    r.app.world_mut().resource_mut::<BoxSelectTool>().0 = true;
    let pa = on_screen(&r, Vec3::new(0.0, 0.0, 0.5));
    let pb = on_screen(&r, Vec3::new(0.0, 3.0, 0.5));
    let before = r.transform();
    // Around the near model only.
    r.drag(
        MouseButton::Left,
        pa - Vec2::splat(10.0),
        &[pa + Vec2::splat(10.0)],
    );
    assert_eq!(selected(&r), [a]);
    // Shift adds; ⌘/Ctrl toggles with this tool.
    r.key(KeyCode::ShiftLeft, ButtonState::Pressed);
    r.drag(
        MouseButton::Left,
        pb - Vec2::splat(10.0),
        &[pb + Vec2::splat(10.0)],
    );
    r.key(KeyCode::ShiftLeft, ButtonState::Released);
    assert_eq!(selected(&r), [a, b]);
    r.key(KeyCode::ControlLeft, ButtonState::Pressed);
    r.drag(
        MouseButton::Left,
        pa - Vec2::splat(10.0),
        &[pa + Vec2::splat(10.0)],
    );
    r.key(KeyCode::ControlLeft, ButtonState::Released);
    assert_eq!(selected(&r), [b]);
    // A marquee on nothing clears; a click still picks.
    r.drag(
        MouseButton::Left,
        Vec2::new(10.0, 10.0),
        &[Vec2::new(40.0, 40.0)],
    );
    assert_eq!(selected(&r), []);
    click(&mut r, pb, &[]);
    assert_eq!(selected(&r), [b]);
    assert_eq!(r.transform(), before);
}

#[test]
fn a_marquee_leaves_out_hidden_models_and_ground_planes() {
    let (mut r, a, b) = two_models();
    let (root, ground) = {
        let world = r.app.world_mut();
        let root = world
            .query_filtered::<Entity, With<GzWorldRoot>>()
            .single(world)
            .unwrap();
        let ground = world
            .spawn((Transform::default(), Visibility::default(), ChildOf(root)))
            .id();
        (root, ground)
    };
    let _ = root;
    let plane = r.cube(Some(ground), Vec3::new(0.0, 0.0, -0.05), 50.0);
    r.app.world_mut().entity_mut(plane).insert(plane_visual());
    r.app.world_mut().entity_mut(b).insert(Visibility::Hidden);
    r.make_visible();
    r.app.world_mut().resource_mut::<BoxSelectTool>().0 = true;
    r.drag(
        MouseButton::Left,
        Vec2::new(1.0, 1.0),
        &[Vec2::new(799.0, 599.0)],
    );
    assert_eq!(selected(&r), [a]);
}

#[test]
fn a_despawned_model_leaves_the_selection() {
    let (mut r, a, b) = two_models();
    r.app.world_mut().resource_mut::<Selection>().entities = vec![a, b];
    r.app.world_mut().entity_mut(a).despawn();
    r.app.update();
    assert_eq!(selected(&r), [b]);
}

#[test]
fn select_modes_replace_add_and_toggle() {
    let (a, b, c) = (
        Entity::from_raw_u32(1).unwrap(),
        Entity::from_raw_u32(2).unwrap(),
        Entity::from_raw_u32(3).unwrap(),
    );
    let mut s = Selection::default();
    let mut step = |mode, picked: &[Entity]| {
        s.entities = s.applied(mode, picked);
        s.entities.clone()
    };
    assert_eq!(step(SelectMode::Replace, &[a, b, a]), [a, b]);
    assert_eq!(step(SelectMode::Add, &[b, c]), [a, b, c]);
    assert_eq!(step(SelectMode::Toggle, &[a, c]), [b]);
    assert_eq!(step(SelectMode::Replace, &[]), []);
    assert_eq!(SelectMode::from_keys(true, true), SelectMode::Toggle);
    assert_eq!(SelectMode::from_keys(true, false), SelectMode::Add);
}

#[test]
fn views_select_only_in_the_full_ui() {
    let (mut r, a, _) = two_models();
    r.app.insert_resource(SelectInViews(false));
    let pa = on_screen(&r, Vec3::new(0.0, 0.0, 0.5));
    click(&mut r, pa, &[]);
    assert_eq!(selected(&r), []);
    // ⌘/Ctrl+drag orbits, as before there was a selection.
    let before = r.transform();
    r.key(KeyCode::ControlLeft, ButtonState::Pressed);
    r.drag(MouseButton::Left, pa, &[pa + Vec2::new(40.0, 0.0)]);
    r.key(KeyCode::ControlLeft, ButtonState::Released);
    assert_ne!(r.transform(), before);
    assert_eq!(r.app.world().resource::<Marquee>().0, None);
    let _ = a;
}

#[test]
fn esc_cancels_a_marquee() {
    let (mut r, a, _) = two_models();
    r.app.world_mut().resource_mut::<BoxSelectTool>().0 = true;
    let pa = on_screen(&r, Vec3::new(0.0, 0.0, 0.5));
    r.cursor(Some(pa - Vec2::splat(10.0)));
    r.button(MouseButton::Left, ButtonState::Pressed);
    r.app.update();
    r.cursor(Some(pa + Vec2::splat(10.0)));
    r.app.update();
    assert!(r.app.world().resource::<Marquee>().0.is_some());
    r.key(KeyCode::Escape, ButtonState::Pressed);
    r.app.update();
    r.key(KeyCode::Escape, ButtonState::Released);
    assert_eq!(r.app.world().resource::<Marquee>().0, None);
    r.button(MouseButton::Left, ButtonState::Released);
    r.app.update();
    assert_eq!(selected(&r), [], "the cancelled marquee selects nothing");
    let _ = a;
}

#[test]
fn a_click_on_the_ground_is_empty_space() {
    let (mut r, a, _) = two_models();
    let root = {
        let world = r.app.world_mut();
        world
            .query_filtered::<Entity, With<GzWorldRoot>>()
            .single(world)
            .unwrap()
    };
    let ground = r
        .app
        .world_mut()
        .spawn((Transform::default(), Visibility::default(), ChildOf(root)))
        .id();
    let plane = r.cube(Some(ground), Vec3::new(0.0, 0.0, -0.05), 50.0);
    r.app.world_mut().entity_mut(plane).insert(plane_visual());
    r.make_visible();
    let pa = on_screen(&r, Vec3::new(0.0, 0.0, 0.5));
    click(&mut r, pa, &[]);
    assert_eq!(selected(&r), [a]);
    // Below the models: the ground plane, which clears.
    click(&mut r, Vec2::new(400.0, 590.0), &[]);
    assert_eq!(selected(&r), []);
}

#[test]
fn a_space_pan_click_and_a_flat_marquee() {
    let (mut r, a, _) = two_models();
    let pa = on_screen(&r, Vec3::new(0.0, 0.0, 0.5));
    // Space held: a temporary pan, which never selects.
    r.key(KeyCode::Space, ButtonState::Pressed);
    r.drag(MouseButton::Left, pa, &[]);
    r.key(KeyCode::Space, ButtonState::Released);
    r.app.update();
    assert_eq!(selected(&r), []);
    // A marquee dragged straight across the model still picks it.
    r.app.world_mut().resource_mut::<BoxSelectTool>().0 = true;
    r.drag(
        MouseButton::Left,
        pa - Vec2::new(30.0, 0.0),
        &[pa + Vec2::new(30.0, 0.0)],
    );
    assert_eq!(selected(&r), [a]);
}

#[test]
fn a_box_reaching_behind_the_camera_covers_the_screen_it_fills() {
    // From 10 m in front of the eye to 10 m behind it, off to the left:
    // its front corners alone stop short of the screen's left edge; the
    // part between them and the eye reaches past it.
    let t = Transform::default();
    let p = Projection::Perspective(PerspectiveProjection::default());
    let b = Bounds {
        min: Vec3::new(-5.0, -1.0, -10.0),
        max: Vec3::new(-1.0, 1.0, 10.0),
    };
    let size = Vec2::new(800.0, 600.0);
    let r = select::screen_rect((&t, &p), size, b.corners()).unwrap();
    assert!(r.min.x < 0.0, "{r:?}");
    assert!(r.max.x > 0.0 && r.max.x < 400.0, "{r:?}");
    // All behind: nothing.
    let behind = Bounds {
        min: Vec3::new(-1.0, -1.0, 1.0),
        max: Vec3::new(1.0, 1.0, 3.0),
    };
    assert!(select::screen_rect((&t, &p), size, behind.corners()).is_none());
}

#[test]
fn mod_toggles_with_ctrl_or_cmd_in_a_browser() {
    use super::input::KeyMods;
    let ctrl = KeyMods {
        ctrl: true,
        ..default()
    };
    let cmd = KeyMods {
        super_key: true,
        ..default()
    };
    assert!(ctrl.mod_key(Platform::Web) && cmd.mod_key(Platform::Web));
    assert!(cmd.mod_key(Platform::Mac) && !ctrl.mod_key(Platform::Mac));
    assert!(ctrl.mod_key(Platform::Linux) && !cmd.mod_key(Platform::Linux));
}

#[test]
fn focus_on_a_joint_or_sensor_frames_its_owner() {
    let (mut r, _, b) = two_models();
    r.app.world_mut().resource_mut::<Selection>().parts = vec![Part {
        kind: PartKind::Joint,
        owner: b,
        name: "j".into(),
    }];
    r.op(ViewOp::Focus { at: None });
    r.settle();
    // The far model's cube, gz (0, 3, 0.5): Bevy (0, 0.5, -3).
    assert_near(r.pivot(), Vec3::new(0.0, 0.5, -3.0), 1e-3);
}

#[test]
fn follow_with_a_joint_or_sensor_selected_follows_its_owner() {
    let (mut r, a, _) = two_models();
    r.app.world_mut().resource_mut::<Selection>().parts = vec![Part {
        kind: PartKind::Sensor,
        owner: a,
        name: "s".into(),
    }];
    r.op(ViewOp::Follow { at: None });
    assert_eq!(r.view().mode, CameraMode::Follow(a));
}

// ------------------------------------------------------- transform gizmo (M5c)

/// `two_models` with the near model a Gazebo model the tools move, edits
/// allowed, and the gizmo's input in front of the views' controls.
fn gizmo_rig(tool: gizmo::TransformTool) -> (Rig, Entity) {
    let (mut r, a, _) = two_models();
    r.app
        .world_mut()
        .entity_mut(a)
        .insert(crate::spawn::GzEntity {
            name: "a".into(),
            id: 3,
            kind: gz_core::scene::NodeKind::Model,
            path: "a".into(),
        });
    {
        let mut status = r.app.world_mut().resource_mut::<GzStatus>();
        status.status = gz_core::transport::Status::Ready;
        status.world = Some("w".into());
        status.edits = Some(true);
    }
    r.app
        .insert_resource(tool)
        .init_resource::<gizmo::Snap>()
        .init_resource::<gizmo::GizmoState>()
        .add_plugins((crate::edit::EditPlugin, crate::undo::UndoPlugin))
        .add_systems(Update, gizmo::gizmo_input.before(controls::mouse_input));
    r.app.world_mut().resource_mut::<Selection>().entities = vec![a];
    r.app.update();
    (r, a)
}

fn set_poses(r: &mut Rig) -> Vec<GzCommand> {
    r.app
        .world_mut()
        .resource_mut::<Messages<GzCommand>>()
        .drain()
        .filter(
            |c| matches!(c, GzCommand::Request { service, .. } if service.ends_with("set_pose")),
        )
        .collect()
}

/// The screen point `share` of the way along Gazebo axis `i`'s arrow.
fn arrow_point(r: &Rig, i: usize, share: f32) -> Vec2 {
    let mut axis = Vec3::ZERO;
    axis[i] = 1.0;
    let world = gz_to_bevy() * axis;
    let len = gizmo::GIZMO_PX
        * gizmo::metres_per_px(
            &r.transform(),
            &r.projection(),
            Vec2::new(800.0, 600.0),
            Vec3::ZERO,
        );
    r.screen(world * len * share)
}

#[test]
fn dragging_an_arrow_moves_the_model_along_it_and_sends_its_pose() {
    let (mut r, a) = gizmo_rig(gizmo::TransformTool::Translate);
    let before = r.transform();
    let from = arrow_point(&r, 0, 0.8);
    let to = arrow_point(&r, 0, 1.6);
    r.cursor(Some(from));
    r.button(MouseButton::Left, ButtonState::Pressed);
    r.app.update();
    assert!(r.app.world().resource::<gizmo::GizmoState>().captures());
    assert!(r.app.world().resource::<crate::edit::Held>().holds(a, 0.0));
    r.cursor(Some(to));
    r.app.update();
    let moved = r.app.world().get::<Transform>(a).unwrap().translation;
    // Gazebo +X, about 0.8 arrow lengths; nothing else.
    assert!(
        moved.x > 0.1 && moved.y.abs() < 1e-3 && moved.z.abs() < 1e-3,
        "{moved}"
    );
    r.button(MouseButton::Left, ButtonState::Released);
    r.app.update();
    r.app.update();
    assert!(!r.app.world().resource::<gizmo::GizmoState>().captures());
    let sent = set_poses(&mut r);
    assert!(!sent.is_empty(), "the edit went out");
    // The camera didn't orbit, and the release selected nothing new.
    assert_eq!(r.transform(), before);
    assert_eq!(selected(&r), [a]);
    // One edit to undo, back to where the press began.
    let stack = r.app.world().resource::<crate::undo::UndoStack>();
    assert_eq!(stack.undo_label(), Some("move a"));
    r.app
        .world_mut()
        .write_message(crate::undo::UndoCommand::Undo);
    r.app.update();
    assert_eq!(
        r.app.world().get::<Transform>(a).unwrap().translation.x,
        0.0
    );
}

#[test]
fn shift_snaps_and_esc_puts_it_back() {
    let (mut r, a) = gizmo_rig(gizmo::TransformTool::Translate);
    let start = *r.app.world().get::<Transform>(a).unwrap();
    r.cursor(Some(arrow_point(&r, 0, 0.8)));
    r.button(MouseButton::Left, ButtonState::Pressed);
    r.app.update();
    r.key(KeyCode::ShiftLeft, ButtonState::Pressed);
    r.cursor(Some(arrow_point(&r, 0, 1.3)));
    r.app.update();
    let x = r.app.world().get::<Transform>(a).unwrap().translation.x;
    assert!(
        (x / 0.25 - (x / 0.25).round()).abs() < 1e-4 && x > 0.0,
        "{x}"
    );
    assert!(r
        .app
        .world()
        .resource::<gizmo::GizmoState>()
        .readout
        .is_some());
    r.key(KeyCode::Escape, ButtonState::Pressed);
    r.app.update();
    assert_eq!(*r.app.world().get::<Transform>(a).unwrap(), start);
    // Put back: nothing to undo.
    let stack = r.app.world().resource::<crate::undo::UndoStack>();
    assert!(stack.undo_label().is_none() && !stack.is_dirty());
    let state = r.app.world().resource::<gizmo::GizmoState>();
    // The Esc was the gizmo's (the shell's Esc → Navigate waits).
    assert!(!state.captures() && state.takes_escape());
    r.key(KeyCode::Escape, ButtonState::Released);
    r.app.update();
    assert!(!r.app.world().resource::<gizmo::GizmoState>().takes_escape());
}

#[test]
fn a_press_released_in_place_sends_nothing_and_ctrl_z_is_not_z() {
    let (mut r, _) = gizmo_rig(gizmo::TransformTool::Translate);
    r.cursor(Some(arrow_point(&r, 0, 0.8)));
    r.button(MouseButton::Left, ButtonState::Pressed);
    r.app.update();
    r.key(KeyCode::ControlLeft, ButtonState::Pressed);
    r.key(KeyCode::KeyZ, ButtonState::Pressed);
    r.app.update();
    let handle = r
        .app
        .world()
        .resource::<gizmo::GizmoState>()
        .drag
        .as_ref()
        .unwrap()
        .handle;
    assert_eq!(handle, gizmo::Handle::Axis(0));
    r.key(KeyCode::ControlLeft, ButtonState::Released);
    r.key(KeyCode::KeyZ, ButtonState::Released);
    r.button(MouseButton::Left, ButtonState::Released);
    r.app.update();
    // A moving model isn't snapped back to where the press found it.
    assert!(set_poses(&mut r).is_empty());
}

#[test]
fn a_press_away_from_the_gizmo_is_the_views_and_no_edits_means_no_gizmo() {
    let (mut r, a) = gizmo_rig(gizmo::TransformTool::Translate);
    let before = r.transform();
    r.drag(
        MouseButton::Left,
        Vec2::new(700.0, 100.0),
        &[Vec2::new(740.0, 100.0)],
    );
    assert_ne!(r.transform(), before, "it orbited");
    // A server that mangles edits: the arrow is just the scene.
    r.app.world_mut().resource_mut::<GzStatus>().edits = Some(false);
    r.cursor(Some(arrow_point(&r, 0, 0.8)));
    r.button(MouseButton::Left, ButtonState::Pressed);
    r.app.update();
    assert!(!r.app.world().resource::<gizmo::GizmoState>().captures());
    r.button(MouseButton::Left, ButtonState::Released);
    r.app.update();
    let _ = a;
}

#[test]
fn the_plugin_hovers_draws_and_a_ring_turns_the_model() {
    let (mut r, a, _) = two_models();
    r.app
        .world_mut()
        .entity_mut(a)
        .insert(crate::spawn::GzEntity {
            name: "a".into(),
            id: 3,
            kind: gz_core::scene::NodeKind::Model,
            path: "a".into(),
        });
    {
        let mut status = r.app.world_mut().resource_mut::<GzStatus>();
        status.status = gz_core::transport::Status::Ready;
        status.world = Some("w".into());
        status.edits = Some(true);
    }
    r.app
        .add_plugins((
            AssetPlugin::default(),
            bevy::gizmos::GizmoPlugin,
            crate::edit::EditPlugin,
            crate::undo::UndoPlugin,
            gizmo::GizmoPlugin,
        ))
        .insert_resource(gizmo::TransformTool::Rotate)
        // bevy_gizmos' skinned-mesh bounds want it; the render plugins add it.
        .init_asset::<bevy::mesh::skinning::SkinnedMeshInverseBindposes>();
    r.app.world_mut().resource_mut::<Selection>().entities = vec![a];
    r.app.update();
    // Points on the ring about Gazebo Z, at +X and +Y.
    let ring = |r: &Rig, axis: Vec3| {
        let len = gizmo::GIZMO_PX
            * gizmo::metres_per_px(
                &r.transform(),
                &r.projection(),
                Vec2::new(800.0, 600.0),
                Vec3::ZERO,
            );
        r.screen(gz_to_bevy() * axis * len * gizmo::RING)
    };
    r.cursor(Some(ring(&r, Vec3::X)));
    // Drawn in another view last frame: nothing to grab here yet (a press
    // that made this view active doesn't take the gizmo).
    r.app
        .world_mut()
        .resource_mut::<gizmo::GizmoState>()
        .shown_in = Some(ViewId(3));
    r.app.update();
    let state = r.app.world().resource::<gizmo::GizmoState>();
    assert_eq!((state.hovered, state.shown_in), (None, None));
    r.app.update();
    let state = r.app.world().resource::<gizmo::GizmoState>();
    assert_eq!(state.hovered, Some(gizmo::Handle::Ring(2)));
    r.button(MouseButton::Left, ButtonState::Pressed);
    r.app.update();
    r.cursor(Some(ring(&r, Vec3::Y)));
    r.app.update();
    let (axis, angle) = r
        .app
        .world()
        .get::<Transform>(a)
        .unwrap()
        .rotation
        .to_axis_angle();
    assert!(
        axis.z > 0.99 && (angle - FRAC_PI_2).abs() < 0.05,
        "{axis} {angle}"
    );
    r.button(MouseButton::Left, ButtonState::Released);
    r.app.update();
    assert!(!set_poses(&mut r).is_empty());
    // The gizmo draws in the active view only (its own layer).
    let layers = |r: &Rig| {
        r.app
            .world()
            .resource::<GizmoConfigStore>()
            .config::<gizmo::TransformGizmos>()
            .0
            .render_layers
            .clone()
    };
    assert_eq!(layers(&r), RenderLayers::default());
    r.app
        .world_mut()
        .resource_mut::<controls::ViewRouting>()
        .active = Some(ViewId(1));
    r.app.update();
    assert_eq!(layers(&r), RenderLayers::layer(ViewId(1).layer()));
    r.app
        .world_mut()
        .resource_mut::<controls::ViewRouting>()
        .active = None;
    // A turn to undo and redo.
    r.app.update();
    let label = |r: &Rig| {
        r.app
            .world()
            .resource::<crate::undo::UndoStack>()
            .undo_label()
            .map(str::to_string)
    };
    assert_eq!(label(&r).as_deref(), Some("rotate a"));
    let turned = r.app.world().get::<Transform>(a).unwrap().rotation;
    for (cmd, want) in [
        (crate::undo::UndoCommand::Undo, Quat::IDENTITY),
        (crate::undo::UndoCommand::Redo, turned),
    ] {
        r.app.world_mut().write_message(cmd);
        r.app.update();
        let now = r.app.world().get::<Transform>(a).unwrap().rotation;
        assert!(now.angle_between(want) < 1e-4, "{cmd:?}");
    }
}

/// A second view beside the rig's: each half the window, the rig's on the
/// left as view 0 (DECISIONS.md D-90).
fn two_views(r: &mut Rig) -> Entity {
    let half = |w: &mut World, e: Entity, x: f32| {
        let mut camera = w.get_mut::<Camera>(e).unwrap();
        camera.computed.target_info = Some(RenderTargetInfo {
            physical_size: UVec2::new((W / 2.0) as u32, H as u32),
            scale_factor: 1.0,
        });
        w.entity_mut(e).insert(ViewOrigin(Vec2::new(x, 0.0)));
    };
    let w = r.app.world_mut();
    w.entity_mut(r.camera).insert(ViewId(0));
    half(w, r.camera, 0.0);
    let camera = w.get::<Camera>(r.camera).unwrap().clone();
    let other = w
        .spawn((
            camera,
            Projection::Perspective(gz_perspective()),
            default_view(),
            ViewCamera::new(Vec3::ZERO, gz_perspective()),
            ViewId(1),
        ))
        .id();
    half(w, other, W / 2.0);
    other
}

#[test]
fn the_pointer_drives_the_view_under_it_and_keys_the_hovered_or_active_one() {
    let mut r = rig(default_view());
    let other = two_views(&mut r);
    let pose = |r: &Rig, e: Entity| *r.app.world().get::<Transform>(e).unwrap();
    let (left, right) = (pose(&r, r.camera), pose(&r, other));
    let set = |r: &mut Rig, hovered: Option<u8>, active: Option<u8>| {
        let mut routing = r.app.world_mut().resource_mut::<controls::ViewRouting>();
        routing.hovered = hovered.map(ViewId);
        routing.active = active.map(ViewId);
    };
    // A drag over the right view orbits it, and keeps it while the pointer
    // crosses into the left one.
    set(&mut r, Some(1), Some(1));
    let at = Vec2::new(W * 0.75, H / 2.0);
    r.cursor(Some(at));
    r.button(MouseButton::Left, ButtonState::Pressed);
    r.app.update();
    for x in [W * 0.6, W * 0.4, W * 0.3] {
        // Into the left view, which the shell now says is hovered.
        set(&mut r, Some(u8::from(x < W / 2.0)).map(|l| 1 - l), Some(1));
        r.cursor(Some(Vec2::new(x, H / 2.0)));
        r.app.update();
    }
    r.button(MouseButton::Left, ButtonState::Released);
    r.app.update();
    assert_eq!(pose(&r, r.camera), left);
    assert_ne!(pose(&r, other), right);
    // Keys: the hovered view, else the active one.
    let moved = pose(&r, other);
    set(&mut r, Some(0), Some(1));
    r.chord(&[KeyCode::Digit7]);
    for _ in 0..30 {
        r.app.update();
    }
    assert_ne!(pose(&r, r.camera), left, "the hovered view snapped");
    assert_eq!(pose(&r, other), moved);
    set(&mut r, None, Some(1));
    r.chord(&[KeyCode::Digit1]);
    for _ in 0..30 {
        r.app.update();
    }
    assert_ne!(pose(&r, other), moved, "then the active one");
    // A view the layout drops (its camera off) gets nothing.
    r.app
        .world_mut()
        .get_mut::<Camera>(other)
        .unwrap()
        .is_active = false;
    let off = pose(&r, other);
    set(&mut r, None, Some(1));
    r.chord(&[KeyCode::Digit3]);
    for _ in 0..30 {
        r.app.update();
    }
    assert_eq!(pose(&r, other), off);
}

#[test]
fn linked_views_follow_pans_zooms_and_focus_but_not_orbits() {
    let mut r = rig(default_view());
    let other = two_views(&mut r);
    r.app.insert_resource(LinkedViews(true));
    let state = |r: &Rig, e: Entity| {
        let w = r.app.world();
        let t = *w.get::<Transform>(e).unwrap();
        let v = w.get::<ViewCamera>(e).unwrap().clone();
        let p = w.get::<Projection>(e).unwrap().clone();
        (t, v.pivot, reach(&v, &t, &p))
    };
    let send = |r: &mut Rig, view: Entity, op: ViewOp| {
        r.app.world_mut().write_message(ViewCommand { view, op });
        r.app.update();
    };
    let (t0, p0, d0) = state(&r, r.camera);
    // A pan in view 1 moves view 0's camera and pivot by as much.
    let (t1, ..) = state(&r, other);
    send(&mut r, other, ViewOp::Pan(Vec2::new(40.0, 0.0)));
    let moved = state(&r, other).0.translation - t1.translation;
    assert!(moved.length() > 1e-3);
    let (t, p, _) = state(&r, r.camera);
    assert!((t.translation - (t0.translation + moved)).length() < 1e-4);
    assert!((p - (p0 + moved)).length() < 1e-4);
    assert_eq!(t.rotation, t0.rotation, "its own direction");
    // A zoom scales view 0's distance by as much.
    let before = state(&r, other).2;
    send(&mut r, other, ViewOp::Zoom(3.0));
    let ratio = state(&r, other).2 / before;
    assert!(ratio < 1.0);
    let (_, _, d) = state(&r, r.camera);
    assert!((d - d0 * ratio).abs() < 1e-3, "{d} {}", d0 * ratio);
    // An orbit stays with its view.
    let still = state(&r, r.camera);
    send(&mut r, other, ViewOp::Orbit(Vec2::new(50.0, 10.0)));
    assert_eq!(state(&r, r.camera).0, still.0);
    // Home in view 1 takes view 0's pivot along as it animates: by the
    // same shift.
    let (p_a, p_b) = (state(&r, r.camera).1, state(&r, other).1);
    send(&mut r, other, ViewOp::Home);
    for _ in 0..40 {
        r.app.update();
    }
    let p_home = state(&r, other).1;
    assert!(p_home.length() < 1e-3);
    assert!((state(&r, r.camera).1 - (p_a + p_home - p_b)).length() < 1e-3);
    // Unlinked: nothing carries.
    r.app.insert_resource(LinkedViews(false));
    let still = state(&r, r.camera);
    send(&mut r, other, ViewOp::Pan(Vec2::new(40.0, 0.0)));
    assert_eq!(state(&r, r.camera).0, still.0);
}
