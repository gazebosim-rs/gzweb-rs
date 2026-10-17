//! Bevy plugin that renders a live Gazebo simulation served by the gz
//! websocket server (gz-launch / gz-sim `WebsocketServer`).
//!
//! Add [`assets::GzAssetSourcesPlugin`] **before** `DefaultPlugins` and
//! [`GzWebPlugin`] after.

pub mod assets;
pub mod camera;
pub mod connection;
pub mod driver;
pub mod hud;
pub mod snapshot;
pub mod spawn;

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
    /// No window, no camera controls or HUD (snapshot / CI mode).
    pub headless: bool,
    /// Reconnect with backoff when the connection drops or fails.
    pub reconnect: bool,
    /// Extra PEM CA certificates to trust for `wss://` (native only).
    pub ca_cert: Option<std::path::PathBuf>,
}

impl Default for GzConfig {
    fn default() -> Self {
        Self {
            url: "ws://localhost:9002".into(),
            key: None,
            enable_lights: true,
            headless: false,
            reconnect: true,
            ca_cert: None,
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
            // gzweb's ambient light colour is 0x191919 until the scene sets it.
            .insert_resource(GlobalAmbientLight {
                color: Color::srgb(25.0 / 255.0, 25.0 / 255.0, 25.0 / 255.0),
                brightness: 300.0,
                affects_lightmapped_meshes: true,
            })
            .add_message::<connection::GzCommand>()
            .init_resource::<connection::GzInbox>()
            .init_resource::<spawn::GzIndex>()
            .init_resource::<spawn::GzLoads>()
            .init_resource::<camera::GzCameraTarget>()
            .add_plugins((bevy_obj::ObjPlugin, bevy_stl::StlPlugin))
            .add_systems(
                Startup,
                (connection::connect, spawn::spawn_root, camera::spawn_camera),
            )
            .add_systems(
                Update,
                (connection::pump, spawn::apply_scene, spawn::apply_poses).chain(),
            );

        if !self.config.headless {
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
                    ),
                );
        }
    }
}

/// A minimal app with the resources the scene systems need, for tests.
#[cfg(test)]
pub(crate) mod test_util {
    use bevy::prelude::*;

    use crate::{camera, connection, spawn, GzAssetSourcesPlugin, GzConfig};

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
        .init_asset::<Scene>()
        .insert_resource(config)
        .insert_resource(ClearColor(camera::CLEAR_COLOR))
        .init_resource::<GlobalAmbientLight>()
        .add_message::<connection::GzCommand>()
        .init_resource::<connection::GzInbox>()
        .init_resource::<spawn::GzIndex>()
        .init_resource::<spawn::GzLoads>()
        .init_resource::<camera::GzCameraTarget>()
        .insert_resource(connection::GzStatus {
            status: gz_core::transport::Status::Ready,
            world: Some("w".into()),
            ..connection::GzStatus::new("ws://test")
        })
        .add_systems(Startup, (spawn::spawn_root, camera::spawn_camera))
        .add_systems(Update, (spawn::apply_scene, spawn::apply_poses).chain());
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
        ))
        .init_asset::<Mesh>()
        .init_asset::<Image>()
        .init_asset::<StandardMaterial>()
        .init_asset::<Scene>()
        .add_plugins(GzWebPlugin {
            config: GzConfig {
                url: format!("ws://127.0.0.1:{port}"),
                headless: false,
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
        ))
        .init_asset::<Mesh>()
        .init_asset::<Image>()
        .init_asset::<StandardMaterial>()
        .init_asset::<Scene>()
        .add_plugins(GzWebPlugin {
            config: GzConfig {
                url: "ws://127.0.0.1:1".into(),
                headless: true,
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
