//! ActivityButton (§5.5, §3.2): 44×44, radius 10, icon 20. Icon `text_3`,
//! hover `hover` / `text`, active (its panel open) `accent_soft` /
//! `accent_text` with an inset 1px `accent_ring`. Optional badge at the top
//! right, offset 6/6. To AccessKit it is a button (it toggles its panel;
//! there is no tab list to belong to).

use accesskit::Role;
use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use bevy::ui_widgets::Button as Headless;

use super::badge::{badge_node, badge_parts};
use super::{clickable, rounded, BadgeKind, Tooltip, TooltipSide};
use crate::icons::{content_icon, IconId};
use crate::theme::{border, radius, ColorRole, Theme, SPACE};
use crate::visuals::Visuals;

/// The activity button's state table: transparent with a `text_3` icon at
/// rest, `hover` / `text` on hover, `accent_soft` / `accent_text` with an
/// inset 1px `accent_ring` when active. The top bar's buttons share it
/// (DECISIONS.md D-48). Needs a 1px border on the node for the ring.
pub fn activity_visuals() -> Visuals {
    let mut visuals = Visuals::new(ColorRole::Clear, ColorRole::Text3).stroked(ColorRole::Clear);
    visuals.active_stroke = Some(ColorRole::AccentRing);
    visuals
}

/// An activity bar button, with an optional `(kind, count)` badge. Its
/// tooltip opens to the right: the bar is at the window's left edge (§5.15).
pub fn activity_button(
    theme: &Theme,
    id: IconId,
    tip: Tooltip,
    badge_count: Option<(BadgeKind, u32)>,
) -> impl Bundle {
    let tip = tip.side(TooltipSide::Right);
    let s = &theme.s;
    let visuals = activity_visuals();
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
        clickable(visuals, Role::Button, tip.accessible_name()),
        tip,
        Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
            p.spawn(content_icon(id, icon_px, ColorRole::Text3));
            if let Some((kind, count)) = badge_count {
                let mut node = badge_node(&theme);
                node.position_type = PositionType::Absolute;
                node.top = px(SPACE[3]);
                node.right = px(SPACE[3]);
                p.spawn((node, badge_parts(kind, count)));
            }
        })),
    )
}
