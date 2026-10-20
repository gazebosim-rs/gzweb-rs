//! The sidebar (spec §3.3): a primary and an optional secondary slot with a
//! row splitter between them (§3.3.2), each holding one panel's shell: a
//! section header with its title, count and slot buttons, and a body in
//! its empty, loading or error state (§3.3.1). The panels' contents come
//! in M4 and M7; the panels register themselves
//! ([`PanelRegistry`]).

pub(crate) mod body;
pub(crate) mod slot_splitter;

use bevy::ecs::spawn::SpawnWith;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use gz_bevy::view::input::InputMap;

use super::commands::UiCommand;
use super::connection::CurrentPhase;
use super::layout::DEFAULT_SPLIT;
use super::regions::side_panel;
use super::splitter::side_splitter;
use super::{PanelStatus, ShellLayout, ShellSystems, Side, Slot};
use crate::assets::icons::IconId;
use crate::panels::{PanelId, PanelRegistry, PanelView};
use crate::theme::Theme;
use crate::widgets::layout::{self, column};
use crate::widgets::text::set_text;
use crate::widgets::SectionCount;
use crate::widgets::{icon_button, section_header, IconButtonSize, Tooltip};
use body::panel_body;
use slot_splitter::SlotSplitter;

/// A sidebar slot's node.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SlotView(pub Slot);

/// A panel header's slot buttons (§3.3.2).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SlotAction {
    /// With two slots: the panels swap.
    Swap,
    /// The primary header's chevron: collapse the sidebar.
    Collapse,
    /// The secondary header's close: back to one slot.
    Close,
}

/// The sidebar region.
pub(crate) fn sidebar(theme: &Theme, layout: &ShellLayout) -> impl Bundle {
    let t = theme.clone();
    (
        side_panel(Side::Sidebar, layout),
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
            flex_basis: px(0),
            flex_grow: match slot {
                Slot::Primary => DEFAULT_SPLIT,
                Slot::Secondary => 1.0 - DEFAULT_SPLIT,
            },
            min_height: px(0),
            overflow: Overflow::clip(),
            ..column(0.0)
        },
    )
}

/// What a slot is built from; rebuilt only when it changes. The header's
/// count isn't part of it: a count changes in place ([`slot_counts`]), so
/// the body (M4's tree, with its scroll and expansion) survives it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SlotContent {
    pub panel: Option<PanelId>,
    pub view: Option<PanelView>,
    pub two: bool,
}

/// What the slots are built from: the layout, the registered panels, the
/// connection their states follow, and the panels' counts.
#[derive(SystemParam)]
pub(crate) struct SlotSources<'w> {
    layout: Res<'w, ShellLayout>,
    panels: Res<'w, PanelRegistry>,
    phase: Res<'w, CurrentPhase>,
    status: Res<'w, PanelStatus>,
    theme: Res<'w, Theme>,
    map: Option<Res<'w, InputMap>>,
}

impl SlotSources<'_> {
    /// Whether what a slot is built from may have changed since the
    /// system's last run.
    fn changed(&self) -> bool {
        self.layout.is_changed() || self.panels.is_changed() || self.phase.is_changed()
    }

    /// What `slot` should show now.
    fn content(&self, slot: Slot) -> SlotContent {
        let panel = self.layout.slot(slot);
        let def = panel.and_then(|p| self.panels.get(p));
        SlotContent {
            panel,
            view: def.map(|d| (d.view)(self.phase.0)),
            two: self.layout.secondary.is_some(),
        }
    }
}

/// Rebuilds a slot's header and body when its panel, state or the number
/// of slots changes.
pub(crate) fn rebuild_slots(
    sources: SlotSources,
    slots: Query<(Entity, &SlotView)>,
    mut shown: Local<[Option<SlotContent>; 2]>,
    mut commands: Commands,
) {
    if !sources.changed() {
        return;
    }
    for (e, slot) in &slots {
        let i = match slot.0 {
            Slot::Primary => 0,
            Slot::Secondary => 1,
        };
        let want = sources.content(slot.0);
        if shown[i].as_ref() == Some(&want) {
            continue;
        }
        commands.entity(e).despawn_children();
        let def = want.panel.and_then(|p| sources.panels.get(p));
        if let (Some(def), Some(view)) = (def, want.view.clone()) {
            let t = sources.theme.clone();
            let map = sources
                .map
                .as_deref()
                .cloned()
                .unwrap_or_else(InputMap::builtin);
            let count = sources.status.count(def.id).map(str::to_string);
            let (which, two) = (slot.0, want.two);
            commands.entity(e).with_children(|p| {
                p.spawn(section_header(&t, def.title, count))
                    .with_children(|h| header_buttons(h, &t, &map, which, two));
                p.spawn(panel_body(&t, def, view));
            });
        }
        shown[i] = Some(want);
    }
}

/// Each slot's header count follows its panel's [`PanelStatus`] in place.
pub(crate) fn slot_counts(
    layout: Res<ShellLayout>,
    status: Res<PanelStatus>,
    mut counts: Query<(Entity, Ref<SectionCount>, &mut Text)>,
    parents: Query<&ChildOf>,
    slots: Query<&SlotView>,
) {
    let added = counts.iter().any(|(_, c, _)| c.is_added());
    if !layout.is_changed() && !status.is_changed() && !added {
        return;
    }
    for (e, _, mut text) in &mut counts {
        let Some(slot) = parents.iter_ancestors(e).find_map(|a| slots.get(a).ok()) else {
            continue;
        };
        let want = layout
            .slot(slot.0)
            .and_then(|p| status.count(p))
            .unwrap_or_default();
        set_text(&mut text, want);
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
                    Tooltip::new("Collapse sidebar").binding(map, "ui.sidebar"),
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

/// Writes the slot split into the slots: one slot takes the sidebar; with
/// two, the primary takes `split` of it and the row splitter shows.
pub(crate) fn apply_slots(
    layout: Res<ShellLayout>,
    mut slots: Query<(&SlotView, &mut Node)>,
    mut splitters: Query<&mut Node, (With<SlotSplitter>, Without<SlotView>)>,
) {
    if !layout.is_changed() {
        return;
    }
    let two = layout.secondary.is_some();
    for (slot, mut node) in &mut slots {
        let (grow, display) = match (slot.0, two) {
            (Slot::Primary, false) => (1.0, Display::Flex),
            (Slot::Primary, true) => (layout.split, Display::Flex),
            (Slot::Secondary, true) => (1.0 - layout.split, Display::Flex),
            (Slot::Secondary, false) => (0.0, Display::None),
        };
        node.flex_grow = grow;
        node.display = display;
    }
    for mut node in &mut splitters {
        node.display = layout::display(two);
    }
}

/// The panel headers' slot buttons.
pub(crate) fn on_slot_action(
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
    app.add_systems(
        Update,
        (apply_slots, rebuild_slots)
            .chain()
            .in_set(ShellSystems::Layout)
            .after(super::regions::apply_layout),
    )
    .add_systems(Update, slot_counts.in_set(ShellSystems::Readouts))
    .add_observer(on_slot_action)
    .add_observer(body::on_panel_action)
    .add_observer(slot_splitter::on_slot_drag_start)
    .add_observer(slot_splitter::on_slot_drag)
    .add_observer(slot_splitter::on_slot_drag_end);
}

#[cfg(test)]
mod tests;
