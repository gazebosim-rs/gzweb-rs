//! On-screen status overlay (bevy_ui), the DOM status line on wasm, and the
//! simulation-control keys.

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;
use bevy::render::renderer::RenderAdapter;
use gz_core::transport::Status;

use crate::connection::{GzCommand, GzStatus};

#[derive(Component)]
pub struct FpsText;

#[derive(Component)]
pub struct AdapterText;

#[derive(Component)]
pub struct StatusText;

#[derive(Component)]
pub struct SimText;

const HINT: &str = "Left drag: orbit point | Right drag: pan | Scroll/middle drag: zoom to point \
                        | Space: play/pause | N: step";

pub fn spawn_hud(mut commands: Commands) {
    let line = |top: f32| Node {
        position_type: PositionType::Absolute,
        top: Val::Px(top),
        left: Val::Px(10.0),
        ..default()
    };
    let font = TextFont {
        font_size: 20.0,
        ..default()
    };
    commands.spawn((
        Text::new("FPS: N/A"),
        font.clone(),
        TextColor(Color::WHITE),
        line(10.0),
        FpsText,
    ));
    commands.spawn((
        Text::new("Backend: ..."),
        font.clone(),
        TextColor(Color::WHITE),
        line(40.0),
        AdapterText,
    ));
    commands.spawn((
        Text::new("WS: ..."),
        font.clone(),
        TextColor(Color::WHITE),
        line(70.0),
        StatusText,
    ));
    commands.spawn((
        Text::new("Sim: -"),
        font,
        TextColor(Color::WHITE),
        line(100.0),
        SimText,
    ));
    commands.spawn((
        Text::new(HINT),
        TextFont {
            font_size: 16.0,
            ..default()
        },
        TextColor(Color::WHITE),
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(10.0),
            left: Val::Px(10.0),
            ..default()
        },
    ));
}

pub fn update_fps(diagnostics: Res<DiagnosticsStore>, mut query: Query<&mut Text, With<FpsText>>) {
    let Some(fps) = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed())
    else {
        return;
    };
    for mut text in &mut query {
        text.0 = format!("FPS: {fps:.1}");
    }
    #[cfg(target_arch = "wasm32")]
    set_dom_status(&format!("Running (FPS: {fps:.1})"));
}

pub fn update_adapter(
    adapter: Option<Res<RenderAdapter>>,
    mut query: Query<(&mut Text, &mut TextColor), With<AdapterText>>,
) {
    for (mut text, mut color) in &mut query {
        match &adapter {
            Some(adapter) => (text.0, color.0) = adapter_style(&adapter.get_info()),
            None => {
                text.0 = "Backend: none".into();
                color.0 = Color::srgb(1.0, 0.0, 0.0);
            }
        }
    }
}

/// HUD label and colour for a render adapter: blue for WebGL, yellow for
/// the no-op backend, green otherwise.
fn adapter_style(info: &wgpu_types::AdapterInfo) -> (String, Color) {
    let color = match info.backend {
        wgpu_types::Backend::Gl => Color::srgb(0.0, 0.5, 1.0),
        wgpu_types::Backend::Noop => Color::srgb(1.0, 1.0, 0.0),
        _ => Color::srgb(0.0, 1.0, 0.0),
    };
    (
        format!("Backend: {:?} ({})", info.backend, info.name),
        color,
    )
}

pub fn update_status(
    status: Res<GzStatus>,
    mut query: Query<(&mut Text, &mut TextColor), With<StatusText>>,
) {
    if !status.is_changed() {
        return;
    }
    let (label, color) = match &status.status {
        Status::Disconnected => (
            format!("Disconnected ({})", status.url),
            Color::srgb(1.0, 1.0, 0.0),
        ),
        Status::Connecting => (
            format!("Connecting to {}", status.url),
            Color::srgb(1.0, 1.0, 0.0),
        ),
        Status::Connected => ("Connected".to_string(), Color::srgb(0.5, 1.0, 0.5)),
        Status::Ready => (
            format!("Ready: {}", status.world.as_deref().unwrap_or("?")),
            Color::srgb(0.0, 1.0, 0.0),
        ),
        Status::Error(e) => (format!("Error: {e}"), Color::srgb(1.0, 0.0, 0.0)),
    };
    let retry = match status.reconnect_at {
        Some(_) => format!(" - retrying (attempt {})", status.attempts + 1),
        None => String::new(),
    };
    for (mut text, mut c) in &mut query {
        text.0 = format!("WS: {label}{retry}");
        c.0 = color;
    }
}

pub fn update_sim(status: Res<GzStatus>, mut query: Query<&mut Text, With<SimText>>) {
    if !status.is_changed() {
        return;
    }
    let label = match &status.sim {
        Some(s) => format!(
            "Sim: {} | t = {:.2} s | RTF {:.2} | iterations {}",
            if s.paused { "paused" } else { "running" },
            s.sim_time,
            s.real_time_factor,
            s.iterations
        ),
        None => "Sim: -".into(),
    };
    for mut text in &mut query {
        text.0.clone_from(&label);
    }
}

/// Space toggles play/pause; N steps a paused simulation. Needs the stats
/// to know which way to toggle.
pub fn sim_keys(
    keys: Res<ButtonInput<KeyCode>>,
    status: Res<GzStatus>,
    mut out: MessageWriter<GzCommand>,
) {
    let Some(sim) = status.sim else { return };
    if keys.just_pressed(KeyCode::Space) {
        out.write(if sim.paused {
            GzCommand::Play
        } else {
            GzCommand::Pause
        });
    }
    if keys.just_pressed(KeyCode::KeyN) && sim.paused {
        out.write(GzCommand::Step(1));
    }
}

/// Writes to the `#bevy-status` element in `index.html`.
#[cfg(target_arch = "wasm32")]
pub fn set_dom_status(msg: &str) {
    if let Some(el) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("bevy-status"))
    {
        el.set_inner_html(msg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::SimStats;
    use bevy::diagnostic::{Diagnostic, DiagnosticMeasurement};
    use bevy::ecs::system::RunSystemOnce;

    fn text_of<M: Component>(world: &mut World) -> (String, Color) {
        let mut q = world.query_filtered::<(&Text, &TextColor), With<M>>();
        let (t, c) = q.single(world).unwrap();
        (t.0.clone(), c.0)
    }

    fn hud_world() -> World {
        let mut world = World::new();
        world.run_system_once(spawn_hud).unwrap();
        world
    }

    #[test]
    fn spawns_four_status_lines_and_the_controls_hint() {
        let mut world = hud_world();
        assert_eq!(world.query::<&Text>().iter(&world).count(), 5);
        assert_eq!(text_of::<FpsText>(&mut world).0, "FPS: N/A");
    }

    #[test]
    fn fps_shows_the_smoothed_frame_rate_once_measured() {
        let mut world = hud_world();
        world.insert_resource(DiagnosticsStore::default());
        world.run_system_once(update_fps).unwrap();
        assert_eq!(text_of::<FpsText>(&mut world).0, "FPS: N/A");

        let mut store = world.resource_mut::<DiagnosticsStore>();
        store.add(Diagnostic::new(FrameTimeDiagnosticsPlugin::FPS));
        store
            .get_mut(&FrameTimeDiagnosticsPlugin::FPS)
            .unwrap()
            .add_measurement(DiagnosticMeasurement {
                time: bevy::platform::time::Instant::now(),
                value: 60.0,
            });
        world.run_system_once(update_fps).unwrap();
        assert_eq!(text_of::<FpsText>(&mut world).0, "FPS: 60.0");
    }

    #[test]
    fn adapter_line_without_a_renderer() {
        let mut world = hud_world();
        world.run_system_once(update_adapter).unwrap();
        assert_eq!(
            text_of::<AdapterText>(&mut world),
            ("Backend: none".into(), Color::srgb(1.0, 0.0, 0.0))
        );
    }

    #[test]
    fn adapter_colour_follows_the_backend() {
        let info = |backend| wgpu_types::AdapterInfo {
            name: "gpu".into(),
            vendor: 0,
            device: 0,
            device_type: wgpu_types::DeviceType::Other,
            driver: String::new(),
            driver_info: String::new(),
            backend,
        };
        assert_eq!(
            adapter_style(&info(wgpu_types::Backend::Vulkan)),
            ("Backend: Vulkan (gpu)".into(), Color::srgb(0.0, 1.0, 0.0))
        );
        assert_eq!(
            adapter_style(&info(wgpu_types::Backend::Gl)).1,
            Color::srgb(0.0, 0.5, 1.0)
        );
        assert_eq!(
            adapter_style(&info(wgpu_types::Backend::Noop)).1,
            Color::srgb(1.0, 1.0, 0.0)
        );
    }

    #[test]
    fn status_line_follows_the_connection() {
        let mut world = hud_world();
        let cases = [
            (Status::Disconnected, "WS: Disconnected (ws://h)"),
            (Status::Connecting, "WS: Connecting to ws://h"),
            (Status::Connected, "WS: Connected"),
            (Status::Ready, "WS: Ready: w"),
            (Status::Error("boom".into()), "WS: Error: boom"),
        ];
        for (status, want) in cases {
            world.insert_resource(GzStatus {
                status,
                world: Some("w".into()),
                ..GzStatus::new("ws://h")
            });
            world.run_system_once(update_status).unwrap();
            assert_eq!(text_of::<StatusText>(&mut world).0, want);
        }
        world.resource_mut::<GzStatus>().world = None;
        world.resource_mut::<GzStatus>().status = Status::Ready;
        world.run_system_once(update_status).unwrap();
        assert_eq!(text_of::<StatusText>(&mut world).0, "WS: Ready: ?");

        let mut status = world.resource_mut::<GzStatus>();
        status.status = Status::Error("refused".into());
        status.reconnect_at = Some(1.0);
        status.attempts = 2;
        world.run_system_once(update_status).unwrap();
        assert_eq!(
            text_of::<StatusText>(&mut world).0,
            "WS: Error: refused - retrying (attempt 3)"
        );
    }

    fn sim(paused: bool) -> SimStats {
        SimStats {
            paused,
            sim_time: 1.234,
            real_time_factor: 0.987,
            iterations: 1234,
        }
    }

    #[test]
    fn sim_line_follows_the_stats() {
        let mut world = hud_world();
        world.insert_resource(GzStatus::new(""));
        world.run_system_once(update_sim).unwrap();
        assert_eq!(text_of::<SimText>(&mut world).0, "Sim: -");
        world.resource_mut::<GzStatus>().sim = Some(sim(true));
        world.run_system_once(update_sim).unwrap();
        assert_eq!(
            text_of::<SimText>(&mut world).0,
            "Sim: paused | t = 1.23 s | RTF 0.99 | iterations 1234"
        );
        world.resource_mut::<GzStatus>().sim = Some(sim(false));
        world.run_system_once(update_sim).unwrap();
        assert!(text_of::<SimText>(&mut world).0.starts_with("Sim: running"));
    }

    #[test]
    fn keys_send_sim_commands() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .insert_resource(GzStatus::new(""))
            .add_message::<GzCommand>()
            .add_systems(Update, (sim_keys, update_sim));
        let press = |app: &mut App, keys: &[KeyCode], sim: Option<SimStats>| {
            app.world_mut().resource_mut::<GzStatus>().sim = sim;
            let mut input = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            input.reset_all();
            for k in keys {
                input.press(*k);
            }
            app.update();
            let msgs = app.world().resource::<Messages<GzCommand>>();
            msgs.iter_current_update_messages()
                .cloned()
                .collect::<Vec<_>>()
        };
        let both = [KeyCode::Space, KeyCode::KeyN];
        assert!(press(&mut app, &both, None).is_empty());
        assert_eq!(press(&mut app, &both, Some(sim(false))), [GzCommand::Pause]);
        assert_eq!(
            press(&mut app, &both, Some(sim(true))),
            [GzCommand::Play, GzCommand::Step(1)]
        );
    }

    #[test]
    fn status_line_is_left_alone_when_nothing_changed() {
        let mut app = App::new();
        app.insert_resource(GzStatus {
            status: Status::Connected,
            ..GzStatus::new("")
        })
        .add_systems(Startup, spawn_hud)
        .add_systems(Update, (update_status, update_sim));
        app.update();
        let world = app.world_mut();
        let mut q = world.query_filtered::<&mut Text, With<StatusText>>();
        q.single_mut(world).unwrap().0 = "sentinel".into();
        let mut q = world.query_filtered::<&mut Text, With<SimText>>();
        q.single_mut(world).unwrap().0 = "sentinel".into();
        app.update();
        assert_eq!(text_of::<StatusText>(app.world_mut()).0, "sentinel");
        assert_eq!(text_of::<SimText>(app.world_mut()).0, "sentinel");
    }
}
