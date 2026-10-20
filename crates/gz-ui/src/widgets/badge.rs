//! Badge (§5.13): a 14 px pill, min width 14, padding 0 3, mono 9/600
//! (`micro`) in `surface`. The fill says what it counts; never `accent`.

use bevy::prelude::*;

use super::rounded;
use crate::assets::fonts::text;
use crate::theme::{radius, ColorRole, Fill, Theme, TypeStyle};

/// What a badge counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BadgeKind {
    /// A neutral count (`text_2`).
    #[default]
    Count,
    /// Warnings (`warn_badge`).
    Warning,
    /// Errors (`danger`).
    Error,
}

impl BadgeKind {
    pub fn fill(self) -> ColorRole {
        match self {
            BadgeKind::Count => ColorRole::Text2,
            BadgeKind::Warning => ColorRole::WarnBadge,
            BadgeKind::Error => ColorRole::Danger,
        }
    }
}

/// Marks a badge.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Badge;

/// A badge's box.
pub fn badge_node(theme: &Theme) -> Node {
    let h = theme.s.badge_h;
    Node {
        height: px(h),
        min_width: px(h),
        padding: UiRect::horizontal(px(3)),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        flex_shrink: 0.0,
        border_radius: rounded(radius::PILL),
        ..default()
    }
}

/// A badge without its box, for callers that place it ([`badge_node`]).
pub fn badge_parts(kind: BadgeKind, count: u32) -> impl Bundle {
    (
        Badge,
        Fill(kind.fill()),
        children![text(
            TypeStyle::MICRO,
            ColorRole::Surface,
            count.to_string()
        )],
    )
}

/// A count badge.
pub fn badge(theme: &Theme, kind: BadgeKind, count: u32) -> impl Bundle {
    (badge_node(theme), badge_parts(kind, count))
}
