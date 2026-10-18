//! Bevy plugin that renders a live Gazebo simulation served by the gz
//! websocket server (gz-launch / gz-sim `WebsocketServer`).
//!
//! Add [`assets::GzAssetSourcesPlugin`] **before** `DefaultPlugins` and
//! [`GzWebPlugin`] after.

pub mod assets;
pub mod camera;
pub mod collada;
pub mod connection;
pub mod driver;
pub mod heightmap;
pub mod hud;
pub mod mesh_file;
pub mod obj;
pub mod render;
pub mod snapshot;
pub mod spawn;
pub mod stl;
pub mod submesh;

use bevy::prelude::*;

pub use assets::GzAssetSourcesPlugin;

/// Connection and behaviour options.
#[derive(Resource, Clone, Debug)]
pub struct GzConfig {
    /// Websocket URL, e.g. `ws://localhost:9002`.
    pub url: String,
    /// Optional auth key for the websocket server.
    pub key: Option<String>,
    /// gzweb `enableLights`: whether lights attached to links start visible.
    pub enable_lights: bool,
    /// Camera controls, HUD and sim stats (false for `--snapshot` and
    /// `--screenshot`, which don't take input).
    pub interactive: bool,
    /// A GPU renders the scene (false for `--snapshot`); without one, GPU-only
    /// work such as environment maps is skipped.
    pub gpu: bool,
    /// Reconnect with backoff when the connection drops or fails.
    pub reconnect: bool,
    /// Extra PEM CA certificates to trust for `wss://` (native only).
    pub ca_cert: Option<std::path::PathBuf>,
    /// How much rendering work to spend (`render::Quality`).
    pub quality: render::Quality,
}

impl Default for GzConfig {
    fn default() -> Self {
        Self {
            url: "ws://localhost:9002".into(),
            key: None,
            enable_lights: true,
            interactive: true,
            gpu: true,
            reconnect: true,
            ca_cert: None,
            quality: render::Quality::default(),
        }
    }
}

pub struct GzWebPlugin {
    pub config: GzConfig,
}

impl Plugin for GzWebPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.config.clone())
            .insert_resource(ClearColor(camera::CLEAR_COLOR))
            // gzweb's colour until the scene sets one, at full fill strength
            // until `render::apply_environment` picks the environment.
            .insert_resource(GlobalAmbientLight {
                color: Color::srgb(25.0 / 255.0, 25.0 / 255.0, 25.0 / 255.0),
                brightness: render::units::ambient_brightness(render::units::FILL),
                affects_lightmapped_meshes: true,
            })
            .add_message::<connection::GzCommand>()
            .add_message::<connection::GzPublish>()
            .add_message::<connection::GzServiceReply>()
            .init_resource::<connection::GzInbox>()
            .init_resource::<spawn::GzIndex>()
            .init_resource::<spawn::GzLoads>()
            .init_resource::<camera::GzCameraTarget>()
            .init_asset_loader::<mesh_file::SceneFileLoader>()
            .init_asset_loader::<stl::StlLoader>()
            .add_plugins(heightmap::TerrainMaterialPlugin)
            .add_systems(
                Startup,
                (connection::connect, spawn::spawn_root, camera::spawn_camera),
            )
            .add_systems(
                Update,
                (
                    connection::pump,
                    spawn::apply_scene,
                    spawn::instance_gltf,
                    heightmap::build,
                    render::apply_environment,
                    render::install_sky,
                    spawn::apply_poses,
                )
                    .chain(),
            )
            .add_systems(Update, collada::drop_failed_textures);
        render::configure(app, self.config.quality);
        app.add_systems(Startup, render::setup_gizmos)
            .add_systems(Update, render::draw_grid);

        if self.config.interactive {
            app.add_plugins(bevy::diagnostic::FrameTimeDiagnosticsPlugin::default())
                .add_systems(Startup, hud::spawn_hud)
                .add_systems(
                    Update,
                    (
                        camera::orbit_controls,
                        hud::update_fps,
                        hud::update_adapter,
                        hud::update_status,
                        hud::update_sim,
                        hud::sim_keys,
                        render::grid_key,
                    ),
                );
        }
    }
}

/// A minimal app with the resources the scene systems need, for tests.
#[cfg(test)]
pub(crate) mod test_util {
    use bevy::prelude::*;

    use crate::{camera, connection, heightmap, render, spawn, GzAssetSourcesPlugin, GzConfig};

    /// No renderer, window or connection; the scene, pose and camera
    /// systems run against resources the test fills in.
    pub(crate) fn scene_app(config: GzConfig) -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            GzAssetSourcesPlugin::default(),
            AssetPlugin::default(),
            TransformPlugin,
        ))
        .init_asset::<Mesh>()
        .init_asset::<Image>()
        .init_asset::<StandardMaterial>()
        .init_asset::<heightmap::TerrainMaterial>()
        .init_asset::<WorldAsset>()
        .init_asset::<bevy::light::atmosphere::ScatteringMedium>()
        .insert_resource(config)
        .insert_resource(ClearColor(camera::CLEAR_COLOR))
        .init_resource::<GlobalAmbientLight>()
        .add_message::<connection::GzCommand>()
        .init_resource::<connection::GzInbox>()
        .init_resource::<spawn::GzIndex>()
        .init_resource::<spawn::GzLoads>()
        .init_resource::<camera::GzCameraTarget>()
        .init_resource::<render::GzEnvironment>()
        .init_resource::<render::GzGrid>()
        .init_resource::<render::PendingSky>()
        .insert_resource(connection::GzStatus {
            status: gz_core::transport::Status::Ready,
            world: Some("w".into()),
            ..connection::GzStatus::new("ws://test")
        })
        .add_systems(Startup, (spawn::spawn_root, camera::spawn_camera))
        .add_systems(
            Update,
            (
                spawn::apply_scene,
                spawn::instance_gltf,
                heightmap::build,
                render::apply_environment,
                render::install_sky,
                spawn::apply_poses,
            )
                .chain(),
        );
        app
    }
}

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests {
    use super::*;

    #[test]
    fn windowed_plugin_adds_controls_hud_and_diagnostics() {
        // Nothing listens on the port; the connection just fails.
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            GzAssetSourcesPlugin::default(),
            AssetPlugin::default(),
            bevy::input::InputPlugin,
            TransformPlugin,
            bevy::mesh::MeshPlugin,
            bevy::gizmos::GizmoPlugin,
        ))
        .init_asset::<Mesh>()
        .init_asset::<Image>()
        .init_asset::<StandardMaterial>()
        .init_asset::<WorldAsset>()
        .init_asset::<bevy::light::atmosphere::ScatteringMedium>()
        .add_plugins(GzWebPlugin {
            config: GzConfig {
                url: format!("ws://127.0.0.1:{port}"),
                interactive: true,
                ..Default::default()
            },
        });
        app.update();
        let world = app.world_mut();
        assert_eq!(
            world
                .query::<&camera::GzOrbitControls>()
                .iter(world)
                .count(),
            1
        );
        assert_eq!(world.query::<&hud::StatusText>().iter(world).count(), 1);
        assert!(world.contains_resource::<bevy::diagnostic::DiagnosticsStore>());
        assert_eq!(
            world.resource::<GzConfig>().url,
            format!("ws://127.0.0.1:{port}")
        );
    }

    #[test]
    fn headless_plugin_has_no_controls_or_hud() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            GzAssetSourcesPlugin::default(),
            AssetPlugin::default(),
            TransformPlugin,
            bevy::mesh::MeshPlugin,
            bevy::gizmos::GizmoPlugin,
        ))
        .init_asset::<Mesh>()
        .init_asset::<Image>()
        .init_asset::<StandardMaterial>()
        .init_asset::<WorldAsset>()
        .init_asset::<bevy::light::atmosphere::ScatteringMedium>()
        .add_plugins(GzWebPlugin {
            config: GzConfig {
                url: "ws://127.0.0.1:1".into(),
                interactive: false,
                ..Default::default()
            },
        });
        app.update();
        let world = app.world_mut();
        assert_eq!(
            world
                .query::<&camera::GzOrbitControls>()
                .iter(world)
                .count(),
            0
        );
        assert_eq!(world.query::<&hud::StatusText>().iter(world).count(), 0);
        assert_eq!(world.query::<&camera::MainCamera>().iter(world).count(), 1);
    }
}
