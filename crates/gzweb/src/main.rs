//! gzweb: renders a live Gazebo simulation.
//!
//! Native: see [`USAGE`] (`GZ_WEBSOCKET_PORT` is still honoured).
//! - `--snapshot out.json` runs with no window or GPU, writes a parity
//!   snapshot once the scene has settled, and exits.
//! - `--screenshot out.png` opens a fixed 1280x720 window, saves a frame once
//!   the scene has settled, and exits (needs a GPU).
//! - `--quality low|high|ultra` picks the rendering level (`render::Quality`).
//! - `--ui full|minimal` picks the UI mode (`gz_bevy::UiMode`, default
//!   full); `--snapshot` runs without UI, and so does `--screenshot` unless
//!   `--ui` is given (the full UI is then captured at 1440x900).
//!   `--no-hud` hides minimal mode's status text. `--ui gallery` shows the
//!   widget gallery instead of a world (no connection); `--screenshot`
//!   captures it at 1440x900.
//! - `--window WxH` sizes the window (and a capture).
//! - `--capture-at-ms MS` takes the screenshot at that time even if the
//!   scene hasn't settled (the connect card, the disconnected banner).
//! - `--theme system|light|dark` picks the UI theme (default system).
//! - `--view top|bottom|front|back|right|left` starts in that axis view
//!   (orthographic) once the world loads, and `--overlays grid,frames,…`
//!   with those overlays on (`gz_bevy::overlays::Overlay` names), so a
//!   capture can show them (DECISIONS.md D-64). Not with `--snapshot`.
//! - `--scene-filter TEXT` and `--scene-expand` start the full UI's Scene
//!   tree filtered or with every row open (DECISIONS.md D-70).
//!
//! Browser: `index.html?url=ws://host:9002&key=KEY&quality=low` (or `?port=N`),
//! plus `ui=full|minimal|gallery` (default minimal), `theme=` and `hud=0`.

use bevy::prelude::*;
use bevy::window::WindowResizeConstraints;
use gz_bevy::{GzAssetSourcesPlugin, GzConfig, GzWebPlugin, UiMode, DEFAULT_PORT};
use gz_ui::theme::ThemeMode;

#[derive(Debug)]
struct Args {
    config: GzConfig,
    mode: Mode,
    snapshot: Option<gz_bevy::snapshot::SnapshotRequest>,
    /// Fetch HTTPS assets through a `gz-parity mirror`.
    http_mirror: Option<String>,
    /// `--ui gallery`: the widget gallery instead of a world.
    gallery: bool,
    theme: ThemeMode,
    /// `--window WxH`.
    window: Option<(u32, u32)>,
    /// `--view`, `--overlays`.
    start: Option<gz_bevy::view::StartView>,
    /// `--scene-filter`, `--scene-expand`.
    scene: Option<gz_ui::panels::SceneTreeStart>,
}

const USAGE: &str = "usage: gzweb [--url URL | --port N] [--key KEY] [--ca-cert PEM] \
                     [--reconnect | --no-reconnect] [--quality low|high|ultra] [--http-mirror URL] \
                     [--ui full|minimal|gallery] [--theme system|light|dark] [--no-hud] \
                     [--window WxH] [--snapshot OUT.json] [--screenshot OUT.png] [--settle-ms MS] \
                     [--timeout-ms MS] [--capture-at-ms MS] [--view top|front|right|…] \
                     [--overlays grid,collisions,inertia,center_of_mass,joints,frames,wireframe] \
                     [--scene-filter TEXT] [--scene-expand]\n\
                     --ui: full (default) is the workbench, minimal the 3D view with camera and \
                     sim keys only, gallery the widget gallery (no server; --screenshot works); \
                     --snapshot runs with no UI, and so does --screenshot unless --ui is given.\n\
                     Reconnecting is on by default, except in --snapshot/--screenshot mode.";

/// The browser's UI mode from `?ui=`: minimal unless it says otherwise
/// (DECISIONS.md D-10).
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn web_ui(value: Option<&str>) -> Result<UiMode, String> {
    value.map_or(Ok(UiMode::Minimal), str::parse)
}

/// The browser's server (spec §7.10): `?url=`, else `?port=` on the page's
/// host, else, in the full UI only, the last server the page connected to
/// (`last`; only the full UI's connect card can change the server, so
/// minimal mode never inherits one it can't change back), else the page's
/// host on the default port. A page served over https gets `wss://`
/// (`secure`): browsers block `ws://` from a secure page.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn web_url(
    url: Option<String>,
    port: Option<String>,
    last: Option<String>,
    host: &str,
    secure: bool,
    ui: UiMode,
) -> String {
    let scheme = if secure { "wss" } else { "ws" };
    url.or_else(|| port.map(|p| format!("{scheme}://{host}:{p}")))
        .or_else(|| last.filter(|_| ui == UiMode::Full))
        .unwrap_or_else(|| format!("{scheme}://{host}:{DEFAULT_PORT}"))
}

/// `--ui gallery` / `?ui=gallery`.
fn is_gallery(value: Option<&str>) -> bool {
    value.is_some_and(|v| v.eq_ignore_ascii_case("gallery"))
}

/// `?hud=0` (or `false`, `off`, `no`) hides the status text.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn web_hud(value: Option<&str>) -> bool {
    !matches!(
        value.map(str::to_ascii_lowercase).as_deref(),
        Some("0" | "false" | "off" | "no")
    )
}

/// `--window WxH`.
fn parse_window(v: &str) -> Result<(u32, u32), String> {
    let bad = || format!("--window: '{v}' is not WIDTHxHEIGHT");
    let (w, h) = v.split_once(['x', 'X']).ok_or_else(bad)?;
    let w: u32 = w.trim().parse().map_err(|_| bad())?;
    let h: u32 = h.trim().parse().map_err(|_| bad())?;
    if w == 0 || h == 0 {
        return Err(bad());
    }
    Ok((w, h))
}

/// The full UI's window: 1440x900 (the boards' size), at least 1024x640
/// (spec §2.1).
const FULL_WINDOW: (u32, u32) = (1440, 900);

/// What kind of window the app runs with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// A resizable window (or the page's canvas) with controls and HUD.
    Interactive,
    /// A fixed window at scale 1 for `--screenshot`: 1280x720 like the
    /// gzweb parity harness with no UI, 1440x900 with the full UI.
    Capture,
    /// No window or GPU (`--snapshot`).
    Headless,
}

/// What the command line asked for.
#[derive(Debug)]
enum Command {
    Run(Box<Args>),
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
    let mut ui_given = false;
    let mut window = None;
    let mut capture_at_ms = None;
    let mut gallery = false;
    let mut theme = ThemeMode::System;
    let mut hud = true;
    let mut settle_ms = 1000.0;
    let mut start = gz_bevy::view::StartView::default();
    let mut scene = gz_ui::panels::SceneTreeStart::default();
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
            "--ui" => {
                let v = value()?;
                gallery = is_gallery(Some(&v));
                ui_given = true;
                if !gallery {
                    ui = v.parse().map_err(|_| {
                        format!("--ui: unknown UI mode '{v}' (full, minimal or gallery)")
                    })?;
                }
            }
            "--theme" => theme = value()?.parse().map_err(|e| format!("--theme: {e}"))?,
            "--no-hud" => hud = false,
            "--window" => window = Some(parse_window(&value()?)?),
            "--capture-at-ms" => capture_at_ms = Some(millis()?),
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
            "--view" => start.view = Some(value()?.parse().map_err(|e| format!("--view: {e}"))?),
            "--overlays" => {
                for name in value()?.split(',').filter(|n| !n.trim().is_empty()) {
                    start
                        .overlays
                        .push(name.parse().map_err(|e| format!("--overlays: {e}"))?);
                }
            }
            "--scene-filter" => scene.filter = value()?,
            "--scene-expand" => scene.expand_all = true,
            "--settle-ms" => settle_ms = millis()?,
            "--timeout-ms" => timeout_ms = millis()?,
            "-h" | "--help" => return Ok(Command::Help),
            other => return Err(format!("unknown argument {other}")),
        }
    }

    if gallery && snapshot.is_some() {
        return Err("--snapshot doesn't apply to --ui gallery (use --screenshot)".into());
    }
    let start = (start != gz_bevy::view::StartView::default()).then_some(start);
    let scene = (scene != gz_ui::panels::SceneTreeStart::default()).then_some(scene);
    let capture = snapshot.is_some() || screenshot.is_some();
    // A screenshot keeps a UI mode asked for by name; a snapshot has no
    // window to show one in.
    let no_ui = snapshot.is_some() || (screenshot.is_some() && !ui_given);
    if start.is_some() && (no_ui || gallery) {
        return Err(
            "--view and --overlays need a view: --ui full or minimal (not --snapshot, a \
             --screenshot without --ui, or --ui gallery)"
                .into(),
        );
    }
    if scene.is_some() && (no_ui || gallery || ui != UiMode::Full) {
        return Err("--scene-filter and --scene-expand need --ui full".into());
    }
    Ok(Command::Run(Box::new(Args {
        config: GzConfig {
            url: url.unwrap_or_else(|| format!("ws://localhost:{port}")),
            key,
            ui: if no_ui { UiMode::None } else { ui },
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
            capture_at_secs: capture_at_ms.map(|ms| ms / 1000.0),
        }),
        http_mirror,
        gallery,
        theme,
        window,
        start,
        scene,
    })))
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
    let ui = web_ui(
        get("ui")
            .filter(|v| !is_gallery(Some(v.as_str())))
            .as_deref(),
    )
    .unwrap_or_else(|e| {
        bevy::log::warn!("?ui: {e}");
        UiMode::Minimal
    });
    Ok(Command::Run(Box::new(Args {
        config: GzConfig {
            url: web_url(
                get("url"),
                get("port"),
                gz_bevy::view::web::last_url(),
                &host,
                gz_bevy::view::web::page_is_secure(),
                ui,
            ),
            key: get("key"),
            quality: get("quality")
                .and_then(|q| {
                    q.parse()
                        .map_err(|e| bevy::log::warn!("?quality={q}: {e}"))
                        .ok()
                })
                .unwrap_or_default(),
            ui,
            hud: web_hud(get("hud").as_deref()),
            ..Default::default()
        },
        mode: Mode::Interactive,
        snapshot: None,
        http_mirror: get("http_mirror"),
        gallery: is_gallery(get("ui").as_deref()),
        theme: get("theme")
            .and_then(|t| t.parse().map_err(|e| bevy::log::warn!("?theme: {e}")).ok())
            .unwrap_or_default(),
        window: None,
        start: None,
        scene: None,
    })))
}

/// The log filter. `bevy_ecs::system::system` logs one thing: "System '…'
/// has not run for N ticks", once the world's change tick has moved about
/// 3.2 billion past a system's last run. In a viewer left running for
/// hours that is every system that ran only at start-up (ours:
/// `init_layout`, `spawn_shell`; Bevy's own) and Bevy's systems behind run
/// conditions that stay false (hover maps without a pointer, asset events
/// for unused types, dialogs); none of them misses a change it needs.
const LOG_FILTER: &str = "wgpu=warn,naga=warn,bevy_ecs::system::system=error";

/// Bevy's default plugins for `mode`: an interactive window (native, or the
/// `#bevy-canvas` element on wasm), a fixed capture window, or headless
/// snapshot mode (no window, no winit, no GPU backend). `size` is the
/// window's size (the capture's: 1280x720 without one); `full` is the full
/// UI, which has a minimum size.
fn default_plugins(
    mode: Mode,
    size: Option<(u32, u32)>,
    full: bool,
) -> bevy::app::PluginGroupBuilder {
    let window = |w: Window| {
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(w),
                ..default()
            })
            .set(bevy::log::LogPlugin {
                filter: LOG_FILTER.into(),
                // Our warnings and errors for the status bar's message chip.
                custom_layer: gz_bevy::messages::log_layer,
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
        Mode::Capture => {
            let (w, h) = size.unwrap_or((1280, 720));
            window(Window {
                title: "gzweb (capture)".into(),
                resolution: bevy::window::WindowResolution::new(w, h)
                    .with_scale_factor_override(1.0),
                resizable: false,
                ..default()
            })
        }
        Mode::Interactive => window(interactive_window(size, full)),
    }
}

/// The interactive window: `size`, else 1440x900 for the full UI (which
/// has a minimum size on native), else Bevy's default; in the browser, the
/// page's `#bevy-canvas`.
fn interactive_window(size: Option<(u32, u32)>, full: bool) -> Window {
    Window {
        title: "gzweb".into(),
        resolution: size
            .or(full.then_some(FULL_WINDOW))
            .map_or_else(default, |(w, h)| bevy::window::WindowResolution::new(w, h)),
        // Native only: in the browser winit turns this into the
        // canvas's CSS min-width/min-height, and a smaller window would
        // overflow and scroll instead of reflowing (the floor rule
        // handles narrow windows).
        resize_constraints: if full && cfg!(not(target_arch = "wasm32")) {
            let (w, h) = gz_ui::shell::MIN_WINDOW;
            WindowResizeConstraints {
                min_width: w,
                min_height: h,
                ..default()
            }
        } else {
            default()
        },
        canvas: Some("#bevy-canvas".into()),
        fit_canvas_to_parent: true,
        // Off: winit would cancel every key and wheel event on the
        // canvas. `gz_bevy::view::web` cancels only what the spec lists
        // (context menu, Ctrl+wheel and pinch, the canvas chords).
        prevent_default_event_handling: false,
        ..default()
    }
}

/// The gallery's window: 1440x900 at scale 1 for `--screenshot`, else a
/// normal window (or the page's canvas).
fn gallery_plugins(mode: Mode) -> bevy::app::PluginGroupBuilder {
    let (w, h) = gz_ui::gallery::SIZE;
    let window = match mode {
        Mode::Capture => Window {
            title: "gzweb gallery (capture)".into(),
            resolution: bevy::window::WindowResolution::new(w, h).with_scale_factor_override(1.0),
            resizable: false,
            ..default()
        },
        _ => Window {
            title: "gzweb gallery".into(),
            resolution: bevy::window::WindowResolution::new(w, h),
            canvas: Some("#bevy-canvas".into()),
            fit_canvas_to_parent: true,
            prevent_default_event_handling: false,
            ..default()
        },
    };
    default_plugins(mode, None, false).set(WindowPlugin {
        primary_window: Some(window),
        ..default()
    })
}

/// `--ui gallery`: the widget gallery, with no world or connection.
fn run_gallery(args: Args) -> AppExit {
    let mut app = App::new();
    app.add_plugins(gallery_plugins(args.mode)).add_plugins((
        gz_ui::GzUiPlugin { theme: args.theme },
        gz_ui::gallery::GalleryPlugin,
    ));
    // The canvas key listener, as in the app: Tab stays in the gallery and
    // Space doesn't scroll the page.
    #[cfg(target_arch = "wasm32")]
    gz_bevy::view::web::install(&mut app);
    if let Some(path) = args.snapshot.and_then(|s| s.screenshot) {
        app.insert_resource(gz_ui::gallery::GalleryCapture::new(path));
    }
    app.run()
}

fn main() -> AppExit {
    #[cfg(target_arch = "wasm32")]
    console_error_panic_hook::set_once();

    let args = match command() {
        Ok(Command::Run(a)) => *a,
        Ok(Command::Help) => {
            println!("{USAGE}");
            return AppExit::Success;
        }
        Err(e) => {
            eprintln!("gzweb: {e}\n{USAGE}");
            return AppExit::from_code(2);
        }
    };

    if args.gallery {
        return run_gallery(args);
    }

    let mut app = App::new();
    app.add_plugins(GzAssetSourcesPlugin {
        http_mirror: args.http_mirror.clone(),
    });

    let full = args.config.ui == UiMode::Full;
    let size = args
        .window
        .or((full && args.mode == Mode::Capture).then_some(FULL_WINDOW));
    app.add_plugins(default_plugins(args.mode, size, full));
    if args.mode == Mode::Headless {
        app.add_plugins(bevy::app::ScheduleRunnerPlugin::run_loop(
            std::time::Duration::from_secs_f64(1.0 / 120.0),
        ));
    }

    app.add_plugins(GzWebPlugin {
        config: args.config,
    });
    if let Some(start) = args.start {
        app.insert_resource(start);
    }
    if full {
        app.add_plugins((
            gz_ui::GzUiPlugin { theme: args.theme },
            gz_ui::shell::ShellPlugins,
        ));
        if let Some(scene) = args.scene {
            app.insert_resource(scene);
        }
    }
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
                Command::Run(a) => Some(*a),
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
        // A snapshot forces no UI; a screenshot keeps one asked for.
        let a = run_args(&["--ui", "minimal", "--snapshot", "o.json"], None);
        assert_eq!(a.config.ui, UiMode::None);
        let a = run_args(&["--ui", "full", "--screenshot", "o.png"], None);
        assert_eq!(a.config.ui, UiMode::Full);
        assert_eq!(a.mode, Mode::Capture);
        let a = run_args(&["--screenshot", "o.png"], None);
        assert_eq!(a.config.ui, UiMode::None);
        assert_eq!(
            parse(&["--ui", "none"], None).unwrap_err(),
            "--ui: unknown UI mode 'none' (full, minimal or gallery)"
        );
        assert_eq!(parse(&["--ui"], None).unwrap_err(), "--ui needs a value");
        assert!(USAGE.contains("[--ui full|minimal|gallery] [--theme system|light|dark]"));
    }

    #[test]
    fn window_and_capture_time_flags() {
        let a = run_args(
            &[
                "--window",
                "1280x800",
                "--screenshot",
                "o.png",
                "--capture-at-ms",
                "2500",
            ],
            None,
        );
        assert_eq!(a.window, Some((1280, 800)));
        assert_eq!(a.snapshot.unwrap().capture_at_secs, Some(2.5));
        assert_eq!(run_args(&[], None).window, None);
        assert_eq!(parse_window("1024X640"), Ok((1024, 640)));
        for bad in ["1024", "0x5", "ax5", "5xb"] {
            assert_eq!(
                parse_window(bad).unwrap_err(),
                format!("--window: '{bad}' is not WIDTHxHEIGHT")
            );
        }
        let windowed = default_plugins(Mode::Interactive, None, true);
        assert!(windowed.enabled::<WindowPlugin>());
        let shot = default_plugins(Mode::Capture, Some((1024, 640)), true);
        assert!(shot.enabled::<WindowPlugin>());
    }

    #[test]
    fn view_and_overlay_flags() {
        use gz_bevy::overlays::Overlay;
        use gz_bevy::view::SnapView;
        let a = run_args(
            &[
                "--ui",
                "full",
                "--screenshot",
                "o.png",
                "--view",
                "top",
                "--overlays",
                "frames,com, wireframe,",
            ],
            None,
        );
        let start = a.start.unwrap();
        assert_eq!(start.view, Some(SnapView::Top));
        assert_eq!(
            start.overlays,
            [Overlay::Frames, Overlay::CenterOfMass, Overlay::Wireframe]
        );
        assert!(run_args(&[], None).start.is_none());
        assert!(run_args(&["--overlays", ""], None).start.is_none());
        assert!(parse(&["--view", "iso"], None)
            .unwrap_err()
            .starts_with("--view: unknown view 'iso'"));
        assert!(parse(&["--overlays", "lidar"], None)
            .unwrap_err()
            .starts_with("--overlays: unknown overlay 'lidar'"));
        for no_view in [
            &["--snapshot", "o.json", "--view", "top"][..],
            &["--screenshot", "o.png", "--view", "top"],
            &["--ui", "gallery", "--overlays", "grid"],
        ] {
            assert!(parse(no_view, None)
                .unwrap_err()
                .starts_with("--view and --overlays need a view"));
        }
        assert!(USAGE.contains("--view") && USAGE.contains("--overlays"));
        let a = run_args(
            &["--scene-filter", "cam", "--scene-expand", "--ui", "full"],
            None,
        );
        assert_eq!(
            a.scene,
            Some(gz_ui::panels::SceneTreeStart {
                filter: "cam".into(),
                expand_all: true,
            })
        );
        assert!(run_args(&[], None).scene.is_none());
        assert_eq!(
            parse(&["--scene-expand", "--ui", "minimal"], None).unwrap_err(),
            "--scene-filter and --scene-expand need --ui full"
        );
    }

    #[test]
    fn gallery_and_theme_flags() {
        let a = run_args(&["--ui", "gallery", "--theme", "dark"], None);
        assert!(a.gallery);
        assert_eq!(a.theme, ThemeMode::Dark);
        assert_eq!(a.mode, Mode::Interactive);
        let shot = run_args(&["--ui", "Gallery", "--screenshot", "g.png"], None);
        assert!(shot.gallery);
        assert_eq!(shot.mode, Mode::Capture);
        let d = run_args(&[], None);
        assert!(!d.gallery);
        assert_eq!(d.theme, ThemeMode::System);
        assert_eq!(
            parse(&["--ui", "gallery", "--snapshot", "o.json"], None).unwrap_err(),
            "--snapshot doesn't apply to --ui gallery (use --screenshot)"
        );
        assert_eq!(
            parse(&["--theme", "blue"], None).unwrap_err(),
            "--theme: unknown theme 'blue' (system, light or dark)"
        );
        assert!(is_gallery(Some("GALLERY")) && !is_gallery(Some("full")) && !is_gallery(None));
        for mode in [Mode::Interactive, Mode::Capture] {
            let plugins = gallery_plugins(mode);
            assert!(plugins.enabled::<WindowPlugin>());
        }
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

    /// `?url=` wins, then `?port=`, then (full UI only) the remembered
    /// server, then the page's host; an https page uses `wss://`.
    #[test]
    fn web_url_precedence() {
        let s = |v: &str| Some(v.to_string());
        let last = || s("ws://remembered:9010");
        let url = |u, p, l, secure, ui| web_url(u, p, l, "example.org", secure, ui);
        let full = UiMode::Full;
        let minimal = UiMode::Minimal;
        assert_eq!(
            url(s("ws://given:1"), s("9003"), last(), false, full),
            "ws://given:1"
        );
        assert_eq!(
            url(None, s("9003"), last(), false, full),
            "ws://example.org:9003"
        );
        assert_eq!(url(None, None, last(), false, full), "ws://remembered:9010");
        // Minimal mode can't change the server, so it never inherits one.
        assert_eq!(
            url(None, None, last(), false, minimal),
            format!("ws://example.org:{DEFAULT_PORT}")
        );
        assert_eq!(
            url(None, s("9003"), None, true, minimal),
            "wss://example.org:9003"
        );
        assert_eq!(
            url(None, None, None, true, full),
            format!("wss://example.org:{DEFAULT_PORT}")
        );
    }

    /// The full UI's minimum window size is a native window's: in the
    /// browser it would become the canvas's CSS minimum.
    #[test]
    fn the_minimum_window_size_is_native_only() {
        let c = interactive_window(None, true).resize_constraints;
        let (w, h) = gz_ui::shell::MIN_WINDOW;
        assert_eq!((c.min_width, c.min_height), (w, h));
        let default = WindowResizeConstraints::default();
        let c = interactive_window(None, false).resize_constraints;
        assert_eq!(
            (c.min_width, c.min_height),
            (default.min_width, default.min_height)
        );
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
        let headless = default_plugins(Mode::Headless, None, false);
        assert!(headless.contains::<bevy::winit::WinitPlugin>());
        assert!(!headless.enabled::<bevy::winit::WinitPlugin>());
        assert!(headless.enabled::<bevy::render::RenderPlugin>());

        for mode in [Mode::Interactive, Mode::Capture] {
            let windowed = default_plugins(mode, None, false);
            assert!(windowed.enabled::<bevy::winit::WinitPlugin>());
            assert!(windowed.enabled::<WindowPlugin>());
        }
    }
}
