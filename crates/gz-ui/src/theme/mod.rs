//! Theme tokens and their resolution (spec §4).
//!
//! The token tables (`Palette`, [`ColorRole`], [`Sizes`], [`SPACE`],
//! [`radius`], [`border`], [`motion`], [`elevation`] and the [`TypeStyle`]
//! constants) are generated from `design/ui/tokens.json` by `build.rs`
//! (DECISIONS.md D-21), so they can't drift from the spec (`tokens.rs`).
//! This module holds the [`Theme`] resource and the OS preference; the
//! role components and their resolution are in `roles.rs`, the widgets'
//! state looks ([`Visuals`]) in `visuals/`.
//!
//! Rules for widget code (spec §4.1):
//! - Never write a colour literal; only `tokens.rs` may (and it doesn't
//!   either: the colours are generated). A test greps the crate.
//! - Widgets store roles ([`Fill`], [`Stroke`], [`Ink`], [`Ring`],
//!   [`Shadow`]), never a resolved `Color`. `apply_theme_system` resolves
//!   them when the theme or a role changes.
//! - Sizes come from [`Theme::s`], spacing from [`SPACE`], radii from
//!   [`radius`].

pub(crate) mod roles;
pub(crate) mod tokens;
pub(crate) mod visuals;

use bevy::prelude::*;
use bevy::window::{WindowTheme, WindowThemeChanged};

pub(crate) use roles::{apply_theme_system, focus_ring_system};
pub use roles::{CaretRoles, Fill, FocusRing, Ink, Opacity, Ring, Shadow, Stroke};
pub use tokens::*;
pub use visuals::{
    Active, Content, ForceState, HoverOnFocus, Look, OwnInk, Visuals, WidgetState, DISABLED_OPACITY,
};

/// The theme setting (spec §4.1): follow the OS, or force one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Reflect)]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

impl std::str::FromStr for ThemeMode {
    type Err = String;

    /// `system`, `light` or `dark`.
    fn from_str(s: &str) -> Result<Self, String> {
        match s.to_ascii_lowercase().as_str() {
            "system" => Ok(ThemeMode::System),
            "light" => Ok(ThemeMode::Light),
            "dark" => Ok(ThemeMode::Dark),
            _ => Err(format!("unknown theme '{s}' (system, light or dark)")),
        }
    }
}

/// The resolved theme (spec §4.3). Change it (or [`ThemeMode`] via
/// [`Theme::set_mode`]) and every role re-resolves on the next frame.
#[derive(Resource, Clone, Debug, Reflect)]
pub struct Theme {
    pub mode: ThemeMode,
    /// The mode after applying `System`.
    pub is_dark: bool,
    pub c: Palette,
    pub s: Sizes,
    /// UI scale; 1.0 is the designs as drawn. Synced to Bevy's `UiScale`
    /// (DECISIONS.md D-23).
    pub scale: f32,
    /// The OS prefers dark: what [`ThemeMode::System`] resolves to. Kept
    /// in step with the OS preference (`OsTheme`) by `follow_os_theme`.
    pub os_dark: bool,
}

impl Default for Theme {
    fn default() -> Self {
        Theme::new(ThemeMode::Light, false)
    }
}

impl Theme {
    pub fn new(mode: ThemeMode, os_prefers_dark: bool) -> Self {
        let mut theme = Self {
            mode,
            is_dark: false,
            c: Palette::light(),
            s: Sizes::default(),
            scale: 1.0,
            os_dark: os_prefers_dark,
        };
        theme.set_mode(mode);
        theme
    }

    /// Sets the mode and re-resolves the palette; `System` follows the
    /// OS preference the theme already tracks ([`Theme::os_dark`]).
    pub fn set_mode(&mut self, mode: ThemeMode) {
        self.mode = mode;
        self.is_dark = match mode {
            ThemeMode::Dark => true,
            ThemeMode::Light => false,
            ThemeMode::System => self.os_dark,
        };
        self.c = if self.is_dark {
            Palette::dark()
        } else {
            Palette::light()
        };
    }

    /// Light ↔ dark, leaving System (the gallery's T key, ⌘K "Toggle theme").
    pub fn toggle(&mut self) {
        let next = if self.is_dark {
            ThemeMode::Light
        } else {
            ThemeMode::Dark
        };
        self.set_mode(next);
    }

    /// The colour of `role` in this theme.
    pub fn color(&self, role: ColorRole) -> Color {
        self.c.role(role)
    }
}

/// The OS light/dark preference, for [`ThemeMode::System`] (spec §4.3).
/// Read from the window's theme: winit queries the platform on the
/// desktop and the `prefers-color-scheme` media query in the browser, and
/// reports changes of either.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct OsTheme {
    pub dark: bool,
}

/// Keeps [`OsTheme`] in step with the primary window's theme: its initial
/// value (winit fills `Window::window_theme` at creation) and
/// [`WindowThemeChanged`] messages.
pub(crate) fn read_os_theme(
    windows: Query<&Window, Changed<Window>>,
    mut changes: MessageReader<WindowThemeChanged>,
    mut os: ResMut<OsTheme>,
    mut seen: Local<Option<WindowTheme>>,
) {
    let mut dark = None;
    for window in &windows {
        // Only a new value counts: other systems touch `Window` every frame.
        if window.window_theme.is_some() && window.window_theme != *seen {
            *seen = window.window_theme;
            dark = window.window_theme.map(|t| t == WindowTheme::Dark);
        }
    }
    for change in changes.read() {
        dark = Some(change.theme == WindowTheme::Dark);
    }
    if let Some(dark) = dark {
        os.set_if_neq(OsTheme { dark });
    }
}

/// Hands [`OsTheme`] to the [`Theme`]; in [`ThemeMode::System`], the
/// palette follows it.
pub(crate) fn follow_os_theme(os: Res<OsTheme>, mut theme: ResMut<Theme>) {
    if !os.is_changed() || theme.os_dark == os.dark {
        return;
    }
    if theme.mode == ThemeMode::System {
        theme.os_dark = os.dark;
        theme.set_mode(ThemeMode::System);
    } else {
        // The palette doesn't change: don't re-resolve every role.
        theme.bypass_change_detection().os_dark = os.dark;
    }
}

/// Mirrors [`Theme::scale`] into Bevy's `UiScale`.
pub(crate) fn sync_ui_scale(theme: Res<Theme>, mut scale: ResMut<UiScale>) {
    if theme.is_changed() && scale.0 != theme.scale {
        scale.0 = theme.scale;
    }
}

#[cfg(test)]
mod tests;
