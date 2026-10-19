//! gzweb: renders a live Gazebo simulation.
//!
//! Native: see [`USAGE`] (`GZ_WEBSOCKET_PORT` is still honoured).
//! - `--snapshot out.json` runs with no window or GPU, writes a parity
//!   snapshot once the scene has settled, and exits.
//! - `--screenshot out.png` opens a fixed 1280x720 window, saves a frame once
//!   the scene has settled, and exits (needs a GPU).
//! - `--quality low|high|ultra` picks the rendering level (`render::Quality`).
//! - `--ui full|minimal` picks the UI mode (`gz_bevy::UiMode`, default
//!   full); the capture modes run without UI. `--no-hud` hides the status
//!   text.
//!
//! Browser: `index.html?url=ws://host:9002&key=KEY&quality=low` (or `?port=N`),
//! plus `ui=full|minimal` (default minimal) and `hud=0`.

use bevy::prelude::*;
use gz_bevy::{GzAssetSourcesPlugin, GzConfig, GzWebPlugin, UiMode};

const DEFAULT_PORT: &str = match option_env!("GZ_WEBSOCKET_PORT") {
    Some(p) => p,
    None => "9002",
};

#[derive(Debug)]
struct Args {
    config: GzConfig,
    mode: Mode,
    snapshot: Option<gz_bevy::snapshot::SnapshotRequest>,
    /// Fetch HTTPS assets through a `gz-parity mirror`.
    http_mirror: Option<String>,
}

const USAGE: &str = "usage: gzweb [--url URL | --port N] [--key KEY] [--ca-cert PEM] \
                     [--reconnect | --no-reconnect] [--quality low|high|ultra] [--http-mirror URL] \
                     [--ui full|minimal] [--no-hud] \
                     [--snapshot OUT.json] [--screenshot OUT.png] [--settle-ms MS] [--timeout-ms MS]\n\
                     --ui: full (default) is the workbench, minimal the 3D view with camera and \
                     sim keys only; --snapshot/--screenshot run with no UI.\n\
                     Reconnecting is on by default, except in --snapshot/--screenshot mode.";

/// The browser's UI mode from `?ui=`: minimal unless it says otherwise
/// (DECISIONS.md D-10).
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn web_ui(value: Option<&str>) -> Result<UiMode, String> {
    value.map_or(Ok(UiMode::Minimal), str::parse)
}

/// `?hud=0` (or `false`, `off`, `no`) hides the status text.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn web_hud(value: Option<&str>) -> bool {
    !matches!(
        value.map(str::to_ascii_lowercase).as_deref(),
        Some("0" | "false" | "off" | "no")
    )
}

/// What kind of window the app runs with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// A resizable window (or the page's canvas) with controls and HUD.
    Interactive,
    /// A fixed 1280x720 window at scale 1, like the gzweb parity harness,
    /// with no HUD, for `--screenshot`.
    Capture,
    /// No window or GPU (`--snapshot`).
    Headless,
}

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
    let mut screenshot = None;
    let mut http_mirror = None;
    let mut ca_cert = None;
    let mut reconnect = None;
    let mut quality = gz_bevy::render::Quality::default();
    let mut ui = UiMode::Full;
    let mut hud = true;
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
            "--http-mirror" => http_mirror = Some(value()?),
            "--ca-cert" => ca_cert = Some(std::path::PathBuf::from(value()?)),
            "--reconnect" => reconnect = Some(true),
            "--no-reconnect" => reconnect = Some(false),
            "--quality" => quality = value()?.parse()?,
            "--ui" => ui = value()?.parse().map_err(|e| format!("--ui: {e}"))?,
            "--no-hud" => hud = false,
            "--snapshot" => snapshot = Some(std::path::PathBuf::from(value()?)),
            "--screenshot" => {
                let path = std::path::PathBuf::from(value()?);
                if !path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("png"))
                {
                    return Err(format!("--screenshot: '{}' is not a .png", path.display()));
                }
                screenshot = Some(path);
            }
            "--settle-ms" => settle_ms = millis()?,
            "--timeout-ms" => timeout_ms = millis()?,
            "-h" | "--help" => return Ok(Command::Help),
            other => return Err(format!("unknown argument {other}")),
        }
    }

    let capture = snapshot.is_some() || screenshot.is_some();
    Ok(Command::Run(Args {
        config: GzConfig {
            url: url.unwrap_or_else(|| format!("ws://localhost:{port}")),
            key,
            ui: if capture { UiMode::None } else { ui },
            hud,
            gpu: screenshot.is_some() || snapshot.is_none(),
            reconnect: reconnect.unwrap_or(!capture),
            ca_cert,
            quality,
            ..Default::default()
        },
        mode: if screenshot.is_some() {
            Mode::Capture
        } else if snapshot.is_some() {
            Mode::Headless
        } else {
            Mode::Interactive
        },
        snapshot: capture.then(|| gz_bevy::snapshot::SnapshotRequest {
            out: snapshot,
            screenshot,
            settle_secs: settle_ms / 1000.0,
            timeout_secs: timeout_ms / 1000.0,
        }),
        http_mirror,
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
            quality: get("quality")
                .and_then(|q| {
                    q.parse()
                        .map_err(|e| bevy::log::warn!("?quality={q}: {e}"))
                        .ok()
                })
                .unwrap_or_default(),
            ui: web_ui(get("ui").as_deref()).unwrap_or_else(|e| {
                bevy::log::warn!("?ui: {e}");
                UiMode::Minimal
            }),
            hud: web_hud(get("hud").as_deref()),
            ..Default::default()
        },
        mode: Mode::Interactive,
        snapshot: None,
        http_mirror: get("http_mirror"),
    }))
}

/// Bevy's default plugins for `mode`: an interactive window (native, or the
/// `#bevy-canvas` element on wasm), a fixed capture window, or headless
/// snapshot mode (no window, no winit, no GPU backend).
fn default_plugins(mode: Mode) -> bevy::app::PluginGroupBuilder {
    let window = |w: Window| {
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(w),
                ..default()
            })
            .set(bevy::log::LogPlugin {
                filter: "wgpu=warn,naga=warn".into(),
                ..default()
            })
    };
    match mode {
        Mode::Headless => {
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
        }
        Mode::Capture => window(Window {
            title: "gzweb (capture)".into(),
            resolution: bevy::window::WindowResolution::new(1280, 720)
                .with_scale_factor_override(1.0),
            resizable: false,
            ..default()
        }),
        Mode::Interactive => window(Window {
            title: "gzweb".into(),
            canvas: Some("#bevy-canvas".into()),
            fit_canvas_to_parent: true,
            // Off: winit would cancel every key and wheel event on the
            // canvas. `gz_bevy::view::web` cancels only what the spec lists
            // (context menu, Ctrl+wheel and pinch, the canvas chords).
            prevent_default_event_handling: false,
            ..default()
        }),
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
    app.add_plugins(GzAssetSourcesPlugin {
        http_mirror: args.http_mirror.clone(),
    });

    app.add_plugins(default_plugins(args.mode));
    if args.mode == Mode::Headless {
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
        assert_eq!(a.config.ui, UiMode::Full);
        assert!(a.config.interactive() && a.config.hud);
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
        assert_eq!(a.http_mirror, None);
        assert!(a.config.reconnect);
        assert_eq!(a.config.ca_cert, None);
        let tls = run_args(&["--ca-cert", "ca.pem", "--no-reconnect"], None);
        assert_eq!(
            tls.config.ca_cert.as_deref(),
            Some(std::path::Path::new("ca.pem"))
        );
        assert!(!tls.config.reconnect);
        assert_eq!(a.config.quality, gz_bevy::render::Quality::High);
        let low = run_args(&["--quality", "low"], None);
        assert_eq!(low.config.quality, gz_bevy::render::Quality::Low);
        assert!(parse(&["--quality", "max"], None).is_err());
        let m = run_args(&["--http-mirror", "http://127.0.0.1:9101"], None);
        assert_eq!(m.http_mirror.as_deref(), Some("http://127.0.0.1:9101"));
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
        assert!(!a.config.interactive() && !a.config.gpu);
        assert_eq!(a.config.ui, UiMode::None);
        assert_eq!(a.mode, Mode::Headless);
        assert!(!a.config.reconnect);
        let shot = run_args(&["--screenshot", "o.png"], None);
        assert_eq!(shot.mode, Mode::Capture);
        assert!(!shot.config.interactive() && shot.config.gpu && !shot.config.reconnect);
        let req = shot.snapshot.unwrap();
        assert_eq!(req.out, None);
        assert_eq!(
            req.screenshot.as_deref(),
            Some(std::path::Path::new("o.png"))
        );
        assert_eq!(run_args(&[], None).mode, Mode::Interactive);
        let forced = run_args(&["--snapshot", "o.json", "--reconnect"], None);
        assert!(forced.config.reconnect);
        let s = a.snapshot.expect("snapshot request");
        assert_eq!(s.out, Some(std::path::PathBuf::from("out.json")));
        assert_eq!(s.settle_secs, 0.25);
        assert_eq!(s.timeout_secs, 5.0);
    }

    #[test]
    fn ui_mode_flag() {
        assert_eq!(
            run_args(&["--ui", "minimal"], None).config.ui,
            UiMode::Minimal
        );
        assert_eq!(run_args(&["--ui", "full"], None).config.ui, UiMode::Full);
        assert!(!run_args(&["--no-hud"], None).config.hud);
        // The capture modes force no UI.
        for capture in [["--snapshot", "o.json"], ["--screenshot", "o.png"]] {
            let a = run_args(&[&["--ui", "minimal"], &capture[..]].concat(), None);
            assert_eq!(a.config.ui, UiMode::None);
        }
        assert_eq!(
            parse(&["--ui", "none"], None).unwrap_err(),
            "--ui: unknown UI mode 'none' (full or minimal)"
        );
        assert_eq!(parse(&["--ui"], None).unwrap_err(), "--ui needs a value");
        assert!(USAGE.contains("[--ui full|minimal] [--no-hud]"));
    }

    #[test]
    fn web_query_defaults_to_minimal() {
        assert_eq!(web_ui(None), Ok(UiMode::Minimal));
        assert_eq!(web_ui(Some("full")), Ok(UiMode::Full));
        assert_eq!(web_ui(Some("minimal")), Ok(UiMode::Minimal));
        assert!(web_ui(Some("kiosk")).is_err());
        assert!(web_hud(None) && web_hud(Some("1")));
        for off in ["0", "false", "OFF", "no"] {
            assert!(!web_hud(Some(off)), "{off}");
        }
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
        assert_eq!(
            parse(&["--screenshot", "shot.jpg"], None).unwrap_err(),
            "--screenshot: 'shot.jpg' is not a .png"
        );
        assert!(parse(&["--screenshot", "SHOT.PNG"], None).is_ok());
    }

    #[test]
    fn headless_plugins_drop_winit_and_windowed_keep_it() {
        let headless = default_plugins(Mode::Headless);
        assert!(headless.contains::<bevy::winit::WinitPlugin>());
        assert!(!headless.enabled::<bevy::winit::WinitPlugin>());
        assert!(headless.enabled::<bevy::render::RenderPlugin>());

        for mode in [Mode::Interactive, Mode::Capture] {
            let windowed = default_plugins(mode);
            assert!(windowed.enabled::<bevy::winit::WinitPlugin>());
            assert!(windowed.enabled::<WindowPlugin>());
        }
    }
}
