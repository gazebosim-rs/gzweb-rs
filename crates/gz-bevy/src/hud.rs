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
        Text::new(
            "Left drag: orbit | Right drag: pan | Scroll: zoom | Left click: orbit around point",
        ),
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
            Some(adapter) => {
                let info = adapter.get_info();
                text.0 = format!("Backend: {:?} ({})", info.backend, info.name);
                color.0 = match info.backend {
                    wgpu_types::Backend::Gl => Color::srgb(0.0, 0.5, 1.0),
                    wgpu_types::Backend::Noop => Color::srgb(1.0, 1.0, 0.0),
                    _ => Color::srgb(0.0, 1.0, 0.0),
                };
            }
            None => {
                text.0 = "Backend: none".into();
                color.0 = Color::srgb(1.0, 0.0, 0.0);
            }
        }
    }
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
