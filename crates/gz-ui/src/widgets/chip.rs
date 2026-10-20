//! Chips (§5.12): filter chips (24 tall, radius 12, 1px `line`, Geist
//! 12/500 `text_2`; selected: fill `text`, label `surface`, and the border
//! clear rather than `text`, which would leave an anti-aliasing seam where
//! border and fill meet; the width stays, so the label doesn't move) and
//! status chips
//! (§3.6 item 5: 22 tall, radius 5, `warn_soft` / `danger_soft` /
//! transparent, warning icon 13, message clipped at `message_chip_max_w`).

use accesskit::Role;
use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use bevy::text::LineBreak;
use bevy::ui_widgets::{checkbox_self_update, observe, Button as Headless, Checkbox};

use super::button::Checkedness;
use super::{clickable, rounded};
use crate::fonts::text;
use crate::icons::{content_icon, IconId};
use crate::theme::{border, ColorRole, FontFamily, Theme, TypeStyle, SPACE};
use crate::visuals::{Content, Visuals};

/// Chip label type: Geist 12/500.
pub const CHIP_TYPE: TypeStyle = TypeStyle::new(FontFamily::Ui, 12.0, 500);
/// Filter chip height and radius.
pub const FILTER_CHIP_H: f32 = 24.0;
/// Status chip height and radius.
pub const STATUS_CHIP_H: f32 = 22.0;
const STATUS_CHIP_RADIUS: f32 = 5.0;

/// A toggleable filter chip (a checkbox: `Checked` = selected).
pub fn filter_chip(label: impl Into<String>, selected: bool) -> impl Bundle {
    let label = label.into();
    let mut visuals = Visuals::new(ColorRole::Clear, ColorRole::Text2).stroked(ColorRole::Line);
    visuals.active_fill = ColorRole::Text;
    visuals.active_ink = ColorRole::Surface;
    visuals.active_stroke = Some(ColorRole::Clear);
    (
        Node {
            height: px(FILTER_CHIP_H),
            padding: UiRect::horizontal(px(SPACE[5])),
            border: UiRect::all(px(border::HAIRLINE)),
            align_items: AlignItems::Center,
            flex_shrink: 0.0,
            border_radius: rounded(FILTER_CHIP_H / 2.0),
            ..default()
        },
        Checkbox,
        clickable(visuals, Role::CheckBox, label.clone()),
        Checkedness(selected),
        observe(checkbox_self_update),
        children![(text(CHIP_TYPE, ColorRole::Text2, label), Content)],
    )
}

/// What a status chip reports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StatusKind {
    #[default]
    Info,
    Warning,
    Error,
}

impl StatusKind {
    /// Fill and text roles.
    pub fn roles(self) -> (ColorRole, ColorRole) {
        match self {
            StatusKind::Info => (ColorRole::Clear, ColorRole::Text2),
            StatusKind::Warning => (ColorRole::WarnSoft, ColorRole::WarnText),
            StatusKind::Error => (ColorRole::DangerSoft, ColorRole::Danger),
        }
    }
}

/// A status-bar message chip: icon, message and an optional count
/// ("2 warnings"). Clicking it opens the message tray (M8).
pub fn status_chip(
    theme: &Theme,
    kind: StatusKind,
    message: impl Into<String>,
    count: Option<String>,
) -> impl Bundle {
    let message = message.into();
    let (fill, ink) = kind.roles();
    let mut visuals = Visuals::new(fill, ink).keep_ink();
    visuals.hover_fill = fill;
    visuals.pressed_fill = fill;
    let max_w = theme.s.message_chip_max_w;
    let label = message.clone();
    (
        Node {
            height: px(STATUS_CHIP_H),
            max_width: px(max_w),
            padding: UiRect::horizontal(px(SPACE[4])),
            column_gap: px(SPACE[3]),
            align_items: AlignItems::Center,
            overflow: Overflow::clip(),
            flex_shrink: 0.0,
            border_radius: rounded(STATUS_CHIP_RADIUS),
            ..default()
        },
        Headless,
        clickable(visuals, Role::Button, label),
        Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
            if kind != StatusKind::Info {
                p.spawn(content_icon(IconId::Warning, 13.0, ink));
            }
            p.spawn((
                text(CHIP_TYPE, ink, message),
                TextLayout {
                    linebreak: LineBreak::NoWrap,
                    ..default()
                },
                Node {
                    flex_shrink: 1.0,
                    overflow: Overflow::clip(),
                    ..default()
                },
                Content,
            ));
            if let Some(count) = count {
                p.spawn(text(TypeStyle::MONO_SMALL, ink, count));
            }
        })),
    )
}

/// A plain coloured dot (status dots, 7 px).
pub fn dot(size: f32, role: ColorRole) -> impl Bundle {
    (
        Node {
            width: px(size),
            height: px(size),
            flex_shrink: 0.0,
            border_radius: rounded(size / 2.0),
            ..default()
        },
        crate::theme::Fill(role),
    )
}
