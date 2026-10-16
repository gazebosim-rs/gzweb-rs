//! gzweb: renders a live Gazebo simulation.
//!
//! Native: `gzweb [--url ws://host:9002 | --port N] [--key KEY]
//!               [--snapshot out.json [--settle-ms 1000] [--timeout-ms 60000]]`
//! (`GZ_WEBSOCKET_PORT` is still honoured.) With `--snapshot` it runs
//! headless (no window, no GPU), writes a parity snapshot once the scene has
//! settled, and exits.
//!
//! Browser: `index.html?url=ws://host:9002&key=KEY` (or `?port=N`).

use bevy::prelude::*;
use gz_bevy::{GzAssetSourcesPlugin, GzConfig, GzWebPlugin};

const DEFAULT_PORT: &str = match option_env!("GZ_WEBSOCKET_PORT") {
    Some(p) => p,
    None => "9002",
};

struct Args {
    config: GzConfig,
    snapshot: Option<gz_bevy::snapshot::SnapshotRequest>,
}

#[cfg(not(target_arch = "wasm32"))]
fn parse_args() -> Result<Args, String> {
    let mut port = std::env::var("GZ_WEBSOCKET_PORT")
        .ok()
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| DEFAULT_PORT.to_string());
    let mut url = None;
    let mut key = None;
    let mut snapshot = None;
    let mut settle_ms = 1000.0;
    let mut timeout_ms = 60000.0;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--url" => url = Some(value()?),
            "--port" => port = value()?,
            "--key" => key = Some(value()?),
            "--snapshot" => snapshot = Some(std::path::PathBuf::from(value()?)),
            "--settle-ms" => settle_ms = value()?.parse::<f64>().map_err(|e| e.to_string())?,
            "--timeout-ms" => timeout_ms = value()?.parse::<f64>().map_err(|e| e.to_string())?,
            "-h" | "--help" => {
                println!(
                    "usage: gzweb [--url URL | --port N] [--key KEY] \
                     [--snapshot OUT.json [--settle-ms MS] [--timeout-ms MS]]"
                );
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }

    Ok(Args {
        config: GzConfig {
            url: url.unwrap_or_else(|| format!("ws://localhost:{port}")),
            key,
            headless: snapshot.is_some(),
            ..Default::default()
        },
        snapshot: snapshot.map(|out| gz_bevy::snapshot::SnapshotRequest {
            out,
            settle_secs: settle_ms / 1000.0,
            timeout_secs: timeout_ms / 1000.0,
        }),
    })
}

#[cfg(target_arch = "wasm32")]
fn parse_args() -> Result<Args, String> {
    let params = web_sys::window()
        .and_then(|w| w.location().search().ok())
        .and_then(|s| web_sys::UrlSearchParams::new_with_str(&s).ok());
    let get = |k: &str| params.as_ref().and_then(|p| p.get(k)).filter(|v| !v.is_empty());
    let host = web_sys::window()
        .and_then(|w| w.location().hostname().ok())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "localhost".into());
    let port = get("port").unwrap_or_else(|| DEFAULT_PORT.to_string());
    Ok(Args {
        config: GzConfig {
            url: get("url").unwrap_or_else(|| format!("ws://{host}:{port}")),
            key: get("key"),
            ..Default::default()
        },
        snapshot: None,
    })
}

fn main() -> AppExit {
    #[cfg(target_arch = "wasm32")]
    console_error_panic_hook::set_once();

    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("gzweb: {e}");
            return AppExit::from_code(2);
        }
    };

    let mut app = App::new();
    app.add_plugins(GzAssetSourcesPlugin);

    if args.config.headless {
        use bevy::render::{settings::WgpuSettings, RenderPlugin};
        use bevy::window::ExitCondition;
        app.add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                })
                .set(RenderPlugin {
                    render_creation: WgpuSettings {
                        backends: None,
                        ..default()
                    }
                    .into(),
                    ..default()
                })
                .disable::<bevy::winit::WinitPlugin>(),
        )
        .add_plugins(bevy::app::ScheduleRunnerPlugin::run_loop(
            std::time::Duration::from_secs_f64(1.0 / 120.0),
        ));
    } else {
        app.add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "gzweb".into(),
                        canvas: Some("#bevy-canvas".into()),
                        fit_canvas_to_parent: true,
                        prevent_default_event_handling: false,
                        ..default()
                    }),
                    ..default()
                })
                .set(bevy::log::LogPlugin {
                    filter: "wgpu=warn,naga=warn".into(),
                    ..default()
                }),
        );
    }

    app.add_plugins(GzWebPlugin {
        config: args.config,
    });
    if let Some(req) = args.snapshot {
        app.insert_resource(req)
            .add_systems(Last, gz_bevy::snapshot::snapshot_when_settled);
    }
    app.run()
}
