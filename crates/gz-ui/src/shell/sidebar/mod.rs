//! The sidebar (spec §3.3): a primary and an optional secondary slot with a
//! row splitter between them (§3.3.2), each holding one panel's shell: a
//! section header with its title, count and slot buttons, and a body in
//! its empty, loading or error state (§3.3.1). The panels' contents come
//! in M4 and M7.
pub mod body;
pub mod slot_splitter;

use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use gz_bevy::spawn::GzIndex;
use gz_bevy::view::input::InputMap;

use super::commands::UiCommand;
use super::connection::CurrentPhase;
use super::layout::DEFAULT_SPLIT;
use super::regions::{region, Region};
use super::splitter::side_splitter;
use super::{Panel, ShellLayout, Side, Slot};
use crate::assets::icons::IconId;
use crate::theme::{border, ColorRole, Theme};
use crate::widgets::menu::shortcut_label;
use crate::widgets::{icon_button, section_header, IconButtonSize, Tooltip};
use body::panel_body;
pub use body::{panel_count, panel_view, PanelAction, PanelBody, PanelView, SkeletonRow};
pub use slot_splitter::{SlotDrag, SlotSplitter};

/// A sidebar slot's node.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotView(pub Slot);

/// A panel header's slot buttons (§3.3.2).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotAction {
    /// With two slots: the panels swap.
    Swap,
    /// The primary header's chevron: collapse the sidebar.
    Collapse,
    /// The secondary header's close: back to one slot.
    Close,
}

/// The sidebar region.
pub fn sidebar(theme: &Theme, layout: &ShellLayout) -> impl Bundle {
    let t = theme.clone();
    (
        region(
            Region::Sidebar,
            Node {
                width: px(layout.sidebar_w),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                border: UiRect::right(px(border::HAIRLINE)),
                display: if layout.sidebar_open {
                    Display::Flex
                } else {
                    Display::None
                },
                ..default()
            },
            ColorRole::Surface,
        ),
        Children::spawn(SpawnWith(move |s: &mut ChildSpawner| {
            s.spawn(slot_node(Slot::Primary));
            s.spawn(slot_splitter::slot_splitter(&t));
            s.spawn(slot_node(Slot::Secondary));
            s.spawn(side_splitter(&t, Side::Sidebar));
        })),
    )
}

fn slot_node(slot: Slot) -> impl Bundle {
    (
        SlotView(slot),
        Node {
            flex_direction: FlexDirection::Column,
            flex_basis: px(0),
            flex_grow: match slot {
                Slot::Primary => DEFAULT_SPLIT,
                Slot::Secondary => 1.0 - DEFAULT_SPLIT,
            },
            min_height: px(0),
            overflow: Overflow::clip(),
            ..default()
        },
    )
}

/// What a slot shows; rebuilt when it changes.
#[derive(Clone, Debug, PartialEq)]
pub struct SlotContent {
    pub panel: Option<Panel>,
    pub view: Option<PanelView>,
    pub count: Option<String>,
    pub two: bool,
}

/// Rebuilds a slot's header and body when its panel, state or count
/// changes.
#[allow(clippy::too_many_arguments)]
pub fn rebuild_slots(
    layout: Res<ShellLayout>,
    phase: Res<CurrentPhase>,
    index: Option<Res<GzIndex>>,
    theme: Res<Theme>,
    map: Option<Res<InputMap>>,
    slots: Query<(Entity, &SlotView)>,
    mut shown: Local<[Option<SlotContent>; 2]>,
    mut commands: Commands,
) {
    let two = layout.secondary.is_some();
    for (e, slot) in &slots {
        let (panel, i) = match slot.0 {
            Slot::Primary => (layout.primary, 0),
            Slot::Secondary => (layout.secondary, 1),
        };
        let want = SlotContent {
            panel,
            view: panel.map(|p| panel_view(p, phase.0)),
            count: panel.and_then(|p| panel_count(p, phase.0, index.as_deref())),
            two,
        };
        if shown[i].as_ref() == Some(&want) {
            continue;
        }
        commands.entity(e).despawn_children();
        if let (Some(panel), Some(view)) = (want.panel, want.view.clone()) {
            let t = theme.clone();
            let map = map.as_deref().cloned().unwrap_or_else(InputMap::builtin);
            let count = want.count.clone();
            let which = slot.0;
            commands.entity(e).with_children(|p| {
                p.spawn(section_header(&t, panel.title(), count))
                    .with_children(|h| header_buttons(h, &t, &map, which, two));
                p.spawn(panel_body(&t, panel, view));
            });
        }
        shown[i] = Some(want);
    }
}

fn header_buttons(
    h: &mut ChildSpawnerCommands,
    theme: &Theme,
    map: &InputMap,
    slot: Slot,
    two: bool,
) {
    let small = IconButtonSize::Small;
    if two {
        h.spawn((
            icon_button(
                theme,
                IconId::MoveTo,
                small,
                Tooltip::new("Swap slots").detail("Move this panel to the other slot"),
            ),
            SlotAction::Swap,
        ));
    }
    match slot {
        Slot::Primary => {
            h.spawn((
                icon_button(
                    theme,
                    IconId::ChevronLeft,
                    small,
                    Tooltip::new("Collapse sidebar")
                        .shortcut_opt(shortcut_label(map, "ui.sidebar")),
                ),
                SlotAction::Collapse,
            ));
        }
        Slot::Secondary => {
            h.spawn((
                icon_button(
                    theme,
                    IconId::Close,
                    small,
                    Tooltip::new("Close second slot"),
                ),
                SlotAction::Close,
            ));
        }
    }
}

/// The panel headers' slot buttons.
pub fn on_slot_action(
    ev: On<Activate>,
    actions: Query<&SlotAction>,
    mut out: MessageWriter<UiCommand>,
) {
    if let Ok(action) = actions.get(ev.entity) {
        let cmd = match action {
            SlotAction::Swap => UiCommand::SwapSlots,
            SlotAction::Collapse => UiCommand::ToggleSidebar,
            SlotAction::Close => UiCommand::CloseSecondary,
        };
        out.write(cmd);
    }
}

pub(crate) fn build(app: &mut App) {
    app.add_observer(on_slot_action)
        .add_observer(body::on_panel_action)
        .add_observer(slot_splitter::on_slot_drag_start)
        .add_observer(slot_splitter::on_slot_drag)
        .add_observer(slot_splitter::on_slot_drag_end);
}
