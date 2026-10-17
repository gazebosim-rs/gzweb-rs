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

#[derive(Debug)]
struct Args {
    config: GzConfig,
    snapshot: Option<gz_bevy::snapshot::SnapshotRequest>,
}

const USAGE: &str = "usage: gzweb [--url URL | --port N] [--key KEY] \
                     [--snapshot OUT.json [--settle-ms MS] [--timeout-ms MS]]";

/// What the command line asked for.
#[derive(Debug)]
enum Command {
    Run(Args),
    Help,
}

/// Parses native arguments (`args` excludes the program name). `env_port` is
/// `GZ_WEBSOCKET_PORT`, if set.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn parse_args(
    args: impl IntoIterator<Item = String>,
    env_port: Option<String>,
) -> Result<Command, String> {
    let mut port = env_port
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| DEFAULT_PORT.to_string());
    let mut url = None;
    let mut key = None;
    let mut snapshot = None;
    let mut settle_ms = 1000.0;
    let mut timeout_ms = 60000.0;

    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or(format!("{arg} needs a value"));
        let mut millis = || -> Result<f64, String> {
            let v = value()?;
            v.parse::<f64>()
                .map_err(|_| format!("{arg}: '{v}' is not a number"))
        };
        match arg.as_str() {
            "--url" => url = Some(value()?),
            "--port" => port = value()?,
            "--key" => key = Some(value()?),
            "--snapshot" => snapshot = Some(std::path::PathBuf::from(value()?)),
            "--settle-ms" => settle_ms = millis()?,
            "--timeout-ms" => timeout_ms = millis()?,
            "-h" | "--help" => return Ok(Command::Help),
            other => return Err(format!("unknown argument {other}")),
        }
    }

    Ok(Command::Run(Args {
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
    }))
}

#[cfg(not(target_arch = "wasm32"))]
fn command() -> Result<Command, String> {
    parse_args(
        std::env::args().skip(1),
        std::env::var("GZ_WEBSOCKET_PORT").ok(),
    )
}

#[cfg(target_arch = "wasm32")]
fn command() -> Result<Command, String> {
    let params = web_sys::window()
        .and_then(|w| w.location().search().ok())
        .and_then(|s| web_sys::UrlSearchParams::new_with_str(&s).ok());
    let get = |k: &str| {
        params
            .as_ref()
            .and_then(|p| p.get(k))
            .filter(|v| !v.is_empty())
    };
    let host = web_sys::window()
        .and_then(|w| w.location().hostname().ok())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "localhost".into());
    let port = get("port").unwrap_or_else(|| DEFAULT_PORT.to_string());
    Ok(Command::Run(Args {
        config: GzConfig {
            url: get("url").unwrap_or_else(|| format!("ws://{host}:{port}")),
            key: get("key"),
            ..Default::default()
        },
        snapshot: None,
    }))
}

/// Bevy's default plugins, configured for a window (native or the
/// `#bevy-canvas` element on wasm) or for headless snapshot mode: no window,
/// no winit and no GPU backend.
fn default_plugins(headless: bool) -> bevy::app::PluginGroupBuilder {
    if headless {
        use bevy::render::{settings::WgpuSettings, RenderPlugin};
        use bevy::window::ExitCondition;
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
            .disable::<bevy::winit::WinitPlugin>()
    } else {
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
            })
    }
}

fn main() -> AppExit {
    #[cfg(target_arch = "wasm32")]
    console_error_panic_hook::set_once();

    let args = match command() {
        Ok(Command::Run(a)) => a,
        Ok(Command::Help) => {
            println!("{USAGE}");
            return AppExit::Success;
        }
        Err(e) => {
            eprintln!("gzweb: {e}\n{USAGE}");
            return AppExit::from_code(2);
        }
    };

    let mut app = App::new();
    app.add_plugins(GzAssetSourcesPlugin);

    app.add_plugins(default_plugins(args.config.headless));
    if args.config.headless {
        app.add_plugins(bevy::app::ScheduleRunnerPlugin::run_loop(
            std::time::Duration::from_secs_f64(1.0 / 120.0),
        ));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str], env_port: Option<&str>) -> Result<Command, String> {
        parse_args(
            args.iter().map(ToString::to_string),
            env_port.map(str::to_string),
        )
    }

    fn run_args(args: &[&str], env_port: Option<&str>) -> Args {
        parse(args, env_port)
            .ok()
            .and_then(Command::into_run)
            .expect("arguments should parse to Command::Run")
    }

    impl Command {
        fn into_run(self) -> Option<Args> {
            match self {
                Command::Run(a) => Some(a),
                Command::Help => None,
            }
        }
    }

    #[test]
    fn defaults_to_localhost_and_the_default_port() {
        let a = run_args(&[], None);
        assert_eq!(a.config.url, format!("ws://localhost:{DEFAULT_PORT}"));
        assert_eq!(a.config.key, None);
        assert!(!a.config.headless);
        assert!(a.snapshot.is_none());
    }

    #[test]
    fn port_comes_from_the_environment_unless_overridden() {
        assert_eq!(
            run_args(&[], Some("9555")).config.url,
            "ws://localhost:9555"
        );
        assert_eq!(
            run_args(&[], Some("")).config.url,
            format!("ws://localhost:{DEFAULT_PORT}")
        );
        assert_eq!(
            run_args(&["--port", "9600"], Some("9555")).config.url,
            "ws://localhost:9600"
        );
    }

    #[test]
    fn url_wins_over_port() {
        let a = run_args(
            &["--port", "1", "--url", "ws://gz:9002", "--key", "k"],
            None,
        );
        assert_eq!(a.config.url, "ws://gz:9002");
        assert_eq!(a.config.key.as_deref(), Some("k"));
    }

    #[test]
    fn snapshot_mode_is_headless_with_timings_in_seconds() {
        let a = run_args(
            &[
                "--snapshot",
                "out.json",
                "--settle-ms",
                "250",
                "--timeout-ms",
                "5000",
            ],
            None,
        );
        assert!(a.config.headless);
        let s = a.snapshot.expect("snapshot request");
        assert_eq!(s.out, std::path::PathBuf::from("out.json"));
        assert_eq!(s.settle_secs, 0.25);
        assert_eq!(s.timeout_secs, 5.0);
    }

    #[test]
    fn help_and_errors() {
        assert!(matches!(parse(&["--help"], None), Ok(Command::Help)));
        assert!(matches!(parse(&["-h"], None), Ok(Command::Help)));
        assert!(Command::Help.into_run().is_none());
        assert_eq!(
            parse(&["--bogus"], None).unwrap_err(),
            "unknown argument --bogus"
        );
        assert_eq!(parse(&["--url"], None).unwrap_err(), "--url needs a value");
        assert_eq!(
            parse(&["--settle-ms", "soon"], None).unwrap_err(),
            "--settle-ms: 'soon' is not a number"
        );
    }

    #[test]
    fn headless_plugins_drop_winit_and_windowed_keep_it() {
        let headless = default_plugins(true);
        assert!(headless.contains::<bevy::winit::WinitPlugin>());
        assert!(!headless.enabled::<bevy::winit::WinitPlugin>());
        assert!(headless.enabled::<bevy::render::RenderPlugin>());

        let windowed = default_plugins(false);
        assert!(windowed.enabled::<bevy::winit::WinitPlugin>());
        assert!(windowed.enabled::<WindowPlugin>());
    }
}
