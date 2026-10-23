//! The viewport rail (spec §3.4 "Rail", §7.2–§7.4; board 03 "W2 —
//! Viewport rail"; DECISIONS.md D-59): the column of camera controls below
//! the view cube. Top to bottom: **Orbit**, **Pan**, **Zoom** (a radio
//! group: the plain left drag), · **Projection** (its icon shows the
//! current one; 5) · **Camera mode** (the Camera menu; accent fill and dot
//! when not Free; the target chip beside it) · **Focus** (F; double-click
//! goes home, as H) · **Overlays** (the Overlays menu; a badge counts
//! what is on) · **Layout** (the Layout popover). Width 40, padding 4,
//! radius 11, `hud` with a 1px `line` border, gap 2; 32 px buttons with
//! 18 px icons; 22 × 1 `line` separators.
//!
//! **Compact rail:** in a view shorter than `rail_compact_below_view_h`
//! (500 px, UI pixels) Orbit/Pan/Zoom fold into one button showing the
//! current drag action, which opens a three-item flyout.
//!
//! The rail changes the view only (§3.4): its buttons send `gz-bevy`'s
//! [`ViewCommand`](gz_bevy::view::ViewCommand)s and
//! [`OverlayCommand`](gz_bevy::overlays::OverlayCommand)s, and show the
//! state `gz-bevy` keeps ([`systems`]). Every control is a tab stop in the
//! viewport area's tab group (D-58; no group of its own: Bevy 0.20 sorts
//! tab groups globally), with a tooltip to its left naming its shortcut.

pub(crate) mod menus;
pub(crate) mod systems;

use accesskit::Role;
use bevy::ecs::spawn::SpawnWith;
use bevy::picking::Pickable;
use bevy::prelude::*;
use bevy::ui_widgets::observe;
use gz_bevy::view::input::InputMap;
use gz_bevy::view::DragAction;

use super::hud::target_chip;
use crate::assets::icons::IconId;
use crate::theme::visuals::Active;
use crate::theme::{border, ColorRole, Fill, Stroke, Theme, SPACE};
use crate::widgets::badge::{badge_node, badge_parts};
use crate::widgets::layout::{separator, Orientation};
use crate::widgets::menu::{menu, MenuSide};
use crate::widgets::segmented::SegmentedGroup;
use crate::widgets::{a11y, icon_button, rounded, tool_button, BadgeKind, IconButtonSize};
use crate::widgets::{Tooltip, TooltipSide};

/// The rail.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct Rail;

/// The Orbit/Pan/Zoom radio group (the full rail's).
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct DragGroup;

/// The compact rail's drag button's menu.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct CompactDrag;

/// The compact rail's drag button.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct CompactDragButton;

/// A control that picks a drag action: a radio button or a flyout item.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DragChoice(pub DragAction);

/// The rail's plain buttons.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RailButton {
    Projection,
    Focus,
}

/// The Camera mode button (the Camera menu's trigger).
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct CameraModeButton;

/// The dot on the Camera mode button when the camera isn't free.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct CameraModeDot;

/// The Overlays button (the Overlays menu's trigger).
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct OverlaysButton;

/// The Overlays button's count badge.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct OverlaysBadge;

/// The Layout button (the Layout popover's trigger).
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct LayoutButton;

/// A drag action's name, icon and what it does.
pub(crate) fn drag_look(action: DragAction) -> (&'static str, IconId, &'static str) {
    match action {
        DragAction::Orbit => ("Orbit", IconId::CamOrbit, "Left drag orbits"),
        DragAction::Pan => ("Pan", IconId::CamPan, "Left drag pans"),
        DragAction::Zoom => ("Zoom", IconId::CamZoom, "Left drag zooms"),
    }
}

/// The drag actions in the rail's order.
pub(crate) const DRAG_ACTIONS: [DragAction; 3] =
    [DragAction::Orbit, DragAction::Pan, DragAction::Zoom];

/// A rail tooltip: to the left of the rail (§5.15).
pub(crate) fn rail_tip(title: &str) -> Tooltip {
    Tooltip::new(title).side(TooltipSide::Left)
}

/// The rail's separators: 22 × 1 `line`, margin 3 0 (§3.4).
fn rail_separator() -> impl Bundle {
    separator(Orientation::Horizontal, 22.0, 3.0)
}

/// The rail (§3.4), below the cube at the view's top right.
pub(crate) fn rail(theme: &Theme, map: &InputMap) -> impl Bundle {
    let s = &theme.s;
    let t = theme.clone();
    let map = map.clone();
    (
        Rail,
        Name::new("viewport rail"),
        Node {
            position_type: PositionType::Absolute,
            top: px(s.viewport_inset + s.view_cube + s.rail_gap_below_cube),
            right: px(s.viewport_inset),
            width: px(s.rail_w),
            padding: UiRect::all(px(RAIL_PADDING)),
            border: UiRect::all(px(border::HAIRLINE)),
            border_radius: rounded(RAIL_RADIUS),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(SPACE[1]),
            ..default()
        },
        Fill(ColorRole::Hud),
        Stroke(ColorRole::Line),
        a11y(Role::Toolbar, "Camera"),
        Children::spawn(SpawnWith(move |r: &mut ChildSpawner| {
            spawn_rail(r, &t, &map);
        })),
    )
}

/// The rail's corner radius (§3.4: 11).
pub(crate) const RAIL_RADIUS: f32 = 11.0;
/// The rail's padding inside its border: §3.4's 40 wide with 32 px
/// buttons leaves 4 a side, the 1px border included (DECISIONS.md D-59).
pub(crate) const RAIL_PADDING: f32 = 3.0;

fn spawn_rail(r: &mut ChildSpawner, t: &Theme, map: &InputMap) {
    // Orbit / Pan / Zoom: the full rail's radio group …
    r.spawn((
        DragGroup,
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(SPACE[1]),
            ..default()
        },
        SegmentedGroup { radio: true },
        a11y(Role::RadioGroup, "Left drag"),
        observe(systems::on_drag_change),
    ))
    .with_children(|g| {
        for action in DRAG_ACTIONS {
            let (name, id, detail) = drag_look(action);
            g.spawn((
                tool_button(
                    t,
                    id,
                    IconButtonSize::Rail,
                    rail_tip(name).detail(detail),
                    action == DragAction::Orbit,
                ),
                DragChoice(action),
            ));
        }
    });
    // … and the compact rail's flyout.
    r.spawn((CompactDrag, menus::compact_drag_menu(t)))
        .insert(Node {
            flex_shrink: 0.0,
            display: Display::None,
            ..default()
        });
    r.spawn(rail_separator());
    r.spawn((
        icon_button(
            t,
            IconId::ProjPerspective,
            IconButtonSize::Rail,
            rail_tip("Perspective").binding(map, "view.projection"),
        ),
        RailButton::Projection,
    ));
    r.spawn(rail_separator());
    let dot = t.s.status_dot;
    r.spawn(menus::camera_menu(t, map))
        .with_child((
            CameraModeDot,
            Node {
                position_type: PositionType::Absolute,
                top: px(SPACE[2] - 1.0),
                right: px(SPACE[2] - 1.0),
                width: px(dot),
                height: px(dot),
                border_radius: rounded(dot / 2.0),
                display: Display::None,
                ..default()
            },
            Fill(ColorRole::Accent),
            Pickable::IGNORE,
        ))
        .with_child(target_chip(t, map));
    r.spawn((
        icon_button(
            t,
            IconId::Focus,
            IconButtonSize::Rail,
            rail_tip("Focus")
                .binding(map, "view.focus")
                .detail("Double-click: home"),
        ),
        RailButton::Focus,
    ));
    let mut badge = badge_node(t);
    badge.position_type = PositionType::Absolute;
    badge.top = px(0);
    badge.right = px(0);
    badge.display = Display::None;
    r.spawn(menus::overlays_menu(t, map)).with_child((
        OverlaysBadge,
        badge,
        badge_parts(BadgeKind::Count, 0),
        Pickable::IGNORE,
    ));
    r.spawn(rail_separator());
    r.spawn(menus::layout_popover(t, map));
}

/// The compact flyout's trigger: the current drag action, on.
pub(crate) fn compact_trigger(t: &Theme) -> impl Bundle {
    let (name, id, detail) = drag_look(DragAction::Orbit);
    (
        icon_button(t, id, IconButtonSize::Rail, rail_tip(name).detail(detail)),
        CompactDragButton,
        Active,
    )
}

/// The trigger of a rail menu: a rail icon button opening `side` of it.
pub(crate) fn rail_menu<T: Bundle, F>(t: &Theme, trigger: T, items: F) -> impl Bundle
where
    F: FnOnce(&mut ChildSpawner) + Send + Sync + 'static,
{
    menu(t, MenuSide::Left, trigger, items)
}

pub(crate) fn build(app: &mut App) {
    app.add_systems(
        Update,
        (
            systems::drag_states,
            systems::compact_rail,
            systems::projection_state,
            systems::camera_state,
            systems::overlay_states,
            menus::layout_states,
        )
            .in_set(crate::shell::ShellSystems::Readouts),
    )
    .add_observer(menus::on_thumbnail)
    .add_observer(menus::on_layout_item)
    .add_observer(systems::on_rail_button)
    .add_observer(systems::on_drag_item)
    .add_observer(menus::on_camera_item)
    .add_observer(menus::on_overlay_item);
}

#[cfg(test)]
mod tests;
