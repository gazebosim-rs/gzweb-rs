//! Rendering quality: the camera's post-processing, shadows, image-based
//! lighting, sky, fog and grid.
//!
//! gzweb renders with three.js r141's legacy lights, no tone mapping and no
//! environment lighting. gzweb-rs doesn't copy that look; it uses Bevy's
//! physically based pipeline (see `.agent/DESIGN.md` "Rendering"). The scene
//! graph and light *parameters* still follow Gazebo, and the parity snapshot
//! compares those, not pixels.

use bevy::anti_alias::taa::TemporalAntiAliasing;
use bevy::asset::{LoadState, RenderAssetUsages};
use bevy::camera::Hdr;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::core_pipeline::Skybox;
use bevy::light::atmosphere::ScatteringMedium;
use bevy::light::Atmosphere;
use bevy::light::{
    AtmosphereEnvironmentMapLight, DirectionalLightShadowMap, GeneratedEnvironmentMapLight,
    PointLightShadowMap, ShadowFilteringMethod,
};
use bevy::pbr::{
    AtmosphereSettings, DistanceFog, FogFalloff, ScreenSpaceAmbientOcclusion,
    ScreenSpaceAmbientOcclusionQualityLevel,
};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension,
};
use bevy::render::view::Msaa;
use gz_core::scene::{FogDesc, SceneDesc};

use crate::assets;
use crate::camera::{self, GzOrbitControls, MainCamera, CLEAR_COLOR};
use crate::spawn::GzLoads;
use crate::GzConfig;

/// How much rendering work to spend.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Quality {
    /// 4x MSAA and 1024 px shadows; no screen-space effects or image-based
    /// light (a flat ambient fills instead).
    Low,
    /// TAA, SSAO, bloom, image-based light, 2048 px cascaded shadows.
    #[default]
    High,
    /// High with 4096 px shadows: sharper distant shadows for 4× the shadow
    /// memory (256 MB per shadow-casting sun).
    Ultra,
}

impl std::str::FromStr for Quality {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "low" => Ok(Quality::Low),
            "high" => Ok(Quality::High),
            "ultra" => Ok(Quality::Ultra),
            other => Err(format!("unknown quality '{other}' (low, high or ultra)")),
        }
    }
}

impl Quality {
    pub fn msaa(self) -> Msaa {
        match self {
            Quality::Low => Msaa::Sample4,
            // TAA and SSAO need MSAA off.
            Quality::High | Quality::Ultra => Msaa::Off,
        }
    }

    /// Directional and point light shadow map sizes.
    pub fn shadow_maps(self) -> (usize, usize) {
        match self {
            Quality::Low => (1024, 512),
            Quality::High => (2048, 1024),
            Quality::Ultra => (4096, 2048),
        }
    }

    /// Whether environments light the scene (GPU-filtered environment maps).
    pub fn image_based_light(self) -> bool {
        self != Quality::Low
    }
}

/// Light units. Gazebo light intensities are unitless; these put them on
/// Bevy's physical scale with one fixed camera exposure, so their ratios stay
/// as Gazebo intends and the tone mapper handles the highlights.
pub mod units {
    use bevy::camera::Exposure;
    use bevy::color::{Color, LinearRgba, Srgba};
    use std::f32::consts::{PI, SQRT_2};

    /// Illuminance of a directional light of intensity 1 (daylight).
    pub const SUN_LUX: f32 = 10_000.0;

    /// Camera exposure factor: a white surface facing a [`SUN_LUX`] light
    /// renders at 1.0 (`π / SUN_LUX`), opened half a stop because the tone
    /// mapper compresses highlights.
    pub const EXPOSURE_FACTOR: f32 = PI / SUN_LUX * SQRT_2;

    /// [`EXPOSURE_FACTOR`] as Bevy's EV100: `1 / (1.2 * 2^ev100)`.
    pub fn exposure() -> Exposure {
        Exposure {
            ev100: (1.0 / (1.2 * EXPOSURE_FACTOR)).log2(),
        }
    }

    /// The luminance (cd/m²) that renders as 1 under [`exposure`].
    pub const UNIT_LUMINANCE: f32 = 1.0 / EXPOSURE_FACTOR;

    /// `StandardMaterial::emissive` for an emission colour (SDF `<emissive>`,
    /// COLLADA `<emission>`; 0–1, sRGB like the other colours): Bevy
    /// multiplies emission by the exposure, so 1 renders at full brightness.
    pub fn emissive(color: Color) -> LinearRgba {
        let c = LinearRgba::from(color);
        let u = UNIT_LUMINANCE;
        LinearRgba::rgb(c.red * u, c.green * u, c.blue * u)
    }

    /// The emission colour [`emissive`] was given (for snapshots).
    pub fn emission_color(emissive: LinearRgba) -> Srgba {
        let u = UNIT_LUMINANCE;
        let e = emissive;
        Srgba::from(LinearRgba::rgb(e.red / u, e.green / u, e.blue / u))
    }

    /// Illuminance of a directional light of Gazebo intensity `intensity`.
    pub fn directional_lux(intensity: f32) -> f32 {
        intensity * SUN_LUX
    }

    /// A point or spot light of intensity 1 lights a surface 1 m away as
    /// brightly as the sun: `4π sr × SUN_LUX`.
    pub fn point_lumens(intensity: f32) -> f32 {
        intensity * 4.0 * PI * SUN_LUX
    }

    /// Share of full sun that a white `<ambient>` adds as fill light.
    pub const FILL: f32 = 0.5;

    /// Share of full sun the flat ambient light always adds (times the
    /// ambient colour) under image-based light, so shadows never go black.
    pub const AMBIENT_FLOOR: f32 = 0.05;

    /// `GlobalAmbientLight::brightness` that adds `share` of full sun to a
    /// white surface. Bevy scales diffuse ambient by
    /// `EnvBRDFApprox(albedo, F_AB(1, N·V)) = 0.4524 * albedo` (rough).
    pub fn ambient_brightness(share: f32) -> f32 {
        share / (0.4524 * EXPOSURE_FACTOR)
    }
}

/// Shadow map resolutions and the environment resources for `quality`.
pub fn configure(app: &mut App, quality: Quality) {
    let (directional, point) = quality.shadow_maps();
    app.insert_resource(DirectionalLightShadowMap { size: directional })
        .insert_resource(PointLightShadowMap { size: point })
        .init_resource::<GzEnvironment>()
        .init_resource::<GzGrid>()
        .init_resource::<PendingSky>()
        .init_gizmo_group::<GridGizmos>();
}

/// The world's environment settings, set by `spawn::apply_scene` from each
/// new scene.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct GzEnvironment {
    /// False until the first scene arrives.
    pub loaded: bool,
    pub ambient: Option<[f32; 3]>,
    pub background: Option<[f32; 4]>,
    pub sky: Option<Option<String>>,
    pub fog: Option<FogDesc>,
    /// The scene has a directional light (needed to light an atmosphere).
    pub has_sun: bool,
}

impl GzEnvironment {
    pub fn from_scene(desc: &SceneDesc) -> Self {
        Self {
            loaded: true,
            ambient: desc.ambient,
            background: desc.background,
            sky: desc.sky.clone(),
            fog: desc.fog,
            has_sun: desc.has_directional_light(),
        }
    }

    /// The ambient colour, gzweb's 0.1 grey when the scene has none.
    pub fn ambient_or_default(&self) -> Vec3 {
        self.ambient
            .map_or(Vec3::splat(25.0 / 255.0), Vec3::from_array)
    }
}

/// Whether the ground grid is drawn (the scene's `grid` flag; G toggles it).
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GzGrid {
    pub visible: bool,
}

/// A cubemap sky that is loading, and the camera to install it on.
#[derive(Resource, Debug, Default)]
pub struct PendingSky(pub Option<(Handle<Image>, Entity)>);

/// What lights and backs the scene.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Environment {
    /// Only the flat ambient light (no GPU, or `Quality::Low` without a sky).
    Ambient,
    /// Image-based light from a procedural studio cubemap.
    Studio,
    /// Bevy's physically based atmosphere.
    Atmosphere,
    /// A cubemap skybox from this URI.
    Cubemap(String),
}

/// The environment for `env` with `config`'s GPU and quality.
pub fn plan(env: &GzEnvironment, config: &GzConfig) -> Environment {
    if !config.gpu {
        return Environment::Ambient;
    }
    match &env.sky {
        Some(Some(uri)) => Environment::Cubemap(uri.clone()),
        // An atmosphere needs a sun, or it is black.
        Some(None) if env.has_sun => Environment::Atmosphere,
        _ if config.quality.image_based_light() => Environment::Studio,
        _ => Environment::Ambient,
    }
}

/// The camera's post-processing for `quality`.
pub fn camera_components(camera: &mut EntityCommands, quality: Quality) {
    camera.insert((
        Hdr,
        Tonemapping::TonyMcMapface,
        units::exposure(),
        quality.msaa(),
    ));
    if quality != Quality::Low {
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
}

/// Clear colour of the scene's background (alpha > 0 means it was set). It
/// goes through the tone mapper like everything else.
pub fn clear_color(env: &GzEnvironment) -> Color {
    match env.background {
        Some([r, g, b, a]) if a > 0.0 => Color::srgb(r, g, b),
        _ => CLEAR_COLOR,
    }
}

/// The planet of a `<sky>` atmosphere. It has no `Transform`, so Bevy places
/// its centre `inner_radius` below the origin, and the ground around the
/// scene is level.
#[derive(Component)]
pub struct GzAtmosphere;

/// Applies a changed [`GzEnvironment`]. The camera is respawned, keeping its
/// view: Bevy keeps private per-camera state for environment maps and the
/// atmosphere that only goes away with the entity, and a camera that
/// switches environment-map sources fails wgpu validation.
#[allow(clippy::too_many_arguments)]
pub fn apply_environment(
    env: Res<GzEnvironment>,
    config: Res<GzConfig>,
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut media: ResMut<Assets<ScatteringMedium>>,
    mut medium: Local<Option<Handle<ScatteringMedium>>>,
    asset_server: Res<AssetServer>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut loads: ResMut<GzLoads>,
    mut pending: ResMut<PendingSky>,
    cameras: Query<(Entity, &Transform, Option<&GzOrbitControls>), With<MainCamera>>,
    planets: Query<Entity, With<GzAtmosphere>>,
) {
    if !env.is_changed() || !env.loaded {
        return;
    }
    let plan = plan(&env, &config);
    for planet in &planets {
        commands.entity(planet).despawn();
    }
    if plan == Environment::Atmosphere {
        // One medium for every atmosphere, so its GPU tables are built once.
        let medium = medium.get_or_insert_with(|| media.add(ScatteringMedium::default()));
        commands.spawn((GzAtmosphere, Atmosphere::earth(medium.clone())));
    }
    if let Some([r, g, b]) = env.ambient {
        ambient.color = Color::srgb(r, g, b);
    }
    let lit_by_environment = plan == Environment::Studio
        || (plan == Environment::Atmosphere && config.quality.image_based_light());
    ambient.brightness = units::ambient_brightness(if lit_by_environment {
        units::AMBIENT_FLOOR
    } else {
        units::FILL
    });
    pending.0 = None;

    for (old, transform, controls) in &cameras {
        let focus = controls.map(|c| c.target);
        commands.entity(old).despawn();
        let id =
            camera::spawn_main_camera(&mut commands, &config, *transform, focus, clear_color(&env));
        let mut cam = commands.entity(id);
        if let Some(fog) = &env.fog {
            cam.insert(distance_fog(fog));
        }
        match &plan {
            Environment::Ambient => {}
            Environment::Studio => {
                cam.insert(studio_light(&mut images, env.ambient_or_default()));
            }
            Environment::Atmosphere => {
                cam.insert(AtmosphereSettings::default());
                if config.quality.image_based_light() {
                    cam.insert(AtmosphereEnvironmentMapLight::default());
                }
            }
            // Installed by `install_sky` once loaded and checked.
            Environment::Cubemap(uri) => {
                let path = assets::asset_path(&gz_core::uri::mesh_load_uri(uri));
                let image: Handle<Image> = asset_server.load(path);
                loads.0.push(image.clone().untyped());
                pending.0 = Some((image, id));
            }
        }
    }
}

fn studio_light(images: &mut Assets<Image>, ambient: Vec3) -> GeneratedEnvironmentMapLight {
    GeneratedEnvironmentMapLight {
        environment_map: images.add(studio_cubemap(ambient)),
        intensity: units::UNIT_LUMINANCE * units::FILL,
        ..default()
    }
}

/// Installs a pending cubemap sky once it has loaded: a skybox that also
/// lights the scene as brightly as it looks. A cubemap Bevy can't use
/// (failed to load, or not six square power-of-two faces of at most 8192 px)
/// falls back to the studio environment.
pub fn install_sky(
    mut pending: ResMut<PendingSky>,
    config: Res<GzConfig>,
    env: Res<GzEnvironment>,
    asset_server: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut commands: Commands,
) {
    let Some((handle, camera)) = pending.0.clone() else {
        return;
    };
    let usable = match images.get_mut(&handle) {
        Some(mut image) => make_cubemap(&mut image),
        None if matches!(asset_server.load_state(&handle), LoadState::Failed(_)) => false,
        None => return,
    };
    pending.0 = None;
    let ibl = config.quality.image_based_light();
    let mut cam = commands.entity(camera);
    if usable {
        cam.insert(Skybox {
            image: Some(handle.clone()),
            brightness: units::UNIT_LUMINANCE,
            ..default()
        });
        if ibl {
            cam.insert(GeneratedEnvironmentMapLight {
                environment_map: handle,
                intensity: units::UNIT_LUMINANCE,
                ..default()
            });
        }
    } else {
        warn!("sky cubemap unusable (failed to load, or not 6 square power-of-two faces); using the studio environment");
        if ibl {
            cam.insert(studio_light(&mut images, env.ambient_or_default()));
        }
    }
    if ibl {
        ambient.brightness = units::ambient_brightness(units::AMBIENT_FLOOR);
    }
}

/// Checks that `image` can be a skybox and an environment-map source, and
/// gives it a cube view.
pub fn make_cubemap(image: &mut Image) -> bool {
    let size = image.texture_descriptor.size;
    let ok = size.depth_or_array_layers == 6
        && size.width == size.height
        && size.width.is_power_of_two()
        && size.width <= 8192;
    if ok {
        image.texture_view_descriptor = Some(TextureViewDescriptor {
            dimension: Some(TextureViewDimension::Cube),
            ..default()
        });
    }
    ok
}

/// Bevy fog for the scene's fog.
pub fn distance_fog(fog: &FogDesc) -> DistanceFog {
    use gz_core::scene::FogFalloff as Gz;
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

/// Size of each face of the studio cubemap: a smooth gradient that depends
/// only on height, so a small power of two is enough.
pub const STUDIO_SIZE: u32 = 64;

/// A neutral "studio" environment lit by the ambient colour: that colour at
/// the horizon, brighter overhead and darker below, scaled down as a whole
/// (keeping its hue) where a channel would pass 1.
pub fn studio_cubemap(ambient: Vec3) -> Image {
    let fit = |c: Vec3| c / c.max_element().max(1.0);
    let horizon = fit(ambient);
    let zenith = fit(ambient * 1.25);
    let ground = horizon * 0.35;
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
                data.extend_from_slice(&Srgba::from(LinearRgba::rgb(c.x, c.y, c.z)).to_u8_array());
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
        TextureFormat::Rgba8UnormSrgb,
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

/// Gizmo settings for the grid alone.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct GridGizmos;

/// Gazebo's default grid: 20 × 20 cells of 1 m on the ground plane.
pub fn draw_grid(grid: Res<GzGrid>, mut gizmos: Gizmos<GridGizmos>) {
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
pub fn setup_gizmos(mut store: ResMut<GizmoConfigStore>) {
    let (config, _) = store.config_mut::<GridGizmos>();
    config.depth_bias = -0.0005;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::GzInbox;
    use crate::test_util::scene_app;
    use bevy::light::EnvironmentMapLight;
    use gz_core::msgs;

    #[test]
    fn quality_levels() {
        assert_eq!("low".parse(), Ok(Quality::Low));
        assert_eq!("high".parse(), Ok(Quality::High));
        assert_eq!("ultra".parse(), Ok(Quality::Ultra));
        assert!("max".parse::<Quality>().is_err());
        assert_eq!(Quality::default(), Quality::High);
        assert_eq!(Quality::Low.msaa(), Msaa::Sample4);
        assert_eq!(Quality::High.msaa(), Msaa::Off);
        assert_eq!(Quality::High.shadow_maps(), (2048, 1024));
        assert_eq!(Quality::Ultra.shadow_maps(), (4096, 2048));
        assert_eq!(Quality::Low.shadow_maps(), (1024, 512));
        assert!(!Quality::Low.image_based_light() && Quality::Ultra.image_based_light());
    }

    #[test]
    fn units_are_consistent() {
        let ev = units::exposure().ev100;
        let factor = 1.0 / (1.2 * 2f32.powf(ev));
        assert!((factor / units::EXPOSURE_FACTOR - 1.0).abs() < 1e-5);
        assert_eq!(units::directional_lux(2.0), 2.0 * units::SUN_LUX);
        // A point light at 1 m matches the sun.
        let at_1m = units::point_lumens(1.0) / (4.0 * std::f32::consts::PI);
        assert!((at_1m - units::SUN_LUX).abs() < 1.0);
        // Ambient brightness × Bevy's 0.4524 × exposure = the share.
        let b = units::ambient_brightness(0.5);
        assert!((b * 0.4524 * units::EXPOSURE_FACTOR - 0.5).abs() < 1e-5);
        // An emission colour of 1 renders at 1, and snapshots get it back.
        let e = units::emissive(Color::srgb(1.0, 0.5, 0.0));
        assert!((e.red * units::EXPOSURE_FACTOR - 1.0).abs() < 1e-5);
        let back = units::emission_color(e);
        assert!(
            (back.green - 0.5).abs() < 1e-5 && back.blue == 0.0,
            "{back:?}"
        );
    }

    fn env(sky: Option<Option<&str>>, has_sun: bool) -> GzEnvironment {
        GzEnvironment {
            loaded: true,
            sky: sky.map(|s| s.map(str::to_string)),
            has_sun,
            ..default()
        }
    }

    #[test]
    fn environment_plans() {
        let high = GzConfig::default();
        let low = GzConfig {
            quality: Quality::Low,
            ..default()
        };
        let no_gpu = GzConfig {
            gpu: false,
            ..default()
        };
        assert_eq!(plan(&env(None, true), &high), Environment::Studio);
        assert_eq!(plan(&env(None, true), &low), Environment::Ambient);
        assert_eq!(plan(&env(Some(None), true), &high), Environment::Atmosphere);
        assert_eq!(plan(&env(Some(None), true), &low), Environment::Atmosphere);
        // No sun: the atmosphere would be black.
        assert_eq!(plan(&env(Some(None), false), &high), Environment::Studio);
        assert_eq!(
            plan(&env(Some(Some("s.dds")), false), &low),
            Environment::Cubemap("s.dds".into())
        );
        assert_eq!(plan(&env(Some(None), true), &no_gpu), Environment::Ambient);
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

    fn centre(image: &Image, face: u32) -> [u8; 4] {
        let at =
            ((face * STUDIO_SIZE * STUDIO_SIZE + STUDIO_SIZE / 2 * STUDIO_SIZE + STUDIO_SIZE / 2)
                * 4) as usize;
        let data = image.data.as_ref().unwrap();
        [data[at], data[at + 1], data[at + 2], data[at + 3]]
    }

    #[test]
    fn studio_follows_the_ambient_colour() {
        let image = studio_cubemap(Vec3::splat(0.4));
        assert_eq!(
            image.texture_descriptor.format,
            TextureFormat::Rgba8UnormSrgb
        );
        // +Y (sky) > +X (horizon) > -Y (ground).
        assert!(centre(&image, 2)[0] > centre(&image, 0)[0]);
        assert!(centre(&image, 0)[0] > centre(&image, 3)[0]);
        // A saturated colour keeps its hue instead of clipping per channel.
        let red = studio_cubemap(Vec3::new(2.0, 1.0, 0.0));
        let [r, g, b, _] = centre(&red, 0);
        let lin = |v: u8| LinearRgba::from(Srgba::rgb_u8(v, 0, 0)).red;
        assert_eq!(b, 0);
        assert!((lin(g) / lin(r) - 0.5).abs() < 0.02, "{r} {g}");
    }

    #[test]
    fn cubemaps_are_checked() {
        let image = |w: u32, h: u32, layers: u32| {
            Image::new_fill(
                Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: layers,
                },
                TextureDimension::D2,
                &[0, 0, 0, 255],
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::RENDER_WORLD,
            )
        };
        let mut ok = image(8, 8, 6);
        assert!(make_cubemap(&mut ok));
        assert_eq!(
            ok.texture_view_descriptor.unwrap().dimension,
            Some(TextureViewDimension::Cube)
        );
        assert!(!make_cubemap(&mut image(8, 8, 1)));
        assert!(!make_cubemap(&mut image(8, 4, 6)));
        assert!(!make_cubemap(&mut image(6, 6, 6)));
    }

    #[test]
    fn fog_maps_each_falloff() {
        use gz_core::scene::FogFalloff as Gz;
        let fog = |falloff| {
            distance_fog(&FogDesc {
                color: [0.5, 0.5, 0.5],
                falloff,
            })
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

    /// Feeds `scene` to the scene systems and runs a frame.
    fn show(app: &mut App, scene: msgs::Scene) {
        app.world_mut().resource_mut::<GzInbox>().scene = Some(Box::new(scene));
        app.update();
    }

    fn cameras(app: &mut App) -> Vec<Entity> {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<MainCamera>>()
            .iter(world)
            .collect()
    }

    /// The number of atmosphere planets, after checking that each sits
    /// `inner_radius` below the origin with no `Transform` (which would put
    /// it back at the origin).
    fn planets(app: &mut App) -> usize {
        let world = app.world_mut();
        world
            .query_filtered::<(&Atmosphere, &GlobalTransform, Has<Transform>), With<GzAtmosphere>>()
            .iter(world)
            .inspect(|(atmosphere, at, has_transform)| {
                assert_eq!(at.translation(), -Vec3::Y * atmosphere.inner_radius);
                assert!(!has_transform);
            })
            .count()
    }

    fn has<C: Component>(app: &mut App) -> bool {
        let world = app.world_mut();
        world
            .query_filtered::<(), (With<MainCamera>, With<C>)>()
            .iter(world)
            .count()
            == 1
    }

    fn sun() -> msgs::Light {
        msgs::Light {
            name: "sun".into(),
            r#type: msgs::light::LightType::Directional as i32,
            ..default()
        }
    }

    #[test]
    fn each_scene_gets_a_fresh_camera_with_its_environment() {
        let mut app = scene_app(GzConfig::default());
        app.update();
        // Nothing is installed before the first scene.
        assert!(!has::<GeneratedEnvironmentMapLight>(&mut app));
        let first = cameras(&mut app);

        // Studio, with fog and a grid; the ambient colour is the scene's.
        show(
            &mut app,
            msgs::Scene {
                grid: true,
                ambient: Some(msgs::Color {
                    r: 0.3,
                    g: 0.3,
                    b: 0.3,
                    a: 1.0,
                    ..default()
                }),
                fog: Some(msgs::Fog {
                    r#type: msgs::fog::FogType::Exponential as i32,
                    density: 0.05,
                    ..default()
                }),
                ..default()
            },
        );
        assert_ne!(cameras(&mut app), first);
        assert_eq!(cameras(&mut app).len(), 1);
        assert!(has::<GeneratedEnvironmentMapLight>(&mut app));
        assert!(has::<DistanceFog>(&mut app));
        assert!(app.world().resource::<GzGrid>().visible);
        let ambient = app.world().resource::<GlobalAmbientLight>();
        assert_eq!(ambient.color, Color::srgb(0.3, 0.3, 0.3));
        assert_eq!(
            ambient.brightness,
            units::ambient_brightness(units::AMBIENT_FLOOR)
        );

        // Bevy derives an EnvironmentMapLight from the generated one; a new
        // scene must not inherit it.
        let cam = cameras(&mut app)[0];
        app.world_mut()
            .entity_mut(cam)
            .insert(EnvironmentMapLight::default());

        // Atmosphere (it has a sun), no fog, and no <ambient>: the colour stays.
        show(
            &mut app,
            msgs::Scene {
                sky: Some(msgs::Sky::default()),
                light: vec![sun()],
                ..default()
            },
        );
        assert!(has::<AtmosphereSettings>(&mut app));
        assert_eq!(planets(&mut app), 1);
        assert!(has::<AtmosphereEnvironmentMapLight>(&mut app));
        assert!(!has::<GeneratedEnvironmentMapLight>(&mut app));
        assert!(!has::<EnvironmentMapLight>(&mut app));
        assert!(!has::<DistanceFog>(&mut app));
        assert_eq!(
            app.world().resource::<GlobalAmbientLight>().color,
            Color::srgb(0.3, 0.3, 0.3)
        );

        // Another sky scene replaces the planet, and shares its medium.
        show(
            &mut app,
            msgs::Scene {
                sky: Some(msgs::Sky::default()),
                light: vec![sun()],
                background: Some(msgs::Color {
                    b: 1.0,
                    a: 1.0,
                    ..default()
                }),
                ..default()
            },
        );
        assert_eq!(planets(&mut app), 1);
        assert_eq!(app.world().resource::<Assets<ScatteringMedium>>().len(), 1);

        // A cubemap that fails to load falls back to the studio.
        show(
            &mut app,
            msgs::Scene {
                sky: Some(msgs::Sky {
                    cubemap_uri: "file:///nowhere/sky.dds".into(),
                    ..default()
                }),
                ..default()
            },
        );
        assert!(!has::<AtmosphereSettings>(&mut app));
        assert_eq!(planets(&mut app), 0);
        assert!(app.world().resource::<PendingSky>().0.is_some());
        for _ in 0..200 {
            app.update();
            if app.world().resource::<PendingSky>().0.is_none() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(has::<GeneratedEnvironmentMapLight>(&mut app));
        assert!(!has::<Skybox>(&mut app));
    }

    #[test]
    fn a_good_cubemap_becomes_the_skybox_and_the_light() {
        let mut app = scene_app(GzConfig::default());
        app.update();
        let cam = cameras(&mut app)[0];
        let sky = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(Image::new_fill(
                Extent3d {
                    width: 4,
                    height: 4,
                    depth_or_array_layers: 6,
                },
                TextureDimension::D2,
                &[0, 0, 255, 255],
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::RENDER_WORLD,
            ));
        app.world_mut().resource_mut::<PendingSky>().0 = Some((sky.clone(), cam));
        app.update();
        assert!(app.world().resource::<PendingSky>().0.is_none());
        assert!(has::<Skybox>(&mut app));
        let world = app.world_mut();
        let light = world
            .query::<&GeneratedEnvironmentMapLight>()
            .single(world)
            .unwrap();
        assert_eq!(light.environment_map, sky);
        assert_eq!(light.intensity, units::UNIT_LUMINANCE);
    }

    #[test]
    fn low_quality_lights_with_a_flat_ambient() {
        let mut app = scene_app(GzConfig {
            quality: Quality::Low,
            ..default()
        });
        app.update();
        show(&mut app, msgs::Scene::default());
        assert!(!has::<GeneratedEnvironmentMapLight>(&mut app));
        assert_eq!(
            app.world().resource::<GlobalAmbientLight>().brightness,
            units::ambient_brightness(units::FILL)
        );
        // A bad cubemap at Low: no studio either.
        let cam = cameras(&mut app)[0];
        let flat = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(Image::default());
        app.world_mut().resource_mut::<PendingSky>().0 = Some((flat, cam));
        app.update();
        assert!(!has::<GeneratedEnvironmentMapLight>(&mut app));
        assert!(!has::<Skybox>(&mut app));
    }

    #[test]
    fn g_toggles_the_grid_and_it_draws() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            bevy::mesh::MeshPlugin,
            bevy::gizmos::GizmoPlugin,
        ))
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<GzGrid>()
        .init_gizmo_group::<GridGizmos>()
        .add_systems(Startup, setup_gizmos)
        .add_systems(Update, (grid_key, draw_grid).chain());
        app.update();
        let (config, _) = app
            .world()
            .resource::<GizmoConfigStore>()
            .config::<GridGizmos>();
        assert!(config.depth_bias < 0.0);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyG);
        app.update();
        assert!(app.world().resource::<GzGrid>().visible);
        // Drawing a visible grid runs (gizmo buffers aren't inspectable).
        app.update();
    }
}
