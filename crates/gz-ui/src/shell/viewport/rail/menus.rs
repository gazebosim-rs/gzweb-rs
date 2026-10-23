//! The rail's menus (spec §7.2–§7.4; board 03), opening left of the rail:
//!
//! - **Camera** (§7.2): Free · Follow ‹selection› ⇧F · Track ‹selection›
//!   ⇧T, the current mode checked; Follow and Track need a selection
//!   (M5) and are disabled without one, saying ⇧F and ⇧T pick the model
//!   under the pointer meanwhile (DECISIONS.md D-60). "Look through" lists
//!   the selection's sensor cameras, so it waits for the selection and
//!   the sensors (M5, M7) and isn't shown. Follow offset… and FOV &
//!   speed… are disabled ("Not available yet").
//! - **Overlays** (§7.3): checkbox items in the spec's order with their
//!   keys, for the rail's view; "Lidar scans / sensor data" waits for the
//!   Sensors panel (M7), disabled; Apply to all views gives every view
//!   this view's set.
//! - **Layout** (§7.4): the five layout presets, the current one marked;
//!   Maximize (Restore while maximized) and Close view, with several
//!   views; Link cameras and Shared selection wait for M6b, disabled.
//! - The compact rail's drag flyout: Orbit, Pan, Zoom, the current one
//!   marked.

use bevy::ecs::spawn::SpawnWith;
use bevy::picking::hover::Hovered;
use bevy::picking::Pickable;
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Activate;
use gz_bevy::overlays::{Overlay, OverlayCommand, OverlaySet, Overlays};
use gz_bevy::view::input::InputMap;
use gz_bevy::view::{CameraMode, Selection, ViewCommand, ViewOp};

use super::{
    compact_trigger, drag_look, rail_menu, rail_tip, CameraModeButton, DragChoice, LayoutButton,
    OverlaysButton, DRAG_ACTIONS,
};
use crate::assets::icons::IconId;
use crate::shell::viewport::views::{LayoutCommand, LayoutPreset, ViewLayout};
use crate::shell::viewport::ViewCameras;
use crate::theme::{border, radius, ColorRole, Fill, Stroke, Theme, SPACE};
use crate::widgets::shortcut::shortcut_label;
use crate::widgets::{
    icon_button, menu_checkbox, menu_item, menu_separator, rounded, IconButtonSize, MenuItemProps,
    Tooltip, TooltipSide,
};

/// What a disabled item's tooltip says (DECISIONS.md D-58).
pub(crate) const NOT_YET: &str = "Not available yet";

/// A Camera menu item.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CameraItem {
    Free,
    Follow,
    Track,
    FollowOffset,
    FovSpeed,
}

/// An Overlays menu checkbox.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OverlayItem(pub Overlay);

/// The compact rail's flyout: Orbit, Pan, Zoom.
pub(crate) fn compact_drag_menu(t: &Theme) -> impl Bundle {
    let theme = t.clone();
    rail_menu(t, compact_trigger(t), move |p: &mut ChildSpawner| {
        for action in DRAG_ACTIONS {
            let (name, id, _) = drag_look(action);
            p.spawn((
                menu_item(&theme, MenuItemProps::new(name).icon(id)),
                DragChoice(action),
            ));
        }
    })
}

/// The Camera menu (§7.2) on the Camera mode button.
pub(crate) fn camera_menu(t: &Theme, map: &InputMap) -> impl Bundle {
    let theme = t.clone();
    let map = map.clone();
    rail_menu(
        t,
        (
            icon_button(
                t,
                IconId::CameraMode,
                IconButtonSize::Rail,
                rail_tip("Camera").detail("Free"),
            ),
            CameraModeButton,
        ),
        move |p: &mut ChildSpawner| {
            let keys = |id| shortcut_label(&map, id);
            p.spawn((
                menu_item(&theme, MenuItemProps::new("Free").icon(IconId::CameraMode)),
                CameraItem::Free,
            ));
            for (item, label, icon, binding) in [
                (CameraItem::Follow, "Follow", IconId::Follow, "view.follow"),
                (CameraItem::Track, "Track", IconId::Track, "view.track"),
            ] {
                p.spawn((
                    menu_item(
                        &theme,
                        MenuItemProps::new(format!("{label} selection"))
                            .icon(icon)
                            .shortcut_opt(keys(binding)),
                    ),
                    item,
                    Tooltip::new(label).side(TooltipSide::Left),
                ));
            }
            p.spawn(menu_separator());
            for (item, label) in [
                (CameraItem::FollowOffset, "Follow offset…"),
                (CameraItem::FovSpeed, "FOV & speed…"),
            ] {
                p.spawn((
                    menu_item(&theme, MenuItemProps::new(label)),
                    item,
                    InteractionDisabled,
                    Tooltip::new(label).detail(NOT_YET).side(TooltipSide::Left),
                ));
            }
        },
    )
}

/// The Overlays menu (§7.3) on the Overlays button.
pub(crate) fn overlays_menu(t: &Theme, map: &InputMap) -> impl Bundle {
    let theme = t.clone();
    let map = map.clone();
    rail_menu(
        t,
        (
            icon_button(
                t,
                IconId::Overlays,
                IconButtonSize::Rail,
                rail_tip("Overlays"),
            ),
            OverlaysButton,
        ),
        move |p: &mut ChildSpawner| {
            for o in Overlay::ALL {
                if o == Overlay::Wireframe {
                    let label = "Lidar scans / sensor data";
                    p.spawn((
                        menu_checkbox(&theme, MenuItemProps::new(label), false),
                        InteractionDisabled,
                        Tooltip::new(label).detail(NOT_YET).side(TooltipSide::Left),
                    ));
                }
                let keys = o.binding().and_then(|b| shortcut_label(&map, b));
                p.spawn((
                    menu_checkbox(
                        &theme,
                        MenuItemProps::new(o.label()).shortcut_opt(keys),
                        false,
                    ),
                    OverlayItem(o),
                ));
            }
            p.spawn(menu_separator());
            p.spawn((
                menu_item(&theme, MenuItemProps::new("Apply to all views")),
                ApplyToAll,
                InteractionDisabled,
                Tooltip::new("Apply to all views")
                    .detail("Every view shows this view's overlays")
                    .side(TooltipSide::Left),
            ));
        },
    )
}

/// A Layout popover item that acts on the layout.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LayoutItem {
    Maximize,
    Close,
}

/// A view in a layout thumbnail.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct ThumbPane;

/// A layout thumbnail (52 × 36, radius 6); [`layout_states`] marks the
/// current one.
fn thumbnail(t: &Theme, preset: LayoutPreset) -> impl Bundle {
    (
        preset,
        Node {
            width: px(t.s.layout_thumb_w),
            height: px(t.s.layout_thumb_h),
            padding: UiRect::all(px(SPACE[1] + 1.0)),
            border: UiRect::all(px(border::HAIRLINE)),
            border_radius: rounded(radius::SM),
            flex_shrink: 0.0,
            ..default()
        },
        Fill(ColorRole::Surface),
        Stroke(ColorRole::Line),
        Hovered::default(),
        bevy::ui_widgets::Button,
        bevy::input_focus::tab_navigation::TabIndex(0),
        crate::widgets::focus::PressFocusless,
        crate::theme::FocusRing,
        bevy::picking::cursor::EntityCursor::System(bevy::window::SystemCursorIcon::Pointer),
        crate::widgets::a11y(accesskit::Role::Button, preset.name()),
        Tooltip::new(preset.name()).side(TooltipSide::Below),
        Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
            p.spawn((
                Node {
                    flex_grow: 1.0,
                    ..default()
                },
                Pickable::IGNORE,
            ))
            .with_children(|area| {
                for [l, top, w, h] in preset.panes() {
                    area.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Percent(l),
                            top: Val::Percent(top),
                            width: Val::Percent(w),
                            height: Val::Percent(h),
                            padding: UiRect::all(px(1.0)),
                            ..default()
                        },
                        Pickable::IGNORE,
                    ))
                    .with_child((
                        ThumbPane,
                        Node {
                            flex_grow: 1.0,
                            border_radius: rounded(2.0),
                            ..default()
                        },
                        Fill(ColorRole::Line),
                        Pickable::IGNORE,
                    ));
                }
            });
        })),
    )
}

/// The Layout popover (§7.4) on the Layout button.
pub(crate) fn layout_popover(t: &Theme, map: &InputMap) -> impl Bundle {
    let theme = t.clone();
    let map = map.clone();
    rail_menu(
        t,
        (
            icon_button(
                t,
                IconId::LayoutSplit,
                IconButtonSize::Rail,
                rail_tip("Layout"),
            ),
            LayoutButton,
        ),
        move |p: &mut ChildSpawner| {
            let keys = |id| shortcut_label(&map, id);
            p.spawn(Node {
                flex_wrap: FlexWrap::Wrap,
                column_gap: px(SPACE[3]),
                row_gap: px(SPACE[3]),
                padding: UiRect::all(px(SPACE[2])),
                width: px(3.0 * theme.s.layout_thumb_w + 2.0 * SPACE[3] + 2.0 * SPACE[2]),
                ..default()
            })
            .with_children(|row| {
                for preset in LayoutPreset::ALL {
                    row.spawn(thumbnail(&theme, preset));
                }
            });
            p.spawn(menu_separator());
            for (item, label, binding) in [
                (LayoutItem::Maximize, "Maximize", "view.maximize"),
                (LayoutItem::Close, "Close view", "view.close"),
            ] {
                p.spawn((
                    menu_item(
                        &theme,
                        MenuItemProps::new(label).shortcut_opt(keys(binding)),
                    ),
                    item,
                    Tooltip::new(label).side(TooltipSide::Left),
                ));
            }
            p.spawn(menu_separator());
            for (label, on) in [("Link cameras", false), ("Shared selection", true)] {
                p.spawn((
                    menu_checkbox(&theme, MenuItemProps::new(label), on),
                    InteractionDisabled,
                    Tooltip::new(label).detail(NOT_YET).side(TooltipSide::Left),
                ));
            }
        },
    )
}

/// The current preset's thumbnail is `accent_soft` with `accent_ring`,
/// its views in `accent_ring`; the others `surface` with `line` views.
/// Maximize reads Restore while a view is maximized, and needs several
/// views; Close view needs several.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(crate) fn layout_states(
    layout: Res<ViewLayout>,
    added: Query<(), Or<(Added<LayoutPreset>, Added<LayoutItem>)>>,
    mut thumbs: Query<(&LayoutPreset, &mut Fill, &mut Stroke, &mut Tooltip), Without<ThumbPane>>,
    mut panes: Query<(&ChildOf, &mut Fill), With<ThumbPane>>,
    parents: Query<&ChildOf, Without<ThumbPane>>,
    mut items: Query<(Entity, &LayoutItem, &Children, Has<InteractionDisabled>)>,
    mut texts: Query<&mut Text>,
    mut commands: Commands,
) {
    if !layout.is_changed() && added.is_empty() {
        return;
    }
    let current = |p: &LayoutPreset| *p == layout.preset;
    for (preset, mut fill, mut stroke, mut tip) in &mut thumbs {
        let on = current(preset);
        fill.set_if_neq(Fill(if on {
            ColorRole::AccentSoft
        } else {
            ColorRole::Surface
        }));
        stroke.set_if_neq(Stroke(if on {
            ColorRole::AccentRing
        } else {
            ColorRole::Line
        }));
        let detail = on.then(|| "Current layout".to_string());
        if tip.detail != detail {
            tip.detail = detail;
        }
    }
    for (parent, mut fill) in &mut panes {
        // Pane → its box → the area → the thumbnail.
        let thumb = parents
            .iter_ancestors(parent.parent())
            .find_map(|e| thumbs.get(e).ok().map(|t| *t.0));
        let on = thumb.is_some_and(|p| current(&p));
        fill.set_if_neq(Fill(if on {
            ColorRole::AccentRing
        } else {
            ColorRole::Line
        }));
    }
    let several = layout.preset.count() > 1 && !layout.fallback;
    for (e, item, children, disabled) in &mut items {
        let (enabled, label) = match item {
            LayoutItem::Maximize if layout.maximized.is_some() => (true, "Restore"),
            LayoutItem::Maximize => (several, "Maximize"),
            LayoutItem::Close => (several && layout.maximized.is_none(), "Close view"),
        };
        if enabled == disabled {
            if enabled {
                commands.entity(e).remove::<InteractionDisabled>();
            } else {
                commands.entity(e).insert(InteractionDisabled);
            }
        }
        for &c in children {
            if let Ok(mut t) = texts.get_mut(c) {
                crate::widgets::text::set_text(&mut t, label);
            }
        }
    }
}

/// A thumbnail picks its preset (a click, or Enter or Space on it).
pub(crate) fn on_thumbnail(
    ev: On<Activate>,
    thumbs: Query<&LayoutPreset>,
    mut out: MessageWriter<LayoutCommand>,
) {
    if let Ok(p) = thumbs.get(ev.entity) {
        out.write(LayoutCommand::Preset(*p));
    }
}

/// Maximize (or Restore) and Close view.
pub(crate) fn on_layout_item(
    ev: On<Activate>,
    items: Query<&LayoutItem, Without<InteractionDisabled>>,
    views: ViewCameras,
    mut out: MessageWriter<LayoutCommand>,
) {
    let Ok(item) = items.get(ev.entity) else {
        return;
    };
    // The view whose rail it is, wherever the menu hangs.
    let view = views.id_of(ev.entity);
    out.write(match item {
        LayoutItem::Maximize => LayoutCommand::Maximize(view),
        LayoutItem::Close => LayoutCommand::Close(view),
    });
}

/// Camera menu choices → the view's camera mode.
pub(crate) fn on_camera_item(
    ev: On<Activate>,
    items: Query<(&CameraItem, Has<InteractionDisabled>)>,
    views: ViewCameras,
    selection: Option<Res<Selection>>,
    mut out: MessageWriter<ViewCommand>,
) {
    let Ok((item, disabled)) = items.get(ev.entity) else {
        return;
    };
    let Some((e, v, ..)) = views.of(ev.entity) else {
        return;
    };
    if disabled {
        return;
    }
    let selected = selection.and_then(|s| s.first_target());
    let mode = match (item, v.mode, selected) {
        (CameraItem::Free, ..) => CameraMode::Free,
        (CameraItem::Follow, CameraMode::Follow(_), _)
        | (CameraItem::Track, CameraMode::Track(_), _) => return,
        (CameraItem::Follow, _, Some(s)) => CameraMode::Follow(s),
        (CameraItem::Track, _, Some(s)) => CameraMode::Track(s),
        _ => return,
    };
    out.write(ViewCommand {
        view: e,
        op: ViewOp::SetMode(mode),
    });
}

/// An Overlays checkbox → turns its overlay on or off in its view, from
/// what is on there (the checkbox shows that, whatever it toggled itself
/// to).
pub(crate) fn on_overlay_item(
    ev: On<Activate>,
    items: Query<&OverlayItem>,
    overlays: Option<Res<Overlays>>,
    views: ViewCameras,
    sets: Query<&OverlaySet>,
    mut out: MessageWriter<OverlayCommand>,
) {
    if let Ok(item) = items.get(ev.entity) {
        let shown = super::systems::view_overlays(&views, &sets, overlays.as_deref(), ev.entity);
        let cmd = OverlayCommand::set(item.0, !shown.is_on(item.0));
        out.write(match views.of(ev.entity) {
            Some((camera, ..)) => cmd.in_view(camera),
            None => cmd,
        });
    }
}

/// The Overlays menu's Apply to all views.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct ApplyToAll;

/// Apply to all views: every view shows what this one does.
pub(crate) fn on_apply_to_all(
    ev: On<Activate>,
    items: Query<(), (With<ApplyToAll>, Without<InteractionDisabled>)>,
    overlays: Option<Res<Overlays>>,
    views: ViewCameras,
    sets: Query<&OverlaySet>,
    mut out: MessageWriter<OverlayCommand>,
) {
    if !items.contains(ev.entity) {
        return;
    }
    let shown = super::systems::view_overlays(&views, &sets, overlays.as_deref(), ev.entity);
    for o in Overlay::ALL {
        out.write(OverlayCommand::set(o, shown.is_on(o)));
    }
}

/// Apply to all views needs another view to apply to.
pub(crate) fn apply_to_all_state(
    layout: Res<ViewLayout>,
    items: Query<(Entity, Has<InteractionDisabled>), With<ApplyToAll>>,
    mut commands: Commands,
) {
    let several = layout.ids().len() > 1;
    for (e, disabled) in &items {
        if several == disabled {
            if several {
                commands.entity(e).remove::<InteractionDisabled>();
            } else {
                commands.entity(e).insert(InteractionDisabled);
            }
        }
    }
}
