//! Mouse cursors (spec §5.18, §5.8). Widgets name their hover cursor with
//! Bevy's `EntityCursor` (col-/row-resize on splitters, ew-resize on a
//! number field's tag, a text beam on inputs); Bevy's `CursorIconPlugin`,
//! which [`GzUiPlugin`](crate::GzUiPlugin) adds, shows it. A mouse drag
//! keeps the cursor it started with until it ends, through
//! `OverrideCursor`: a splitter's hit area is 6 px wide, and the pointer
//! leaves it as soon as the drag moves faster than the pane.

use bevy::picking::cursor::{DefaultCursor, EntityCursor, OverrideCursor};
use bevy::picking::events::{PointerDragEnd, PointerDragStart};
use bevy::picking::pointer::PointerId;
use bevy::prelude::*;

/// The cursor an entity shows: its own `EntityCursor` or its nearest
/// ancestor's, else the default (as Bevy's hover cursor picks it).
fn cursor_of(
    entity: Entity,
    cursors: &Query<&EntityCursor>,
    parents: &Query<&ChildOf>,
    default: &DefaultCursor,
) -> EntityCursor {
    std::iter::once(entity)
        .chain(parents.iter_ancestors(entity))
        .find_map(|e| cursors.get(e).ok())
        .unwrap_or(&default.0)
        .clone()
}

/// A mouse drag starts: hold the cursor of what it started on.
pub(crate) fn hold_cursor_on_drag(
    ev: On<PointerDragStart>,
    cursors: Query<&EntityCursor>,
    parents: Query<&ChildOf>,
    default: Res<DefaultCursor>,
    mut hold: ResMut<OverrideCursor>,
) {
    // Once per drag: at the entity it started on, not again as it bubbles.
    if ev.pointer.id == PointerId::Mouse && ev.event_target() == ev.original_event_target() {
        hold.0 = Some(cursor_of(ev.entity, &cursors, &parents, &default));
    }
}

/// The drag ends: the cursor follows hover again.
pub(crate) fn release_cursor_on_drag_end(ev: On<PointerDragEnd>, mut hold: ResMut<OverrideCursor>) {
    if ev.pointer.id == PointerId::Mouse && ev.event_target() == ev.original_event_target() {
        hold.0 = None;
    }
}

pub(crate) fn build(app: &mut App) {
    if !app.is_plugin_added::<bevy::picking::cursor::CursorIconPlugin>() {
        app.add_plugins(bevy::picking::cursor::CursorIconPlugin);
    }
    app.add_observer(hold_cursor_on_drag)
        .add_observer(release_cursor_on_drag_end);
}
