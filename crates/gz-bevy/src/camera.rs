//! Main camera, matching gzweb's defaults (`Scene.ts`): position (-6, 0, 6)
//! looking at the origin, vertical FOV 60°, near 0.01, far 1000.

use bevy::input::mouse::MouseWheel;
use bevy::picking::mesh_picking::ray_cast::{MeshRayCast, MeshRayCastSettings};
use bevy::prelude::*;

use crate::render;
use crate::spawn::gz_to_bevy;
use crate::GzConfig;

#[derive(Component)]
pub struct MainCamera;

/// Orbit target in Gazebo coordinates.
#[derive(Resource, Clone, Copy)]
pub struct GzCameraTarget(pub [f32; 3]);

impl Default for GzCameraTarget {
    fn default() -> Self {
        GzCameraTarget([0.0; 3])
    }
}

/// gzweb's renderer clear colour, `0xb2b2b2`.
pub const CLEAR_COLOR: Color = Color::srgb(178.0 / 255.0, 178.0 / 255.0, 178.0 / 255.0);

pub fn spawn_camera(mut commands: Commands, config: Res<GzConfig>, target: Res<GzCameraTarget>) {
    let to_bevy = gz_to_bevy();
    let pos = to_bevy * Vec3::new(-6.0, 0.0, 6.0);
    let focus = to_bevy * Vec3::from_array(target.0);
    let transform = Transform::from_translation(pos).looking_at(focus, Vec3::Y);
    spawn_main_camera(&mut commands, &config, transform, Some(focus), CLEAR_COLOR);
}

/// Spawns the main camera at `transform` with the configured quality, and
/// (when interactive) orbit controls about `focus`.
pub fn spawn_main_camera(
    commands: &mut Commands,
    config: &GzConfig,
    transform: Transform,
    focus: Option<Vec3>,
    clear_color: Color,
) -> Entity {
    let mut camera = commands.spawn((
        Camera3d::default(),
        Camera {
            clear_color: ClearColorConfig::Custom(clear_color),
            ..default()
        },
        Projection::Perspective(PerspectiveProjection {
            fov: 60f32.to_radians(),
            near: 0.01,
            far: 1000.0,
            ..default()
        }),
        transform,
        MainCamera,
    ));
    render::camera_components(&mut camera, config.quality);
    if config.interactive {
        camera.insert(GzOrbitControls {
            target: focus.unwrap_or(Vec3::ZERO),
            ..default()
        });
    }
    camera.id()
}

/// Port of gzweb's patched three.js `OrbitControls` (`include/OrbitControls.js`).
///
/// Unlike stock orbit controls, the camera is never re-aimed at the target:
/// - Any button press or wheel step picks the point under the cursor as the
///   pivot (`Scene.ts` `onMouseDown` / `onMouseScroll`). The camera doesn't move.
/// - Left drag rotates the camera about the pivot: yaw around world up, then
///   pitch around the camera's own right axis. The pivot stays put on screen.
/// - Right drag pans in screen space; middle drag and the wheel dolly along
///   the camera-to-pivot line.
#[derive(Component)]
pub struct GzOrbitControls {
    /// Pivot, in Bevy world coordinates.
    pub target: Vec3,
    pub rotate_speed: f32,
    pub zoom_speed: f32,
    pub pan_speed: f32,
    last_cursor: Option<Vec2>,
}

impl Default for GzOrbitControls {
    fn default() -> Self {
        Self {
            target: Vec3::ZERO,
            rotate_speed: 1.0,
            zoom_speed: 1.0,
            pan_speed: 1.0,
            last_cursor: None,
        }
    }
}

/// three.js `Spherical.makeSafe` epsilon.
const POLAR_EPS: f32 = 1e-6;

/// The scene point under `cursor`: the nearest visible mesh, else the
/// Gazebo z = 0 plane (gzweb's grid). `None` keeps the current pivot.
fn pick(
    ray_cast: &mut MeshRayCast,
    camera: &Camera,
    camera_transform: &GlobalTransform,
    cursor: Vec2,
) -> Option<Vec3> {
    let ray = camera.viewport_to_world(camera_transform, cursor).ok()?;
    if let Some((_, hit)) = ray_cast
        .cast_ray(ray, &MeshRayCastSettings::default())
        .first()
    {
        return Some(hit.point);
    }
    // Gazebo's z = 0 plane is Bevy's y = 0 plane.
    let t = -ray.origin.y / ray.direction.y;
    (t.is_finite() && t > 0.0).then(|| ray.origin + ray.direction * t)
}

pub fn orbit_controls(
    mut camera_query: Query<(
        &mut GzOrbitControls,
        &mut Transform,
        &Camera,
        &GlobalTransform,
        &Projection,
    )>,
    windows: Query<&Window>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut wheel: MessageReader<MouseWheel>,
    mut ray_cast: MeshRayCast,
    mut target_res: ResMut<GzCameraTarget>,
) {
    let Ok(window) = windows.single() else { return };
    let Ok((mut controls, mut transform, camera, camera_transform, projection)) =
        camera_query.single_mut()
    else {
        return;
    };
    let cursor = window.cursor_position();
    let height = window.height().max(1.0);

    let pressed = [MouseButton::Left, MouseButton::Right, MouseButton::Middle]
        .iter()
        .any(|b| buttons.just_pressed(*b));
    let wheel_steps: Vec<f32> = wheel.read().map(|e| e.y).filter(|y| *y != 0.0).collect();

    if let Some(cursor) = cursor {
        if pressed || !wheel_steps.is_empty() {
            if let Some(hit) = pick(&mut ray_cast, camera, camera_transform, cursor) {
                controls.target = hit;
            }
        }
    }

    let delta = match (cursor, controls.last_cursor) {
        (Some(now), Some(last)) if !pressed => now - last,
        _ => Vec2::ZERO,
    };
    controls.last_cursor = cursor;
    let target = controls.target;

    if buttons.pressed(MouseButton::Left) && delta != Vec2::ZERO {
        let d = delta * controls.rotate_speed;
        let d_theta = -std::f32::consts::TAU * d.x / height;
        let d_phi = -std::f32::consts::TAU * d.y / height;
        // Clamp the polar angle of the camera-to-pivot offset to (0, π).
        let offset = transform.translation - target;
        let phi = (offset.y / offset.length().max(f32::EPSILON))
            .clamp(-1.0, 1.0)
            .acos();
        let new_phi = (phi + d_phi).clamp(POLAR_EPS, std::f32::consts::PI - POLAR_EPS);
        transform.rotate_around(target, Quat::from_axis_angle(Vec3::Y, d_theta));
        let right = transform.rotation * Vec3::X;
        transform.rotate_around(target, Quat::from_axis_angle(right, new_phi - phi));
    } else if buttons.pressed(MouseButton::Right) && delta != Vec2::ZERO {
        let fov = match projection {
            Projection::Perspective(p) => p.fov,
            _ => 60f32.to_radians(),
        };
        let distance = (transform.translation - target).length() * (fov / 2.0).tan();
        let d = delta * controls.pan_speed;
        let right = transform.rotation * Vec3::X;
        let up = transform.rotation * Vec3::Y;
        let pan = -right * (2.0 * d.x * distance / height) + up * (2.0 * d.y * distance / height);
        transform.translation += pan;
        controls.target += pan;
    } else {
        // three's dollyIn/dollyOut: one 0.95^zoomSpeed step per wheel event or
        // per middle-drag move.
        let step = 0.95f32.powf(controls.zoom_speed);
        let mut scale = 1.0;
        for y in &wheel_steps {
            scale *= if *y > 0.0 { step } else { 1.0 / step };
        }
        if buttons.pressed(MouseButton::Middle) && delta.y != 0.0 {
            scale *= if delta.y > 0.0 { 1.0 / step } else { step };
        }
        if scale != 1.0 {
            transform.translation = target + (transform.translation - target) * scale;
        }
    }

    target_res.0 = (gz_to_bevy().inverse() * controls.target).to_array();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::Quality;
    use bevy::camera::primitives::Aabb;
    use bevy::camera::visibility::SetViewVisibility;
    use bevy::camera::{CameraProjection, RenderTargetInfo};
    use bevy::ecs::system::RunSystemOnce;
    use bevy::input::mouse::MouseScrollUnit;
    use bevy::window::WindowResolution;
    use std::f32::consts::PI;

    const W: f32 = 800.0;
    const H: f32 = 600.0;
    const CENTER: Vec2 = Vec2::new(W / 2.0, H / 2.0);

    struct Rig {
        app: App,
        window: Entity,
        camera: Entity,
    }

    fn perspective() -> PerspectiveProjection {
        PerspectiveProjection {
            fov: 60f32.to_radians(),
            near: 0.01,
            far: 1000.0,
            aspect_ratio: W / H,
            ..default()
        }
    }

    /// The default gzweb view: gz (-6, 0, 6) looking at the origin.
    fn default_view() -> Transform {
        Transform::from_translation(gz_to_bevy() * Vec3::new(-6.0, 0.0, 6.0))
            .looking_at(Vec3::ZERO, Vec3::Y)
    }

    fn rig(view: Transform) -> Rig {
        let mut app = App::new();
        app.add_plugins(TransformPlugin)
            .init_resource::<ButtonInput<MouseButton>>()
            .add_message::<MouseWheel>()
            .insert_resource(Assets::<Mesh>::default())
            .init_resource::<GzCameraTarget>()
            .add_systems(Update, orbit_controls);
        let window = app
            .world_mut()
            .spawn(Window {
                resolution: WindowResolution::new(W as u32, H as u32),
                ..default()
            })
            .id();
        // Fill in what the render camera system would compute, so
        // `viewport_to_world` works without a renderer.
        let mut camera = Camera::default();
        camera.computed.clip_from_view = perspective().get_clip_from_view();
        camera.computed.target_info = Some(RenderTargetInfo {
            physical_size: UVec2::new(W as u32, H as u32),
            scale_factor: 1.0,
        });
        let camera = app
            .world_mut()
            .spawn((
                camera,
                Projection::Perspective(perspective()),
                view,
                GzOrbitControls::default(),
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
        /// One frame with the cursor at `cursor`, `press`ed buttons going
        /// down, `release`d ones coming up, and `wheel` scroll events.
        fn frame(
            &mut self,
            cursor: Option<Vec2>,
            press: &[MouseButton],
            release: &[MouseButton],
            wheel: &[f32],
        ) {
            let world = self.app.world_mut();
            world
                .get_mut::<Window>(self.window)
                .unwrap()
                .set_cursor_position(cursor);
            let mut buttons = world.resource_mut::<ButtonInput<MouseButton>>();
            buttons.clear();
            for b in press {
                buttons.press(*b);
            }
            for b in release {
                buttons.release(*b);
            }
            for y in wheel {
                world.write_message(MouseWheel {
                    unit: MouseScrollUnit::Line,
                    x: 0.0,
                    y: *y,
                    window: self.window,
                });
            }
            self.app.update();
        }

        fn at(&mut self, cursor: Vec2) {
            self.frame(Some(cursor), &[], &[], &[]);
        }

        fn transform(&self) -> Transform {
            *self.app.world().get::<Transform>(self.camera).unwrap()
        }

        fn pivot(&self) -> Vec3 {
            self.app
                .world()
                .get::<GzOrbitControls>(self.camera)
                .unwrap()
                .target
        }

        fn ray(&self, cursor: Vec2) -> Ray3d {
            let w = self.app.world();
            let cam = w.get::<Camera>(self.camera).unwrap();
            let gt = w.get::<GlobalTransform>(self.camera).unwrap();
            cam.viewport_to_world(gt, cursor).unwrap()
        }
    }

    fn assert_near(a: Vec3, b: Vec3, eps: f32) {
        assert!((a - b).length() < eps, "{a} vs {b}");
    }

    /// The pivot's direction in the camera's own frame: what fixes its
    /// position on screen.
    fn view_dir(t: &Transform, pivot: Vec3) -> Vec3 {
        (t.rotation.inverse() * (pivot - t.translation)).normalize()
    }

    fn polar(t: &Transform, pivot: Vec3) -> f32 {
        let o = t.translation - pivot;
        (o.y / o.length()).clamp(-1.0, 1.0).acos()
    }

    #[test]
    fn pressing_picks_the_point_under_the_cursor_without_moving_the_camera() {
        let mut r = rig(default_view());
        let before = r.transform();
        let cursor = Vec2::new(520.0, 380.0);
        r.frame(Some(cursor), &[MouseButton::Left], &[], &[]);
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
        let res = r.app.world().resource::<GzCameraTarget>().0;
        assert_near(Vec3::from_array(res), gz, 1e-5);
    }

    #[test]
    fn left_drag_orbits_the_pivot_keeping_it_fixed_on_screen() {
        let mut r = rig(default_view());
        r.frame(
            Some(Vec2::new(520.0, 380.0)),
            &[MouseButton::Left],
            &[],
            &[],
        );
        let pivot = r.pivot();
        let before = r.transform();
        r.at(Vec2::new(560.0, 405.0));
        let after = r.transform();
        assert!((after.translation - before.translation).length() > 0.1);
        assert!(
            ((after.translation - pivot).length() - (before.translation - pivot).length()).abs()
                < 1e-4
        );
        assert_near(view_dir(&after, pivot), view_dir(&before, pivot), 1e-4);
        assert_eq!(r.pivot(), pivot);

        // Horizontal drags only yaw: the height above the pivot is kept.
        let h = r.transform().translation.y - pivot.y;
        r.at(Vec2::new(620.0, 405.0));
        assert!((r.transform().translation.y - pivot.y - h).abs() < 1e-4);

        // Without a button held, moving does nothing.
        r.frame(
            Some(Vec2::new(620.0, 405.0)),
            &[],
            &[MouseButton::Left],
            &[],
        );
        let still = r.transform();
        r.at(Vec2::new(700.0, 500.0));
        assert_eq!(r.transform(), still);
    }

    #[test]
    fn orbiting_stops_at_the_poles_instead_of_flipping() {
        // 300 px is a half turn (2π per window height), well past either pole.
        let mut r = rig(default_view());
        r.frame(
            Some(Vec2::new(400.0, 100.0)),
            &[MouseButton::Left],
            &[],
            &[],
        );
        let pivot = r.pivot();
        // Drag down: the polar angle shrinks to the top pole.
        r.at(Vec2::new(400.0, 400.0));
        assert!(polar(&r.transform(), pivot) < 1e-2);
        let top = r.transform();
        r.at(Vec2::new(400.0, 590.0));
        assert_near(r.transform().translation, top.translation, 1e-3);
        // Drag up: all the way to the bottom pole.
        r.at(Vec2::new(400.0, 10.0));
        assert!(polar(&r.transform(), pivot) > PI - 1e-2);
        assert_near(view_dir(&r.transform(), pivot), view_dir(&top, pivot), 1e-3);
    }

    #[test]
    fn right_drag_pans_camera_and_pivot_together() {
        let mut r = rig(default_view());
        r.frame(Some(CENTER), &[MouseButton::Right], &[], &[]);
        let pivot = r.pivot();
        let before = r.transform();
        r.at(CENTER + Vec2::new(60.0, -30.0));
        let after = r.transform();
        let d = (before.translation - pivot).length() * (30f32.to_radians()).tan();
        let right = before.rotation * Vec3::X;
        let up = before.rotation * Vec3::Y;
        let want = -right * (2.0 * 60.0 * d / H) + up * (2.0 * -30.0 * d / H);
        assert_near(after.translation - before.translation, want, 1e-4);
        assert_near(r.pivot() - pivot, want, 1e-4);
        assert_eq!(after.rotation, before.rotation);
    }

    #[test]
    fn pan_speed_falls_back_to_60_degrees_for_other_projections() {
        let mut r = rig(default_view());
        *r.app.world_mut().get_mut::<Projection>(r.camera).unwrap() =
            Projection::Orthographic(OrthographicProjection::default_3d());
        r.frame(Some(CENTER), &[MouseButton::Right], &[], &[]);
        let pivot = r.pivot();
        let before = r.transform();
        r.at(CENTER + Vec2::new(60.0, 0.0));
        let d = (before.translation - pivot).length() * (30f32.to_radians()).tan();
        let moved = (r.transform().translation - before.translation).length();
        assert!((moved - 2.0 * 60.0 * d / H).abs() < 1e-4);
    }

    #[test]
    fn wheel_dollies_toward_the_point_under_the_cursor() {
        let mut r = rig(default_view());
        let cursor = Vec2::new(300.0, 350.0);
        r.frame(Some(cursor), &[], &[], &[1.0]);
        // The wheel picked a new pivot under the cursor...
        let pivot = r.pivot();
        assert!(pivot.y.abs() < 1e-4);
        let start = default_view();
        let d0 = (start.translation - pivot).length();
        // ...and moved 5% closer to it along the same line.
        let t = r.transform();
        assert!(((t.translation - pivot).length() - 0.95 * d0).abs() < 1e-3);
        assert_near(
            (t.translation - pivot).normalize(),
            (start.translation - pivot).normalize(),
            1e-4,
        );
        assert_eq!(t.rotation, start.rotation);

        // Two steps out in one frame, around the same point.
        let pivot = r.pivot();
        let d1 = (r.transform().translation - pivot).length();
        r.frame(Some(cursor), &[], &[], &[-1.0, -1.0, 0.0]);
        let d2 = (r.transform().translation - r.pivot()).length();
        assert!((d2 - d1 / 0.95 / 0.95).abs() < 1e-2, "{d1} {d2}");
    }

    #[test]
    fn middle_drag_dollies() {
        let mut r = rig(default_view());
        r.frame(Some(CENTER), &[MouseButton::Middle], &[], &[]);
        let pivot = r.pivot();
        let d0 = (r.transform().translation - pivot).length();
        r.at(CENTER + Vec2::new(0.0, 5.0));
        let d1 = (r.transform().translation - pivot).length();
        assert!((d1 - d0 / 0.95).abs() < 1e-3);
        r.at(CENTER);
        let d2 = (r.transform().translation - pivot).length();
        assert!((d2 - d0).abs() < 1e-3);
        // Sideways middle drags don't zoom.
        r.at(CENTER + Vec2::new(40.0, 0.0));
        assert!(((r.transform().translation - pivot).length() - d0).abs() < 1e-3);
    }

    #[test]
    fn a_click_on_nothing_keeps_the_pivot() {
        // Looking at the horizon; the top of the screen sees only sky.
        let view =
            Transform::from_xyz(0.0, 1.0, 0.0).looking_at(Vec3::new(0.0, 1.0, -10.0), Vec3::Y);
        let mut r = rig(view);
        let sentinel = Vec3::new(5.0, 5.0, 5.0);
        r.app
            .world_mut()
            .get_mut::<GzOrbitControls>(r.camera)
            .unwrap()
            .target = sentinel;
        r.frame(Some(Vec2::new(400.0, 10.0)), &[MouseButton::Left], &[], &[]);
        assert_eq!(r.pivot(), sentinel);
        assert_eq!(r.transform(), view);
        // No cursor (outside the window): nothing is picked either.
        r.frame(None, &[MouseButton::Right], &[MouseButton::Left], &[1.0]);
        assert_eq!(r.pivot(), sentinel);
    }

    #[test]
    fn meshes_are_picked_before_the_ground() {
        let mut r = rig(default_view());
        let world = r.app.world_mut();
        let cube = Cuboid::new(2.0, 2.0, 2.0);
        let mesh = world.resource_mut::<Assets<Mesh>>().add(cube);
        world.spawn((
            Mesh3d(mesh),
            Transform::from_xyz(0.0, 1.0, 0.0),
            Aabb::from_min_max(Vec3::splat(-1.0), Vec3::splat(1.0)),
            InheritedVisibility::VISIBLE,
            ViewVisibility::default(),
        ));
        world
            .run_system_once(|mut q: Query<&mut ViewVisibility>| {
                for mut v in &mut q {
                    v.set_visible();
                }
            })
            .unwrap();
        r.app.update();
        r.frame(Some(CENTER), &[MouseButton::Left], &[], &[]);
        // The centre ray from (-6, 6, 0) towards the origin meets the cube's
        // -X face at (-1, 1, 0).
        assert_near(r.pivot(), Vec3::new(-1.0, 1.0, 0.0), 1e-3);
    }

    #[test]
    fn controls_wait_for_a_window_and_a_camera() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<MouseButton>>()
            .add_message::<MouseWheel>()
            .insert_resource(Assets::<Mesh>::default())
            .init_resource::<GzCameraTarget>()
            .add_systems(Update, orbit_controls);
        app.update();
        app.world_mut().spawn(Window::default());
        app.update();
    }

    #[test]
    fn spawned_camera_uses_gzweb_defaults_and_controls_only_with_a_window() {
        for (interactive, quality) in [(true, Quality::High), (false, Quality::Low)] {
            let mut world = World::new();
            world.insert_resource(GzConfig {
                interactive,
                quality,
                ..default()
            });
            world.init_resource::<GzCameraTarget>();
            world.run_system_once(spawn_camera).unwrap();
            let (t, proj, cam) = world
                .query_filtered::<(&Transform, &Projection, &Camera), With<MainCamera>>()
                .single(&world)
                .unwrap();
            assert_near(t.translation, Vec3::new(-6.0, 6.0, 0.0), 1e-5);
            assert!(matches!(
                proj,
                Projection::Perspective(p)
                    if (p.fov - 60f32.to_radians()).abs() < 1e-6 && (p.near, p.far) == (0.01, 1000.0)
            ));
            assert!(matches!(cam.clear_color, ClearColorConfig::Custom(c) if c == CLEAR_COLOR));
            let controls = world.query::<&GzOrbitControls>().iter(&world).count();
            assert_eq!(controls, usize::from(interactive));
            // The screen-space effects only above Low.
            let taa = world
                .query_filtered::<(), With<bevy::anti_alias::taa::TemporalAntiAliasing>>()
                .iter(&world)
                .count();
            assert_eq!(taa, usize::from(quality != Quality::Low));
            let msaa = *world.query::<&Msaa>().single(&world).unwrap();
            assert_eq!(msaa, quality.msaa());
        }
    }
}
