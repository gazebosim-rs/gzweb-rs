//! Theme tokens for the Gazebo Bevy UI.
//!
//! Mirrors `tokens.json` (the source of truth). If the two disagree, tokens.json wins;
//! a later step can replace this hand-written mirror with a `build.rs` generator.
//!
//! Rules for widget code:
//!   * Never write a color literal in a widget. Read `Res<Theme>` and use `theme.c.<token>`.
//!   * Never write a size literal for anything listed in `Sizes`. Use `theme.s.<token>`.
//!   * Widgets store a semantic `ColorRole`, not a resolved `Color`, so a theme switch
//!     only has to re-resolve roles (see `apply_theme_system` in DESIGN.md §4.3).
//!
//! Written against Bevy's `Color::srgba_u8`; check the Bevy version pinned in Cargo.toml
//! and adjust imports if the API moved.

use bevy::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Reflect)]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

/// Every semantic color the UI uses. Field names match tokens.json keys.
#[derive(Clone, Debug, Reflect)]
pub struct Palette {
    pub bg: Color,
    pub surface: Color,
    pub field: Color,
    pub hover: Color,
    pub pressed: Color,
    pub line: Color,
    pub line_soft: Color,
    pub text: Color,
    pub text_2: Color,
    pub text_3: Color,
    pub accent: Color,
    pub accent_text: Color,
    pub accent_soft: Color,
    pub accent_ring: Color,
    pub primary: Color,
    pub on_primary: Color,
    pub warn: Color,
    pub warn_soft: Color,
    pub warn_text: Color,
    pub danger: Color,
    pub danger_soft: Color,
    pub ok: Color,
    pub info: Color,
    pub hud: Color,
    pub tooltip: Color,
    pub tooltip_text: Color,
    pub tooltip_text_2: Color,
    pub toggle_track_off: Color,
    pub toggle_knob_off: Color,
    pub toggle_knob_on: Color,
    pub shadow: Color,
    pub axis_x: Color,
    pub axis_y: Color,
    pub axis_z: Color,
    pub viewport_sky: Color,
    pub viewport_ground: Color,
    pub viewport_grid: Color,
    pub selection_outline: Color,
    pub entity_world: Color,
    pub entity_model: Color,
    pub entity_light: Color,
    pub entity_link: Color,
    pub entity_joint: Color,
    pub entity_sensor: Color,
    pub entity_badge_bg: Color,
    // v0.2 additions
    pub icon: Color,
    pub disabled: Color,
    pub surface_2: Color,
    pub warn_badge: Color,
    pub axis_x_text: Color,
    pub axis_y_text: Color,
    pub axis_z_text: Color,
    pub sensor_soft: Color,
    pub chart_1: Color,
    pub chart_2: Color,
    pub chart_3: Color,
    pub chart_4: Color,
    pub cube_top: Color,
    pub cube_right: Color,
    pub cube_left: Color,
    pub cube_edge: Color,
    pub cube_label: Color,
    pub cube_snapped: Color,
}

const fn rgb(r: u8, g: u8, b: u8) -> (u8, u8, u8, u8) {
    (r, g, b, 255)
}
fn c(v: (u8, u8, u8, u8)) -> Color {
    Color::srgba_u8(v.0, v.1, v.2, v.3)
}

impl Palette {
    pub fn light() -> Self {
        Self {
            bg: c(rgb(0xF4, 0xF5, 0xF7)),
            surface: c(rgb(0xFF, 0xFF, 0xFF)),
            field: c(rgb(0xF9, 0xFA, 0xFB)),
            hover: c(rgb(0xEE, 0xF0, 0xF3)),
            pressed: c(rgb(0xE4, 0xE8, 0xEE)),
            line: c(rgb(0xE2, 0xE5, 0xEA)),
            line_soft: c(rgb(0xEE, 0xF0, 0xF3)),
            text: c(rgb(0x12, 0x15, 0x1A)),
            text_2: c(rgb(0x4D, 0x55, 0x62)),
            text_3: c(rgb(0x64, 0x6C, 0x79)),
            accent: c(rgb(0x1C, 0x64, 0xF2)),
            accent_text: c(rgb(0x18, 0x58, 0xD6)),
            accent_soft: c(rgb(0xE8, 0xF0, 0xFE)),
            accent_ring: c(rgb(0x8A, 0xB0, 0xF7)),
            primary: c(rgb(0x12, 0x15, 0x1A)),
            on_primary: c(rgb(0xFF, 0xFF, 0xFF)),
            warn: c(rgb(0xD9, 0x77, 0x06)),
            warn_soft: c(rgb(0xFF, 0xF4, 0xE5)),
            warn_text: c(rgb(0x8A, 0x4B, 0x00)),
            danger: c(rgb(0xB4, 0x23, 0x18)),
            danger_soft: c(rgb(0xFE, 0xE4, 0xE2)),
            ok: c(rgb(0x12, 0x7A, 0x43)),
            info: c(rgb(0x1C, 0x64, 0xF2)),
            hud: c((0xFF, 0xFF, 0xFF, 0xF0)),
            tooltip: c(rgb(0x12, 0x15, 0x1A)),
            tooltip_text: c(rgb(0xFF, 0xFF, 0xFF)),
            tooltip_text_2: c(rgb(0xC3, 0xCA, 0xD3)),
            toggle_track_off: c(rgb(0xD5, 0xDA, 0xE1)),
            toggle_knob_off: c(rgb(0xFF, 0xFF, 0xFF)),
            toggle_knob_on: c(rgb(0xFF, 0xFF, 0xFF)),
            shadow: c((0x12, 0x15, 0x1A, 0x1F)),
            axis_x: c(rgb(0xE5, 0x48, 0x4D)),
            axis_y: c(rgb(0x2F, 0x9E, 0x44)),
            axis_z: c(rgb(0x2E, 0x7C, 0xF6)),
            viewport_sky: c(rgb(0xEC, 0xEF, 0xF3)),
            viewport_ground: c(rgb(0xE1, 0xE5, 0xEB)),
            viewport_grid: c(rgb(0xCB, 0xD2, 0xDB)),
            selection_outline: c(rgb(0x1C, 0x64, 0xF2)),
            entity_world: c(rgb(0x4D, 0x55, 0x62)),
            entity_model: c(rgb(0x1C, 0x64, 0xF2)),
            entity_light: c(rgb(0x8A, 0x61, 0x00)),
            entity_link: c(rgb(0x4D, 0x55, 0x62)),
            entity_joint: c(rgb(0x7A, 0x3F, 0xC2)),
            entity_sensor: c(rgb(0x0E, 0x74, 0x90)),
            entity_badge_bg: c(rgb(0xF1, 0xF3, 0xF6)),
            icon: c(rgb(0x2B, 0x31, 0x3A)),
            disabled: c(rgb(0x9A, 0xA3, 0xAF)),
            surface_2: c(rgb(0xFF, 0xFF, 0xFF)),
            warn_badge: c(rgb(0xB4, 0x53, 0x09)),
            axis_x_text: c(rgb(0xC8, 0x33, 0x3A)),
            axis_y_text: c(rgb(0x1F, 0x7A, 0x32)),
            axis_z_text: c(rgb(0x1F, 0x5F, 0xD1)),
            sensor_soft: c(rgb(0xE0, 0xF2, 0xF7)),
            chart_1: c(rgb(0x1C, 0x64, 0xF2)),
            chart_2: c(rgb(0x7A, 0x3F, 0xC2)),
            chart_3: c(rgb(0x0E, 0x74, 0x90)),
            chart_4: c(rgb(0xB4, 0x53, 0x09)),
            cube_top: c(rgb(0xFF, 0xFF, 0xFF)),
            cube_right: c(rgb(0xF1, 0xF3, 0xF6)),
            cube_left: c(rgb(0xE4, 0xE8, 0xEE)),
            cube_edge: c(rgb(0xB9, 0xC1, 0xCC)),
            cube_label: c(rgb(0x4D, 0x55, 0x62)),
            cube_snapped: c(rgb(0xE8, 0xF0, 0xFE)),
        }
    }

    pub fn dark() -> Self {
        Self {
            bg: c(rgb(0x0F, 0x11, 0x15)),
            surface: c(rgb(0x16, 0x19, 0x1F)),
            field: c(rgb(0x0F, 0x11, 0x15)),
            hover: c(rgb(0x26, 0x2B, 0x34)),
            pressed: c(rgb(0x2E, 0x34, 0x3E)),
            line: c(rgb(0x2A, 0x2F, 0x38)),
            line_soft: c(rgb(0x21, 0x25, 0x2D)),
            text: c(rgb(0xE6, 0xE8, 0xEC)),
            text_2: c(rgb(0xA9, 0xB0, 0xBC)),
            text_3: c(rgb(0x8A, 0x92, 0x9F)),
            accent: c(rgb(0x4C, 0x8D, 0xFF)),
            accent_text: c(rgb(0x7A, 0xAB, 0xFF)),
            accent_soft: c(rgb(0x1A, 0x2A, 0x47)),
            accent_ring: c(rgb(0x3A, 0x6B, 0xC4)),
            primary: c(rgb(0xE6, 0xE8, 0xEC)),
            on_primary: c(rgb(0x0F, 0x11, 0x15)),
            warn: c(rgb(0xF0, 0xA6, 0x3A)),
            warn_soft: c(rgb(0x3A, 0x2A, 0x12)),
            warn_text: c(rgb(0xF5, 0xC2, 0x7A)),
            danger: c(rgb(0xFF, 0x8A, 0x84)),
            danger_soft: c(rgb(0x3D, 0x1A, 0x19)),
            ok: c(rgb(0x3F, 0xB9, 0x7A)),
            info: c(rgb(0x4C, 0x8D, 0xFF)),
            hud: c((0x16, 0x19, 0x1F, 0xEB)),
            tooltip: c(rgb(0x2C, 0x31, 0x3A)),
            tooltip_text: c(rgb(0xE6, 0xE8, 0xEC)),
            tooltip_text_2: c(rgb(0xA9, 0xB0, 0xBC)),
            toggle_track_off: c(rgb(0x3A, 0x40, 0x4B)),
            toggle_knob_off: c(rgb(0xA9, 0xB0, 0xBC)),
            toggle_knob_on: c(rgb(0x0F, 0x11, 0x15)),
            shadow: c((0x00, 0x00, 0x00, 0x80)),
            axis_x: c(rgb(0xF2, 0x55, 0x5A)),
            axis_y: c(rgb(0x45, 0xC3, 0x5A)),
            axis_z: c(rgb(0x5B, 0x9B, 0xFF)),
            viewport_sky: c(rgb(0x1A, 0x1E, 0x25)),
            viewport_ground: c(rgb(0x20, 0x25, 0x2C)),
            viewport_grid: c(rgb(0x2E, 0x34, 0x3D)),
            selection_outline: c(rgb(0x4C, 0x8D, 0xFF)),
            entity_world: c(rgb(0xA9, 0xB0, 0xBC)),
            entity_model: c(rgb(0x7A, 0xAB, 0xFF)),
            entity_light: c(rgb(0xE8, 0xB4, 0x4A)),
            entity_link: c(rgb(0xA9, 0xB0, 0xBC)),
            entity_joint: c(rgb(0xC4, 0x9B, 0xF5)),
            entity_sensor: c(rgb(0x4F, 0xC6, 0xDC)),
            entity_badge_bg: c(rgb(0x26, 0x2B, 0x34)),
            icon: c(rgb(0xC9, 0xCE, 0xD6)),
            disabled: c(rgb(0x5A, 0x62, 0x70)),
            surface_2: c(rgb(0x1E, 0x22, 0x2A)),
            warn_badge: c(rgb(0xF0, 0xA6, 0x3A)),
            axis_x_text: c(rgb(0xF2, 0x55, 0x5A)),
            axis_y_text: c(rgb(0x45, 0xC3, 0x5A)),
            axis_z_text: c(rgb(0x5B, 0x9B, 0xFF)),
            sensor_soft: c(rgb(0x0F, 0x2A, 0x33)),
            chart_1: c(rgb(0x4C, 0x8D, 0xFF)),
            chart_2: c(rgb(0xC4, 0x9B, 0xF5)),
            chart_3: c(rgb(0x4F, 0xC6, 0xDC)),
            chart_4: c(rgb(0xF0, 0xA6, 0x3A)),
            cube_top: c(rgb(0x3A, 0x40, 0x4B)),
            cube_right: c(rgb(0x2C, 0x31, 0x3A)),
            cube_left: c(rgb(0x23, 0x27, 0x2F)),
            cube_edge: c(rgb(0x5A, 0x62, 0x70)),
            cube_label: c(rgb(0xA9, 0xB0, 0xBC)),
            cube_snapped: c(rgb(0x1A, 0x2A, 0x47)),
        }
    }
}

/// Fixed sizes in logical px. Multiply by `Theme::scale` when building nodes.
#[derive(Clone, Debug, Reflect)]
pub struct Sizes {
    pub top_bar_h: f32,
    pub status_bar_h: f32,
    pub activity_bar_w: f32,
    pub activity_btn: f32,
    pub activity_icon: f32,
    pub sidebar_w: f32,
    pub sidebar_min_w: f32,
    pub sidebar_max_w: f32,
    pub inspector_w: f32,
    pub inspector_min_w: f32,
    pub inspector_max_w: f32,
    pub section_header_h: f32,
    pub tree_row_h: f32,
    pub tree_indent: f32,
    pub entity_badge: f32,
    pub icon_btn: f32,
    pub icon_btn_sm: f32,
    pub icon: f32,
    pub icon_sm: f32,
    pub text_btn_h: f32,
    pub input_h: f32,
    pub input_h_compact: f32,
    pub button_h: f32,
    pub toggle_w: f32,
    pub toggle_h: f32,
    pub toggle_knob: f32,
    pub menu_item_h: f32,
    pub menu_min_w: f32,
    pub rail_w: f32,
    pub rail_btn: f32,
    pub rail_icon: f32,
    pub view_cube: f32,
    pub view_cube_inactive: f32,
    pub rail_gap_below_cube: f32,
    pub viewport_inset: f32,
    pub viewport_divider: f32,
    pub viewport_min: f32,
    pub viewport_divider_hit: f32,
    pub rail_compact_below_view_h: f32,
    pub message_chip_max_w: f32,
    pub world_menu_w: f32,
    pub layout_thumb_w: f32,
    pub layout_thumb_h: f32,
    pub logo: f32,
}

impl Default for Sizes {
    fn default() -> Self {
        Self {
            top_bar_h: 44.0,
            status_bar_h: 28.0,
            activity_bar_w: 56.0,
            activity_btn: 44.0,
            activity_icon: 20.0,
            sidebar_w: 300.0,
            sidebar_min_w: 240.0,
            sidebar_max_w: 480.0,
            inspector_w: 300.0,
            inspector_min_w: 260.0,
            inspector_max_w: 480.0,
            section_header_h: 36.0,
            tree_row_h: 28.0,
            tree_indent: 14.0,
            entity_badge: 16.0,
            icon_btn: 32.0,
            icon_btn_sm: 28.0,
            icon: 17.0,
            icon_sm: 15.0,
            text_btn_h: 32.0,
            input_h: 32.0,
            input_h_compact: 30.0,
            button_h: 30.0,
            toggle_w: 34.0,
            toggle_h: 20.0,
            toggle_knob: 16.0,
            menu_item_h: 30.0,
            menu_min_w: 200.0,
            rail_w: 40.0,
            rail_btn: 32.0,
            rail_icon: 18.0,
            view_cube: 56.0,
            view_cube_inactive: 34.0,
            rail_gap_below_cube: 8.0,
            viewport_inset: 10.0,
            viewport_divider: 2.0,
            viewport_min: 240.0,
            viewport_divider_hit: 6.0,
            rail_compact_below_view_h: 500.0,
            message_chip_max_w: 420.0,
            world_menu_w: 280.0,
            layout_thumb_w: 52.0,
            layout_thumb_h: 36.0,
            logo: 28.0,
        }
    }
}

/// Spacing scale (tokens.json `space`). Index 1..=10.
pub const SPACE: [f32; 11] = [0.0, 2.0, 4.0, 6.0, 8.0, 10.0, 12.0, 14.0, 16.0, 20.0, 24.0];

pub mod radius {
    pub const XS: f32 = 4.0;
    pub const SM: f32 = 6.0;
    pub const MD: f32 = 8.0;
    pub const LG: f32 = 10.0;
    pub const XL: f32 = 12.0;
}

/// Semantic color roles. Widgets store roles; `apply_theme_system` resolves them.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Reflect)]
pub enum ColorRole {
    Bg,
    Surface,
    Field,
    Hover,
    Line,
    Text,
    Text2,
    Text3,
    Accent,
    AccentText,
    AccentSoft,
    AccentRing,
    Primary,
    OnPrimary,
    Warn,
    WarnSoft,
    WarnText,
    Danger,
    Ok,
    Hud,
    Tooltip,
    TooltipText,
    Icon,
    Disabled,
    Surface2,
    WarnBadge,
    AxisXText,
    AxisYText,
    AxisZText,
    Chart1,
    Chart2,
    Chart3,
    Chart4,
}

impl Palette {
    pub fn role(&self, r: ColorRole) -> Color {
        use ColorRole::*;
        match r {
            Bg => self.bg,
            Surface => self.surface,
            Field => self.field,
            Hover => self.hover,
            Line => self.line,
            Text => self.text,
            Text2 => self.text_2,
            Text3 => self.text_3,
            Accent => self.accent,
            AccentText => self.accent_text,
            AccentSoft => self.accent_soft,
            AccentRing => self.accent_ring,
            Primary => self.primary,
            OnPrimary => self.on_primary,
            Warn => self.warn,
            WarnSoft => self.warn_soft,
            WarnText => self.warn_text,
            Danger => self.danger,
            Ok => self.ok,
            Hud => self.hud,
            Tooltip => self.tooltip,
            TooltipText => self.tooltip_text,
            Icon => self.icon,
            Disabled => self.disabled,
            Surface2 => self.surface_2,
            WarnBadge => self.warn_badge,
            AxisXText => self.axis_x_text,
            AxisYText => self.axis_y_text,
            AxisZText => self.axis_z_text,
            Chart1 => self.chart_1,
            Chart2 => self.chart_2,
            Chart3 => self.chart_3,
            Chart4 => self.chart_4,
        }
    }
}

#[derive(Resource, Clone, Debug, Reflect)]
pub struct Theme {
    pub mode: ThemeMode,
    /// Resolved mode after applying `System`.
    pub is_dark: bool,
    pub c: Palette,
    pub s: Sizes,
    /// UI scale; 1.0 = designs as drawn. Persisted per user.
    pub scale: f32,
}

impl Theme {
    pub fn new(mode: ThemeMode, os_prefers_dark: bool) -> Self {
        let is_dark = match mode {
            ThemeMode::Dark => true,
            ThemeMode::Light => false,
            ThemeMode::System => os_prefers_dark,
        };
        Self {
            mode,
            is_dark,
            c: if is_dark { Palette::dark() } else { Palette::light() },
            s: Sizes::default(),
            scale: 1.0,
        }
    }

    /// Logo asset for the current mode (dark mode uses light strokes).
    pub fn logo_path(&self) -> &'static str {
        if self.is_dark {
            "ui/brand/gazebo-icon-dark.png"
        } else {
            "ui/brand/gazebo-icon.png"
        }
    }
}
