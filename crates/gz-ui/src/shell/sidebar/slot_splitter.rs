//! The row splitter between the sidebar's slots (spec §3.3.2): a 6 px
//! [`Splitter`] with a 32×2 grip that sets the primary slot's share with
//! both slots at least [`MIN_SLOT_H`] tall, and resets it on a
//! double-click.

use bevy::prelude::*;
use bevy::ui::ComputedNode;

use crate::shell::layout::{DEFAULT_SPLIT, MIN_SLOT_H};
use crate::shell::ShellLayout;
use crate::theme::{border, ColorRole, Fill, Theme};
use crate::widgets::{
    rounded, splitter_control, SplitAxis, Splitter, SplitterMoved, SplitterReset,
};

/// The row splitter between the slots.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct SlotSplitter;

/// 6 px row splitter with a 32×2 grip in `cube_edge` (§3.3.2).
pub(super) fn slot_splitter(theme: &Theme) -> impl Bundle {
    (
        SlotSplitter,
        Node {
            height: px(theme.s.viewport_divider_hit),
            flex_shrink: 0.0,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::top(px(border::HAIRLINE)),
            display: Display::None,
            ..default()
        },
        crate::theme::Stroke(ColorRole::Line),
        splitter_control(Splitter::new(SplitAxis::Rows, 1.0), "Resize slots"),
        children![(
            Node {
                width: px(32),
                height: px(2),
                border_radius: rounded(1.0),
                ..default()
            },
            Fill(ColorRole::CubeEdge),
        )],
    )
}

/// The slots' height: the sidebar body's, less the splitter.
fn slots_height(theme: &Theme, body: Option<&ComputedNode>) -> f32 {
    body.map_or(0.0, |n| n.size().y * n.inverse_scale_factor()) - theme.s.viewport_divider_hit
}

/// Keeps the splitter's size the primary slot's height, and its range
/// both slots' minimum (an even split when they can't both have it).
pub(crate) fn slot_splitter_ranges(
    layout: Res<ShellLayout>,
    theme: Res<Theme>,
    nodes: Query<&ComputedNode>,
    mut splitters: Query<(&ChildOf, &mut Splitter), With<SlotSplitter>>,
) {
    for (body, mut s) in &mut splitters {
        let h = slots_height(&theme, nodes.get(body.parent()).ok()).max(0.0);
        let (min, max) = if h < 2.0 * MIN_SLOT_H {
            (h / 2.0, h / 2.0)
        } else {
            (MIN_SLOT_H, h - MIN_SLOT_H)
        };
        let want = s.with_range(layout.split * h, min, max);
        s.set_if_neq(want);
    }
}

/// A drag or a key sets the primary slot's share.
pub(crate) fn on_slots_resized(
    ev: On<SplitterMoved>,
    splitters: Query<&ChildOf, With<SlotSplitter>>,
    nodes: Query<&ComputedNode>,
    theme: Res<Theme>,
    mut layout: ResMut<ShellLayout>,
) {
    let Ok(body) = splitters.get(ev.entity) else {
        return;
    };
    let h = slots_height(&theme, nodes.get(body.parent()).ok());
    let split = ShellLayout::split_after(0.0, ev.size, h);
    if layout.split != split {
        layout.split = split;
    }
}

/// A double-click puts the slots back to their default split.
pub(crate) fn on_slots_reset(
    ev: On<SplitterReset>,
    splitters: Query<(), With<SlotSplitter>>,
    mut layout: ResMut<ShellLayout>,
) {
    if splitters.contains(ev.entity) && layout.split != DEFAULT_SPLIT {
        layout.split = DEFAULT_SPLIT;
    }
}
