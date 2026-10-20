//! The board's token list: each colour token's swatch, name, use and
//! value in the current theme.

use bevy::prelude::*;

use super::card_node;
use crate::assets::fonts::{span, text};
use crate::theme::{border, radius, ColorRole, Fill, FontFamily, Stroke, TypeStyle, SPACE};
use crate::widgets::layout::flex_spacer;
use crate::widgets::rounded;

/// A token's hex value, shown next to its swatch.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct TokenHex(pub(crate) ColorRole);

/// `#RRGGBB` (or `#RRGGBBAA`) for a colour.
pub(crate) fn hex(color: Color) -> String {
    let c = color.to_srgba();
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    if c.alpha < 1.0 {
        format!(
            "#{:02X}{:02X}{:02X}{:02X}",
            b(c.red),
            b(c.green),
            b(c.blue),
            b(c.alpha)
        )
    } else {
        format!("#{:02X}{:02X}{:02X}", b(c.red), b(c.green), b(c.blue))
    }
}

/// The board's token list: role, name, what it's for.
pub(crate) const TOKENS: [(ColorRole, &str, &str); 20] = [
    (ColorRole::Bg, "bg", "app ground, activity bar"),
    (
        ColorRole::Surface,
        "surface",
        "bars, sidebar, inspector, menus",
    ),
    (ColorRole::Field, "field", "inputs"),
    (ColorRole::Hover, "hover", "hovered buttons and rows"),
    (ColorRole::Line, "line", "borders, dividers"),
    (ColorRole::Text, "text", "primary text"),
    (ColorRole::Text2, "text-2", "secondary text, icons"),
    (ColorRole::Text3, "text-3", "hints, units, placeholders"),
    (ColorRole::Accent, "accent", "selection, focus, on states"),
    (
        ColorRole::AccentText,
        "accent-text",
        "accent on text and icons",
    ),
    (ColorRole::AccentSoft, "accent-soft", "active button fill"),
    (ColorRole::AccentRing, "accent-ring", "active button ring"),
    (ColorRole::Primary, "primary", "play / pause button"),
    (ColorRole::Warn, "warn", "unsaved dot, warnings"),
    (ColorRole::WarnSoft, "warn-soft", "warning chip fill"),
    (ColorRole::Danger, "danger", "delete, errors"),
    (ColorRole::Ok, "ok", "connected, RTF healthy"),
    (ColorRole::AxisX, "axis-x / y / z", "gizmo and pose labels"),
    (
        ColorRole::ViewportSky,
        "viewport-sky",
        "default scene background",
    ),
    (
        ColorRole::ViewportGrid,
        "viewport-grid",
        "ground grid lines",
    ),
];

const NAME_TYPE: TypeStyle = TypeStyle::new(FontFamily::Ui, 14.0, 500);
const DESC_TYPE: TypeStyle = TypeStyle::new(FontFamily::Ui, 14.0, 400);
const HEX_TYPE: TypeStyle = TypeStyle::new(FontFamily::Mono, 12.0, 400);

pub(super) fn token_list(c: &mut ChildSpawnerCommands) {
    c.spawn(card_node(Node {
        flex_direction: FlexDirection::Column,
        overflow: Overflow::clip(),
        ..default()
    }))
    .with_children(|list| {
        for (i, (role, name, what)) in TOKENS.into_iter().enumerate() {
            list.spawn((
                Node {
                    height: px(27),
                    padding: UiRect::horizontal(px(SPACE[7])),
                    column_gap: px(SPACE[6]),
                    align_items: AlignItems::Center,
                    border: UiRect::top(px(if i == 0 { 0.0 } else { border::HAIRLINE })),
                    ..default()
                },
                Stroke(ColorRole::Line),
            ))
            .with_children(|r| {
                r.spawn((
                    Node {
                        width: px(20),
                        height: px(20),
                        border: UiRect::all(px(border::HAIRLINE)),
                        border_radius: rounded(radius::SM - 1.0),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    Fill(role),
                    Stroke(ColorRole::Line),
                ));
                r.spawn(text(NAME_TYPE, ColorRole::Text, name))
                    .with_children(|t| {
                        t.spawn(span(DESC_TYPE, ColorRole::Text2, format!(" · {what}")));
                    });
                r.spawn(flex_spacer());
                r.spawn((TokenHex(role), text(HEX_TYPE, ColorRole::Text2, "")));
            });
        }
    });
}
