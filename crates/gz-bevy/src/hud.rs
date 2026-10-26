//! On-screen status overlay (bevy_ui) and the DOM status line on wasm.
//! The keys it lists are handled by [`crate::view::controls`].

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;
use bevy::render::renderer::RenderAdapter;
use gz_core::transport::Status;

use crate::connection::GzStatus;
use crate::view::{InputMap, KeyLayout};

#[derive(Component)]
pub struct FpsText;

#[derive(Component)]
pub struct AdapterText;

#[derive(Component)]
pub struct StatusText;

#[derive(Component)]
pub struct SimText;

/// The controls hint.
#[derive(Component)]
pub struct HintText;

/// The mouse part of the controls hint (the mouse isn't in the key map).
pub const MOUSE_HINT: &str =
    "Drag: orbit | Right drag, Space+drag: pan | Wheel, middle drag: zoom (Ctrl/pinch: to cursor)";

/// The key part of the hint: binding ids, and what they do in a word.
/// The keys themselves come from the [`InputMap`].
const KEY_HINTS: &[(&[&str], &str)] = &[
    (&["view.focus"], "focus"),
    (&["view.home"], "home"),
    (&["view.projection"], "ortho"),
    (
        &["view.top", "view.front", "view.right"],
        "views (Shift: opposite)",
    ),
    (&["overlay.grid"], "grid"),
    (
        &["overlay.collisions", "overlay.inertia", "overlay.joints"],
        "collisions, inertia, joints",
    ),
    (&["view.follow"], "follow"),
    (&["view.track"], "track"),
    (
        &[
            "view.fly_forward",
            "view.fly_left",
            "view.fly_back",
            "view.fly_right",
            "view.fly_down",
            "view.fly_up",
        ],
        "move",
    ),
    (&["sim.play_pause"], "play/pause"),
    (&["sim.step"], "step"),
];

/// The controls, as `map` binds them on its platform: each binding's first
/// chord, so the hint follows `shortcuts.json`; the fly keys by what their
/// caps show on `layout`, and no letter a fly key takes there (D-119).
pub fn hint(map: &InputMap, layout: &KeyLayout) -> String {
    let mut parts = vec![MOUSE_HINT.to_string()];
    for (ids, what) in KEY_HINTS {
        let keys: Vec<String> = ids
            .iter()
            .filter_map(|id| map.binding(id))
            .filter_map(|b| {
                let c = map.shown_chords(b).next()?;
                Some(b.chord_label(c, map.platform, layout))
            })
            .collect();
        if !keys.is_empty() {
            parts.push(format!("{} {what}", keys.join(" ")));
        }
    }
    parts.join(" | ")
}

pub fn spawn_hud(
    mut commands: Commands,
    map: Option<Res<InputMap>>,
    layout: Option<Res<KeyLayout>>,
) {
    let layout = layout.as_deref().unwrap_or(&crate::view::layout::US);
    let hint = map.map_or_else(|| hint(&InputMap::builtin(), layout), |m| hint(&m, layout));
    let line = |top: f32| Node {
        position_type: PositionType::Absolute,
        top: Val::Px(top),
        left: Val::Px(10.0),
        ..default()
    };
    let font = TextFont {
        font_size: FontSize::Px(20.0),
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
        Text::new(hint),
        TextFont {
            font_size: FontSize::Px(16.0),
            ..default()
        },
        TextColor(Color::WHITE),
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(10.0),
            left: Val::Px(10.0),
            ..default()
        },
        HintText,
    ));
}

/// The hint again once the layout says what the fly keys' caps show.
pub fn update_hint(
    map: Option<Res<InputMap>>,
    layout: Option<Res<KeyLayout>>,
    mut query: Query<&mut Text, With<HintText>>,
) {
    let changed = map.as_ref().is_some_and(|m| m.is_changed());
    let Some(layout) = layout.filter(|l| changed || l.is_changed()) else {
        return;
    };
    let text = map.map_or_else(
        || hint(&InputMap::builtin(), &layout),
        |m| hint(&m, &layout),
    );
    for mut t in &mut query {
        if t.0 != text {
            t.0.clone_from(&text);
        }
    }
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
            Some(adapter) => {
                let info = adapter.get_info();
                (text.0, color.0) = adapter_style(&format!("{:?}", info.backend), &info.name);
            }
            None => {
                text.0 = "Backend: none".into();
                color.0 = Color::srgb(1.0, 0.0, 0.0);
            }
        }
    }
}

/// HUD label and colour for a render adapter, from its `wgpu::Backend` (as
/// `Debug` prints it; Bevy doesn't re-export the type) and name: blue for
/// WebGL, yellow for the no-op backend, green otherwise.
fn adapter_style(backend: &str, name: &str) -> (String, Color) {
    let color = match backend {
        "Gl" => Color::srgb(0.0, 0.5, 1.0),
        "Noop" => Color::srgb(1.0, 1.0, 0.0),
        _ => Color::srgb(0.0, 1.0, 0.0),
    };
    (format!("Backend: {backend} ({name})"), color)
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

/// The host page's status elements (`index.html`): the WebGPU check and
/// the line [`update_fps`] writes.
pub const DOM_STATUS_IDS: [&str; 2] = ["webgpu-status", "bevy-status"];

/// Removes the host page's status elements, for a page without the HUD
/// (`?hud=0`): nothing would update them.
#[cfg(target_arch = "wasm32")]
pub fn remove_dom_status() {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    for id in DOM_STATUS_IDS {
        if let Some(el) = document.get_element_by_id(id) {
            el.remove();
        }
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
    fn the_hint_reads_its_keys_from_the_map() {
        use crate::view::input::Platform;
        let mut map = InputMap::builtin();
        map.platform = Platform::Mac;
        let us = &crate::view::layout::US;
        let mac = hint(&map, us);
        assert!(mac.starts_with(MOUSE_HINT));
        assert!(
            mac.contains("| F focus | H home | 5 ortho | 7 1 3 views (Shift: opposite) | G grid")
        );
        assert!(mac.contains("| W A S D Q E move | Space play/pause | Cmd+Right step"));
        map.platform = Platform::Linux;
        assert!(hint(&map, us).ends_with("Ctrl+Right step"));
        map.platform = Platform::Web;
        assert!(hint(&map, us).ends_with("Ctrl/Cmd+Right step"));
        // A binding the map lacks drops out.
        map.bindings.retain(|b| b.id != "sim.step");
        assert!(hint(&map, us).ends_with("Space play/pause"));
    }

    /// The fly keys read as their caps on the layout, once it is known.
    #[test]
    fn the_hint_names_the_fly_keys_by_the_layout() {
        let mut world = hud_world();
        world.insert_resource(InputMap::builtin());
        let mut layout = KeyLayout::default();
        for (k, s) in [
            (KeyCode::KeyW, "z"),
            (KeyCode::KeyA, "q"),
            (KeyCode::KeyQ, "a"),
            (KeyCode::KeyF, "f"),
        ] {
            layout.learn(k, s);
        }
        world.insert_resource(layout);
        world.run_system_once(update_hint).unwrap();
        let mut hints = world.query_filtered::<&Text, With<HintText>>();
        let text = hints.single(&world).unwrap().0.clone();
        assert!(text.contains("| F focus |"), "{text}");
        assert!(text.contains("| Z Q S D A E move |"), "{text}");
    }

    /// On Colemak F is fly up's key: the hint doesn't offer F for focus.
    #[test]
    fn the_hint_leaves_out_a_letter_a_fly_key_takes() {
        let mut world = hud_world();
        world.insert_resource(InputMap::builtin());
        let mut layout = KeyLayout::default();
        for (k, s) in [
            (KeyCode::KeyE, "f"),
            (KeyCode::KeyF, "t"),
            (KeyCode::KeyS, "r"),
            (KeyCode::KeyD, "s"),
        ] {
            layout.learn(k, s);
        }
        world.insert_resource(layout);
        world
            .run_system_once(crate::view::layout::mark_unreachable)
            .unwrap();
        world.run_system_once(update_hint).unwrap();
        let mut hints = world.query_filtered::<&Text, With<HintText>>();
        let text = hints.single(&world).unwrap().0.clone();
        assert!(!text.contains("F focus"), "{text}");
        assert!(text.contains("| H home |"), "{text}");
        assert!(text.contains("| W A R S Q F move |"), "{text}");
        assert!(text.contains("| Shift+F follow |"), "{text}");
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
        assert_eq!(
            adapter_style("Vulkan", "gpu"),
            ("Backend: Vulkan (gpu)".into(), Color::srgb(0.0, 1.0, 0.0))
        );
        assert_eq!(adapter_style("Gl", "gpu").1, Color::srgb(0.0, 0.5, 1.0));
        assert_eq!(adapter_style("Noop", "gpu").1, Color::srgb(1.0, 1.0, 0.0));
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
            real_time: 1.25,
            real_time_factor: 0.987,
            iterations: 1234,
            step_size: 0.001,
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
