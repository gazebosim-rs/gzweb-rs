//! The search / command bar trigger (spec §3.1): a field-looking button
//! with the search icon, the placeholder and the ⌘K key cap. It opens the
//! command palette in M8. Narrow windows squeeze it first.

use accesskit::Role;
use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use bevy::ui::ComputedNode;
use bevy::ui_widgets::Button as Headless;
use gz_bevy::view::input::InputMap;

use crate::assets::fonts::text;
use crate::assets::icons::{content_icon, IconId};
use crate::theme::visuals::Visuals;
use crate::theme::{border, radius, ColorRole, FontFamily, Stroke, Theme, TypeStyle, SPACE};
use crate::widgets::layout::{clip_box, row};
use crate::widgets::shortcut::shortcut_label;
use crate::widgets::text::set_text;
use crate::widgets::{clickable, rounded};

/// The search / command bar trigger (opens ⌘K in M8).
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct SearchTrigger;

/// Search placeholder and key cap text: mono 11 for the cap.
pub(crate) const KEYCAP_TYPE: TypeStyle = TypeStyle::new(FontFamily::Mono, 11.0, 400);
/// Width of the search trigger (§3.1).
pub(crate) const SEARCH_W: f32 = 220.0;

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
                text(TypeStyle::BODY, ColorRole::Text3, SEARCH_PLACEHOLDERS[0]),
                SearchPlaceholder::default(),
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

/// The search trigger's placeholder, longest first: the longest that fits
/// shows, so a narrow trigger (or a wide key cap, "Ctrl+K") never cuts a
/// word (bevy_ui text has no ellipsis; DECISIONS.md D-56).
pub(crate) const SEARCH_PLACEHOLDERS: [&str; 3] = ["Search or run a command", "Search…", ""];

/// The search placeholder: the widths its texts measured, and which shows.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct SearchPlaceholder {
    pub widths: [Option<f32>; SEARCH_PLACEHOLDERS.len()],
    pub shown: usize,
}

impl SearchPlaceholder {
    /// The text to show in `room` logical pixels: the longest known to fit,
    /// or a longer one not measured yet (tried, then measured).
    pub(crate) fn pick(&self, room: f32) -> usize {
        self.widths
            .iter()
            .position(|w| w.is_none_or(|w| w <= room + 0.5))
            .unwrap_or(SEARCH_PLACEHOLDERS.len() - 1)
    }
}

/// Measures the placeholder shown (its box doesn't shrink, so its width is
/// the text's) against the clipping box around it, and swaps texts.
pub(crate) fn fit_search_placeholder(
    mut texts: Query<(&mut Text, &mut SearchPlaceholder, &ComputedNode, &ChildOf)>,
    boxes: Query<&ComputedNode>,
) {
    for (mut text, mut fit, node, parent) in &mut texts {
        let Ok(room) = boxes.get(parent.parent()) else {
            continue;
        };
        let k = node.inverse_scale_factor();
        let width = node.size().x * k;
        if width <= 0.0 && fit.shown != SEARCH_PLACEHOLDERS.len() - 1 {
            // Not laid out yet.
            continue;
        }
        let shown = fit.shown;
        if fit.widths[shown] != Some(width) {
            fit.widths[shown] = Some(width);
        }
        let pick = fit.pick(room.size().x * room.inverse_scale_factor());
        if pick != shown {
            fit.shown = pick;
            set_text(&mut text, SEARCH_PLACEHOLDERS[pick]);
        }
    }
}
