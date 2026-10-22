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
//!   keys; "Lidar scans / sensor data" waits for the Sensors panel (M7)
//!   and "Apply to all views" for split views (M6): both disabled.
//! - **Layout** (§7.4): the five layout presets with Single current; the
//!   others, Maximize, Close view and the two checkboxes wait for split
//!   views (M6) and are disabled.
//! - The compact rail's drag flyout: Orbit, Pan, Zoom, the current one
//!   marked.

use bevy::ecs::spawn::SpawnWith;
use bevy::picking::hover::Hovered;
use bevy::picking::Pickable;
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Activate;
use gz_bevy::overlays::{Overlay, OverlayCommand, Overlays};
use gz_bevy::view::input::InputMap;
use gz_bevy::view::{CameraMode, Selection, ViewCommand, ViewOp};

use super::{
    compact_trigger, drag_look, rail_menu, rail_tip, CameraModeButton, DragChoice, LayoutButton,
    OverlaysButton, DRAG_ACTIONS,
};
use crate::assets::icons::IconId;
use crate::shell::viewport::MainView;
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

/// A layout preset (§3.4, §7.4).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LayoutPreset {
    Single,
    TwoColumns,
    TwoRows,
    OnePlusTwo,
    Quad,
}

impl LayoutPreset {
    pub(crate) const ALL: [LayoutPreset; 5] = [
        LayoutPreset::Single,
        LayoutPreset::TwoColumns,
        LayoutPreset::TwoRows,
        LayoutPreset::OnePlusTwo,
        LayoutPreset::Quad,
    ];

    pub(crate) fn name(self) -> &'static str {
        match self {
            LayoutPreset::Single => "Single",
            LayoutPreset::TwoColumns => "Two columns",
            LayoutPreset::TwoRows => "Two rows",
            LayoutPreset::OnePlusTwo => "One plus two",
            LayoutPreset::Quad => "Quad",
        }
    }

    /// Its views as (left, top, width, height) in percent.
    pub(crate) fn panes(self) -> &'static [[f32; 4]] {
        match self {
            LayoutPreset::Single => &[[0.0, 0.0, 100.0, 100.0]],
            LayoutPreset::TwoColumns => &[[0.0, 0.0, 60.0, 100.0], [60.0, 0.0, 40.0, 100.0]],
            LayoutPreset::TwoRows => &[[0.0, 0.0, 100.0, 50.0], [0.0, 50.0, 100.0, 50.0]],
            LayoutPreset::OnePlusTwo => &[
                [0.0, 0.0, 50.0, 100.0],
                [50.0, 0.0, 50.0, 50.0],
                [50.0, 50.0, 50.0, 50.0],
            ],
            LayoutPreset::Quad => &[
                [0.0, 0.0, 50.0, 50.0],
                [50.0, 0.0, 50.0, 50.0],
                [0.0, 50.0, 50.0, 50.0],
                [50.0, 50.0, 50.0, 50.0],
            ],
        }
    }
}

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
                InteractionDisabled,
                Tooltip::new("Apply to all views")
                    .detail(NOT_YET)
                    .side(TooltipSide::Left),
            ));
        },
    )
}

/// A layout thumbnail (52 × 36, radius 6): the current one `accent_soft`
/// with `accent_ring`, its views in `accent_ring`; the others on `surface`
/// with `line` views.
fn thumbnail(t: &Theme, preset: LayoutPreset) -> impl Bundle {
    let current = preset == LayoutPreset::Single;
    let (fill, stroke, pane) = if current {
        (
            ColorRole::AccentSoft,
            ColorRole::AccentRing,
            ColorRole::AccentRing,
        )
    } else {
        (ColorRole::Surface, ColorRole::Line, ColorRole::Line)
    };
    let tip = if current {
        Tooltip::new(preset.name()).detail("Current layout")
    } else {
        Tooltip::new(preset.name()).detail(NOT_YET)
    };
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
        Fill(fill),
        Stroke(stroke),
        Hovered::default(),
        tip.side(TooltipSide::Below),
        Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
            p.spawn(Node {
                flex_grow: 1.0,
                ..default()
            })
            .with_children(|area| {
                for [l, top, w, h] in preset.panes() {
                    area.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Percent(*l),
                            top: Val::Percent(*top),
                            width: Val::Percent(*w),
                            height: Val::Percent(*h),
                            padding: UiRect::all(px(1.0)),
                            ..default()
                        },
                        Pickable::IGNORE,
                    ))
                    .with_child((
                        Node {
                            flex_grow: 1.0,
                            border_radius: rounded(2.0),
                            ..default()
                        },
                        Fill(pane),
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
            for (label, binding) in [("Maximize", "view.maximize"), ("Close view", "view.close")] {
                p.spawn((
                    menu_item(
                        &theme,
                        MenuItemProps::new(label).shortcut_opt(keys(binding)),
                    ),
                    InteractionDisabled,
                    Tooltip::new(label).detail(NOT_YET).side(TooltipSide::Left),
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

/// Camera menu choices → the view's camera mode.
pub(crate) fn on_camera_item(
    ev: On<Activate>,
    items: Query<(&CameraItem, Has<InteractionDisabled>)>,
    view: MainView,
    selection: Option<Res<Selection>>,
    mut out: MessageWriter<ViewCommand>,
) {
    let Ok((item, disabled)) = items.get(ev.entity) else {
        return;
    };
    let Some((e, v, ..)) = view.get() else {
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

/// An Overlays checkbox → turns its overlay on or off, from what is on
/// (the checkbox shows that, whatever it toggled itself to).
pub(crate) fn on_overlay_item(
    ev: On<Activate>,
    items: Query<&OverlayItem>,
    overlays: Option<Res<Overlays>>,
    mut out: MessageWriter<OverlayCommand>,
) {
    if let Ok(item) = items.get(ev.entity) {
        let on = overlays.is_some_and(|o| o.is_on(item.0));
        out.write(OverlayCommand::set(item.0, !on));
    }
}
