//! The side splitters (spec §2.1): a 6 px [`Splitter`] centred on a side
//! panel's border that sets its width within its limits and short of the
//! viewport floor, resets it on a double-click, and turns the border
//! `accent` while it drags.

use bevy::prelude::*;

use super::commands::UiCommand;
use super::layout::{ShellLayout, Side};
use super::regions::Region;
use super::ShellSystems;
use crate::theme::{border, ColorRole, Stroke, Theme};
use crate::widgets::splitter::SplitterDrag;
use crate::widgets::{splitter_control, SplitAxis, Splitter, SplitterMoved, SplitterReset};

/// A splitter between a side panel and the viewport area (§2.1): a 6 px
/// hit area centred on the panel's border.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ShellSplitter(pub Side);

/// A side splitter: absolute, 6 px wide, centred on the panel's 1 px
/// border (`right` for the sidebar, `left` for the inspector).
pub(crate) fn side_splitter(theme: &Theme, side: Side) -> impl Bundle {
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
    // The sidebar is before its splitter, the inspector after.
    let (sign, name) = match side {
        Side::Sidebar => (1.0, "Resize sidebar"),
        Side::Inspector => (-1.0, "Resize inspector"),
    };
    (
        node,
        ShellSplitter(side),
        splitter_control(Splitter::new(SplitAxis::Columns, sign), name),
        GlobalZIndex(10),
    )
}

/// Keeps each splitter's width and range the layout's.
pub(crate) fn side_splitter_ranges(
    layout: Res<ShellLayout>,
    mut splitters: Query<(&ShellSplitter, &mut Splitter)>,
) {
    for (side, mut s) in &mut splitters {
        let (min, max) = layout.width_range(side.0);
        let want = s.with_range(layout.width(side.0), min, max);
        s.set_if_neq(want);
    }
}

/// A drag or a key sets the width (the drag is in UI pixels, D-41).
pub(crate) fn on_side_resized(
    ev: On<SplitterMoved>,
    splitters: Query<&ShellSplitter>,
    mut layout: ResMut<ShellLayout>,
) {
    if let Ok(side) = splitters.get(ev.entity) {
        let (min, max) = layout.width_range(side.0);
        if layout.startup || ev.size.clamp(min, max) != layout.width(side.0) {
            layout.resize(side.0, ev.size);
        }
    }
}

/// A double-click resets the width (§2.1).
pub(crate) fn on_side_reset(
    ev: On<SplitterReset>,
    splitters: Query<&ShellSplitter>,
    mut out: MessageWriter<UiCommand>,
) {
    if let Ok(side) = splitters.get(ev.entity) {
        out.write(UiCommand::ResetWidth(side.0));
    }
}

/// The panel's border turns `accent` while its splitter drags (§2.1).
pub(crate) fn splitter_strokes(
    splitters: Query<(&ShellSplitter, Has<SplitterDrag>)>,
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
    app.add_systems(
        Update,
        (side_splitter_ranges, splitter_strokes).in_set(ShellSystems::Readouts),
    )
    .add_observer(on_side_resized)
    .add_observer(on_side_reset);
}
