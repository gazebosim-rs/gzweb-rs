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

    /// A key that types `typed` on the user's layout (D-119).
    fn typed(&mut self, key_code: KeyCode, typed: &str, state: ButtonState) {
        let window = self.window;
        self.app.world_mut().write_message(KeyboardInput {
            key_code,
            logical_key: Key::Character(typed.into()),
            state,
            text: Some(typed.into()),
            repeat: false,
            window,
        });
    }

    /// Presses and releases a key that types `typed`.
    fn tap_typed(&mut self, key_code: KeyCode, typed: &str) {
        self.typed(key_code, typed, ButtonState::Pressed);
        self.app.update();
        self.typed(key_code, typed, ButtonState::Released);
        self.app.update();
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

fn box_visual() -> GzVisual {
    GzVisual(VisualDesc {
        proto_type: "box".into(),
        geometry: GeometryDesc::Box {
            size: [1.0, 1.0, 1.0],
        },
        ..plane_visual().0
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

/// With reduce-motion a snap, home, focus or look-from lands in the frame
/// it starts, where the animation would have ended (spec §10, D-118).
#[test]
fn reduced_motion_lands_each_move_at_once() {
    let mut r = rig(default_view());
    let (_, model) = r.world_root();
    let cube = r.cube(Some(model), Vec3::new(2.0, 0.0, 0.5), 0.5);
    r.app.world_mut().insert_resource(ReduceMotion(true));
    r.op(ViewOp::Snap(SnapView::Top));
    assert!(!r.view().animating());
    assert_eq!(r.view().snapped, Some(SnapView::Top));
    assert!(r.ortho().is_some());
    r.op(ViewOp::Home);
    assert!(!r.view().animating());
    let home = home_transform();
    assert_near(r.transform().translation, home.translation, 1e-4);
    r.op(ViewOp::LookFrom(Vec3::new(1.0, 1.0, 1.0)));
    assert!(!r.view().animating());
    let back = gz_to_bevy().inverse() * *r.transform().back();
    assert_near(back, Vec3::ONE.normalize(), 1e-4);
    r.app.world_mut().resource_mut::<Selection>().entities = vec![cube];
    r.op(ViewOp::Focus { at: None });
    assert!(!r.view().animating());
    assert_near(r.pivot(), Vec3::new(2.0, 0.5, 0.0), 1e-3);
    // Off again, a move animates.
    r.app.world_mut().insert_resource(ReduceMotion(false));
    r.op(ViewOp::Home);
    assert!(r.view().animating());
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

/// D-119: a letter is the key that types it on the user's layout (Dvorak's
/// G is `KeyU`, and its `KeyG` types I); the fly keys keep their places
/// (AZERTY's `KeyW` types Z and flies forward; its `KeyZ`, W, doesn't).
#[test]
fn letters_follow_the_layout_and_the_fly_keys_their_places() {
    use crate::overlays::Overlays;
    let mut r = rig(default_view());
    r.tap_typed(KeyCode::KeyU, "g");
    assert!(r.app.world().resource::<Overlays>().grid);
    r.tap_typed(KeyCode::KeyG, "i");
    let o = r.app.world().resource::<Overlays>();
    assert!(o.grid && o.inertia);

    r.typed(KeyCode::KeyZ, "w", ButtonState::Pressed);
    r.frames(5);
    assert_eq!(r.transform(), default_view());
    r.typed(KeyCode::KeyZ, "w", ButtonState::Released);
    r.typed(KeyCode::KeyW, "z", ButtonState::Pressed);
    r.frames(5);
    let moved = r.transform().translation - default_view().translation;
    assert!(moved.x > 0.0, "{moved}");
}

/// While Teleop drives (armed), W A S D are its: the camera stays.
#[test]
fn an_armed_teleop_takes_the_fly_keys() {
    let mut r = rig(default_view());
    r.app.insert_resource(DriveArmed(true));
    r.key(KeyCode::KeyW, ButtonState::Pressed);
    r.frames(5);
    assert_eq!(r.transform(), default_view());
    r.app.insert_resource(DriveArmed(false));
    r.frames(5);
    assert_ne!(r.transform(), default_view());
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

/// A run binding does what its key does (the palette, D-107); a held
/// action's binding does nothing, and an id that isn't one is ignored.
#[test]
fn a_run_binding_acts_as_its_key() {
    use crate::overlays::Overlays;
    use crate::view::controls::RunBinding;
    let mut r = rig(default_view());
    let run = |r: &mut Rig, id: &str| {
        r.app.world_mut().write_message(RunBinding(id.into()));
        r.frames(1);
    };
    run(&mut r, "overlay.grid");
    assert!(r.app.world().resource::<Overlays>().grid);
    run(&mut r, "overlay.grid");
    assert!(!r.app.world().resource::<Overlays>().grid);
    let before = r.pivot();
    run(&mut r, "view.fly_forward");
    run(&mut r, "nope");
    r.frames(2);
    assert_eq!(r.pivot(), before);
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

/// A right click on a model asks for its context menu, selecting it
/// unless it already is (a selection it's in stays); on empty space, or
/// after a right drag (a pan), nothing.
#[test]
fn a_right_click_asks_for_the_context_menu() {
    use crate::view::EntityContext;
    let (mut r, a, b) = two_models();
    let pa = on_screen(&r, Vec3::new(0.0, 0.0, 0.5));
    let pb = on_screen(&r, Vec3::new(0.0, 3.0, 0.5));
    let asked = |r: &mut Rig| -> Vec<Entity> {
        r.app
            .world_mut()
            .resource_mut::<Messages<EntityContext>>()
            .drain()
            .map(|c| c.entity)
            .collect()
    };
    r.drag(MouseButton::Right, pa, &[]);
    r.app.update();
    assert_eq!(asked(&mut r), [a]);
    assert_eq!(selected(&r), [a]);
    click(&mut r, pb, &[KeyCode::ShiftLeft]);
    r.drag(MouseButton::Right, pb, &[]);
    r.app.update();
    assert_eq!(asked(&mut r), [b]);
    assert_eq!(selected(&r), [a, b], "a selection it's in stays");
    r.drag(MouseButton::Right, Vec2::new(400.0, 5.0), &[]);
    r.app.update();
    assert!(asked(&mut r).is_empty());
    r.drag(MouseButton::Right, pa, &[Vec2::new(60.0, 0.0)]);
    r.app.update();
    assert!(asked(&mut r).is_empty(), "a pan");
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
    // A right click selects the model it opens the context menu for.
    r.drag(MouseButton::Right, pa, &[]);
    assert_eq!(selected(&r), [a]);
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

/// AZERTY's Z is on fly forward's key: during a drag it constrains to Z,
/// and the camera stays (a drag holds the fly keys, D-119).
#[test]
fn a_drag_constrains_by_the_letter_and_holds_the_fly_keys() {
    let (mut r, _) = gizmo_rig(gizmo::TransformTool::Translate);
    let before = r.transform();
    r.cursor(Some(arrow_point(&r, 0, 0.8)));
    r.button(MouseButton::Left, ButtonState::Pressed);
    r.app.update();
    r.typed(KeyCode::KeyW, "z", ButtonState::Pressed);
    r.frames(3);
    let handle = |r: &Rig| {
        r.app
            .world()
            .resource::<gizmo::GizmoState>()
            .drag
            .as_ref()
            .unwrap()
            .handle
    };
    assert_eq!(handle(&r), gizmo::Handle::Axis(2));
    assert_eq!(r.transform(), before);
    // `KeyZ` types W there: no axis.
    r.typed(KeyCode::KeyY, "y", ButtonState::Pressed);
    r.app.update();
    assert_eq!(handle(&r), gizmo::Handle::Axis(1));
    r.typed(KeyCode::KeyZ, "w", ButtonState::Pressed);
    r.app.update();
    assert_eq!(handle(&r), gizmo::Handle::Axis(1));
    // Z still held as the drag ends: no flying until it is pressed again.
    r.button(MouseButton::Left, ButtonState::Released);
    r.frames(4);
    assert!(!r.app.world().resource::<gizmo::GizmoState>().captures());
    assert_eq!(r.transform(), before);
    r.typed(KeyCode::KeyW, "z", ButtonState::Released);
    r.frames(2);
    assert_eq!(r.transform(), before);
    r.typed(KeyCode::KeyW, "z", ButtonState::Pressed);
    r.frames(3);
    assert_ne!(r.transform(), before);
}

/// While Teleop drives, a press on an arrow doesn't start a drag.
#[test]
fn an_armed_teleop_leaves_the_gizmo_alone() {
    let (mut r, a) = gizmo_rig(gizmo::TransformTool::Translate);
    r.app.insert_resource(DriveArmed(true));
    let from = arrow_point(&r, 0, 0.8);
    r.cursor(Some(from));
    r.button(MouseButton::Left, ButtonState::Pressed);
    r.app.update();
    assert!(!r.app.world().resource::<gizmo::GizmoState>().captures());
    r.cursor(Some(arrow_point(&r, 0, 1.6)));
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

#[test]
fn linked_wheel_zooms_carry_the_zoom_not_the_pivot_move() {
    let mut r = rig(default_view());
    let other = two_views(&mut r);
    r.app.insert_resource(LinkedViews(true));
    let send = |r: &mut Rig, view: Entity, op: ViewOp| {
        r.app.world_mut().write_message(ViewCommand { view, op });
        r.app.update();
    };
    let dist = |r: &Rig, e: Entity| {
        let w = r.app.world();
        let t = w.get::<Transform>(e).unwrap();
        (t.translation - w.get::<ViewCamera>(e).unwrap().pivot).length()
    };
    // Off-centre and off-axis: the wheel's zoom about the point under
    // the cursor moves view 1's pivot there first.
    let d0 = dist(&r, r.camera);
    send(
        &mut r,
        other,
        ViewOp::ZoomAt {
            at: Vec2::new(W / 2.0 * 0.15, H * 0.3),
            steps: 1.0,
        },
    );
    let ratio = dist(&r, r.camera) / d0;
    assert!((ratio - ZOOM_STEP).abs() < 1e-3, "{ratio}");
}

#[test]
fn a_linked_pan_mid_snap_carries_the_pan_only() {
    let mut r = rig(default_view());
    let other = two_views(&mut r);
    r.app.insert_resource(LinkedViews(true));
    let pivot = |r: &Rig, e: Entity| r.app.world().get::<ViewCamera>(e).unwrap().pivot;
    let send = |r: &mut Rig, view: Entity, op: ViewOp| {
        r.app.world_mut().write_message(ViewCommand { view, op });
        r.app.update();
    };
    send(&mut r, other, ViewOp::Snap(SnapView::Top));
    let p0 = pivot(&r, r.camera);
    // Mid-snap: the pan lands the snap first, and only the pan (a few
    // centimetres) carries, not the snap's jump.
    send(&mut r, other, ViewOp::Pan(Vec2::new(10.0, 0.0)));
    let moved = pivot(&r, r.camera) - p0;
    assert!(moved.length() < 0.5, "{moved}");
}

#[test]
fn a_home_every_linked_view_got_lands_each_at_home() {
    let mut r = rig(default_view());
    let other = two_views(&mut r);
    r.app.insert_resource(LinkedViews(true));
    // Move both away, then home both, as the inspector's focus sends to
    // every view.
    for e in [r.camera, other] {
        r.app.world_mut().write_message(ViewCommand {
            view: e,
            op: ViewOp::Pan(Vec2::new(60.0, 20.0)),
        });
    }
    r.app.update();
    for e in [r.camera, other] {
        r.app.world_mut().write_message(ViewCommand {
            view: e,
            op: ViewOp::Home,
        });
    }
    for _ in 0..40 {
        r.app.update();
    }
    for e in [r.camera, other] {
        let p = r.app.world().get::<ViewCamera>(e).unwrap().pivot;
        assert!(p.length() < 1e-3, "{p}");
    }
}

#[test]
fn a_linked_orthographic_fly_up_carries_its_zoom() {
    let mut r = rig(default_view());
    let other = two_views(&mut r);
    r.app.insert_resource(LinkedViews(true));
    for (e, op) in [
        (other, ViewOp::SetProjection(ProjectionKind::Orthographic)),
        (
            r.camera,
            ViewOp::SetProjection(ProjectionKind::Orthographic),
        ),
    ] {
        r.app.world_mut().write_message(ViewCommand { view: e, op });
    }
    r.app.update();
    let height =
        |r: &Rig, e: Entity| ortho_height(r.app.world().get::<Projection>(e).unwrap()).unwrap();
    let (h0, h1) = (height(&r, r.camera), height(&r, other));
    r.app.world_mut().write_message(ViewCommand {
        view: other,
        op: ViewOp::Fly {
            dir: Vec3::Y,
            secs: 0.1,
        },
    });
    r.app.update();
    let ratio = height(&r, other) / h1;
    assert!(ratio > 1.0);
    assert!((height(&r, r.camera) / h0 - ratio).abs() < 1e-4);
}

/// A place lands on the nearest mesh under the point, else the ground,
/// in Gazebo coordinates, and says whether to keep placing.
#[test]
fn a_place_lands_on_the_surface_under_the_point() {
    let mut r = rig(default_view());
    let placed = |r: &mut Rig| -> Vec<place::PlaceAt> {
        r.app
            .world_mut()
            .resource_mut::<Messages<place::PlaceAt>>()
            .drain()
            .collect()
    };
    r.op(ViewOp::Place {
        at: CENTER,
        keep: false,
    });
    let p = placed(&mut r);
    assert_eq!(p.len(), 1);
    let at = Vec3::new(
        p[0].point[0] as f32,
        p[0].point[1] as f32,
        p[0].point[2] as f32,
    );
    assert!(
        at.distance(Vec3::ZERO) < 1e-3,
        "the ground at the origin: {at}"
    );
    assert!(
        (p[0].normal[2] - 1.0).abs() < 1e-6,
        "the ground faces up: {:?}",
        p[0].normal
    );
    assert!(!p[0].keep);
    r.cube(None, Vec3::new(0.0, 1.0, 0.0), 1.0);
    r.op(ViewOp::Place {
        at: CENTER,
        keep: true,
    });
    let p = placed(&mut r);
    // The cube's -X face at Bevy (-1, 1, 0): Gazebo (-1, 0, 1).
    let at = Vec3::new(
        p[0].point[0] as f32,
        p[0].point[1] as f32,
        p[0].point[2] as f32,
    );
    assert!(at.distance(Vec3::new(-1.0, 0.0, 1.0)) < 1e-3, "{at}");
    let n = Vec3::new(
        p[0].normal[0] as f32,
        p[0].normal[1] as f32,
        p[0].normal[2] as f32,
    );
    assert!(
        n.distance(Vec3::NEG_X) < 1e-3,
        "the face towards the camera: {n}"
    );
    assert!(p[0].keep);
    // The sky, from a level camera: nothing.
    let mut r = rig(horizon());
    r.op(ViewOp::Place {
        at: Vec2::new(W / 2.0, 1.0),
        keep: false,
    });
    assert!(placed(&mut r).is_empty());
}

/// While placing, a click places (Shift keeps placing) instead of
/// selecting, and the point under the pointer is followed.
#[test]
fn placing_turns_a_click_into_a_place() {
    let (mut r, a, _) = two_models();
    r.app.insert_resource(place::Placing(true));
    let pa = on_screen(&r, Vec3::new(0.0, 0.0, 0.5));
    let drain = |r: &mut Rig| -> Vec<place::PlaceAt> {
        r.app
            .world_mut()
            .resource_mut::<Messages<place::PlaceAt>>()
            .drain()
            .collect()
    };
    r.cursor(Some(pa));
    r.frames(2);
    assert!(r.app.world().resource::<place::PlaceHover>().0.is_some());
    click(&mut r, pa, &[]);
    assert_eq!(selected(&r), [], "no select while placing");
    let p = drain(&mut r);
    assert_eq!(p.len(), 1);
    assert!(!p[0].keep);
    click(&mut r, pa, &[KeyCode::ShiftLeft]);
    assert!(drain(&mut r)[0].keep);
    r.app.insert_resource(place::Placing(false));
    r.frames(1);
    assert_eq!(r.app.world().resource::<place::PlaceHover>().0, None);
    click(&mut r, pa, &[]);
    assert_eq!(selected(&r), [a]);
    assert!(drain(&mut r).is_empty());
}

/// Align moves the rest of the selection along a world axis so their
/// bounds line up with the first's, as one edit; with nothing to move, or
/// on a server that doesn't take edits, nothing happens.
#[test]
fn align_lines_the_selection_up_with_the_first() {
    use crate::connection::GzStatus;
    use crate::edit::SetPose;
    use crate::undo::{Change, Record};
    use crate::view::align::{AlignEdge, AlignModels};
    let (mut r, a, b) = two_models();
    for (e, name) in [(a, "a"), (b, "b")] {
        r.app
            .world_mut()
            .entity_mut(e)
            .insert(crate::spawn::GzEntity {
                name: name.into(),
                id: 3,
                kind: gz_core::scene::NodeKind::Model,
                path: name.into(),
            });
    }
    let pa = on_screen(&r, Vec3::new(0.0, 0.0, 0.5));
    let pb = on_screen(&r, Vec3::new(0.0, 3.0, 0.5));
    click(&mut r, pa, &[]);
    click(&mut r, pb, &[KeyCode::ShiftLeft]);
    let align = |r: &mut Rig, axis, edge| {
        r.app.world_mut().write_message(AlignModels { axis, edge });
        r.app.update();
        let poses: Vec<SetPose> = r
            .app
            .world_mut()
            .resource_mut::<Messages<SetPose>>()
            .drain()
            .collect();
        let records: Vec<Record> = r
            .app
            .world_mut()
            .resource_mut::<Messages<Record>>()
            .drain()
            .collect();
        (poses, records)
    };
    // Not without edits.
    let (poses, _) = align(&mut r, 1, AlignEdge::Min);
    assert!(poses.is_empty());
    r.app.insert_resource(GzStatus {
        edits: Some(true),
        ..GzStatus::new("ws://h")
    });
    let (poses, records) = align(&mut r, 1, AlignEdge::Min);
    assert_eq!(poses.len(), 1);
    assert_eq!(poses[0].entity, b);
    assert!((poses[0].pose.translation - Vec3::new(0.0, -3.0, 0.0)).length() < 1e-4);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].verb, "align");
    // Held, so the stream's older poses don't pull it back meanwhile.
    let now = r.app.world().resource::<Time<Real>>().elapsed_secs_f64();
    assert!(r.app.world().resource::<crate::edit::Held>().holds(b, now));
    assert!(matches!(
        records[0].changes[..],
        [Change::Pose { entity, .. }] if entity == b
    ));
    // Already lined up: nothing to do.
    r.app.update();
    let (poses, records) = align(&mut r, 0, AlignEdge::Center);
    assert!(poses.is_empty() && records.is_empty());
}

/// Delete, Duplicate, Copy and Paste ask for the world's SDFormat, then
/// remove the models or create copies of them (named by the client,
/// beside them), as edits; a copy is selected once it appears.
#[test]
fn model_edits_go_through_the_worlds_sdformat() {
    use crate::connection::{GzServiceReply, GzStatus};
    use crate::undo::{Change, Record};
    use crate::view::model_edits::{EditModels, ModelEdit, ModelsEdited};
    use gz_core::msgs;
    use prost::Message as _;
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
    r.app.insert_resource(GzStatus {
        status: gz_core::transport::Status::Ready,
        world: Some("w".into()),
        edits: Some(true),
        ..GzStatus::new("ws://h")
    });
    let pa = on_screen(&r, Vec3::new(0.0, 0.0, 0.5));
    click(&mut r, pa, &[]);
    let sdf = r#"<sdf version="1.9"><world name="w"><model name="a"><link name="l"/></model></world></sdf>"#;
    let reply = GzServiceReply {
        service: "/world/w/generate_world_sdf".into(),
        msg_type: "gz.msgs.StringMsg".into(),
        payload: msgs::StringMsg {
            data: sdf.into(),
            ..Default::default()
        }
        .encode_to_vec(),
    };
    let drain = |r: &mut Rig| {
        let sent: Vec<GzCommand> = r
            .app
            .world_mut()
            .resource_mut::<Messages<GzCommand>>()
            .drain()
            .collect();
        let records: Vec<Record> = r
            .app
            .world_mut()
            .resource_mut::<Messages<Record>>()
            .drain()
            .collect();
        let done: Vec<ModelsEdited> = r
            .app
            .world_mut()
            .resource_mut::<Messages<ModelsEdited>>()
            .drain()
            .collect();
        (sent, records, done)
    };
    let service = |c: &GzCommand| match c {
        GzCommand::Request { service, .. } => service.clone(),
        _ => String::new(),
    };
    let created = |c: &GzCommand| match c {
        GzCommand::Request { payload, .. } => {
            msgs::EntityFactory::decode(payload.as_slice()).unwrap()
        }
        _ => panic!("not a request"),
    };
    let run = |r: &mut Rig, edit| {
        drain(r);
        r.app.world_mut().write_message(EditModels(edit));
        r.app.update();
        let (sent, ..) = drain(r);
        assert_eq!(
            sent.iter().map(service).collect::<Vec<_>>(),
            ["/world/w/generate_world_sdf"]
        );
        r.app.world_mut().write_message(reply.clone());
        r.app.update();
        drain(r)
    };
    // Duplicate: a copy named a_1, one size and a gap along Y.
    let (sent, records, done) = run(&mut r, ModelEdit::Duplicate);
    assert_eq!(service(&sent[0]), "/world/w/create");
    let f = created(&sent[0]);
    assert_eq!(f.name, "a_1");
    assert!((f.pose.unwrap().position.unwrap().y - 1.5).abs() < 1e-4);
    assert!(matches!(
        &f.from,
        Some(msgs::entity_factory::From::Sdf(s)) if s.contains(r#"<model name="a"><link name="l"/></model>"#)
    ));
    assert_eq!(records[0].verb, "duplicate");
    assert_eq!(done[0].result, Ok("Duplicated a_1".into()));
    // It shows: selected.
    let copy = r
        .app
        .world_mut()
        .spawn(crate::spawn::GzEntity {
            name: "a_1".into(),
            id: 9,
            kind: gz_core::scene::NodeKind::Model,
            path: "a_1".into(),
        })
        .id();
    r.app.update();
    assert_eq!(selected(&r), [copy]);
    r.app.world_mut().entity_mut(copy).despawn();
    r.app.update();
    click(&mut r, pa, &[]);
    // Copy, then Paste twice: a_1 and a_2 (the first still on its way).
    let (sent, _, done) = run(&mut r, ModelEdit::Copy);
    assert!(sent.is_empty());
    assert_eq!(done[0].result, Ok("Copied a".into()));
    for (name, y) in [("a_1", 1.5), ("a_2", 3.0)] {
        r.app
            .world_mut()
            .write_message(EditModels(ModelEdit::Paste));
        r.app.update();
        let (sent, records, _) = drain(&mut r);
        let f = created(&sent[0]);
        assert_eq!(f.name, name);
        assert!((f.pose.unwrap().position.unwrap().y - y).abs() < 1e-4);
        assert_eq!(records[0].verb, "paste");
    }
    // A reply without the model fails, and the model can be deleted
    // again.
    drain(&mut r);
    r.app
        .world_mut()
        .write_message(EditModels(ModelEdit::Delete));
    r.app.update();
    drain(&mut r);
    r.app.world_mut().write_message(GzServiceReply {
        payload: msgs::StringMsg {
            data: r#"<sdf version="1.9"><world name="w"/></sdf>"#.into(),
            ..Default::default()
        }
        .encode_to_vec(),
        ..reply.clone()
    });
    r.app.update();
    let (sent, _, done) = drain(&mut r);
    assert!(sent.is_empty());
    assert_eq!(done[0].result, Err("no model a in the world".into()));
    // Delete: removed by its id, kept for undo with its pose as loaded.
    let (sent, records, done) = run(&mut r, ModelEdit::Delete);
    assert_eq!(service(&sent[0]), "/world/w/remove");
    let GzCommand::Request { payload, .. } = &sent[0] else {
        unreachable!()
    };
    let removed = msgs::Entity::decode(payload.as_slice()).unwrap();
    assert_eq!((removed.name.as_str(), removed.id), ("a", 3));
    assert!(matches!(
        &records[0].changes[..],
        [Change::Exists { model, after: false }] if model.name == "a" && model.sdf.contains("<link")
    ));
    assert_eq!(done[0].result, Ok("Deleted a".into()));
    // Delete again before it's gone: nothing more (one edit, one remove).
    r.app
        .world_mut()
        .write_message(EditModels(ModelEdit::Delete));
    r.app.update();
    let (sent, records, done) = drain(&mut r);
    assert!(sent.is_empty() && records.is_empty() && done.is_empty());
    // Not without edits.
    r.app.world_mut().resource_mut::<GzStatus>().edits = Some(false);
    r.app
        .world_mut()
        .write_message(EditModels(ModelEdit::Delete));
    r.app.update();
    let (sent, _, done) = drain(&mut r);
    assert!(sent.is_empty());
    assert_eq!(done[0].result, Err("This server doesn't take edits".into()));
}

/// A model too large for the server to take whole (its `create` would be
/// lost) isn't deleted, duplicated or pasted: nothing is sent, and the
/// edit says why. One that joins a message's pieces takes it.
#[test]
fn a_model_too_large_to_send_is_left_alone() {
    use crate::connection::{GzServiceReply, GzStatus};
    use crate::view::model_edits::{EditModels, ModelEdit, ModelsEdited};
    use gz_core::msgs;
    use prost::Message as _;
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
    r.app.insert_resource(GzStatus {
        status: gz_core::transport::Status::Ready,
        world: Some("w".into()),
        edits: Some(true),
        ..GzStatus::new("ws://h")
    });
    let pa = on_screen(&r, Vec3::new(0.0, 0.0, 0.5));
    click(&mut r, pa, &[]);
    // Plugin text the compacting keeps.
    let sdf = format!(
        r#"<sdf version="1.9"><world name="w"><model name="a"><link name="l"/><plugin name="p" filename="f"><x>{}</x></plugin></model></world></sdf>"#,
        "y".repeat(gz_core::transport::MAX_FRAME)
    );
    let reply = GzServiceReply {
        service: "/world/w/generate_world_sdf".into(),
        msg_type: "gz.msgs.StringMsg".into(),
        payload: msgs::StringMsg {
            data: sdf,
            ..Default::default()
        }
        .encode_to_vec(),
    };
    let run = |r: &mut Rig, edit| {
        r.app.world_mut().write_message(EditModels(edit));
        r.app.update();
        r.app.world_mut().write_message(reply.clone());
        r.app.update();
        let sent: Vec<String> = r
            .app
            .world_mut()
            .resource_mut::<Messages<GzCommand>>()
            .drain()
            .filter_map(|c| match c {
                GzCommand::Request { service, .. } => Some(service),
                _ => None,
            })
            .collect();
        let done: Vec<ModelsEdited> = r
            .app
            .world_mut()
            .resource_mut::<Messages<ModelsEdited>>()
            .drain()
            .collect();
        (sent, done)
    };
    let (sent, done) = run(&mut r, ModelEdit::Delete);
    assert_eq!(sent, ["/world/w/generate_world_sdf"], "no remove");
    let why = done[0].result.clone().unwrap_err();
    assert!(
        why.starts_with("Not deleted: undo couldn't bring a back (its 4,")
            && why.ends_with(" bytes are over the 4,088 the server takes)"),
        "{why}"
    );
    let (sent, done) = run(&mut r, ModelEdit::Duplicate);
    assert_eq!(sent, ["/world/w/generate_world_sdf"], "no create");
    assert!(done[0]
        .result
        .clone()
        .unwrap_err()
        .starts_with("a is too large to create on this server"));
    let (_, done) = run(&mut r, ModelEdit::Copy);
    assert_eq!(done[0].result, Ok("Copied a".into()));
    r.app
        .world_mut()
        .write_message(EditModels(ModelEdit::Paste));
    r.app.update();
    let creates = r
        .app
        .world_mut()
        .resource_mut::<Messages<GzCommand>>()
        .drain()
        .filter(|c| matches!(c, GzCommand::Request { .. }))
        .count();
    assert_eq!(creates, 0);
    let done: Vec<ModelsEdited> = r
        .app
        .world_mut()
        .resource_mut::<Messages<ModelsEdited>>()
        .drain()
        .collect();
    assert!(done[0]
        .result
        .clone()
        .unwrap_err()
        .starts_with("a is too large to create on this server"));
    // A server that joins a message's pieces takes it (D-114).
    r.app.world_mut().resource_mut::<GzStatus>().long_frames = Some(true);
    let (sent, done) = run(&mut r, ModelEdit::Duplicate);
    assert_eq!(
        sent,
        ["/world/w/generate_world_sdf", "/world/w/create"],
        "{done:?}"
    );
}

/// Restore puts the camera where a workspace saved it: pose, pivot and
/// projection (orthographic with its height, or perspective), free and
/// unsnapped; a pose read back is the one restored.
#[test]
fn restore_puts_the_camera_back() {
    let mut r = rig(default_view());
    r.op(ViewOp::Snap(SnapView::Top));
    for _ in 0..30 {
        r.app.update();
    }
    let pose = ViewPose {
        translation: Vec3::new(2.0, 5.0, 3.0),
        rotation: Transform::from_xyz(2.0, 5.0, 3.0)
            .looking_at(Vec3::new(1.0, 0.0, 0.0), Vec3::Y)
            .rotation,
        pivot: Vec3::new(1.0, 0.0, 0.0),
        ortho: Some(7.5),
        snapped: None,
    };
    r.op(ViewOp::Restore(pose));
    let (v, t, p) = (r.view(), r.transform(), r.projection());
    assert_eq!(v.snapped, None);
    assert_eq!(v.mode, CameraMode::Free);
    assert!((ortho_height(&p).unwrap() - 7.5).abs() < 1e-4);
    let back = ViewPose::of(&v, &t, &p);
    // Orthographic, the view may stand back along its axis (the same
    // picture).
    let along = (back.translation - pose.translation).normalize_or_zero();
    let forward = pose.rotation * Vec3::NEG_Z;
    assert!(along.cross(forward).length() < 1e-3, "{back:?}");
    assert!(back.rotation.angle_between(pose.rotation) < 1e-4);
    assert_eq!((back.pivot, back.ortho), (pose.pivot, pose.ortho));
    // Perspective, it's where it was.
    let pose = ViewPose {
        ortho: None,
        ..pose
    };
    r.op(ViewOp::Restore(pose));
    assert!(matches!(r.projection(), Projection::Perspective(_)));
    let back = ViewPose::of(&r.view(), &r.transform(), &r.projection());
    assert!(
        back.translation.distance(pose.translation) < 1e-4,
        "{back:?}"
    );
}

/// A pose read mid-animation is where the animation ends, snapped too,
/// and restoring it puts the axis view back.
#[test]
fn a_pose_read_mid_animation_is_its_end() {
    let mut r = rig(default_view());
    r.op(ViewOp::Snap(SnapView::Top));
    r.app.update();
    assert!(r.view().animating());
    let mid = ViewPose::of(&r.view(), &r.transform(), &r.projection());
    assert_eq!(mid.snapped, Some(SnapView::Top));
    assert!(mid.ortho.is_some());
    for _ in 0..30 {
        r.app.update();
    }
    let end = ViewPose::of(&r.view(), &r.transform(), &r.projection());
    assert!(
        mid.rotation.angle_between(end.rotation) < 1e-4,
        "{mid:?} {end:?}"
    );
    r.op(ViewOp::Orbit(Vec2::new(40.0, 0.0)));
    r.op(ViewOp::Restore(mid));
    assert_eq!(r.view().snapped, Some(SnapView::Top));
}

/// Two Gazebo models, each a link with a cube visual that has a material;
/// (rig, model a, link a, mesh a, model b, mesh b).
fn two_editable_models() -> (Rig, Entity, Entity, Entity, Entity, Entity) {
    use crate::spawn::GzEntity;
    use gz_core::scene::NodeKind;
    let (mut r, a, b) = two_models();
    r.app.init_resource::<Assets<StandardMaterial>>();
    let world = r.app.world_mut();
    let gz = |name: &str, kind| GzEntity {
        name: name.into(),
        id: 0,
        kind,
        path: name.into(),
    };
    world.entity_mut(a).insert(gz("a", NodeKind::Model));
    world.entity_mut(b).insert(gz("b", NodeKind::Model));
    let link_of = |world: &World, m: Entity| world.get::<Children>(m).unwrap()[0];
    let (la, lb) = (link_of(world, a), link_of(world, b));
    let mesh_of = |world: &World, l: Entity| world.get::<Children>(l).unwrap()[0];
    let (ma, mb) = (mesh_of(world, la), mesh_of(world, lb));
    for (l, m) in [(la, ma), (lb, mb)] {
        world.entity_mut(l).insert(gz("link", NodeKind::Link));
        let material = world
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial::default());
        world
            .entity_mut(m)
            .insert((MeshMaterial3d(material), box_visual()));
    }
    r.app.update();
    (r, a, la, ma, b, mb)
}

fn editing(r: &Rig) -> Option<Entity> {
    r.app.world().resource::<editing::EditingModel>().model()
}

fn edit(r: &mut Rig, cmd: editing::EditCommand) {
    r.app.world_mut().write_message(cmd);
    r.app.update();
    r.app.update();
}

/// A double click with Translate or Rotate edits the model; then a click
/// picks its link, the other model can't be picked, and its meshes are
/// faded; Done puts their materials back.
#[test]
fn editing_picks_the_models_links_and_dims_the_rest() {
    let (mut r, a, la, ma, _, mb) = two_editable_models();
    let pa = on_screen(&r, Vec3::new(0.0, 0.0, 0.5));
    let pb = on_screen(&r, Vec3::new(0.0, 3.0, 0.5));
    // Navigate: a double click only selects.
    click(&mut r, pa, &[]);
    click(&mut r, pa, &[]);
    assert_eq!(editing(&r), None);
    r.app.insert_resource(gizmo::TransformTool::Translate);
    r.app.update();
    click(&mut r, pb, &[]);
    click(&mut r, pa, &[]);
    click(&mut r, pa, &[]);
    r.app.update();
    assert_eq!(editing(&r), Some(a));
    assert_eq!(selected(&r), [], "entering clears the selection");
    let original = |r: &Rig, m: Entity| {
        r.app
            .world()
            .get::<MeshMaterial3d<StandardMaterial>>(m)
            .unwrap()
            .0
            .clone()
    };
    let before_b = r
        .app
        .world()
        .get::<editing::Dimmed>(mb)
        .map(|d| d.original.clone())
        .expect("b is dimmed");
    assert_ne!(original(&r, mb), before_b);
    assert!(r.app.world().get::<editing::Dimmed>(ma).is_none());
    let faded = r
        .app
        .world()
        .resource::<Assets<StandardMaterial>>()
        .get(&original(&r, mb))
        .unwrap()
        .base_color
        .alpha();
    assert!((faded - editing::DIMMED_OPACITY).abs() < 1e-3, "{faded}");
    // Its link picks; b is empty space.
    r.app.update();
    click(&mut r, pa, &[]);
    assert_eq!(selected(&r), [la]);
    click(&mut r, pb, &[]);
    assert_eq!(selected(&r), []);
    edit(&mut r, editing::EditCommand::Done);
    assert_eq!(editing(&r), None);
    assert_eq!(original(&r, mb), before_b);
    assert!(r.app.world().get::<editing::Dimmed>(mb).is_none());
}

/// Entering pauses a running sim and turns the views' joints and frames
/// on; Done puts them back and resumes. A paused sim stays paused.
#[test]
fn editing_pauses_and_shows_joints_and_frames() {
    use crate::overlays::{Overlay, OverlaySet};
    let (mut r, a, ..) = two_editable_models();
    let camera = r.camera;
    r.app
        .world_mut()
        .entity_mut(camera)
        .insert(OverlaySet::default());
    r.set_sim(Some(false));
    r.sent();
    edit(&mut r, editing::EditCommand::Enter(a));
    assert!(r.sent().contains(&GzCommand::Pause));
    let set = *r.app.world().get::<OverlaySet>(camera).unwrap();
    assert!(set.is_on(Overlay::Joints) && set.is_on(Overlay::Frames));
    edit(&mut r, editing::EditCommand::Done);
    assert!(r.sent().contains(&GzCommand::Play));
    let set = *r.app.world().get::<OverlaySet>(camera).unwrap();
    assert!(!set.is_on(Overlay::Joints) && !set.is_on(Overlay::Frames));
    // Entered before the stats: the first running ones pause it.
    r.set_sim(None);
    r.sent();
    edit(&mut r, editing::EditCommand::Enter(a));
    assert!(!r.sent().contains(&GzCommand::Pause));
    r.set_sim(Some(false));
    r.app.update();
    assert!(r.sent().contains(&GzCommand::Pause));
    edit(&mut r, editing::EditCommand::Done);
    assert!(r.sent().contains(&GzCommand::Play));
    // Paused: it stays paused after Done.
    r.set_sim(Some(true));
    r.sent();
    edit(&mut r, editing::EditCommand::Enter(a));
    edit(&mut r, editing::EditCommand::Done);
    let sent = r.sent();
    assert!(!sent.contains(&GzCommand::Pause) && !sent.contains(&GzCommand::Play));
}

/// While editing, the selection holds the model's parts only; a model
/// that goes ends the editing; only a top-level model can be edited.
#[test]
fn editing_limits_the_selection_and_ends_with_its_model() {
    let (mut r, a, la, _, b, _) = two_editable_models();
    edit(&mut r, editing::EditCommand::Enter(la));
    assert_eq!(editing(&r), None, "a link isn't a model");
    edit(&mut r, editing::EditCommand::Enter(a));
    r.app.world_mut().resource_mut::<Selection>().entities = vec![a, la, b];
    r.app.update();
    assert_eq!(selected(&r), [la]);
    r.app.world_mut().entity_mut(a).despawn();
    r.app.update();
    assert_eq!(editing(&r), None);
}

/// How many frames [`editing::EditingModel`] was changed in.
#[derive(Resource, Default)]
struct EditChanges(usize);

fn count_edit_changes(editing: Res<editing::EditingModel>, mut n: ResMut<EditChanges>) {
    if editing.is_changed() {
        n.0 += 1;
    }
}

/// With stats coming, idle frames don't mark the editing changed, editing
/// or not: what reads it as changed (the dimming, the tree) works only
/// when it does change.
#[test]
fn idle_frames_leave_the_editing_unchanged() {
    let (mut r, a, ..) = two_editable_models();
    r.app
        .init_resource::<EditChanges>()
        .add_systems(PostUpdate, count_edit_changes);
    r.set_sim(Some(false));
    r.frames(3);
    r.app.world_mut().resource_mut::<EditChanges>().0 = 0;
    r.frames(5);
    assert_eq!(r.app.world().resource::<EditChanges>().0, 0);
    edit(&mut r, editing::EditCommand::Enter(a));
    r.set_sim(Some(true));
    r.frames(2);
    r.app.world_mut().resource_mut::<EditChanges>().0 = 0;
    r.frames(5);
    assert_eq!(r.app.world().resource::<EditChanges>().0, 0);
}

/// Entering another model while one is edited moves to it: the sim stays
/// paused (no Play, though the stats still say paused), and Done resumes
/// it as the first would have.
#[test]
fn editing_another_model_keeps_the_sim_paused() {
    let (mut r, a, _, _, b, _) = two_editable_models();
    r.set_sim(Some(false));
    r.sent();
    edit(&mut r, editing::EditCommand::Enter(a));
    assert!(r.sent().contains(&GzCommand::Pause));
    r.set_sim(Some(true));
    edit(&mut r, editing::EditCommand::Enter(b));
    assert_eq!(editing(&r), Some(b));
    let sent = r.sent();
    assert!(!sent.contains(&GzCommand::Play) && !sent.contains(&GzCommand::Pause));
    edit(&mut r, editing::EditCommand::Done);
    assert!(r.sent().contains(&GzCommand::Play));
}

/// Done turns off only what entering turned on: overlays the user changed
/// while editing stay as they are. A view that comes while editing shows
/// joints and frames too, and a workspace's copy of a view leaves them
/// out.
#[test]
fn done_keeps_the_overlays_changed_while_editing() {
    use crate::overlays::{Overlay, OverlaySet};
    let (mut r, a, ..) = two_editable_models();
    let camera = r.camera;
    let mut joints = OverlaySet::default();
    joints.set(Overlay::Joints, true);
    r.app.world_mut().entity_mut(camera).insert(joints);
    edit(&mut r, editing::EditCommand::Enter(a));
    let set = |r: &Rig, e: Entity| *r.app.world().get::<OverlaySet>(e).unwrap();
    assert!(set(&r, camera).is_on(Overlay::Frames));
    let own = r
        .app
        .world()
        .resource::<editing::EditingModel>()
        .own(camera, &set(&r, camera));
    assert_eq!(own, joints, "a workspace saves the view's own");
    {
        let mut s = r.app.world_mut().get_mut::<OverlaySet>(camera).unwrap();
        s.set(Overlay::Collisions, true);
    }
    let other = r.app.world_mut().spawn(OverlaySet::default()).id();
    r.frames(2);
    let s = set(&r, other);
    assert!(
        s.is_on(Overlay::Joints) && s.is_on(Overlay::Frames),
        "{s:?}"
    );
    edit(&mut r, editing::EditCommand::Done);
    let s = set(&r, camera);
    assert!(
        s.is_on(Overlay::Joints) && s.is_on(Overlay::Collisions),
        "{s:?}"
    );
    assert!(!s.is_on(Overlay::Frames));
    assert_eq!(set(&r, other), OverlaySet::default());
}

/// While editing, the camera still pivots on the dimmed models: only
/// picking skips them.
#[test]
fn the_pivot_lands_on_a_dimmed_model() {
    let (mut r, a, ..) = two_editable_models();
    edit(&mut r, editing::EditCommand::Enter(a));
    let pb = on_screen(&r, Vec3::new(0.0, 3.0, 0.5));
    r.op(ViewOp::PivotAt {
        at: pb,
        orbit: true,
    });
    // On b's cube (0.5 m about (0, 3, 0.5)), not the ground behind it.
    let pivot = gz_to_bevy().inverse() * r.pivot();
    let off = (pivot - Vec3::new(0.0, 3.0, 0.5)).abs();
    assert!(off.max_element() < 0.5 + 1e-3 && pivot.z > 0.01, "{pivot}");
}

/// A right click, or a camera drag, between two clicks: no double click.
#[test]
fn only_two_clicks_in_a_row_are_a_double_click() {
    let (mut r, ..) = two_editable_models();
    r.app.insert_resource(gizmo::TransformTool::Translate);
    let pa = on_screen(&r, Vec3::new(0.0, 0.0, 0.5));
    click(&mut r, pa, &[]);
    r.drag(MouseButton::Right, pa, &[]);
    click(&mut r, pa, &[]);
    assert_eq!(editing(&r), None, "a right click between");
    r.drag(MouseButton::Left, pa, &[pa + Vec2::new(40.0, 0.0)]);
    click(&mut r, pa, &[]);
    assert_eq!(editing(&r), None, "an orbit between");
}

/// [`two_editable_models`] on a server that takes edits, with the gizmo
/// and undo.
fn editable_gizmo_rig() -> (Rig, Entity, Entity) {
    let (mut r, a, _, _, b, _) = two_editable_models();
    {
        let mut status = r.app.world_mut().resource_mut::<GzStatus>();
        status.status = gz_core::transport::Status::Ready;
        status.world = Some("w".into());
        status.edits = Some(true);
    }
    r.app
        .insert_resource(gizmo::TransformTool::Translate)
        .init_resource::<gizmo::Snap>()
        .init_resource::<gizmo::GizmoState>()
        .add_plugins((crate::edit::EditPlugin, crate::undo::UndoPlugin))
        .add_systems(Update, gizmo::gizmo_input.before(controls::mouse_input));
    r.app.update();
    (r, a, b)
}

/// The first click selects the model and brings the gizmo: a second on a
/// handle is still the double click, not the gizmo's.
#[test]
fn a_double_click_on_a_gizmo_handle_edits_the_model() {
    let (mut r, a, _) = editable_gizmo_rig();
    let p = arrow_point(&r, 0, 0.3);
    click(&mut r, p, &[]);
    assert_eq!(selected(&r), [a]);
    click(&mut r, p, &[]);
    r.app.update();
    assert_eq!(editing(&r), Some(a));
    assert!(!r.app.world().resource::<gizmo::GizmoState>().captures());
}

/// Undo and redo wait while a model is edited.
#[test]
fn undo_waits_while_editing() {
    let (mut r, a, b) = editable_gizmo_rig();
    r.app.world_mut().resource_mut::<Selection>().entities = vec![a];
    r.app.update();
    let (from, to) = (arrow_point(&r, 0, 0.8), arrow_point(&r, 0, 1.6));
    r.drag(MouseButton::Left, from, &[to]);
    r.app.update();
    let x = |r: &Rig| r.app.world().get::<Transform>(a).unwrap().translation.x;
    let moved = x(&r);
    assert!(moved > 0.1, "{moved}");
    edit(&mut r, editing::EditCommand::Enter(b));
    r.app
        .world_mut()
        .write_message(crate::undo::UndoCommand::Undo);
    r.app.update();
    assert_eq!(x(&r), moved);
    edit(&mut r, editing::EditCommand::Done);
    r.app
        .world_mut()
        .write_message(crate::undo::UndoCommand::Undo);
    r.app.update();
    assert_eq!(x(&r), 0.0);
}

/// A faded material follows its own: a change to it (a texture combined
/// in, say) reaches the copy. A heightmap's terrain fades too.
#[test]
fn faded_copies_follow_their_materials_and_terrain_fades() {
    use crate::heightmap::TerrainMaterial;
    let (mut r, a, _, _, b, mb) = two_editable_models();
    r.app
        .add_message::<AssetEvent<StandardMaterial>>()
        .init_resource::<Assets<TerrainMaterial>>();
    let lb = r.app.world().get::<Children>(b).unwrap()[0];
    let terrain = {
        let world = r.app.world_mut();
        let m = world
            .resource_mut::<Assets<TerrainMaterial>>()
            .add(TerrainMaterial::default());
        world
            .spawn((MeshMaterial3d(m), box_visual(), ChildOf(lb)))
            .id()
    };
    edit(&mut r, editing::EditCommand::Enter(a));
    let shown = |r: &Rig| {
        r.app
            .world()
            .get::<MeshMaterial3d<StandardMaterial>>(mb)
            .unwrap()
            .0
            .clone()
    };
    let original = r
        .app
        .world()
        .get::<editing::Dimmed>(mb)
        .unwrap()
        .original
        .clone();
    {
        let mut materials = r.app.world_mut().resource_mut::<Assets<StandardMaterial>>();
        materials.get_mut(&original).unwrap().emissive = LinearRgba::RED;
    }
    r.app
        .world_mut()
        .write_message(AssetEvent::Modified { id: original.id() });
    r.app.update();
    let copy = r
        .app
        .world()
        .resource::<Assets<StandardMaterial>>()
        .get(&shown(&r))
        .cloned()
        .unwrap();
    assert_eq!(copy.emissive, LinearRgba::RED);
    assert!((copy.base_color.alpha() - editing::DIMMED_OPACITY).abs() < 1e-3);
    let faded = r
        .app
        .world()
        .get::<MeshMaterial3d<TerrainMaterial>>(terrain)
        .unwrap()
        .0
        .clone();
    let alpha = r
        .app
        .world()
        .resource::<Assets<TerrainMaterial>>()
        .get(&faded)
        .unwrap()
        .base
        .base_color
        .alpha();
    assert!((alpha - editing::DIMMED_OPACITY).abs() < 1e-3, "{alpha}");
    edit(&mut r, editing::EditCommand::Done);
    assert!(r
        .app
        .world()
        .get::<editing::Dimmed<TerrainMaterial>>(terrain)
        .is_none());
}

/// Parts added while editing: a link spawns under the model, selected; a
/// sensor joins its link's; the extras get them; undo and redo take the
/// last back and put it back; Done drops them, the extras the server's
/// again.
#[test]
fn parts_join_the_edited_model_until_done() {
    use crate::overlays::extras::SdfExtras;
    use crate::spawn::{GzEntity, GzSensors};
    use editing::parts::{EditParts, PartCommand, PartsDiscarded};
    use gz_core::insert::Shape;
    use gz_core::parts::{Part, SensorKind};
    use gz_core::scene::Pose;
    let (mut r, a, la, ..) = two_editable_models();
    r.app.init_resource::<SdfExtras>();
    r.app.world_mut().get_mut::<GzEntity>(la).unwrap().path = "a/base".into();
    edit(&mut r, editing::EditCommand::Enter(a));
    let add = |r: &mut Rig, part: Part| {
        r.app.world_mut().write_message(PartCommand::Add(part));
        r.app.update();
        r.app.update();
    };
    add(
        &mut r,
        Part::Link {
            name: "link_1".into(),
            shape: Shape::Box,
            pose: Pose {
                pos: [1.0, 0.0, 0.25],
                quat: [0.0, 0.0, 0.0, 1.0],
            },
        },
    );
    let link = |r: &mut Rig| {
        let world = r.app.world_mut();
        world
            .query::<(Entity, &GzEntity, &ChildOf)>()
            .iter(world)
            .find(|(_, g, _)| g.path == "a/link_1")
            .map(|(e, _, p)| (e, p.parent()))
    };
    let (l1, parent) = link(&mut r).expect("the link");
    assert_eq!(parent, a);
    assert_eq!(selected(&r), [l1]);
    let t = r.app.world().get::<Transform>(l1).unwrap().translation;
    assert_eq!(t, Vec3::new(1.0, 0.0, 0.25));
    add(
        &mut r,
        Part::Sensor {
            link: "a/base".into(),
            name: "imu_1".into(),
            kind: SensorKind::Imu,
            pose: Pose::IDENTITY,
        },
    );
    let sensors = |r: &Rig| r.app.world().get::<GzSensors>(la).map_or(0, |s| s.0.len());
    assert_eq!(sensors(&r), 1);
    let extras = |r: &Rig| r.app.world().resource::<SdfExtras>().extras.clone();
    let e = extras(&r).expect("the draft's");
    assert!(e.link("a/link_1").is_some());
    assert_eq!(e.link("a/base").unwrap().sensors[0].name, "imu_1");
    r.app
        .world_mut()
        .write_message(crate::undo::UndoCommand::Undo);
    r.app.update();
    assert_eq!(sensors(&r), 0);
    assert!(extras(&r).unwrap().link("a/base").is_none());
    r.app
        .world_mut()
        .write_message(crate::undo::UndoCommand::Undo);
    r.app.update();
    assert!(link(&mut r).is_none());
    assert_eq!(extras(&r), None, "the server's: none yet");
    r.app
        .world_mut()
        .write_message(crate::undo::UndoCommand::Redo);
    r.app.update();
    r.app.update();
    assert!(link(&mut r).is_some());
    assert_eq!(
        r.app
            .world()
            .resource::<EditParts>()
            .draft
            .undo_label()
            .as_deref(),
        Some("add link_1")
    );
    // A reply while editing gets the draft's parts too.
    {
        let mut x = r.app.world_mut().resource_mut::<SdfExtras>();
        x.extras = Some(gz_core::sdf::WorldExtras::default());
        x.generation += 1;
    }
    r.app.update();
    assert!(extras(&r).unwrap().link("a/link_1").is_some());
    r.app
        .world_mut()
        .resource_mut::<Messages<PartsDiscarded>>()
        .clear();
    edit(&mut r, editing::EditCommand::Discard);
    assert!(link(&mut r).is_none());
    assert_eq!(extras(&r), Some(gz_core::sdf::WorldExtras::default()));
    let dropped: Vec<PartsDiscarded> = r
        .app
        .world_mut()
        .resource_mut::<Messages<PartsDiscarded>>()
        .drain()
        .collect();
    assert_eq!(dropped, [PartsDiscarded(1)]);
}

/// A model being edited with a link added, on a server that takes edits
/// and long frames (`long_frames`); (rig, model a, link a/base).
fn a_model_with_a_part(long_frames: Option<bool>) -> (Rig, Entity, Entity) {
    use crate::overlays::extras::SdfExtras;
    use crate::spawn::GzEntity;
    use editing::parts::PartCommand;
    use gz_core::insert::Shape;
    use gz_core::parts::Part;
    let (mut r, a, la, ..) = two_editable_models();
    r.app
        .init_resource::<SdfExtras>()
        .add_plugins((crate::edit::EditPlugin, crate::undo::UndoPlugin));
    r.app.world_mut().get_mut::<GzEntity>(la).unwrap().path = "a/base".into();
    r.app.insert_resource(GzStatus {
        status: gz_core::transport::Status::Ready,
        world: Some("w".into()),
        edits: Some(true),
        long_frames,
        ..GzStatus::new("ws://h")
    });
    edit(&mut r, editing::EditCommand::Enter(a));
    r.app
        .world_mut()
        .write_message(PartCommand::Add(Part::Link {
            name: "link_1".into(),
            shape: Shape::Box,
            pose: gz_core::scene::Pose::IDENTITY,
        }));
    r.frames(2);
    r.sent();
    (r, a, la)
}

/// The world's SDFormat, model a's link `base` with inertia or without.
fn world_reply(inertia: bool) -> crate::connection::GzServiceReply {
    use gz_core::msgs;
    use prost::Message as _;
    let inertial = if inertia {
        "<inertial><mass>1</mass></inertial>"
    } else {
        ""
    };
    let sdf = format!(
        "<sdf version=\"1.9\"><world name=\"w\"><model name=\"a\">\
         <link name=\"base\">{inertial}</link></model></world></sdf>"
    );
    crate::connection::GzServiceReply {
        service: "/world/w/generate_world_sdf".into(),
        msg_type: "gz.msgs.StringMsg".into(),
        payload: msgs::StringMsg {
            data: sdf,
            ..Default::default()
        }
        .encode_to_vec(),
    }
}

fn notes(r: &mut Rig) -> Vec<String> {
    r.app
        .world_mut()
        .resource_mut::<Messages<editing::parts::EditNote>>()
        .drain()
        .map(|n| n.0)
        .collect()
}

fn services(sent: &[GzCommand]) -> Vec<&str> {
    sent.iter()
        .filter_map(|c| match c {
            GzCommand::Request { service, .. } => Some(service.as_str()),
            _ => None,
        })
        .collect()
}

/// Done with parts asks for the world's SDFormat and stays in editing;
/// the reply writes the parts into the model, which is removed (to be
/// created again once it has gone) as one "edit a", and editing ends,
/// resuming the sim, without the parts being called discarded.
#[test]
fn done_writes_the_parts_into_the_model_as_one_edit() {
    use crate::undo::{Change, Record, Replacing};
    use editing::parts::{EditParts, PartsDiscarded};
    let (mut r, a, _) = a_model_with_a_part(Some(true));
    assert!(r.app.world().resource::<EditParts>().holds());
    r.app.world_mut().resource_mut::<Messages<Record>>().clear();
    edit(&mut r, editing::EditCommand::Done);
    assert_eq!(editing(&r), Some(a), "writing first");
    let sent = r.sent();
    assert_eq!(services(&sent), ["/world/w/generate_world_sdf"]);
    assert!(r.app.world().resource::<EditParts>().writing());
    assert_eq!(notes(&mut r), ["Writing a…"]);
    // Done again while it is on its way asks nothing more.
    edit(&mut r, editing::EditCommand::Done);
    assert!(r.sent().is_empty());
    r.app.world_mut().write_message(world_reply(true));
    r.frames(3);
    let sent = r.sent();
    assert_eq!(services(&sent), ["/world/w/remove"], "{sent:?}");
    assert_eq!(editing(&r), None);
    let records: Vec<Record> = r
        .app
        .world_mut()
        .resource_mut::<Messages<Record>>()
        .drain()
        .collect();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].verb, "edit");
    let [Change::Replaced { before, after }] = &records[0].changes[..] else {
        panic!("{:?}", records[0].changes);
    };
    assert_eq!(before.name, "a");
    assert!(!before.sdf.contains("link_1"));
    assert!(
        after.sdf.contains("<link name=\"link_1\">"),
        "{}",
        after.sdf
    );
    let replacing = &r.app.world().resource::<Replacing>().0;
    assert_eq!(replacing.len(), 1);
    assert_eq!(replacing[0].model.sdf, after.sdf);
    assert!(notes(&mut r).is_empty(), "written once it shows");
    let dropped = r
        .app
        .world_mut()
        .resource_mut::<Messages<PartsDiscarded>>()
        .drain()
        .count();
    assert_eq!(dropped, 0);
    assert!(!r.app.world().resource::<EditParts>().holds());
    // The model gone, it's created from the SDFormat as written; once it
    // shows, it's written.
    r.app.world_mut().entity_mut(a).despawn();
    r.frames(2);
    let sent = r.sent();
    assert_eq!(services(&sent), ["/world/w/create"]);
    r.app.world_mut().spawn(crate::spawn::GzEntity {
        name: "a".into(),
        id: 9,
        kind: gz_core::scene::NodeKind::Model,
        path: "a".into(),
    });
    r.frames(2);
    let said: Vec<String> = r
        .app
        .world_mut()
        .resource_mut::<Messages<crate::undo::ReplaceNote>>()
        .drain()
        .map(|n| n.0)
        .collect();
    assert_eq!(said, ["Wrote a: 1 change"]);
}

/// Done can't write a model whose check fails, on a server that cuts long
/// messages, or with no reply: editing goes on with the parts, saying why,
/// and Done tries again. Entering another model is refused while there
/// are parts; Discard leaves without them.
#[test]
fn done_says_why_it_cant_write_and_discard_leaves() {
    use crate::view::model_edits::EDIT_TIMEOUT_SECS;
    use editing::parts::{EditParts, PartsDiscarded};
    for long_frames in [None, Some(false)] {
        let (mut r, a, _) = a_model_with_a_part(long_frames);
        edit(&mut r, editing::EditCommand::Done);
        assert!(r.sent().is_empty());
        assert_eq!(editing(&r), Some(a));
        assert_eq!(
            notes(&mut r),
            [
                "Can't write a: this server cuts long messages, so it would lose the model \
              · Discard the parts"
            ]
        );
    }
    let (mut r, a, _) = a_model_with_a_part(Some(true));
    edit(&mut r, editing::EditCommand::Done);
    notes(&mut r);
    r.sent();
    r.app.world_mut().write_message(world_reply(false));
    r.frames(3);
    let sent = r.sent();
    assert!(services(&sent).is_empty(), "{sent:?}");
    assert_eq!(editing(&r), Some(a));
    assert_eq!(
        notes(&mut r),
        ["Can't write a: base has no inertia (a link needs it unless its model is static)"]
    );
    assert!(r.app.world().resource::<EditParts>().holds());
    // No reply in time.
    edit(&mut r, editing::EditCommand::Done);
    assert_eq!(services(&r.sent()), ["/world/w/generate_world_sdf"]);
    notes(&mut r);
    let mut said = vec![];
    for _ in 0..(EDIT_TIMEOUT_SECS * 1000.0) as usize / FRAME_MS as usize + 2 {
        r.app.update();
        said.extend(notes(&mut r));
    }
    assert_eq!(said, ["Can't write a: the server didn't answer"]);
    assert!(!r.app.world().resource::<EditParts>().writing());
    // Another model: refused while there are parts.
    let b = {
        let world = r.app.world_mut();
        world
            .query::<(Entity, &crate::spawn::GzEntity)>()
            .iter(world)
            .find(|(_, g)| g.path == "b")
            .map(|(e, _)| e)
            .unwrap()
    };
    edit(&mut r, editing::EditCommand::Enter(b));
    assert_eq!(editing(&r), Some(a));
    assert_eq!(notes(&mut r), ["Done or Discard this model's parts first"]);
    r.app
        .world_mut()
        .resource_mut::<Messages<PartsDiscarded>>()
        .clear();
    edit(&mut r, editing::EditCommand::Discard);
    assert_eq!(editing(&r), None);
    assert!(services(&r.sent()).is_empty());
    let dropped: Vec<PartsDiscarded> = r
        .app
        .world_mut()
        .resource_mut::<Messages<PartsDiscarded>>()
        .drain()
        .collect();
    assert_eq!(dropped, [PartsDiscarded(1)]);
}

/// Undo takes a sensor selected as a part out of the selection; idle
/// frames after Done leave the draft unchanged; the server's SDFormat is
/// known once it comes, the draft's parts added or not.
#[test]
fn undo_unselects_a_part_and_idle_frames_leave_the_draft() {
    use crate::overlays::extras::SdfExtras;
    use crate::spawn::GzEntity;
    use editing::parts::{EditParts, PartCommand};
    use gz_core::parts::{Part, SensorKind};
    use gz_core::scene::Pose;
    let (mut r, a, la, ..) = two_editable_models();
    r.app.init_resource::<SdfExtras>();
    r.app.world_mut().get_mut::<GzEntity>(la).unwrap().path = "a/base".into();
    edit(&mut r, editing::EditCommand::Enter(a));
    let known = |r: &Rig| {
        let w = r.app.world();
        w.resource::<EditParts>()
            .server_known(w.resource::<SdfExtras>())
    };
    r.app
        .world_mut()
        .write_message(PartCommand::Add(Part::Sensor {
            link: "a/base".into(),
            name: "imu_1".into(),
            kind: SensorKind::Imu,
            pose: Pose::IDENTITY,
        }));
    r.frames(2);
    assert!(!known(&r), "the draft's extras aren't the server's");
    let imu = super::Part {
        kind: PartKind::Sensor,
        owner: la,
        name: "imu_1".into(),
    };
    r.app.world_mut().resource_mut::<Selection>().parts = vec![imu];
    r.app
        .world_mut()
        .write_message(crate::undo::UndoCommand::Undo);
    r.app.update();
    assert!(r.app.world().resource::<Selection>().parts.is_empty());
    {
        let mut x = r.app.world_mut().resource_mut::<SdfExtras>();
        x.extras = Some(gz_core::sdf::WorldExtras::default());
        x.generation += 1;
    }
    r.app.update();
    assert!(known(&r));
    edit(&mut r, editing::EditCommand::Done);
    r.frames(2);
    let tick = |r: &Rig| r.app.world().resource_ref::<EditParts>().last_changed();
    let before = tick(&r);
    r.frames(3);
    assert_eq!(tick(&r), before);
}

/// While a model is edited, a place reaches its links through a dimmed
/// model in front of them, and the hover follows them too; outside
/// editing, the model in front takes it.
#[test]
fn a_place_reaches_the_edited_model_through_a_dimmed_one() {
    let (mut r, a, _, ma, b, mb) = two_editable_models();
    let target = Vec3::new(0.0, 0.0, 0.5);
    let camera = gz_to_bevy().inverse() * r.transform().translation;
    // b's cube halfway from the camera to a's.
    let mid = camera.lerp(target, 0.5);
    r.app
        .world_mut()
        .get_mut::<Transform>(b)
        .unwrap()
        .translation = mid - Vec3::new(0.0, 3.0, 0.5);
    r.app.update();
    let pa = on_screen(&r, target);
    let hit = |r: &mut Rig| {
        r.op(ViewOp::Place {
            at: pa,
            keep: false,
        });
        let p: Vec<place::PlaceAt> = r
            .app
            .world_mut()
            .resource_mut::<Messages<place::PlaceAt>>()
            .drain()
            .collect();
        p[0].hit
    };
    assert_eq!(hit(&mut r), Some(mb), "b is in front");
    edit(&mut r, editing::EditCommand::Enter(a));
    assert_eq!(hit(&mut r), Some(ma));
    r.app.insert_resource(place::Placing(true));
    r.cursor(Some(pa));
    r.frames(2);
    let hover = r.app.world().resource::<place::PlaceHover>().0;
    assert_eq!(hover.and_then(|(_, _, e)| e), Some(ma));
}

/// Following the browser's motion (minimal mode), natively motion stays
/// full: a reduced one is put back.
#[test]
fn the_views_can_follow_the_browsers_motion() {
    let mut r = rig(default_view());
    r.app.insert_resource(ReduceMotion(true));
    r.app.update();
    assert_eq!(
        *r.app.world().resource::<ReduceMotion>(),
        ReduceMotion(true)
    );
    r.app.insert_resource(FollowBrowserMotion);
    r.app.update();
    assert_eq!(
        *r.app.world().resource::<ReduceMotion>(),
        ReduceMotion(false)
    );
}
