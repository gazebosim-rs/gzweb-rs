//! The side splitters (spec §2.1): a 6 px hit area centred on a side
//! panel's border that drags its width within its limits and short of the
//! viewport floor, resets it on a double-click, and turns the border
//! `accent` while it drags.

use bevy::picking::events::{PointerClick, PointerDrag, PointerDragEnd, PointerDragStart};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::window::SystemCursorIcon;

use super::commands::UiCommand;
use super::layout::{ShellLayout, Side};
use super::regions::Region;
use crate::theme::{border, ColorRole, Stroke, Theme};

/// A splitter between a side panel and the viewport area (§2.1): a 6 px
/// hit area centred on the panel's border.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShellSplitter(pub Side);

/// A splitter drag in progress: the width it started from.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct SplitDrag(pub f32);

/// A side splitter: absolute, 6 px wide, centred on the panel's 1 px
/// border (`right` for the sidebar, `left` for the inspector).
pub fn side_splitter(theme: &Theme, side: Side) -> impl Bundle {
    let hit = theme.s.viewport_divider_hit;
    // Absolute offsets are from the padding box, inside the border: centre
    // the hit area on the border pixel.
    let offset = px(-(hit + border::HAIRLINE) / 2.0);
    let mut node = Node {
        position_type: PositionType::Absolute,
        top: px(0),
        bottom: px(0),
        width: px(hit),
        ..default()
    };
    match side {
        Side::Sidebar => node.right = offset,
        Side::Inspector => node.left = offset,
    }
    (
        node,
        ShellSplitter(side),
        Hovered::default(),
        GlobalZIndex(10),
        bevy::picking::cursor::EntityCursor::System(SystemCursorIcon::ColResize),
        crate::widgets::a11y(
            accesskit::Role::Splitter,
            match side {
                Side::Sidebar => "Resize sidebar",
                Side::Inspector => "Resize inspector",
            },
        ),
    )
}

pub fn on_splitter_drag_start(
    ev: On<PointerDragStart>,
    splitters: Query<&ShellSplitter>,
    layout: Res<ShellLayout>,
    mut commands: Commands,
) {
    if let Ok(s) = splitters.get(ev.entity) {
        commands
            .entity(ev.entity)
            .insert(SplitDrag(layout.width(s.0)));
    }
}

/// The pointer moves in window pixels and widths are UI pixels: the drag
/// divides by `UiScale` (DECISIONS.md D-41).
pub fn on_splitter_drag(
    ev: On<PointerDrag>,
    scale: Res<UiScale>,
    splitters: Query<(&ShellSplitter, &SplitDrag)>,
    mut layout: ResMut<ShellLayout>,
) {
    if let Ok((s, drag)) = splitters.get(ev.entity) {
        layout.drag(s.0, drag.0, ev.distance.x / scale.0);
    }
}

pub fn on_splitter_drag_end(
    ev: On<PointerDragEnd>,
    splitters: Query<(), With<ShellSplitter>>,
    mut commands: Commands,
) {
    if splitters.contains(ev.entity) {
        commands.entity(ev.entity).remove::<SplitDrag>();
    }
}

/// A double-click resets the width (§2.1).
pub fn on_splitter_click(
    ev: On<PointerClick>,
    splitters: Query<&ShellSplitter>,
    mut out: MessageWriter<UiCommand>,
) {
    if let Ok(s) = splitters.get(ev.entity) {
        if ev.count >= 2 {
            out.write(UiCommand::ResetWidth(s.0));
        }
    }
}

/// The panel's border turns `accent` while its splitter drags (§2.1).
pub fn splitter_strokes(
    splitters: Query<(&ShellSplitter, Has<SplitDrag>)>,
    mut regions: Query<(&Region, &mut Stroke)>,
) {
    for (s, dragging) in &splitters {
        let want = Stroke(if dragging {
            ColorRole::Accent
        } else {
            ColorRole::Line
        });
        let region = match s.0 {
            Side::Sidebar => Region::Sidebar,
            Side::Inspector => Region::Inspector,
        };
        for (r, mut stroke) in &mut regions {
            if *r == region {
                stroke.set_if_neq(want);
            }
        }
    }
}

pub(crate) fn build(app: &mut App) {
    app.add_observer(on_splitter_drag_start)
        .add_observer(on_splitter_drag)
        .add_observer(on_splitter_drag_end)
        .add_observer(on_splitter_click);
}
