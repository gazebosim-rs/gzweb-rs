//! Layout primitives: the rows, columns, spacers, separators and property
//! groups the shell, the panels and the gallery are built from, so a
//! region says what it is rather than spelling out its flexbox.
//!
//! The node builders return a plain [`Node`]; callers add what is theirs
//! with struct update syntax (`Node { padding, ..row(SPACE[4]) }`). Names
//! follow `bevy_feathers` where it has the same thing (`flex_spacer`,
//! `group`, `group_header`).

use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use bevy::ui::{ComputedNode, UiGlobalTransform};

use crate::assets::fonts::text;
use crate::theme::{border, ColorRole, Fill, Stroke, TypeStyle, SPACE};

/// A row: children left to right, centred across it, `gap` apart.
pub fn row(gap: f32) -> Node {
    Node {
        column_gap: px(gap),
        align_items: AlignItems::Center,
        ..default()
    }
}

/// A column: children top to bottom, `gap` apart, stretched across it.
pub fn column(gap: f32) -> Node {
    Node {
        flex_direction: FlexDirection::Column,
        row_gap: px(gap),
        ..default()
    }
}

/// Takes the room left along its parent's main axis (`bevy_feathers`'
/// name): two of them centre what is between.
pub fn flex_spacer() -> Node {
    Node {
        flex_grow: 1.0,
        ..default()
    }
}

/// The whole of its parent: 100% wide and tall.
pub fn fill_parent() -> Node {
    Node {
        width: Val::Percent(100.0),
        height: Val::Percent(100.0),
        ..default()
    }
}

/// Over the whole of its parent, out of the flow: absolute, every edge 0.
pub fn overlay() -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: px(0),
        right: px(0),
        top: px(0),
        bottom: px(0),
        ..default()
    }
}

/// A box that takes the room left and clips what overflows it, down to
/// nothing (text can't clip itself: put it in one of these).
pub fn clip_box() -> Node {
    Node {
        flex_grow: 1.0,
        flex_basis: px(0),
        min_width: px(0),
        overflow: Overflow::clip(),
        ..default()
    }
}

/// Which way a [`separator`] runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    /// A horizontal rule, between items in a column.
    Horizontal,
    /// A vertical rule, between items in a row.
    Vertical,
}

/// The length of the top bar's vertical separators between its groups
/// (§3.1 item 3: 1×18), for the shell and the gallery's copy of the bar.
pub(crate) const BAR_SEPARATOR_H: f32 = 18.0;

/// A 1px `line` rule `length` long, with `margin` on both sides across it:
/// vertical between the items of a row (the top bar's groups), horizontal
/// between those of a column (the activity bar's groups).
pub fn separator(orientation: Orientation, length: f32, margin: f32) -> impl Bundle {
    let (width, height, margin) = match orientation {
        Orientation::Vertical => (
            px(border::HAIRLINE),
            px(length),
            UiRect::horizontal(px(margin)),
        ),
        Orientation::Horizontal => (
            px(length),
            px(border::HAIRLINE),
            UiRect::vertical(px(margin)),
        ),
    };
    (
        Node {
            width,
            height,
            flex_shrink: 0.0,
            margin,
            ..default()
        },
        Fill(ColorRole::Line),
    )
}

/// `Flex` when shown, `None` when not (a hidden node takes no room).
pub fn display(shown: bool) -> Display {
    if shown {
        Display::Flex
    } else {
        Display::None
    }
}

/// Shows or hides a node, marking it changed only when that changes.
pub fn set_shown(node: &mut Mut<Node>, shown: bool) {
    let want = display(shown);
    if node.display != want {
        node.display = want;
    }
}

/// A laid-out node's rect, in physical pixels.
pub fn node_rect(node: &ComputedNode, transform: &UiGlobalTransform) -> Rect {
    Rect::from_center_size(transform.translation, node.size())
}

/// A property group (spec §3.5 item 5; `bevy_feathers`' name): a column
/// with the inspector's padding and gaps and a `line` rule below it. Spawn
/// a [`group_header`] and [`property_row`]s in it.
pub fn group() -> impl Bundle {
    (
        Node {
            padding: UiRect::axes(px(SPACE[8]), px(SPACE[7])),
            flex_shrink: 0.0,
            border: UiRect::bottom(px(border::HAIRLINE)),
            ..column(SPACE[5])
        },
        Stroke(ColorRole::Line),
    )
}

/// A group's heading (13/600 `text`) and an optional hint at the right in
/// `small` `text_3`.
pub fn group_header(title: impl Into<String>, hint: Option<String>) -> impl Bundle {
    let title = title.into();
    (
        row(0.0),
        Children::spawn(SpawnWith(move |h: &mut ChildSpawner| {
            h.spawn(text(TypeStyle::HEADING, ColorRole::Text, title));
            h.spawn(flex_spacer());
            if let Some(hint) = hint {
                h.spawn(text(TypeStyle::SMALL, ColorRole::Text3, hint));
            }
        })),
    )
}

/// A property row: its label in `row` type `text_2` at the left, `value`
/// at the right.
pub fn property_row<B: Bundle>(label: impl Into<String>, value: B) -> impl Bundle {
    (
        row(0.0),
        children![
            text(TypeStyle::ROW, ColorRole::Text2, label.into()),
            flex_spacer(),
            value
        ],
    )
}
