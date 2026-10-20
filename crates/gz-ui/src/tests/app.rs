//! A headless app with the UI stack: no renderer, no winit, a 1440×900
//! window entity and a UI camera.

use bevy::a11y::AccessibilityPlugin;
use bevy::camera::{CameraPlugin, ComputedCameraValues, RenderTargetInfo};
use bevy::input::InputPlugin;
use bevy::input_focus::{InputDispatchPlugin, InputFocusPlugin};
use bevy::picking::{DefaultPickingPlugins, PickingSettings};
use bevy::prelude::*;
use bevy::text::TextPlugin;
use bevy::ui::UiPlugin;
use bevy::ui_widgets::UiWidgetsPlugins;
use bevy::window::{ExitCondition, WindowResolution};

use crate::theme::ThemeMode;
use crate::GzUiPlugin;

/// The app, before its first update.
pub(crate) fn ui_app(theme: ThemeMode) -> App {
    ui_app_at(theme, 1.0)
}

/// The app with a window `scale` physical pixels per logical pixel.
pub(crate) fn ui_app_at(theme: ThemeMode, scale: f32) -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        InputPlugin,
        WindowPlugin {
            primary_window: Some(Window {
                resolution: WindowResolution::new(1440, 900).with_scale_factor_override(scale),
                ..default()
            }),
            exit_condition: ExitCondition::DontExit,
            ..default()
        },
        AccessibilityPlugin,
        TransformPlugin,
        CameraPlugin,
        InputFocusPlugin,
        InputDispatchPlugin,
    ))
    .init_asset::<Image>()
    .init_asset::<TextureAtlasLayout>()
    .init_asset::<Mesh>()
    .init_asset::<bevy::mesh::skinning::SkinnedMeshInverseBindposes>()
    .add_plugins((
        TextPlugin,
        UiPlugin,
        DefaultPickingPlugins,
        UiWidgetsPlugins,
    ))
    // Tests set `Hovered` and trigger pointer events themselves; with hover
    // off, picking doesn't reset them.
    .insert_resource(PickingSettings {
        is_hover_enabled: false,
        ..default()
    })
    .add_plugins(GzUiPlugin { theme });
    // Without a renderer nothing computes the camera's target, so give it
    // one, as bevy_ui's own layout tests do.
    let (w, h) = (1440, 900);
    app.world_mut().spawn((
        Camera2d,
        IsDefaultUiCamera,
        Camera {
            computed: ComputedCameraValues {
                target_info: Some(RenderTargetInfo {
                    physical_size: UVec2::new((w as f32 * scale) as u32, (h as f32 * scale) as u32),
                    scale_factor: scale,
                }),
                ..default()
            },
            ..default()
        },
    ));
    app
}

/// Runs `n` frames.
pub(crate) fn frames(app: &mut App, n: usize) {
    for _ in 0..n {
        app.update();
    }
}

/// Every descendant of `e`.
pub(crate) fn descendants(world: &World, e: Entity) -> Vec<Entity> {
    let mut out = Vec::new();
    let mut stack = vec![e];
    while let Some(next) = stack.pop() {
        if let Some(c) = world.get::<Children>(next) {
            out.extend(c.iter());
            stack.extend(c.iter());
        }
    }
    out
}
