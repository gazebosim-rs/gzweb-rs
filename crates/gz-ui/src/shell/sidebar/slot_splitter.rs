//! The row splitter between the sidebar's slots (spec §3.3.2): 6 px with
//! a 32×2 grip, dragging the primary slot's share with both slots at least
//! [`MIN_SLOT_H`](crate::shell::layout::MIN_SLOT_H) tall.

use bevy::picking::events::{PointerDrag, PointerDragEnd, PointerDragStart};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::ComputedNode;
use bevy::window::SystemCursorIcon;

use crate::shell::ShellLayout;
use crate::theme::{border, ColorRole, Fill, Theme};
use crate::widgets::rounded;

/// The row splitter between the slots.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct SlotSplitter;

/// A row-splitter drag: the split and the sidebar's height at its start.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub(crate) struct SlotDrag {
    pub split: f32,
    pub height: f32,
}

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
        Hovered::default(),
        bevy::picking::cursor::EntityCursor::System(SystemCursorIcon::RowResize),
        crate::widgets::a11y(accesskit::Role::Splitter, "Resize slots"),
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

pub(crate) fn on_slot_drag_start(
    ev: On<PointerDragStart>,
    splitters: Query<&ChildOf, With<SlotSplitter>>,
    nodes: Query<&ComputedNode>,
    layout: Res<ShellLayout>,
    mut commands: Commands,
) {
    if let Ok(parent) = splitters.get(ev.entity) {
        let height = nodes
            .get(parent.parent())
            .map_or(0.0, |n| n.size().y * n.inverse_scale_factor());
        commands.entity(ev.entity).insert(SlotDrag {
            split: layout.split,
            height,
        });
    }
}

pub(crate) fn on_slot_drag(
    ev: On<PointerDrag>,
    drags: Query<&SlotDrag>,
    theme: Res<Theme>,
    scale: Res<UiScale>,
    mut layout: ResMut<ShellLayout>,
) {
    if let Ok(d) = drags.get(ev.entity) {
        // The slots share the height less the splitter; the pointer moves
        // in window pixels, the height is in UI pixels.
        let h = d.height - theme.s.viewport_divider_hit;
        let split = ShellLayout::split_after(d.split, ev.distance.y / scale.0, h);
        if layout.split != split {
            layout.split = split;
        }
    }
}

pub(crate) fn on_slot_drag_end(
    ev: On<PointerDragEnd>,
    drags: Query<(), With<SlotDrag>>,
    mut commands: Commands,
) {
    if drags.contains(ev.entity) {
        commands.entity(ev.entity).remove::<SlotDrag>();
    }
}
