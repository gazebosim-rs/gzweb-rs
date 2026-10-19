//! Splitter (§5.18): a `viewport_divider_hit` (6 px) hit area between two
//! panes with a col-resize (or row-resize) cursor, held for the whole drag
//! ([`crate::cursor`]), and a `viewport_divider` `accent` line while
//! dragging. Dragging resizes one pane within its min/max, in UI pixels
//! (the pointer moves in window pixels: [`UiScale`] divides them). It is a
//! tab stop: the arrow keys along its axis move it by [`KEY_STEP`], Home
//! and End to the pane's min and max (WAI-ARIA window splitter).

use accesskit::Role;
use bevy::input::keyboard::KeyboardInput;
use bevy::input::ButtonState;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::FocusedInput;
use bevy::picking::cursor::EntityCursor;
use bevy::picking::events::{PointerDrag, PointerDragEnd, PointerDragStart};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::ComputedNode;
use bevy::window::SystemCursorIcon;

use super::a11y;
use crate::theme::{ColorRole, Fill, FocusRing, Theme, SPACE};
use crate::GzUiSystems;

/// How far an arrow key moves a splitter.
pub const KEY_STEP: f32 = SPACE[4];

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

/// A splitter for `pane`, named `name` for AccessKit ("Resize sidebar").
/// Place it between the two panes.
pub fn splitter(theme: &Theme, splitter: Splitter, name: impl Into<String>) -> impl Bundle {
    let hit = theme.s.viewport_divider_hit;
    let thickness = theme.s.viewport_divider;
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
                width: px(thickness),
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
                height: px(thickness),
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
        TabIndex(0),
        FocusRing,
        a11y(Role::Splitter, name),
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

pub(crate) fn on_splitter_drag_start(
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

/// Sets the pane's width (or height) along `s`'s axis.
fn set_pane_size(s: &Splitter, node: &mut Node, size: f32) {
    match s.axis {
        SplitAxis::Columns => node.width = px(size),
        SplitAxis::Rows => node.height = px(size),
    }
}

pub(crate) fn on_splitter_drag(
    ev: On<PointerDrag>,
    scale: Res<UiScale>,
    splitters: Query<(&Splitter, &SplitterDrag)>,
    mut panes: Query<&mut Node>,
) {
    if let Ok((s, drag)) = splitters.get(ev.entity) {
        if let Ok(mut node) = panes.get_mut(s.pane) {
            let size = s.size_after(drag.start, ev.distance / scale.0);
            set_pane_size(s, &mut node, size);
        }
    }
}

/// Arrow keys along the axis move the splitter; Home and End go to the
/// pane's min and max.
pub(crate) fn on_splitter_key(
    mut ev: On<FocusedInput<KeyboardInput>>,
    splitters: Query<&Splitter>,
    mut panes: Query<(&mut Node, &ComputedNode)>,
) {
    if ev.input.state != ButtonState::Pressed || ev.event_target() != ev.focused_entity {
        return;
    }
    let Ok(s) = splitters.get(ev.focused_entity) else {
        return;
    };
    let Ok((mut node, computed)) = panes.get_mut(s.pane) else {
        return;
    };
    let current = pane_size(s.axis, computed);
    let along = |d: f32| match s.axis {
        SplitAxis::Columns => Vec2::new(d, 0.0),
        SplitAxis::Rows => Vec2::new(0.0, d),
    };
    let size = match (s.axis, ev.input.key_code) {
        (SplitAxis::Columns, KeyCode::ArrowLeft) | (SplitAxis::Rows, KeyCode::ArrowUp) => {
            s.size_after(current, along(-KEY_STEP))
        }
        (SplitAxis::Columns, KeyCode::ArrowRight) | (SplitAxis::Rows, KeyCode::ArrowDown) => {
            s.size_after(current, along(KEY_STEP))
        }
        (_, KeyCode::Home) => s.min,
        (_, KeyCode::End) => s.max,
        _ => return,
    };
    ev.propagate(false);
    set_pane_size(s, &mut node, size);
}

pub(crate) fn on_splitter_drag_end(ev: On<PointerDragEnd>, mut commands: Commands) {
    if let Ok(mut e) = commands.get_entity(ev.entity) {
        e.remove::<SplitterDrag>();
    }
}

/// `accent` line while dragging.
#[allow(clippy::type_complexity)]
pub(crate) fn splitter_line_system(
    started: Query<Entity, Or<(Added<Splitter>, Added<SplitterDrag>)>>,
    mut ended: RemovedComponents<SplitterDrag>,
    splitters: Query<(&Children, Has<SplitterDrag>), With<Splitter>>,
    mut lines: Query<&mut Fill, With<SplitterLine>>,
) {
    for (children, dragging) in started
        .iter()
        .chain(ended.read())
        .filter_map(|e| splitters.get(e).ok())
    {
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
        .add_observer(on_splitter_key)
        .add_systems(Update, splitter_line_system.in_set(GzUiSystems::Visuals));
}
