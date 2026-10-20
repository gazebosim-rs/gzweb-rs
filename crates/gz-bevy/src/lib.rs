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
pub mod material;
pub mod mesh_file;
pub mod obj;
pub mod render;
pub mod snapshot;
pub mod spawn;
pub mod stl;
pub mod submesh;
pub mod view;

use bevy::prelude::*;

pub use assets::GzAssetSourcesPlugin;

/// How much UI runs (`.agent/UI_PLAN.md` §2.1, DECISIONS.md D-10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UiMode {
    /// Scene, rendering and connection only: no input, no HUD
    /// (`--snapshot`, `--screenshot`).
    None,
    /// The 3D view with [`view::ViewerControls`]: camera, viewport and sim
    /// keys, and the status text.
    Minimal,
    /// The whole workbench: the app adds `gz_ui::GzUiPlugin` on top of
    /// [`UiMode::Minimal`] (gz-ui depends on this crate, not the other way
    /// round). Until its shell lands (M2) it shows nothing more.
    #[default]
    Full,
}

impl UiMode {
    /// Takes input and shows status: anything but [`UiMode::None`].
    pub fn interactive(self) -> bool {
        self != UiMode::None
    }
}

impl std::str::FromStr for UiMode {
    type Err = String;

    /// `full` or `minimal`, as `--ui` and `?ui=` take them. [`UiMode::None`]
    /// comes only from the capture modes.
    fn from_str(s: &str) -> Result<Self, String> {
        match s.to_ascii_lowercase().as_str() {
            "full" => Ok(UiMode::Full),
            "minimal" => Ok(UiMode::Minimal),
            _ => Err(format!("unknown UI mode '{s}' (full or minimal)")),
        }
    }
}

/// Connection and behaviour options.
#[derive(Resource, Clone, Debug)]
pub struct GzConfig {
    /// Websocket URL, e.g. `ws://localhost:9002`.
    pub url: String,
    /// Optional auth key for the websocket server.
    pub key: Option<String>,
    /// gzweb `enableLights`: whether lights attached to links start visible.
    pub enable_lights: bool,
    /// How much UI runs. [`UiMode::None`] for `--snapshot` and
    /// `--screenshot`, which don't take input: no controls, HUD or sim
    /// stats.
    pub ui: UiMode,
    /// The status text overlay, when [`GzConfig::interactive`].
    pub hud: bool,
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
            ui: UiMode::default(),
            hud: true,
            gpu: true,
            reconnect: true,
            ca_cert: None,
            quality: render::Quality::default(),
        }
    }
}

impl GzConfig {
    /// Camera controls, HUD and sim stats: the UI mode isn't
    /// [`UiMode::None`] (what the old `interactive` flag meant).
    pub fn interactive(&self) -> bool {
        self.ui.interactive()
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
            .init_resource::<material::GzMaterials>()
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
            .add_systems(
                Update,
                (
                    material::combine_metal_rough,
                    collada::drop_failed_textures,
                    heightmap::drop_failed_textures,
                ),
            );
        render::configure(app, self.config.quality);
        app.add_systems(Startup, render::setup_gizmos).add_systems(
            Update,
            (
                render::draw_grid,
                render::projection_effects.after(render::apply_environment),
            ),
        );

        if self.config.interactive() {
            // `UiMode::Full`: the app adds `gz_ui::GzUiPlugin` on top.
            app.add_plugins(view::ViewerControls {
                mode: self.config.ui,
            });
        }
        // The host page's own status lines go with the HUD.
        #[cfg(target_arch = "wasm32")]
        if !(self.config.interactive() && self.config.hud) {
            hud::remove_dom_status();
        }
        if self.config.interactive() && self.config.hud {
            app.add_plugins(bevy::diagnostic::FrameTimeDiagnosticsPlugin::default())
                .add_systems(Startup, hud::spawn_hud)
                .add_systems(
                    Update,
                    (
                        hud::update_fps,
                        hud::update_adapter,
                        hud::update_status,
                        hud::update_sim,
                    ),
                );
        }
    }
}

/// A minimal app with the resources the scene systems need, for tests.
#[cfg(test)]
pub(crate) mod test_util {
    use bevy::prelude::*;

    use crate::{
        camera, connection, heightmap, material, render, spawn, GzAssetSourcesPlugin, GzConfig,
    };

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
        .init_resource::<material::GzMaterials>()
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
                material::combine_metal_rough,
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

    /// Runs one frame of the plugin with `ui` and `hud`; nothing listens
    /// on the port, so the connection just fails.
    fn plugin_app(ui: UiMode, hud: bool) -> App {
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
                ui,
                hud,
                ..Default::default()
            },
        });
        app.update();
        assert_eq!(
            app.world().resource::<GzConfig>().url,
            format!("ws://127.0.0.1:{port}")
        );
        app
    }

    fn count<C: Component>(app: &mut App) -> usize {
        let world = app.world_mut();
        world.query::<&C>().iter(world).count()
    }

    #[test]
    fn windowed_plugin_adds_controls_hud_and_diagnostics() {
        for ui in [UiMode::Minimal, UiMode::Full] {
            let mut app = plugin_app(ui, true);
            assert_eq!(count::<view::ViewCamera>(&mut app), 1);
            assert_eq!(count::<hud::StatusText>(&mut app), 1);
            assert!(app
                .world()
                .contains_resource::<bevy::diagnostic::DiagnosticsStore>());
            assert_eq!(
                app.world().resource::<view::controls::ActiveScopes>().0,
                view::ViewerControls::scopes(ui)
            );
        }
    }

    #[test]
    fn the_hud_can_be_turned_off() {
        let mut app = plugin_app(UiMode::Minimal, false);
        assert_eq!(count::<view::ViewCamera>(&mut app), 1);
        assert_eq!(count::<hud::StatusText>(&mut app), 0);
    }

    #[test]
    fn headless_plugin_has_no_controls_or_hud() {
        let mut app = plugin_app(UiMode::None, true);
        assert_eq!(count::<view::ViewCamera>(&mut app), 0);
        assert_eq!(count::<hud::StatusText>(&mut app), 0);
        assert_eq!(count::<camera::MainCamera>(&mut app), 1);
        assert!(!app.world().contains_resource::<view::InputMap>());
    }

    #[test]
    fn ui_modes() {
        assert_eq!("full".parse(), Ok(UiMode::Full));
        assert_eq!("Minimal".parse(), Ok(UiMode::Minimal));
        assert_eq!(
            "none".parse::<UiMode>(),
            Err("unknown UI mode 'none' (full or minimal)".into())
        );
        assert_eq!(UiMode::default(), UiMode::Full);
        assert!(UiMode::Full.interactive() && UiMode::Minimal.interactive());
        assert!(!UiMode::None.interactive());
        let config = GzConfig::default();
        assert!(config.interactive() && config.hud);
        assert!(!GzConfig {
            ui: UiMode::None,
            ..config
        }
        .interactive());
        assert!(view::ViewerControls::scopes(UiMode::None).is_empty());
    }
}
