//! Splitter (§5.18): a `viewport_divider_hit` (6 px) hit area between two
//! panes with a col-resize (or row-resize) cursor, held for the whole drag
//! ([`crate::widgets::cursor`]), and a `viewport_divider` `accent` line while
//! dragging. It is controlled: its owner keeps the pane's size and range
//! in the [`Splitter`], and answers a [`SplitterMoved`] (a drag, or a key)
//! by applying the size, and a [`SplitterReset`] (a primary double-click)
//! by going back to its default. Drags are in UI pixels (the pointer
//! moves in window pixels: [`UiScale`] divides them). It is a tab stop:
//! the arrow keys along its axis move it by [`KEY_STEP`], Home and End to
//! the pane's min and max (WAI-ARIA window splitter).

use accesskit::Role;
use bevy::input::keyboard::KeyboardInput;
use bevy::input::ButtonState;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::FocusedInput;
use bevy::picking::cursor::EntityCursor;
use bevy::picking::events::{PointerClick, PointerDrag, PointerDragEnd, PointerDragStart};
use bevy::picking::hover::Hovered;
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::window::SystemCursorIcon;

use super::a11y;
use crate::theme::{ColorRole, Fill, FocusRing, Theme, SPACE};
use crate::GzUiSystems;

/// How far an arrow key moves a splitter.
pub(crate) const KEY_STEP: f32 = SPACE[4];

/// Which way a splitter moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitAxis {
    /// A vertical bar between side-by-side panes: drags change widths.
    Columns,
    /// A horizontal bar between stacked panes: drags change heights.
    Rows,
}

/// A splitter and the pane it sizes. The owner keeps `size`, `min` and
/// `max` current ([`Splitter::with_range`]).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Splitter {
    pub axis: SplitAxis,
    /// +1 when the pane is before the splitter (left/top), −1 after.
    pub sign: f32,
    /// The pane's width (or height) now, in UI pixels.
    pub size: f32,
    pub min: f32,
    pub max: f32,
}

/// The size a splitter asks for, from a drag or a key: its owner applies
/// it (and the size it gave back with [`Splitter::with_range`]).
#[derive(EntityEvent, Clone, Copy, Debug, PartialEq)]
pub struct SplitterMoved {
    /// The splitter.
    pub entity: Entity,
    pub size: f32,
}

/// A primary double-click on a splitter, neither click ending a drag: its
/// owner puts the pane back to its default size.
#[derive(EntityEvent, Clone, Copy, Debug, PartialEq, Eq)]
pub struct SplitterReset {
    /// The splitter.
    pub entity: Entity,
}

/// A drag in progress, from the pane size at its start. Owners that draw
/// their own line read it.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub(crate) struct SplitterDrag {
    pub start: f32,
}

/// Whether a splitter's last click ended a drag: bevy_picking sends a click
/// on every release, drags included, and counts on across them, so a drag
/// followed by a quick grab would read as a double-click.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct LastClickDragged(pub bool);

/// The line in the middle of a splitter.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct SplitterLine;

impl Splitter {
    /// A splitter along `axis`; `sign` as [`Splitter::sign`]. Its range is
    /// empty until the owner sets it.
    pub fn new(axis: SplitAxis, sign: f32) -> Self {
        Self {
            axis,
            sign,
            size: 0.0,
            min: 0.0,
            max: 0.0,
        }
    }

    /// This splitter with the pane's size and range: the owner writes it
    /// back (`set_if_neq`) as the pane or its room changes. A range too
    /// small for `min` is just `min`.
    pub fn with_range(self, size: f32, min: f32, max: f32) -> Self {
        Self {
            size,
            min,
            max: max.max(min),
            ..self
        }
    }

    /// `size` within the range.
    fn clamp(&self, size: f32) -> f32 {
        size.clamp(self.min, self.max.max(self.min))
    }

    /// The pane size after dragging `delta` px (along the axis) from `start`.
    pub fn size_after(&self, start: f32, delta: Vec2) -> f32 {
        let d = match self.axis {
            SplitAxis::Columns => delta.x,
            SplitAxis::Rows => delta.y,
        };
        self.clamp(start + self.sign * d)
    }
}

/// What makes a node a splitter for its owner: [`Splitter`], its cursor,
/// tab stop and focus ring, named `name` for AccessKit ("Resize
/// sidebar"). The owner gives it a node (its hit area) and its looks.
pub fn splitter_control(splitter: Splitter, name: impl Into<String>) -> impl Bundle {
    let cursor = match splitter.axis {
        SplitAxis::Columns => SystemCursorIcon::ColResize,
        SplitAxis::Rows => SystemCursorIcon::RowResize,
    };
    (
        splitter,
        LastClickDragged::default(),
        Hovered::default(),
        EntityCursor::System(cursor),
        TabIndex(0),
        FocusRing,
        a11y(Role::Splitter, name),
    )
}

/// A splitter with its own looks: 6 px in the flow with the line in its
/// middle. Place it between the two panes.
pub fn splitter(theme: &Theme, splitter: Splitter, name: impl Into<String>) -> impl Bundle {
    let hit = theme.s.viewport_divider_hit;
    let thickness = theme.s.viewport_divider;
    let (node, line) = match splitter.axis {
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
        ),
    };
    (
        node,
        splitter_control(splitter, name),
        children![(SplitterLine, line, Fill(ColorRole::Clear))],
    )
}

pub(crate) fn on_splitter_drag_start(
    ev: On<PointerDragStart>,
    splitters: Query<&Splitter>,
    mut commands: Commands,
) {
    if let Ok(s) = splitters.get(ev.entity) {
        commands
            .entity(ev.entity)
            .insert(SplitterDrag { start: s.size });
    }
}

pub(crate) fn on_splitter_drag(
    ev: On<PointerDrag>,
    scale: Res<UiScale>,
    splitters: Query<(&Splitter, &SplitterDrag)>,
    mut commands: Commands,
) {
    if let Ok((s, drag)) = splitters.get(ev.entity) {
        // Asked for even when it looks the same: `size` is as of the
        // owner's last update, and a drag can move twice in a frame.
        let size = s.size_after(drag.start, ev.distance / scale.0);
        commands.trigger(SplitterMoved {
            entity: ev.entity,
            size,
        });
    }
}

/// Arrow keys along the axis move the splitter; Home and End go to the
/// pane's min and max.
pub(crate) fn on_splitter_key(
    mut ev: On<FocusedInput<KeyboardInput>>,
    splitters: Query<&Splitter>,
    mut commands: Commands,
) {
    if ev.input.state != ButtonState::Pressed || ev.event_target() != ev.focused_entity {
        return;
    }
    let Ok(s) = splitters.get(ev.focused_entity) else {
        return;
    };
    let along = |d: f32| match s.axis {
        SplitAxis::Columns => Vec2::new(d, 0.0),
        SplitAxis::Rows => Vec2::new(0.0, d),
    };
    let size = match (s.axis, ev.input.key_code) {
        (SplitAxis::Columns, KeyCode::ArrowLeft) | (SplitAxis::Rows, KeyCode::ArrowUp) => {
            s.size_after(s.size, along(-KEY_STEP))
        }
        (SplitAxis::Columns, KeyCode::ArrowRight) | (SplitAxis::Rows, KeyCode::ArrowDown) => {
            s.size_after(s.size, along(KEY_STEP))
        }
        (_, KeyCode::Home) => s.min,
        (_, KeyCode::End) => s.max.max(s.min),
        _ => return,
    };
    ev.propagate(false);
    let entity = ev.focused_entity;
    commands.trigger(SplitterMoved { entity, size });
}

pub(crate) fn on_splitter_drag_end(
    ev: On<PointerDragEnd>,
    splitters: Query<(), With<SplitterDrag>>,
    mut commands: Commands,
) {
    if splitters.contains(ev.entity) {
        commands.entity(ev.entity).remove::<SplitterDrag>();
    }
}

/// A primary double-click asks for a reset: two clicks in a row, neither
/// of them the end of a drag (a release after a drag is a click too,
/// still [`SplitterDrag`] then).
pub(crate) fn on_splitter_click(
    ev: On<PointerClick>,
    mut splitters: Query<(&mut LastClickDragged, Has<SplitterDrag>), With<Splitter>>,
    mut commands: Commands,
) {
    let Ok((mut last, dragged)) = splitters.get_mut(ev.entity) else {
        return;
    };
    let after_drag = std::mem::replace(&mut last.0, dragged);
    if ev.button == PointerButton::Primary && ev.count >= 2 && !dragged && !after_drag {
        commands.trigger(SplitterReset { entity: ev.entity });
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
        .add_observer(on_splitter_click)
        .add_systems(Update, splitter_line_system.in_set(GzUiSystems::Visuals));
}

#[cfg(test)]
mod tests;
