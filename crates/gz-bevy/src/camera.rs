//! Main camera, matching gzweb's defaults (`Scene.ts`): position (-6, 0, 6)
//! looking at the origin, vertical FOV 60°, near 0.01, far 1000.
//!
//! The controls that move it live in [`crate::view`].

use bevy::prelude::*;

use crate::render;
use crate::spawn::gz_to_bevy;
use crate::view::{self, ViewCamera};
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

/// gzweb's perspective: vertical FOV 60°, near 0.01, far 1000.
pub fn perspective() -> PerspectiveProjection {
    PerspectiveProjection {
        fov: 60f32.to_radians(),
        near: 0.01,
        far: 1000.0,
        ..default()
    }
}

pub fn spawn_camera(mut commands: Commands, config: Res<GzConfig>, target: Res<GzCameraTarget>) {
    let focus = gz_to_bevy() * Vec3::from_array(target.0);
    let transform =
        Transform::from_translation(view::home_transform().translation).looking_at(focus, Vec3::Y);
    let controls = ViewCamera::new(focus, perspective());
    let camera = spawn_main_camera(
        &mut commands,
        &config,
        transform,
        Some(controls),
        CLEAR_COLOR,
    );
    if config.interactive() {
        let first = view::ViewId(0);
        commands.entity(camera).insert((first, first.layers()));
    }
}

/// Spawns the main camera at `transform` with the configured quality, and
/// (when interactive) the view controls: `view`, or new ones about the
/// origin.
pub fn spawn_main_camera(
    commands: &mut Commands,
    config: &GzConfig,
    transform: Transform,
    view: Option<ViewCamera>,
    clear_color: Color,
) -> Entity {
    let mut camera = commands.spawn((
        Camera3d::default(),
        Camera {
            clear_color: ClearColorConfig::Custom(clear_color),
            ..default()
        },
        Projection::Perspective(perspective()),
        transform,
        MainCamera,
    ));
    render::camera_components(&mut camera, config.quality);
    if config.interactive() {
        camera.insert(view.unwrap_or_else(|| ViewCamera::new(Vec3::ZERO, perspective())));
    }
    camera.id()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::Quality;
    use crate::UiMode;
    use bevy::ecs::system::RunSystemOnce;

    fn assert_near(a: Vec3, b: Vec3, eps: f32) {
        assert!((a - b).length() < eps, "{a} vs {b}");
    }

    #[test]
    fn spawned_camera_uses_gzweb_defaults_and_controls_only_with_a_window() {
        for (ui, quality) in [
            (UiMode::Full, Quality::High),
            (UiMode::Minimal, Quality::Ultra),
            (UiMode::None, Quality::Low),
        ] {
            let mut world = World::new();
            world.insert_resource(GzConfig {
                ui,
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
            assert_near(*t.forward(), Vec3::new(1.0, -1.0, 0.0).normalize(), 1e-5);
            assert!(matches!(
                proj,
                Projection::Perspective(p)
                    if (p.fov - 60f32.to_radians()).abs() < 1e-6 && (p.near, p.far) == (0.01, 1000.0)
            ));
            assert!(matches!(cam.clear_color, ClearColorConfig::Custom(c) if c == CLEAR_COLOR));
            let controls = world.query::<&ViewCamera>().iter(&world).count();
            assert_eq!(controls, usize::from(ui.interactive()));
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

    #[test]
    fn the_camera_starts_looking_at_the_published_target() {
        let mut world = World::new();
        world.insert_resource(GzConfig::default());
        world.insert_resource(GzCameraTarget([1.0, 2.0, 0.0]));
        world.run_system_once(spawn_camera).unwrap();
        let (t, view) = world
            .query::<(&Transform, &ViewCamera)>()
            .single(&world)
            .unwrap();
        let focus = gz_to_bevy() * Vec3::new(1.0, 2.0, 0.0);
        assert_near(view.pivot, focus, 1e-5);
        assert_near(*t.forward(), (focus - t.translation).normalize(), 1e-5);
        // Respawned without state: controls about the origin.
        let config = GzConfig::default();
        world
            .run_system_once(move |mut commands: Commands| {
                spawn_main_camera(
                    &mut commands,
                    &config,
                    Transform::default(),
                    None,
                    CLEAR_COLOR,
                );
            })
            .unwrap();
        let pivots: Vec<Vec3> = world
            .query::<&ViewCamera>()
            .iter(&world)
            .map(|v| v.pivot)
            .collect();
        assert!(pivots.contains(&Vec3::ZERO));
    }
}
