//! IconButton (§5.1), TextButton (§5.2) and the bordered Button (§5.3).

use accesskit::Role;
use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use bevy::ui_widgets::{Button as Headless, RadioButton};

use super::layout::row;
use super::{clickable, rounded, Tooltip};
use crate::assets::fonts::text;
use crate::assets::icons::{content_icon, IconId};
use crate::theme::visuals::{Content, Visuals};
use crate::theme::{border, radius, ColorRole, FontFamily, Theme, TypeStyle, SPACE};

/// The three IconButton sizes (§5.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconButtonSize {
    /// 32, icon 17, radius 7.
    Regular,
    /// 28, icon 15, radius 7.
    Small,
    /// Viewport rail: 32, icon 18, radius 8.
    Rail,
}

impl IconButtonSize {
    /// Button side, icon size and corner radius.
    pub fn metrics(self, theme: &Theme) -> (f32, f32, f32) {
        let s = &theme.s;
        match self {
            IconButtonSize::Regular => (s.icon_btn, s.icon, ICON_BUTTON_RADIUS),
            IconButtonSize::Small => (s.icon_btn_sm, s.icon_sm, ICON_BUTTON_RADIUS),
            IconButtonSize::Rail => (s.rail_btn, s.rail_icon, radius::MD),
        }
    }
}

/// IconButton and TextButton corner radius (§5.1, §5.2: 7, between the
/// `sm` and `md` tokens).
pub const ICON_BUTTON_RADIUS: f32 = 7.0;

fn icon_button_node(side: f32, r: f32) -> Node {
    Node {
        width: px(side),
        height: px(side),
        flex_shrink: 0.0,
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        border_radius: rounded(r),
        ..default()
    }
}

/// An icon-only button. The tooltip is required: it is also the accessible
/// name (spec §1.6, §10).
pub fn icon_button(theme: &Theme, id: IconId, size: IconButtonSize, tip: Tooltip) -> impl Bundle {
    let (side, icon, r) = size.metrics(theme);
    (
        icon_button_node(side, r),
        Headless,
        clickable(
            Visuals::new(ColorRole::Clear, ColorRole::Icon),
            Role::Button,
            tip.accessible_name(),
        ),
        tip,
        children![content_icon(id, icon, ColorRole::Icon)],
    )
}

/// An icon button with radio semantics, for a [`segmented_group`]: `Checked`
/// is its active state.
///
/// [`segmented_group`]: super::segmented_group
pub fn tool_button(theme: &Theme, id: IconId, tip: Tooltip, checked: bool) -> impl Bundle {
    let (side, icon, r) = IconButtonSize::Regular.metrics(theme);
    (
        icon_button_node(side, r),
        RadioButton,
        clickable(
            Visuals::new(ColorRole::Clear, ColorRole::Icon),
            Role::RadioButton,
            tip.accessible_name(),
        ),
        tip,
        children![content_icon(id, icon, ColorRole::Icon)],
        Checkedness(checked),
    )
}

/// A top-bar icon button (DECISIONS.md D-48): 32 px, icon 17, radius 7,
/// with the activity button's look (transparent at rest, `hover` on hover,
/// the active look with its `accent_ring`), no container behind it.
pub fn bar_button(theme: &Theme, id: IconId, tip: Tooltip) -> impl Bundle {
    let (side, icon, r) = IconButtonSize::Regular.metrics(theme);
    (
        bar_button_node(side, r),
        Headless,
        clickable(
            super::activity::activity_visuals(),
            Role::Button,
            tip.accessible_name(),
        ),
        tip,
        children![content_icon(id, icon, ColorRole::Text3)],
    )
}

/// A top-bar tool button: [`bar_button`]'s look with radio semantics, for a
/// radio [`button_group`](super::segmented::button_group); `Checked` is the
/// active look.
pub fn bar_tool_button(theme: &Theme, id: IconId, tip: Tooltip, checked: bool) -> impl Bundle {
    let (side, icon, r) = IconButtonSize::Regular.metrics(theme);
    (
        bar_button_node(side, r),
        RadioButton,
        clickable(
            super::activity::activity_visuals(),
            Role::RadioButton,
            tip.accessible_name(),
        ),
        tip,
        children![content_icon(id, icon, ColorRole::Text3)],
        Checkedness(checked),
    )
}

/// An icon button's box with the 1px border the active ring needs.
fn bar_button_node(side: f32, r: f32) -> Node {
    Node {
        border: UiRect::all(px(border::HAIRLINE)),
        ..icon_button_node(side, r)
    }
}

/// Play/Pause (§3.1): 40×32, radius 7, `primary` fill, icon `on_primary`
/// 14 px. The only `primary`-filled control (§4.1).
pub fn primary_button(theme: &Theme, id: IconId, tip: Tooltip) -> impl Bundle {
    let mut visuals = Visuals::new(ColorRole::Primary, ColorRole::OnPrimary).keep_ink();
    visuals.hover_fill = ColorRole::Primary;
    visuals.pressed_fill = ColorRole::Text2;
    visuals.active_fill = ColorRole::Primary;
    visuals.active_ink = ColorRole::OnPrimary;
    (
        Node {
            width: px(40),
            height: px(theme.s.icon_btn),
            flex_shrink: 0.0,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border_radius: rounded(ICON_BUTTON_RADIUS),
            ..default()
        },
        Headless,
        clickable(visuals, Role::Button, tip.accessible_name()),
        tip,
        children![content_icon(id, 14.0, ColorRole::OnPrimary)],
    )
}

/// Initial `Checked` state for radio and checkbox widgets, applied on
/// spawn (a bundle can't hold an optional component).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[component(on_add = apply_checkedness)]
pub struct Checkedness(pub bool);

fn apply_checkedness(
    mut world: bevy::ecs::world::DeferredWorld,
    ctx: bevy::ecs::lifecycle::HookContext,
) {
    let on = world.get::<Checkedness>(ctx.entity).is_some_and(|c| c.0);
    let mut commands = world.commands();
    let mut e = commands.entity(ctx.entity);
    e.remove::<Checkedness>();
    if on {
        e.insert(bevy::ui::Checked);
    }
}

/// A borderless text button (§5.2): label 13/500, optional leading icon
/// and trailing chevron (15).
#[derive(Clone, Debug, Default)]
pub struct TextButton {
    pub label: String,
    pub leading: Option<IconId>,
    pub chevron: bool,
    /// Mono 12 label ("Snap 0.25", "1 step").
    pub mono: bool,
    /// A 7 px dot after the label (the world menu's unsaved dot).
    pub dot: Option<ColorRole>,
    /// Label size (13; the world name is 15).
    pub size: Option<f32>,
}

impl TextButton {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            ..default()
        }
    }

    pub fn leading(mut self, icon: IconId) -> Self {
        self.leading = Some(icon);
        self
    }

    pub fn chevron(mut self) -> Self {
        self.chevron = true;
        self
    }

    pub fn mono(mut self) -> Self {
        self.mono = true;
        self
    }

    pub fn dot(mut self, role: ColorRole) -> Self {
        self.dot = Some(role);
        self
    }

    /// The world menu's 15 px label.
    pub fn large(mut self) -> Self {
        self.size = Some(15.0);
        self
    }
}

/// A [`TextButton`]'s box (§5.2): text button height, padding 0 9, gap 6,
/// radius 7. The world menu button has its own content in one.
pub(crate) fn text_button_node(theme: &Theme) -> Node {
    Node {
        height: px(theme.s.text_btn_h),
        padding: UiRect::horizontal(px(9)),
        flex_shrink: 0.0,
        border_radius: rounded(ICON_BUTTON_RADIUS),
        ..row(SPACE[3])
    }
}

/// The parts of a [`TextButton`] without the headless button, so menu and
/// popover triggers can reuse the look.
pub(crate) fn text_button_parts(theme: &Theme, spec: TextButton) -> impl Bundle {
    let style = if spec.mono {
        TypeStyle::new(FontFamily::Mono, 12.0, 500)
    } else {
        TypeStyle::new(
            FontFamily::Ui,
            spec.size.unwrap_or(TypeStyle::LABEL.size),
            500,
        )
    };
    let dot = theme.s.status_dot;
    let icon_px = theme.s.icon_sm;
    let label = spec.label.clone();
    (
        text_button_node(theme),
        clickable(
            Visuals::new(ColorRole::Clear, ColorRole::Text),
            Role::Button,
            label.clone(),
        ),
        Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
            if let Some(icon) = spec.leading {
                p.spawn(content_icon(icon, icon_px, ColorRole::Text));
            }
            p.spawn((text(style, ColorRole::Text, label), Content));
            if let Some(role) = spec.dot {
                p.spawn(super::chip::dot(dot, role));
            }
            if spec.chevron {
                p.spawn(content_icon(IconId::ChevronDown, icon_px, ColorRole::Text));
            }
        })),
    )
}

/// A borderless text button (§5.2).
pub fn text_button(theme: &Theme, spec: TextButton) -> impl Bundle {
    (text_button_parts(theme, spec), Headless)
}

/// Bordered button variants (§5.3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonVariant {
    #[default]
    Normal,
    /// Text in `danger`.
    Destructive,
}

/// Bordered button label type: Geist 12/500 (§5.3).
pub const BUTTON_TYPE: TypeStyle = TypeStyle::new(FontFamily::Ui, 12.0, 500);

/// A bordered button (§5.3): 30 tall, 1px `line`, fill `surface_2`.
pub fn button(theme: &Theme, label: impl Into<String>, variant: ButtonVariant) -> impl Bundle {
    let label = label.into();
    let ink = match variant {
        ButtonVariant::Normal => ColorRole::Text,
        ButtonVariant::Destructive => ColorRole::Danger,
    };
    let visuals = Visuals::new(ColorRole::Surface2, ink)
        .stroked(ColorRole::Line)
        .keep_ink();
    (
        Node {
            height: px(theme.s.button_h),
            padding: UiRect::horizontal(px(SPACE[6])),
            border: UiRect::all(px(border::HAIRLINE)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border_radius: rounded(radius::SM),
            ..default()
        },
        Headless,
        clickable(visuals, Role::Button, label.clone()),
        children![(text(BUTTON_TYPE, ink, label), Content)],
    )
}

pub(crate) fn build(_app: &mut App) {}
