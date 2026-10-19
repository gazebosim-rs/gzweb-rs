//! ActivityButton (§5.5, §3.2): 44×44, radius 10, icon 20. Icon `text_3`,
//! hover `hover` / `text`, active (its panel open) `accent_soft` /
//! `accent_text` with an inset 1px `accent_ring`. Optional badge at the top
//! right, offset 6/6.

use accesskit::Role;
use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use bevy::ui_widgets::Button as Headless;

use super::badge::{badge_node, badge_parts};
use super::{clickable, rounded, BadgeKind, Tooltip};
use crate::icons::{content_icon, IconId};
use crate::theme::{border, radius, ColorRole, Theme};
use crate::visuals::Visuals;

/// An activity bar button, with an optional `(kind, count)` badge.
pub fn activity_button(
    theme: &Theme,
    id: IconId,
    tip: Tooltip,
    badge_count: Option<(BadgeKind, u32)>,
) -> impl Bundle {
    let s = &theme.s;
    let mut visuals = Visuals::new(ColorRole::Clear, ColorRole::Text3).stroked(ColorRole::Clear);
    visuals.active_stroke = Some(ColorRole::AccentRing);
    let icon_px = s.activity_icon;
    let theme = theme.clone();
    (
        Node {
            width: px(s.activity_btn),
            height: px(s.activity_btn),
            flex_shrink: 0.0,
            border: UiRect::all(px(border::HAIRLINE)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border_radius: rounded(radius::LG),
            ..default()
        },
        Headless,
        clickable(visuals, Role::Tab, tip.accessible_name()),
        tip,
        Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
            p.spawn(content_icon(id, icon_px, ColorRole::Text3));
            if let Some((kind, count)) = badge_count {
                let mut node = badge_node(&theme);
                node.position_type = PositionType::Absolute;
                node.top = px(6);
                node.right = px(6);
                p.spawn((node, badge_parts(kind, count)));
            }
        })),
    )
}
