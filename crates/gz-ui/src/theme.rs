//! Theme tokens and their resolution (spec §4).
//!
//! The token tables (`Palette`, [`ColorRole`], [`Sizes`], [`SPACE`],
//! [`radius`], [`border`], [`motion`], [`elevation`] and the [`TypeStyle`]
//! constants) are generated from `design/ui/tokens.json` by `build.rs`
//! (DECISIONS.md D-21), so they can't drift from the spec. This file holds
//! the hand-written parts: the [`Theme`] resource, the role components and
//! the systems that resolve them.
//!
//! Rules for widget code (spec §4.1):
//! - Never write a colour literal; this is the only file allowed to (and
//!   it doesn't either: the colours are generated). A test greps the crate.
//! - Widgets store roles ([`Fill`], [`Stroke`], [`Ink`], [`Ring`],
//!   [`Shadow`]), never a resolved `Color`. `apply_theme_system` resolves
//!   them when the theme or a role changes.
//! - Sizes come from [`Theme::s`], spacing from [`SPACE`], radii from
//!   [`radius`].

use bevy::ecs::query::QueryData;
use bevy::input_focus::{InputFocus, InputFocusVisible};
use bevy::prelude::*;
use bevy::text::TextCursorStyle;
use bevy::window::{WindowTheme, WindowThemeChanged};

include!(concat!(env!("OUT_DIR"), "/tokens.rs"));

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

/// Which of the two embedded families a [`TypeStyle`] uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Reflect)]
pub enum FontFamily {
    /// Geist.
    Ui,
    /// Geist Mono.
    Mono,
}

/// A `tokens.json` `type` entry. The constants (`TypeStyle::LABEL`, …) are
/// generated; widgets that need an off-scale style (bordered buttons use
/// Geist 12/500) build one with [`TypeStyle::new`].
#[derive(Clone, Copy, Debug, PartialEq, Reflect)]
pub struct TypeStyle {
    pub font: FontFamily,
    /// Logical pixels.
    pub size: f32,
    /// 400, 500 or 600.
    pub weight: u16,
    /// Render the text in capitals.
    pub uppercase: bool,
    /// Letter spacing, in ems.
    pub tracking: f32,
}

impl TypeStyle {
    /// A plain style: no capitals, no tracking.
    pub const fn new(font: FontFamily, size: f32, weight: u16) -> Self {
        Self {
            font,
            size,
            weight,
            uppercase: false,
            tracking: 0.0,
        }
    }

    /// Letter spacing in logical pixels.
    pub fn tracking_px(&self) -> f32 {
        self.tracking * self.size
    }
}

/// A `tokens.json` `elevation` entry: a drop shadow `y` below, blurred by
/// `blur`, in the `color` token (`None`: no shadow).
#[derive(Clone, Copy, Debug, PartialEq, Reflect)]
pub struct Elevation {
    pub blur: f32,
    pub y: f32,
    pub color: Option<ColorRole>,
}

impl Elevation {
    /// The Bevy shadow for this elevation (colour set by [`Shadow`]).
    pub fn box_shadow(self) -> BoxShadow {
        BoxShadow::new(Color::NONE, px(0), px(self.y), px(0), px(self.blur))
    }

    /// The shadow and its colour role.
    pub fn shadow(self) -> (BoxShadow, Shadow) {
        (
            self.box_shadow(),
            Shadow(self.color.unwrap_or(ColorRole::Clear)),
        )
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
    /// in step with [`OsTheme`] by `follow_os_theme`.
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
pub struct OsTheme {
    pub dark: bool,
}

/// Background colour role → `BackgroundColor`.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(BackgroundColor)]
pub struct Fill(pub ColorRole);

/// Border colour role → `BorderColor` (all sides).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(BorderColor)]
pub struct Stroke(pub ColorRole);

/// Content colour role → `TextColor` on text, the tint on icons (`ImageNode`).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ink(pub ColorRole);

/// Outline colour role → `Outline` (focus rings, input halos).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ring(pub ColorRole);

/// Shadow colour role → every `BoxShadow` layer.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shadow(pub ColorRole);

/// Multiplies the alpha of every colour resolved on this entity (disabled
/// content at 40%, hidden tree rows at 50%).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Opacity(pub f32);

/// Selection and caret colours for a text input, from its [`Ink`].
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[require(TextCursorStyle)]
pub struct CaretRoles;

type RoleRefs = (
    Option<Ref<'static, Fill>>,
    Option<Ref<'static, Stroke>>,
    Option<Ref<'static, Ink>>,
    Option<Ref<'static, Ring>>,
    Option<Ref<'static, Shadow>>,
    Option<Ref<'static, Opacity>>,
    Has<CaretRoles>,
);

type Targets = (
    Option<&'static mut BackgroundColor>,
    Option<&'static mut BorderColor>,
    Option<&'static mut TextColor>,
    Option<&'static mut ImageNode>,
    Option<&'static mut Outline>,
    Option<&'static mut BoxShadow>,
    Option<&'static mut TextCursorStyle>,
);

type RoleChanged = Or<(
    Changed<Fill>,
    Changed<Stroke>,
    Changed<Ink>,
    Changed<Ring>,
    Changed<Shadow>,
    Changed<Opacity>,
)>;

type WithRole = Or<(
    With<Fill>,
    With<Stroke>,
    With<Ink>,
    With<Ring>,
    With<Shadow>,
)>;

/// Re-resolves role components: all of them when the [`Theme`] changes,
/// otherwise only entities whose role (or [`Opacity`]) changed, found by
/// change detection rather than a scan.
#[allow(clippy::type_complexity)]
pub(crate) fn apply_theme_system(
    theme: Res<Theme>,
    mut q: ParamSet<(
        Query<(RoleRefs, Targets), WithRole>,
        Query<(RoleRefs, Targets), (WithRole, RoleChanged)>,
    )>,
) {
    if theme.is_changed() {
        for (roles, targets) in &mut q.p0() {
            resolve_roles(&theme, roles, targets);
        }
    } else {
        for (roles, targets) in &mut q.p1() {
            resolve_roles(&theme, roles, targets);
        }
    }
}

/// Writes one entity's colours from its roles.
fn resolve_roles(
    theme: &Theme,
    (fill, stroke, ink, ring, shadow, opacity, caret): <RoleRefs as QueryData>::Item<'_, '_>,
    targets: <Targets as QueryData>::Item<'_, '_>,
) {
    let alpha = opacity.as_deref().map_or(1.0, |o| o.0);
    let resolve = |role: ColorRole| {
        let c = theme.color(role);
        c.with_alpha(c.alpha() * alpha)
    };
    let (bg, border, text, image, outline, box_shadow, cursor) = targets;
    if let (Some(fill), Some(mut bg)) = (fill, bg) {
        bg.set_if_neq(BackgroundColor(resolve(fill.0)));
    }
    if let (Some(stroke), Some(mut border)) = (stroke, border) {
        border.set_if_neq(BorderColor::all(resolve(stroke.0)));
    }
    if let Some(ink) = ink {
        let c = resolve(ink.0);
        if let Some(mut text) = text {
            text.set_if_neq(TextColor(c));
        }
        if let Some(mut image) = image {
            if image.color != c {
                image.color = c;
            }
        }
        if let (true, Some(mut cursor)) = (caret, cursor) {
            cursor.color = c;
            cursor.selection_color = theme.color(ColorRole::AccentSoft);
            cursor.unfocused_selection_color = theme.color(ColorRole::Hover);
        }
    }
    if let (Some(ring), Some(mut outline)) = (ring, outline) {
        let c = resolve(ring.0);
        if outline.color != c {
            outline.color = c;
        }
    }
    if let (Some(shadow), Some(mut box_shadow)) = (shadow, box_shadow) {
        let c = resolve(shadow.0);
        for layer in &mut box_shadow.0 {
            layer.color = c;
        }
    }
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

/// A widget that shows the focus-visible ring (spec §5: 2px `accent`,
/// offset 2) while it has keyboard focus.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct FocusRing;

/// Draws or clears the [`FocusRing`]s.
pub(crate) fn focus_ring_system(
    focus: Res<InputFocus>,
    visible: Res<InputFocusVisible>,
    mut q: Query<(Entity, Option<&mut Ring>), With<FocusRing>>,
    mut commands: Commands,
) {
    if !focus.is_changed() && !visible.is_changed() {
        return;
    }
    for (entity, ring) in &mut q {
        let on = visible.0 && focus.get() == Some(entity);
        let role = if on {
            ColorRole::Accent
        } else {
            ColorRole::Clear
        };
        match ring {
            Some(mut ring) => {
                ring.set_if_neq(Ring(role));
            }
            None => {
                commands.entity(entity).insert((
                    Ring(role),
                    Outline::new(px(border::FOCUS_RING), px(2), Color::NONE),
                ));
            }
        }
    }
}
