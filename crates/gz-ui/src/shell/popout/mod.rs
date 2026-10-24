//! Pop-out windows (spec §7.11; DECISIONS.md D-106): a panel popped out of
//! the sidebar floats over the viewport area in a window, one more
//! [`PanelHost`], so the sidebar's [`rebuild_hosts`](super::sidebar::rebuild_hosts)
//! fills it with the panel's header, body and footer like a slot. The
//! window is `surface` with a 1px `line`, radius 12 and the menu's
//! shadow, at least 320×240, dragged by its header, resized at its edges
//! and corners, and kept in the viewport area; its header has a drag grip,
//! the title and count, the panel's actions, Dock and Close. A press in a
//! window brings it to the top. What the windows are, where they sit and
//! their order is [`ShellLayout::popouts`].

use bevy::picking::cursor::EntityCursor;
use bevy::picking::events::{PointerDrag, PointerDragStart, PointerPress};
use bevy::prelude::*;
use bevy::ui::ComputedNode;
use bevy::ui_widgets::Activate;
use bevy::window::SystemCursorIcon;

use super::host::PanelHost;
use super::layout::{Edges, ShellLayout};
use super::regions::pass_through;
use super::sidebar::PanelHeader;
use super::{ShellSystems, UiCommand};
use crate::assets::icons::IconId;
use crate::panels::{PanelId, PanelRegistry};
use crate::theme::{border, elevation, radius, ColorRole, Fill, Stroke, Theme, SPACE};
use crate::widgets::layout::{column, overlay};
use crate::widgets::{icon_button, rounded, IconButtonSize, Tooltip};

/// How far into a window its edges take a resize drag.
const EDGE: f32 = 6.0;

/// The layer the windows sit in, over the views (under the disconnected
/// banner and the connect card).
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct PopOutLayer;

/// A pop-out window's frame: its panel.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PopOutWindow(pub PanelId);

/// The [`PanelHost`] inside a window.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PopOutHost(pub PanelId);

/// A window header's buttons.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WindowAction {
    Dock,
    Close,
}

/// A window's header: dragging it moves the window, from where the drag
/// started.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct HeaderDrag {
    from: Vec2,
}

/// A window's edge or corner: dragging it moves those edges, from the
/// rect the drag started with.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub(crate) struct EdgeDrag {
    edges: Edges,
    from: Rect,
}

/// Panels to pop out at start (`gzweb --pop-out`, for a capture), by
/// name; applied once, then removed.
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct StartPopOuts(pub Vec<String>);

/// Applies [`StartPopOuts`]: each registered panel named pops out, in
/// order; another name is warned about.
pub(crate) fn apply_start_popouts(
    start: Option<Res<StartPopOuts>>,
    panels: Res<PanelRegistry>,
    mut out: MessageWriter<UiCommand>,
    mut commands: Commands,
) {
    let Some(start) = start else {
        return;
    };
    for name in &start.0 {
        match panels.iter().find(|d| d.id.name() == name) {
            Some(def) => {
                out.write(UiCommand::PopOut(def.id));
            }
            None => warn!("--pop-out: no panel '{name}'"),
        }
    }
    commands.remove_resource::<StartPopOuts>();
}

/// The windows' layer.
pub(crate) fn popout_layer() -> impl Bundle {
    (PopOutLayer, overlay(), pass_through())
}

/// A window's frame for `panel`: the host, then the edges over it.
fn window(panel: PanelId, rect: Rect) -> impl Bundle {
    let (shadow, tint) = elevation::MENU.shadow();
    (
        PopOutWindow(panel),
        Node {
            position_type: PositionType::Absolute,
            left: px(rect.min.x),
            top: px(rect.min.y),
            width: px(rect.width()),
            height: px(rect.height()),
            border: UiRect::all(px(border::HAIRLINE)),
            border_radius: rounded(radius::XL),
            overflow: Overflow::clip(),
            ..default()
        },
        Fill(ColorRole::Surface),
        Stroke(ColorRole::Line),
        shadow,
        tint,
        children![
            (
                PopOutHost(panel),
                PanelHost {
                    panel: Some(panel),
                    shown: true,
                },
                Node {
                    flex_grow: 1.0,
                    min_width: px(0),
                    min_height: px(0),
                    overflow: Overflow::clip(),
                    ..column(0.0)
                },
            ),
            edge(Edges {
                left: true,
                ..default()
            }),
            edge(Edges {
                right: true,
                ..default()
            }),
            edge(Edges {
                top: true,
                ..default()
            }),
            edge(Edges {
                bottom: true,
                ..default()
            }),
            edge(Edges {
                left: true,
                top: true,
                ..default()
            }),
            edge(Edges {
                right: true,
                top: true,
                ..default()
            }),
            edge(Edges {
                left: true,
                bottom: true,
                ..default()
            }),
            edge(Edges {
                right: true,
                bottom: true,
                ..default()
            }),
        ],
    )
}

/// An edge's or a corner's handle, [`EDGE`] into the window, with its
/// resize cursor.
fn edge(edges: Edges) -> impl Bundle {
    let corner = (edges.left || edges.right) && (edges.top || edges.bottom);
    let at = |on: bool| if on { px(0) } else { Val::Auto };
    let span = |along: bool| {
        if corner {
            px(EDGE * 2.0)
        } else if along {
            Val::Percent(100.0)
        } else {
            px(EDGE)
        }
    };
    let horizontal = edges.top || edges.bottom;
    let vertical = edges.left || edges.right;
    let cursor = match (edges.left, edges.top, edges.right, edges.bottom) {
        (true, true, _, _) | (_, _, true, true) => SystemCursorIcon::NwseResize,
        (_, true, true, _) | (true, _, _, true) => SystemCursorIcon::NeswResize,
        _ if vertical => SystemCursorIcon::EwResize,
        _ => SystemCursorIcon::NsResize,
    };
    (
        EdgeDrag {
            edges,
            from: Rect::default(),
        },
        Node {
            position_type: PositionType::Absolute,
            left: at(edges.left || (horizontal && !corner)),
            right: at(edges.right),
            top: at(edges.top || (vertical && !corner)),
            bottom: at(edges.bottom),
            width: span(horizontal && !vertical),
            height: span(vertical && !horizontal),
            ..default()
        },
        EntityCursor::System(cursor),
    )
}

/// The windows follow [`ShellLayout::popouts`]: one per panel popped out,
/// where the layout puts it, in its order (the last on top).
pub(crate) fn sync_windows(
    layout: Res<ShellLayout>,
    layers: Query<Entity, With<PopOutLayer>>,
    mut windows: Query<(Entity, &PopOutWindow, &mut Node, Option<&mut ZIndex>)>,
    mut commands: Commands,
) {
    if !layout.is_changed() {
        return;
    }
    let Ok(layer) = layers.single() else {
        return;
    };
    let mut have = vec![];
    for (e, w, mut node, z) in &mut windows {
        let Some(i) = layout.popouts.iter().position(|p| p.panel == w.0) else {
            commands.entity(e).despawn();
            continue;
        };
        have.push(w.0);
        let r = layout.popouts[i].rect;
        let (left, top, width, height) = (px(r.min.x), px(r.min.y), px(r.width()), px(r.height()));
        if (node.left, node.top, node.width, node.height) != (left, top, width, height) {
            (node.left, node.top, node.width, node.height) = (left, top, width, height);
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let want = ZIndex(i as i32);
        match z {
            Some(mut z) if *z != want => *z = want,
            Some(_) => {}
            None => {
                commands.entity(e).insert(want);
            }
        }
    }
    for (i, p) in layout.popouts.iter().enumerate() {
        if !have.contains(&p.panel) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
            let z = ZIndex(i as i32);
            let w = commands.spawn((window(p.panel, p.rect), z)).id();
            commands.entity(layer).add_child(w);
        }
    }
}

/// The viewport area's size goes into the layout, which keeps the windows
/// in it.
pub(crate) fn track_area(
    layers: Query<&ComputedNode, (With<PopOutLayer>, Changed<ComputedNode>)>,
    mut layout: ResMut<ShellLayout>,
) {
    let Ok(node) = layers.single() else {
        return;
    };
    let size = node.size() * node.inverse_scale_factor();
    if size != Vec2::ZERO && (size - layout.area).abs().max_element() > 0.5 {
        layout.set_area(size);
    }
}

/// A window header's parts: the drag grip before the title, Dock and
/// Close after the panel's actions.
pub(crate) fn window_header(commands: &mut Commands, header: Entity, theme: &Theme) {
    let grip = commands.spawn(grip()).id();
    commands
        .entity(header)
        .insert((
            HeaderDrag::default(),
            EntityCursor::System(SystemCursorIcon::Move),
        ))
        .insert_children(0, &[grip]);
    commands.entity(header).with_children(|h| {
        let small = IconButtonSize::Small;
        h.spawn((
            icon_button(
                theme,
                IconId::PopIn,
                small,
                Tooltip::new("Dock").detail("Put this panel back in the sidebar"),
            ),
            WindowAction::Dock,
        ));
        h.spawn((
            icon_button(theme, IconId::Close, small, Tooltip::new("Close")),
            WindowAction::Close,
        ));
    });
}

/// The drag grip: two columns of three dots, `text_3`.
fn grip() -> impl Bundle {
    let dot = || {
        (
            Node {
                width: px(2),
                height: px(2),
                border_radius: rounded(1.0),
                ..default()
            },
            Fill(ColorRole::Text3),
        )
    };
    let col = || (column(SPACE[1]), children![dot(), dot(), dot()]);
    (
        Node {
            column_gap: px(SPACE[1]),
            margin: UiRect::right(px(-SPACE[2])),
            flex_shrink: 0.0,
            ..default()
        },
        Pickable::IGNORE,
        children![col(), col()],
    )
}

/// The panel a window's part belongs to.
fn panel_of(
    e: Entity,
    parents: &Query<&ChildOf>,
    windows: &Query<&PopOutWindow>,
) -> Option<PanelId> {
    std::iter::once(e)
        .chain(parents.iter_ancestors(e))
        .find_map(|a| windows.get(a).ok().map(|w| w.0))
}

/// A press anywhere in a window brings it to the top.
pub(crate) fn on_window_press(
    ev: On<PointerPress>,
    windows: Query<&PopOutWindow>,
    mut out: MessageWriter<UiCommand>,
) {
    if let Ok(w) = windows.get(ev.entity) {
        out.write(UiCommand::RaisePopOut(w.0));
    }
}

/// Dock and Close.
pub(crate) fn on_window_action(
    ev: On<Activate>,
    actions: Query<&WindowAction>,
    parents: Query<&ChildOf>,
    windows: Query<&PopOutWindow>,
    mut out: MessageWriter<UiCommand>,
) {
    let Ok(action) = actions.get(ev.entity) else {
        return;
    };
    let Some(panel) = panel_of(ev.entity, &parents, &windows) else {
        return;
    };
    out.write(match action {
        WindowAction::Dock => UiCommand::Dock(panel),
        WindowAction::Close => UiCommand::ClosePopOut(panel),
    });
}

/// A drag starts on a window's header or an edge: from the window's rect
/// now.
pub(crate) fn on_drag_start(
    ev: On<PointerDragStart>,
    layout: Res<ShellLayout>,
    parents: Query<&ChildOf>,
    windows: Query<&PopOutWindow>,
    mut headers: Query<&mut HeaderDrag>,
    mut edges: Query<&mut EdgeDrag>,
) {
    let Some(panel) = panel_of(ev.entity, &parents, &windows) else {
        return;
    };
    let Some(rect) = layout
        .popouts
        .iter()
        .find(|p| p.panel == panel)
        .map(|p| p.rect)
    else {
        return;
    };
    if let Ok(mut h) = headers.get_mut(ev.entity) {
        h.from = rect.min;
    }
    if let Ok(mut e) = edges.get_mut(ev.entity) {
        e.from = rect;
    }
}

/// A header drag moves its window; an edge's resizes it (in window
/// pixels: [`UiScale`] divides them).
pub(crate) fn on_drag(
    ev: On<PointerDrag>,
    scale: Res<UiScale>,
    parents: Query<&ChildOf>,
    windows: Query<&PopOutWindow>,
    headers: Query<&HeaderDrag, With<PanelHeader>>,
    edges: Query<&EdgeDrag>,
    mut out: MessageWriter<UiCommand>,
) {
    let delta = ev.distance / scale.0;
    if let Ok(h) = headers.get(ev.entity) {
        if let Some(panel) = panel_of(ev.entity, &parents, &windows) {
            out.write(UiCommand::MovePopOut {
                panel,
                at: h.from + delta,
            });
        }
    } else if let Ok(e) = edges.get(ev.entity) {
        if let Some(panel) = panel_of(ev.entity, &parents, &windows) {
            out.write(UiCommand::ResizePopOut {
                panel,
                start: e.from,
                edges: e.edges,
                delta,
            });
        }
    }
}

pub(crate) fn build(app: &mut App) {
    app.add_systems(
        Update,
        apply_start_popouts
            .in_set(ShellSystems::Layout)
            .before(super::commands::apply_ui_commands),
    )
    .add_systems(
        Update,
        (track_area, sync_windows)
            .chain()
            .in_set(ShellSystems::Layout)
            .after(super::regions::apply_layout),
    )
    .add_observer(on_window_press)
    .add_observer(on_window_action)
    .add_observer(on_drag_start)
    .add_observer(on_drag);
}

#[cfg(test)]
mod tests;
