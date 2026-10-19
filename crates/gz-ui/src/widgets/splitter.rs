//! Splitter (§5.18): a 6 px hit area between two panes with a col-resize
//! (or row-resize) cursor and an `accent` line while dragging. Dragging
//! resizes one pane within its min/max.

use accesskit::Role;
use bevy::picking::cursor::EntityCursor;
use bevy::picking::events::{PointerDrag, PointerDragEnd, PointerDragStart};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::ComputedNode;
use bevy::window::SystemCursorIcon;

use super::a11y;
use crate::theme::{ColorRole, Fill};
use crate::GzUiSystems;

/// Which way a splitter moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitAxis {
    /// A vertical bar between side-by-side panes: drags change widths.
    Columns,
    /// A horizontal bar between stacked panes: drags change heights.
    Rows,
}

/// A splitter and the pane it sizes.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Splitter {
    pub axis: SplitAxis,
    /// The pane whose width (or height) the drag sets.
    pub pane: Entity,
    /// +1 when the pane is before the splitter (left/top), −1 after.
    pub sign: f32,
    pub min: f32,
    pub max: f32,
}

/// A drag in progress, from the pane size at its start.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct SplitterDrag {
    pub start: f32,
}

/// The line in the middle of a splitter.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct SplitterLine;

impl Splitter {
    /// The pane size after dragging `delta` px (along the axis) from `start`.
    pub fn size_after(&self, start: f32, delta: Vec2) -> f32 {
        let d = match self.axis {
            SplitAxis::Columns => delta.x,
            SplitAxis::Rows => delta.y,
        };
        (start + self.sign * d).clamp(self.min, self.max)
    }
}

/// A splitter for `pane`. Place it between the two panes.
pub fn splitter(hit: f32, splitter: Splitter) -> impl Bundle {
    let (node, line, cursor) = match splitter.axis {
        SplitAxis::Columns => (
            Node {
                width: px(hit),
                height: Val::Percent(100.0),
                flex_shrink: 0.0,
                justify_content: JustifyContent::Center,
                ..default()
            },
            Node {
                width: px(2),
                height: Val::Percent(100.0),
                ..default()
            },
            SystemCursorIcon::ColResize,
        ),
        SplitAxis::Rows => (
            Node {
                height: px(hit),
                width: Val::Percent(100.0),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                ..default()
            },
            Node {
                height: px(2),
                width: Val::Percent(100.0),
                ..default()
            },
            SystemCursorIcon::RowResize,
        ),
    };
    (
        node,
        splitter,
        Hovered::default(),
        EntityCursor::System(cursor),
        a11y(Role::Splitter, "Resize"),
        children![(SplitterLine, line, Fill(ColorRole::Clear))],
    )
}

fn pane_size(axis: SplitAxis, node: &ComputedNode) -> f32 {
    let size = node.size() * node.inverse_scale_factor();
    match axis {
        SplitAxis::Columns => size.x,
        SplitAxis::Rows => size.y,
    }
}

pub fn on_splitter_drag_start(
    ev: On<PointerDragStart>,
    splitters: Query<&Splitter>,
    panes: Query<&ComputedNode>,
    mut commands: Commands,
) {
    if let Ok(s) = splitters.get(ev.entity) {
        let start = panes.get(s.pane).map_or(s.min, |n| pane_size(s.axis, n));
        commands.entity(ev.entity).insert(SplitterDrag { start });
    }
}

pub fn on_splitter_drag(
    ev: On<PointerDrag>,
    splitters: Query<(&Splitter, &SplitterDrag)>,
    mut panes: Query<&mut Node>,
) {
    if let Ok((s, drag)) = splitters.get(ev.entity) {
        if let Ok(mut node) = panes.get_mut(s.pane) {
            let size = px(s.size_after(drag.start, ev.distance));
            match s.axis {
                SplitAxis::Columns => node.width = size,
                SplitAxis::Rows => node.height = size,
            }
        }
    }
}

pub fn on_splitter_drag_end(ev: On<PointerDragEnd>, mut commands: Commands) {
    if let Ok(mut e) = commands.get_entity(ev.entity) {
        e.remove::<SplitterDrag>();
    }
}

/// `accent` line while dragging.
pub fn splitter_line_system(
    splitters: Query<(&Children, Has<SplitterDrag>), With<Splitter>>,
    mut lines: Query<&mut Fill, With<SplitterLine>>,
) {
    for (children, dragging) in &splitters {
        for &c in children {
            if let Ok(mut fill) = lines.get_mut(c) {
                fill.set_if_neq(Fill(if dragging {
                    ColorRole::Accent
                } else {
                    ColorRole::Clear
                }));
            }
        }
    }
}

pub(crate) fn build(app: &mut App) {
    app.add_observer(on_splitter_drag_start)
        .add_observer(on_splitter_drag)
        .add_observer(on_splitter_drag_end)
        .add_systems(Update, splitter_line_system.in_set(GzUiSystems::Visuals));
}
