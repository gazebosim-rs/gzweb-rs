//! Rendering quality: the camera's post-processing, shadows, image-based
//! lighting, sky, fog and grid.
//!
//! gzweb renders with three.js r141's legacy lights, no tone mapping and no
//! environment lighting. gzweb-rs doesn't copy that look; it uses Bevy's
//! physically based pipeline at its best (see `.agent/DESIGN.md`
//! "Rendering"). The scene graph and light *parameters* still follow
//! Gazebo, and the parity snapshot compares those, not pixels.

use bevy::anti_alias::taa::TemporalAntiAliasing;
use bevy::asset::RenderAssetUsages;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::core_pipeline::Skybox;
use bevy::light::{
    AtmosphereEnvironmentMapLight, DirectionalLightShadowMap, GeneratedEnvironmentMapLight,
    PointLightShadowMap, ShadowFilteringMethod,
};
use bevy::pbr::{
    Atmosphere, AtmosphereSettings, DistanceFog, FogFalloff, ScatteringMedium,
    ScreenSpaceAmbientOcclusion, ScreenSpaceAmbientOcclusionQualityLevel,
};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension,
};
use bevy::render::view::{Hdr, Msaa};
use gz_core::scene::{FogDesc, SceneDesc};

use crate::assets;
use crate::camera::{MainCamera, CLEAR_COLOR};

/// How much rendering work to spend.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Quality {
    /// TAA, SSAO, bloom, 4096 px cascaded shadows.
    #[default]
    High,
    /// 4x MSAA, 2048 px shadows, no screen-space effects.
    Low,
}

impl std::str::FromStr for Quality {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "high" => Ok(Quality::High),
            "low" => Ok(Quality::Low),
            other => Err(format!("unknown quality '{other}' (high or low)")),
        }
    }
}

/// Light units. Gazebo light intensities are unitless; these put them on
/// Bevy's physical scale with one fixed camera exposure, so their ratios stay
/// as Gazebo intends and AgX tone mapping handles the highlights.
pub mod units {
    use bevy::camera::Exposure;
    use std::f32::consts::PI;

    /// Illuminance of a directional light of intensity 1 (daylight).
    pub const SUN_LUX: f32 = 10_000.0;

    /// Exposure that renders a white surface facing a [`SUN_LUX`] light at
    /// 1.0 before tone mapping (`1 / (1.2 * 2^ev100) = π / SUN_LUX`), opened
    /// up half a stop because the tone mapper compresses highlights.
    pub const EXPOSURE: Exposure = Exposure {
        ev100: 11.373 - 0.5,
    };

    /// Factor that turns a scene-referred value of 1 into the luminance (cd/m²)
    /// that renders as 1 under [`EXPOSURE`].
    pub const UNIT_LUMINANCE: f32 = SUN_LUX / PI;

    pub fn directional_lux(intensity: f32) -> f32 {
        intensity * SUN_LUX
    }

    /// A point or spot light of intensity 1 lights a surface 1 m away as
    /// brightly as the sun: `4π sr × SUN_LUX`.
    pub fn point_lumens(intensity: f32) -> f32 {
        intensity * 4.0 * PI * SUN_LUX
    }

    /// Image-based fill light for a white ambient colour: the share of
    /// [`UNIT_LUMINANCE`] a surface facing up receives from the environment.
    pub const FILL: f32 = 0.35;
}

/// Shadow map resolutions and the camera's anti-aliasing for `quality`.
pub fn configure(app: &mut App, quality: Quality) {
    let (directional, point, msaa) = match quality {
        Quality::High => (4096, 2048, Msaa::Off),
        Quality::Low => (2048, 1024, Msaa::Sample4),
    };
    app.insert_resource(DirectionalLightShadowMap { size: directional })
        .insert_resource(PointLightShadowMap { size: point })
        .insert_resource(RenderQuality { quality, msaa })
        .init_resource::<GzEnvironment>()
        .init_resource::<GzGrid>();
}

#[derive(Resource, Debug, Clone, Copy)]
pub struct RenderQuality {
    pub quality: Quality,
    pub msaa: Msaa,
}

impl Default for RenderQuality {
    fn default() -> Self {
        Self {
            quality: Quality::High,
            msaa: Msaa::Off,
        }
    }
}

/// The world's environment settings, set by `spawn::apply_scene` from each
/// new scene.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct GzEnvironment {
    /// False until the first scene arrives; nothing is applied before that.
    pub loaded: bool,
    pub ambient: Option<[f32; 3]>,
    pub background: Option<[f32; 4]>,
    pub sky: Option<Option<String>>,
    pub fog: Option<FogDesc>,
}

impl GzEnvironment {
    pub fn from_scene(desc: &SceneDesc) -> Self {
        Self {
            loaded: true,
            ambient: desc.ambient,
            background: desc.background,
            sky: desc.sky.clone(),
            fog: desc.fog,
        }
    }
}

/// Whether the ground grid is drawn (the scene's `grid` flag; G toggles it).
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GzGrid {
    pub visible: bool,
}

/// The camera's post-processing for `quality`.
pub fn camera_components(camera: &mut EntityCommands, quality: RenderQuality) {
    camera.insert((
        Hdr,
        Tonemapping::TonyMcMapface,
        units::EXPOSURE,
        quality.msaa,
        DistanceFog {
            // Off until the scene asks for fog.
            falloff: FogFalloff::Linear {
                start: f32::MAX,
                end: f32::MAX,
            },
            ..default()
        },
    ));
    match quality.quality {
        Quality::High => {
            camera.insert((
                TemporalAntiAliasing::default(),
                ShadowFilteringMethod::Temporal,
                ScreenSpaceAmbientOcclusion {
                    quality_level: ScreenSpaceAmbientOcclusionQualityLevel::High,
                    ..default()
                },
                Bloom::NATURAL,
            ));
        }
        Quality::Low => {
            camera.insert(ShadowFilteringMethod::Gaussian);
        }
    }
}

/// Clear colour of the scene's background (alpha > 0 means it was set).
pub fn clear_color(env: &GzEnvironment) -> Color {
    match env.background {
        Some([r, g, b, a]) if a > 0.0 => Color::srgb(r, g, b),
        _ => CLEAR_COLOR,
    }
}

/// Applies a changed [`GzEnvironment`] to the camera and ambient light.
#[allow(clippy::too_many_arguments)]
pub fn apply_environment(
    env: Res<GzEnvironment>,
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut media: ResMut<Assets<ScatteringMedium>>,
    asset_server: Res<AssetServer>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut cameras: Query<(Entity, &mut Camera, &mut DistanceFog), With<MainCamera>>,
) {
    // Switching environment-map sources on a camera within a few frames
    // trips a mip-count mismatch in Bevy 0.18's generator, so the first one
    // is installed only once the scene is known.
    if !env.is_changed() || !env.loaded {
        return;
    }
    let tint = env.ambient.map_or(Vec3::splat(0.4), Vec3::from_array);
    ambient.color = Color::srgb(tint.x, tint.y, tint.z);
    // The environment map below is the fill light; ambient only remains for
    // the snapshot's record of the colour.
    ambient.brightness = 0.0;

    let background = clear_color(&env);
    for (entity, mut camera, mut fog) in &mut cameras {
        camera.clear_color = ClearColorConfig::Custom(background);
        *fog = distance_fog(env.fog.as_ref());
        let mut cam = commands.entity(entity);
        cam.remove::<(
            Skybox,
            Atmosphere,
            AtmosphereSettings,
            AtmosphereEnvironmentMapLight,
            GeneratedEnvironmentMapLight,
        )>();
        match &env.sky {
            // A cubemap sky lights the scene too.
            Some(Some(uri)) => {
                let path = assets::asset_path(&gz_core::uri::mesh_load_uri(uri));
                let image: Handle<Image> = asset_server.load(path);
                cam.insert((
                    Skybox {
                        image: image.clone(),
                        brightness: units::UNIT_LUMINANCE,
                        ..default()
                    },
                    GeneratedEnvironmentMapLight {
                        environment_map: image,
                        intensity: units::UNIT_LUMINANCE * units::FILL,
                        ..default()
                    },
                ));
            }
            // A sky without a cubemap: Bevy's physically based atmosphere,
            // lit by the scene's directional lights.
            Some(None) => {
                cam.insert((
                    Atmosphere::earthlike(media.add(ScatteringMedium::default())),
                    AtmosphereSettings::default(),
                    AtmosphereEnvironmentMapLight::default(),
                ));
            }
            // No sky: a soft studio environment in the background colour.
            None => {
                let studio = images.add(studio_cubemap(background, tint));
                cam.insert(GeneratedEnvironmentMapLight {
                    environment_map: studio,
                    intensity: units::UNIT_LUMINANCE * units::FILL,
                    ..default()
                });
            }
        }
    }
}

/// Bevy fog for the scene's fog; effectively none without it.
pub fn distance_fog(fog: Option<&FogDesc>) -> DistanceFog {
    use gz_core::scene::FogFalloff as Gz;
    let Some(fog) = fog else {
        return DistanceFog {
            falloff: FogFalloff::Linear {
                start: f32::MAX,
                end: f32::MAX,
            },
            ..default()
        };
    };
    let [r, g, b] = fog.color;
    DistanceFog {
        color: Color::srgb(r, g, b),
        falloff: match fog.falloff {
            Gz::Linear { start, end } => FogFalloff::Linear { start, end },
            Gz::Exponential { density } => FogFalloff::Exponential { density },
            Gz::ExponentialSquared { density } => FogFalloff::ExponentialSquared { density },
        },
        ..default()
    }
}

/// Size of each face of the procedural studio cubemap: a power of two, as
/// `GeneratedEnvironmentMapLight` requires, and the same as the atmosphere's
/// environment map (Bevy's generator caches its textures by size).
const STUDIO_SIZE: u32 = 512;

/// A neutral "studio" environment: the background colour at the horizon,
/// brighter overhead and darker below, tinted by the ambient colour. It
/// gives every material soft fill light and reflections.
pub fn studio_cubemap(background: Color, tint: Vec3) -> Image {
    let horizon = background.to_linear().to_vec3() * tint;
    let zenith = (horizon * 1.25).min(Vec3::ONE);
    let ground = horizon * 0.3;
    let n = STUDIO_SIZE as usize;
    let mut data = Vec::with_capacity(n * n * 6 * 4);
    for face in 0..6 {
        for y in 0..n {
            for x in 0..n {
                let s = (x as f32 + 0.5) / n as f32 * 2.0 - 1.0;
                let t = (y as f32 + 0.5) / n as f32 * 2.0 - 1.0;
                let up = cube_direction(face, s, t).normalize().y;
                let c = if up >= 0.0 {
                    horizon.lerp(zenith, up.sqrt())
                } else {
                    horizon.lerp(ground, (-up).sqrt())
                };
                let srgb = LinearRgba::rgb(c.x, c.y, c.z).to_u8_array();
                data.extend_from_slice(&[srgb[0], srgb[1], srgb[2], 255]);
            }
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: STUDIO_SIZE,
            height: STUDIO_SIZE,
            depth_or_array_layers: 6,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::Cube),
        ..default()
    });
    image
}

/// The direction through texel (`s`, `t`) ∈ [-1, 1]² of cubemap face `face`
/// (wgpu order: +X, -X, +Y, -Y, +Z, -Z).
pub fn cube_direction(face: usize, s: f32, t: f32) -> Vec3 {
    match face {
        0 => Vec3::new(1.0, -t, -s),
        1 => Vec3::new(-1.0, -t, s),
        2 => Vec3::new(s, 1.0, t),
        3 => Vec3::new(s, -1.0, -t),
        4 => Vec3::new(s, -t, 1.0),
        _ => Vec3::new(-s, -t, -1.0),
    }
}

/// G toggles the grid.
pub fn grid_key(keys: Res<ButtonInput<KeyCode>>, mut grid: ResMut<GzGrid>) {
    if keys.just_pressed(KeyCode::KeyG) {
        grid.visible = !grid.visible;
    }
}

/// Gazebo's default grid: 20 × 20 cells of 1 m on the ground plane.
pub fn draw_grid(grid: Res<GzGrid>, mut gizmos: Gizmos) {
    if !grid.visible {
        return;
    }
    gizmos.grid(
        // Gizmo grids lie in their local XY plane; Bevy's ground is XZ.
        Isometry3d::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
        UVec2::splat(20),
        Vec2::splat(1.0),
        // Dark lines: gizmos aren't exposed, so light ones vanish on sunlit
        // ground.
        Color::srgba(0.15, 0.15, 0.15, 0.45),
    );
}

/// Keeps grid lines out of the ground plane they sit on.
pub fn setup_gizmos(store: Option<ResMut<GizmoConfigStore>>) {
    if let Some(mut store) = store {
        let (config, _) = store.config_mut::<DefaultGizmoConfigGroup>();
        config.depth_bias = -0.0005;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::GzInbox;
    use crate::test_util::scene_app;
    use crate::GzConfig;
    use gz_core::msgs;

    /// Runs `scene` through the scene systems; returns the app.
    fn with_scene(scene: msgs::Scene) -> App {
        let mut app = scene_app(GzConfig::default());
        app.update();
        app.world_mut().resource_mut::<GzInbox>().scene = Some(Box::new(scene));
        app.update();
        app
    }

    fn camera_has<C: Component>(app: &mut App) -> bool {
        let world = app.world_mut();
        world
            .query_filtered::<(), (With<MainCamera>, With<C>)>()
            .iter(world)
            .count()
            == 1
    }

    #[test]
    fn nothing_is_installed_before_the_scene() {
        let mut app = scene_app(GzConfig::default());
        app.update();
        app.update();
        assert!(!camera_has::<GeneratedEnvironmentMapLight>(&mut app));
    }

    #[test]
    fn no_sky_gets_the_studio_environment() {
        let mut app = with_scene(msgs::Scene {
            grid: true,
            ..default()
        });
        assert!(camera_has::<GeneratedEnvironmentMapLight>(&mut app));
        assert!(!camera_has::<Atmosphere>(&mut app));
        assert!(app.world().resource::<GzGrid>().visible);
        assert_eq!(app.world().resource::<GlobalAmbientLight>().brightness, 0.0);
    }

    #[test]
    fn a_sky_without_a_cubemap_is_the_atmosphere() {
        let mut app = with_scene(msgs::Scene {
            sky: Some(msgs::Sky::default()),
            ..default()
        });
        assert!(camera_has::<Atmosphere>(&mut app));
        assert!(camera_has::<AtmosphereEnvironmentMapLight>(&mut app));
        assert!(!camera_has::<GeneratedEnvironmentMapLight>(&mut app));
    }

    #[test]
    fn a_cubemap_sky_is_a_skybox_and_lights_the_scene() {
        let mut app = with_scene(msgs::Scene {
            sky: Some(msgs::Sky {
                cubemap_uri: "file:///sky.dds".into(),
                ..default()
            }),
            fog: Some(msgs::Fog {
                r#type: msgs::fog::FogType::Exponential as i32,
                density: 0.05,
                ..default()
            }),
            ..default()
        });
        assert!(camera_has::<Skybox>(&mut app));
        assert!(camera_has::<GeneratedEnvironmentMapLight>(&mut app));
        let world = app.world_mut();
        let fog = world
            .query_filtered::<&DistanceFog, With<MainCamera>>()
            .single(world)
            .unwrap();
        assert!(matches!(fog.falloff, FogFalloff::Exponential { density } if density == 0.05));
    }

    #[test]
    fn g_toggles_the_grid_and_it_draws() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            bevy::gizmos::GizmoPlugin,
        ))
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<GzGrid>()
        .add_systems(Startup, setup_gizmos)
        .add_systems(Update, (grid_key, draw_grid).chain());
        app.update();
        let depth_bias = app
            .world()
            .resource::<GizmoConfigStore>()
            .config::<DefaultGizmoConfigGroup>()
            .0
            .depth_bias;
        assert!(depth_bias < 0.0);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyG);
        app.update();
        assert!(app.world().resource::<GzGrid>().visible);
        // Without gizmos, setting up is a no-op.
        let mut bare = App::new();
        bare.add_systems(Startup, setup_gizmos);
        bare.update();
    }

    #[test]
    fn quality_parses() {
        assert_eq!("high".parse(), Ok(Quality::High));
        assert_eq!("low".parse(), Ok(Quality::Low));
        assert!("ultra".parse::<Quality>().is_err());
        assert_eq!(Quality::default(), Quality::High);
    }

    #[test]
    fn exposure_matches_the_sun() {
        // 1 / (1.2 * 2^ev100) == π / SUN_LUX, opened half a stop, to 0.1%.
        let exposure = 1.0 / (1.2 * 2f32.powf(units::EXPOSURE.ev100));
        let want = std::f32::consts::PI / units::SUN_LUX * 2f32.sqrt();
        assert!((exposure / want - 1.0).abs() < 1e-3, "{exposure} vs {want}");
        assert_eq!(units::directional_lux(2.0), 2.0 * units::SUN_LUX);
        // A point light at 1 m matches the sun.
        let at_1m = units::point_lumens(1.0) / (4.0 * std::f32::consts::PI);
        assert!((at_1m - units::SUN_LUX).abs() < 1.0);
    }

    #[test]
    fn cube_faces_point_outward() {
        let centres = [
            Vec3::X,
            Vec3::NEG_X,
            Vec3::Y,
            Vec3::NEG_Y,
            Vec3::Z,
            Vec3::NEG_Z,
        ];
        for (face, want) in centres.iter().enumerate() {
            assert_eq!(cube_direction(face, 0.0, 0.0), *want);
        }
    }

    #[test]
    fn studio_is_bright_above_and_dark_below() {
        let image = studio_cubemap(Color::srgb(0.8, 0.8, 0.8), Vec3::ONE);
        assert_eq!(image.texture_descriptor.size.depth_or_array_layers, 6);
        let face = (STUDIO_SIZE * STUDIO_SIZE * 4) as usize;
        let data = image.data.as_ref().unwrap();
        let centre = |f: usize| {
            let i = f * face + ((STUDIO_SIZE / 2 * STUDIO_SIZE + STUDIO_SIZE / 2) * 4) as usize;
            data[i]
        };
        // +Y (sky) > +X (horizon) > -Y (ground).
        assert!(centre(2) > centre(0) && centre(0) > centre(3));
    }

    #[test]
    fn fog_follows_the_scene() {
        use gz_core::scene::FogFalloff as Gz;
        let none = distance_fog(None);
        assert!(matches!(none.falloff, FogFalloff::Linear { start, .. } if start == f32::MAX));
        let fog = |falloff| {
            distance_fog(Some(&FogDesc {
                color: [0.5, 0.5, 0.5],
                falloff,
            }))
        };
        assert!(matches!(
            fog(Gz::Linear {
                start: 1.0,
                end: 9.0
            })
            .falloff,
            FogFalloff::Linear {
                start: 1.0,
                end: 9.0
            }
        ));
        assert!(matches!(
            fog(Gz::Exponential { density: 0.2 }).falloff,
            FogFalloff::Exponential { density: 0.2 }
        ));
        let sq = fog(Gz::ExponentialSquared { density: 0.3 });
        assert!(matches!(
            sq.falloff,
            FogFalloff::ExponentialSquared { density: 0.3 }
        ));
        assert_eq!(sq.color, Color::srgb(0.5, 0.5, 0.5));
    }

    #[test]
    fn clear_colour_needs_an_explicit_background() {
        let env = |background| GzEnvironment {
            background,
            ..default()
        };
        assert_eq!(clear_color(&env(None)), CLEAR_COLOR);
        assert_eq!(clear_color(&env(Some([0.1, 0.2, 0.3, 0.0]))), CLEAR_COLOR);
        assert_eq!(
            clear_color(&env(Some([0.1, 0.2, 0.3, 1.0]))),
            Color::srgb(0.1, 0.2, 0.3)
        );
    }
}
