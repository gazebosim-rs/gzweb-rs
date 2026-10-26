//! The selection marquee (spec §6.1): the rectangle a Box select drag, or
//! ⌘/Ctrl+drag in Navigate, draws over the view while it selects.

use bevy::prelude::*;
use gz_bevy::view::Marquee;

use crate::shell::regions::pass_through;
use crate::shell::viewport::ViewCameras;
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

/// The box follows the marquee in the view it is drawn in: the marquee
/// is in the view's logical pixels (the window's), the box in its node's
/// (those over the UI scale).
pub(crate) fn marquee_box(
    marquee: Res<Marquee>,
    ui_scale: Res<UiScale>,
    views: ViewCameras,
    mut boxes: Query<(Entity, &mut Node), With<MarqueeBox>>,
) {
    if !marquee.is_changed() && !ui_scale.is_changed() {
        return;
    }
    let k = 1.0 / ui_scale.0.max(f32::EPSILON);
    for (e, mut node) in &mut boxes {
        let rect = marquee
            .0
            .filter(|m| views.of(e).is_some_and(|v| v.0 == m.view))
            .map(|m| Rect::from_corners(m.from * k, m.to * k));
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
