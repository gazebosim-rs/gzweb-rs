//! The Gazebo workbench UI (`design/ui/DESIGN.md`) for gzweb-rs, on
//! `bevy_ui` and Bevy's headless `bevy_ui_widgets`.
//!
//! [`GzUiPlugin`] is the full UI mode's plugin (`.agent/UI_PLAN.md` §2).
//! It reads `gz-bevy` state and writes messages; it never reaches into
//! rendering. Milestone M1 has the foundation:
//!
//! - [`theme`]: tokens generated from `tokens.json`, the [`theme::Theme`]
//!   resource, colour roles and their resolution, the OS light/dark
//!   preference;
//! - [`visuals`]: hover, pressed, active and disabled looks for every
//!   clickable widget;
//! - [`icons`]: the icon atlas generated from `icons.json`;
//! - [`focus`]: shortcuts stand down while a text field has focus;
//! - [`cursor`]: hover cursors, held through a drag;
//! - [`fonts`]: Geist and Geist Mono, embedded, and the type styles;
//! - [`widgets`]: the spec §5.1–5.16 components and the splitter;
//! - [`gallery`]: a screen with every widget in every state (`--ui gallery`).
//!
//! Milestone M2 adds [`shell`]: the workbench's regions, their layout
//! rules, the 3D view's placement, and the connect card and banner
//! ([`shell::ShellPlugin`], added with this plugin in the full mode).

pub mod brand;
pub mod cursor;
pub mod focus;
pub mod fonts;
pub mod gallery;
pub mod icons;
pub mod shell;
pub mod theme;
pub mod visuals;
pub mod widgets;

use bevy::input::InputSystems;
use bevy::input_focus::tab_navigation::TabNavigationPlugin;
use bevy::input_focus::InputFocusSystems;
use bevy::prelude::*;
use bevy::ui::UiSystems;

use theme::{OsTheme, Theme, ThemeMode};

/// The UI's systems, in order. Everything a widget does to its own state
/// runs in [`GzUiSystems::Behaviour`]; looks follow in
/// [`GzUiSystems::Visuals`]; colours resolve in PostUpdate before layout.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum GzUiSystems {
    /// Input routing that must see modifiers before widgets get keys.
    Input,
    Behaviour,
    Visuals,
    Resolve,
}

/// Adds the theme, fonts, icons and widget behaviour.
pub struct GzUiPlugin {
    /// The theme setting at startup.
    pub theme: ThemeMode,
}

impl Default for GzUiPlugin {
    fn default() -> Self {
        Self {
            theme: ThemeMode::System,
        }
    }
}

impl Plugin for GzUiPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<TabNavigationPlugin>() {
            app.add_plugins(TabNavigationPlugin);
        }
        app.insert_resource(Theme::new(self.theme, false))
            .init_resource::<OsTheme>()
            .init_resource::<fonts::UiFonts>()
            .init_resource::<icons::IconAtlas>()
            .init_resource::<brand::Logos>()
            .init_resource::<focus::TextFocus>()
            .configure_sets(
                Update,
                (GzUiSystems::Behaviour, GzUiSystems::Visuals).chain(),
            )
            .configure_sets(
                PreUpdate,
                GzUiSystems::Input
                    .after(InputSystems)
                    .before(InputFocusSystems::Dispatch),
            )
            .configure_sets(PostUpdate, GzUiSystems::Resolve.before(UiSystems::Prepare))
            .init_resource::<focus::MenuTakesEscape>()
            .init_resource::<focus::PointerLed>()
            .add_systems(
                PreUpdate,
                (
                    focus::text_focus_system.after(InputFocusSystems::Dispatch),
                    (focus::note_input, focus::menu_escape).in_set(GzUiSystems::Input),
                ),
            )
            .add_systems(
                PostUpdate,
                focus::release_focus
                    .after(bevy::ui_widgets::MenuFocusSystem)
                    .before(InputFocusSystems::FocusChangeEvents),
            )
            .add_systems(
                Update,
                (theme::read_os_theme, theme::follow_os_theme)
                    .chain()
                    .before(GzUiSystems::Behaviour),
            )
            .add_systems(
                Update,
                (
                    visuals::button_visuals_system,
                    theme::focus_ring_system,
                    fonts::apply_type_system,
                    icons::update_icons,
                    brand::logo_system,
                )
                    .in_set(GzUiSystems::Visuals),
            )
            .add_systems(
                PostUpdate,
                (theme::sync_ui_scale, theme::apply_theme_system).in_set(GzUiSystems::Resolve),
            );
        cursor::build(app);
        widgets::build(app);
    }
}

#[cfg(test)]
mod tests;
