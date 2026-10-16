//! Main camera, matching gzweb's defaults (`Scene.ts`): position (-6, 0, 6)
//! looking at the origin, vertical FOV 60°, near 0.01, far 1000.

use bevy::prelude::*;
use bevy_panorbit_camera::PanOrbitCamera;

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
        camera.insert(PanOrbitCamera {
            focus,
            button_orbit: MouseButton::Left,
            button_pan: MouseButton::Right,
            modifier_orbit: None,
            modifier_pan: None,
            ..default()
        });
    }
}

/// Left click re-centres the orbit on the clicked point of the ground plane
/// (gzweb picks against scene geometry; that comes with picking support).
pub fn update_orbit_focus(
    mut camera_query: Query<(&mut PanOrbitCamera, &Camera, &GlobalTransform)>,
    mut gizmos: Gizmos,
    windows: Query<&Window>,
    mouse_input: Res<ButtonInput<MouseButton>>,
    mut target: ResMut<GzCameraTarget>,
) {
    if !mouse_input.just_pressed(MouseButton::Left) {
        return;
    }
    let Ok(window) = windows.single() else { return };
    let Some(cursor) = window.cursor_position() else { return };
    let Ok((mut orbit, camera, camera_transform)) = camera_query.single_mut() else {
        return;
    };
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor) else {
        return;
    };
    // Gazebo's z = 0 plane is Bevy's y = 0 plane.
    let t = -ray.origin.y / ray.direction.y;
    if t > 0.0 {
        let hit = ray.origin + ray.direction * t;
        orbit.target_focus = hit;
        orbit.radius = Some((camera_transform.translation() - hit).length());
        target.0 = (gz_to_bevy().inverse() * hit).to_array();
        gizmos.sphere(hit, 0.2, Color::srgb(1.0, 0.0, 0.0));
    }
}
