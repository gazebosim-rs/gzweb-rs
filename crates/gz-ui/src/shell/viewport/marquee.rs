//! The selection marquee (spec §6.1): the rectangle a Box select drag, or
//! ⌘/Ctrl+drag in Navigate, draws over the view while it selects.

use bevy::prelude::*;
use gz_bevy::view::Marquee;

use crate::shell::regions::pass_through;
use crate::theme::{border, ColorRole, Fill, Opacity, Stroke};
use crate::widgets::layout::{display, overlay};

/// How much of `accent_soft` the marquee's inside shows.
pub(crate) const MARQUEE_FILL_OPACITY: f32 = 0.5;

/// The marquee's box.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct MarqueeBox;

/// A hidden 1px `selection_outline` box over a half-opacity `accent_soft`
/// fill, on the view's node.
pub(crate) fn marquee() -> impl Bundle {
    (
        MarqueeBox,
        Node {
            position_type: PositionType::Absolute,
            border: UiRect::all(px(border::HAIRLINE)),
            display: Display::None,
            ..default()
        },
        Stroke(ColorRole::SelectionOutline),
        pass_through(),
        children![(
            overlay(),
            Fill(ColorRole::AccentSoft),
            Opacity(MARQUEE_FILL_OPACITY),
            pass_through(),
        )],
    )
}

/// The box follows the marquee (the view's logical pixels are the view
/// node's).
pub(crate) fn marquee_box(marquee: Res<Marquee>, mut boxes: Query<&mut Node, With<MarqueeBox>>) {
    if !marquee.is_changed() {
        return;
    }
    for mut node in &mut boxes {
        let rect = marquee.0.map(|m| Rect::from_corners(m.from, m.to));
        let want = display(rect.is_some());
        if node.display != want {
            node.display = want;
        }
        if let Some(r) = rect {
            node.left = px(r.min.x);
            node.top = px(r.min.y);
            node.width = px(r.width());
            node.height = px(r.height());
        }
    }
}
