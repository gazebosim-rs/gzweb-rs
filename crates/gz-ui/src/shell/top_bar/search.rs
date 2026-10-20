//! The search / command bar trigger (spec §3.1): a field-looking button
//! with the search icon, the placeholder and the ⌘K key cap. It opens the
//! command palette in M8. Narrow windows squeeze it first.

use accesskit::Role;
use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use bevy::ui_widgets::Button as Headless;
use gz_bevy::view::input::InputMap;

use crate::assets::fonts::text;
use crate::assets::icons::{content_icon, IconId};
use crate::theme::visuals::Visuals;
use crate::theme::{border, radius, ColorRole, FontFamily, Stroke, Theme, TypeStyle, SPACE};
use crate::widgets::layout::{clip_box, row};
use crate::widgets::shortcut::shortcut_label;
use crate::widgets::{clickable, rounded};

/// The search / command bar trigger (opens ⌘K in M8).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct SearchTrigger;

/// Search placeholder and key cap text: mono 11 for the cap.
pub const KEYCAP_TYPE: TypeStyle = TypeStyle::new(FontFamily::Mono, 11.0, 400);
/// Width of the search trigger (§3.1).
pub const SEARCH_W: f32 = 220.0;

pub(super) fn search_trigger(theme: &Theme, map: &InputMap) -> impl Bundle {
    let cap = shortcut_label(map, "ui.command_palette").unwrap_or_default();
    let icon_px = theme.s.icon_sm;
    let visuals = Visuals::new(ColorRole::Field, ColorRole::Text3)
        .stroked(ColorRole::Line)
        .keep_ink();
    (
        SearchTrigger,
        Node {
            width: px(SEARCH_W),
            height: px(theme.s.input_h),
            // Narrow windows squeeze the search first (down to its icon).
            flex_shrink: 1.0,
            min_width: px(theme.s.input_h + SPACE[4]),
            padding: UiRect {
                left: px(SPACE[5]),
                right: px(SPACE[3]),
                ..default()
            },
            border: UiRect::all(px(border::HAIRLINE)),
            overflow: Overflow::clip(),
            border_radius: rounded(radius::MD),
            ..row(SPACE[4])
        },
        Headless,
        clickable(visuals, Role::Button, "Search or run a command"),
        Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
            p.spawn(content_icon(IconId::Search, icon_px, ColorRole::Text3));
            // Text can't clip itself: a clipping box around it.
            p.spawn(clip_box()).with_child((
                text(TypeStyle::BODY, ColorRole::Text3, "Search or run a command"),
                TextLayout::no_wrap(),
                Node {
                    flex_shrink: 0.0,
                    ..default()
                },
            ));
            p.spawn((
                Node {
                    padding: UiRect::axes(px(SPACE[2] + 1.0), px(1)),
                    border: UiRect::all(px(border::HAIRLINE)),
                    flex_shrink: 0.0,
                    border_radius: rounded(radius::XS),
                    ..default()
                },
                Stroke(ColorRole::Line),
                children![text(KEYCAP_TYPE, ColorRole::Text3, cap)],
            ));
        })),
    )
}
