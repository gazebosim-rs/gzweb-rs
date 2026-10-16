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
use bevy_panorbit_camera::PanOrbitCameraPlugin;

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
}

impl Default for GzConfig {
    fn default() -> Self {
        Self {
            url: "ws://localhost:9002".into(),
            key: None,
            enable_lights: true,
            headless: false,
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
            app.add_plugins((
                PanOrbitCameraPlugin,
                bevy::diagnostic::FrameTimeDiagnosticsPlugin::default(),
            ))
            .add_systems(Startup, hud::spawn_hud)
            .add_systems(
                Update,
                (
                    camera::update_orbit_focus,
                    hud::update_fps,
                    hud::update_adapter,
                    hud::update_status,
                ),
            );
        }
    }
}
