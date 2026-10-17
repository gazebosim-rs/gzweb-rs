//! Main camera, matching gzweb's defaults (`Scene.ts`): position (-6, 0, 6)
//! looking at the origin, vertical FOV 60°, near 0.01, far 1000.

use bevy::input::mouse::MouseWheel;
use bevy::picking::mesh_picking::ray_cast::{MeshRayCast, MeshRayCastSettings};
use bevy::prelude::*;

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
    let mut camera = commands.spawn((
        Camera3d::default(),
        Camera {
            clear_color: ClearColorConfig::Custom(CLEAR_COLOR),
            ..default()
        },
        Projection::Perspective(PerspectiveProjection {
            fov: 60f32.to_radians(),
            near: 0.01,
            far: 1000.0,
            ..default()
        }),
        Transform::from_translation(pos).looking_at(focus, Vec3::Y),
        MainCamera,
    ));
    if !config.headless {
        camera.insert(GzOrbitControls {
            target: focus,
            ..default()
        });
    }
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
