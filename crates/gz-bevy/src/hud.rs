//! On-screen status overlay (bevy_ui), plus the DOM status line on wasm.

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;
use bevy::render::renderer::RenderAdapter;
use gz_core::transport::Status;

use crate::connection::GzStatus;

#[derive(Component)]
pub struct FpsText;

#[derive(Component)]
pub struct AdapterText;

#[derive(Component)]
pub struct StatusText;

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
        font,
        TextColor(Color::WHITE),
        line(70.0),
        StatusText,
    ));
    commands.spawn((
        Text::new("Left drag: orbit point | Right drag: pan | Scroll/middle drag: zoom to point"),
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
    for (mut text, mut c) in &mut query {
        text.0 = format!("WS: {label}");
        c.0 = color;
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
    fn spawns_three_status_lines_and_the_controls_hint() {
        let mut world = hud_world();
        assert_eq!(world.query::<&Text>().iter(&world).count(), 4);
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
                url: "ws://h".into(),
                world: Some("w".into()),
                malformed: 0,
            });
            world.run_system_once(update_status).unwrap();
            assert_eq!(text_of::<StatusText>(&mut world).0, want);
        }
        world.resource_mut::<GzStatus>().world = None;
        world.resource_mut::<GzStatus>().status = Status::Ready;
        world.run_system_once(update_status).unwrap();
        assert_eq!(text_of::<StatusText>(&mut world).0, "WS: Ready: ?");
    }

    #[test]
    fn status_line_is_left_alone_when_nothing_changed() {
        let mut app = App::new();
        app.insert_resource(GzStatus {
            status: Status::Connected,
            url: String::new(),
            world: None,
            malformed: 0,
        })
        .add_systems(Startup, spawn_hud)
        .add_systems(Update, update_status);
        app.update();
        let world = app.world_mut();
        let mut q = world.query_filtered::<&mut Text, With<StatusText>>();
        q.single_mut(world).unwrap().0 = "sentinel".into();
        app.update();
        assert_eq!(text_of::<StatusText>(app.world_mut()).0, "sentinel");
    }
}
