//! The sidebar (spec §3.3): a primary and an optional secondary slot with a
//! row splitter between them (§3.3.2), each holding one panel: a section
//! header with its title, count, header actions and slot buttons, and a
//! body with its content or its empty, loading or error state (§3.3.1).
//! The panels register themselves ([`PanelRegistry`]) and write their
//! views into [`PanelStatus`]; the slots rebuild only what changed
//! (DECISIONS.md D-58).

pub(crate) mod body;
pub(crate) mod slot_splitter;

use bevy::ecs::spawn::SpawnWith;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Activate;
use gz_bevy::view::input::InputMap;

use super::commands::UiCommand;
use super::layout::DEFAULT_SPLIT;
use super::regions::side_panel;
use super::splitter::side_splitter;
use super::{PanelStatus, ShellLayout, ShellSystems, Side, Slot};
use crate::assets::icons::IconId;
use crate::panels::{HeaderAction, PanelBody, PanelDef, PanelId, PanelRegistry, PanelView};
use crate::theme::Theme;
use crate::widgets::layout::{self, column};
use crate::widgets::text::set_text;
use crate::widgets::SectionCount;
use crate::widgets::{icon_button, section_header, IconButtonSize, Tooltip};
use body::{ActionButton, BodyView};
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

/// A panel's section header in a slot: rebuilt when its panel or the
/// number of slots changes (its slot buttons depend on both), never for
/// the body's sake. Its count changes in place ([`slot_counts`]).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PanelHeader {
    pub panel: PanelId,
    pub two: bool,
}

/// What the slots are built from: the layout, the registered panels, what
/// they wrote into [`PanelStatus`], the theme and the input map.
#[derive(SystemParam)]
pub(crate) struct SlotSources<'w> {
    layout: Res<'w, ShellLayout>,
    panels: Res<'w, PanelRegistry>,
    status: Res<'w, PanelStatus>,
    theme: Res<'w, Theme>,
    map: Option<Res<'w, InputMap>>,
}

impl SlotSources<'_> {
    /// Whether what the slots are built from may have changed since the
    /// system's last run.
    fn changed(&self) -> bool {
        self.layout.is_changed() || self.panels.is_changed() || self.status.is_changed()
    }

    /// What `def`'s body shows: what its systems wrote, else its default.
    fn view(&self, def: &PanelDef) -> PanelView {
        self.status.view(def.id).unwrap_or(&def.view).clone()
    }

    fn map(&self) -> InputMap {
        self.map
            .as_deref()
            .cloned()
            .unwrap_or_else(InputMap::builtin)
    }
}

/// Keeps each slot's header and body in step with the layout and the
/// panels' views, rebuilding as little as it can (DECISIONS.md D-58): a
/// header when its panel or the number of slots changes; a body only when
/// its panel's view changes. A body whose panel moved to the other slot
/// (a swap) moves with it, and a second slot opening or closing leaves
/// the first slot's body alone, so its entities (M4's tree) survive.
pub(crate) fn rebuild_slots(
    sources: SlotSources,
    slots: Query<(Entity, &SlotView)>,
    headers: Query<(Entity, &PanelHeader, &ChildOf)>,
    bodies: Query<(Entity, &PanelBody, &BodyView, &ChildOf)>,
    mut commands: Commands,
) {
    if !sources.changed() {
        return;
    }
    let two = sources.layout.secondary.is_some();
    // Bodies not claimed by a slot below are despawned.
    let mut spare: Vec<_> = bodies.iter().collect();
    for (slot_e, slot) in &slots {
        let def = sources
            .layout
            .slot(slot.0)
            .and_then(|p| sources.panels.get(p));
        let want = def.map(|d| PanelHeader { panel: d.id, two });
        let header = headers.iter().find(|h| h.2.parent() == slot_e);
        if header.map(|h| *h.1) != want {
            if let Some((e, ..)) = header {
                commands.entity(e).despawn();
            }
            if let (Some(def), Some(want)) = (def, want) {
                let h = spawn_header(&mut commands, &sources, def, slot.0, want);
                commands.entity(slot_e).insert_children(0, &[h]);
            }
        }
        let Some(def) = def else {
            continue;
        };
        let view = sources.view(def);
        let kept = spare
            .iter()
            .position(|(_, b, shown, _)| b.0 == def.id && shown.0 == view);
        match kept {
            Some(i) => {
                let (body, _, _, parent) = spare.swap_remove(i);
                if parent.parent() != slot_e {
                    commands.entity(slot_e).add_child(body);
                }
            }
            None => {
                body::spawn_body(&mut commands, slot_e, &sources.theme, def, view);
            }
        }
    }
    for (body, ..) in spare {
        commands.entity(body).despawn();
    }
}

/// A slot's section header: the panel's title and count, its header
/// actions, then the slot buttons.
fn spawn_header(
    commands: &mut Commands,
    sources: &SlotSources,
    def: &PanelDef,
    slot: Slot,
    header: PanelHeader,
) -> Entity {
    let t = &sources.theme;
    let map = sources.map();
    let count = sources.status.count(def.id).map(str::to_string);
    commands
        .spawn((section_header(t, def.title, count), header))
        .with_children(|h| {
            for a in &def.header_actions {
                let enabled = sources
                    .status
                    .action_enabled(def.id, a.id)
                    .unwrap_or(a.enabled);
                header_action(h, t, &map, def.id, a, enabled);
            }
            slot_buttons(h, t, &map, slot, header.two);
        })
        .id()
}

/// Each slot's header count follows its panel's [`PanelStatus`] in place.
pub(crate) fn slot_counts(
    status: Res<PanelStatus>,
    mut counts: Query<(Entity, Ref<SectionCount>, &mut Text)>,
    parents: Query<&ChildOf>,
    headers: Query<&PanelHeader>,
) {
    let added = counts.iter().any(|(_, c, _)| c.is_added());
    if !status.is_changed() && !added {
        return;
    }
    for (e, _, mut text) in &mut counts {
        let Some(header) = parents.iter_ancestors(e).find_map(|a| headers.get(a).ok()) else {
            continue;
        };
        set_text(&mut text, status.count(header.panel).unwrap_or_default());
    }
}

/// A panel's header action: an icon button that triggers
/// [`PanelActionActivated`](crate::panels::PanelActionActivated).
fn header_action(
    h: &mut ChildSpawnerCommands,
    theme: &Theme,
    map: &InputMap,
    panel: PanelId,
    a: &HeaderAction,
    enabled: bool,
) {
    let mut tip = Tooltip::new(a.tooltip);
    if let Some(b) = a.binding {
        tip = tip.binding(map, b);
    }
    if !enabled {
        tip = tip.detail(body::NOT_YET);
    }
    let mut e = h.spawn((
        icon_button(theme, a.icon, IconButtonSize::Small, tip),
        ActionButton {
            panel,
            action: a.id,
        },
    ));
    if !enabled {
        e.insert(InteractionDisabled);
    }
}

/// Each header action's button follows its panel's
/// [`PanelStatus::action_enabled`] in place: dimmed with "Not available
/// yet" while disabled (the header isn't rebuilt).
pub(crate) fn header_action_states(
    status: Res<PanelStatus>,
    panels: Res<PanelRegistry>,
    mut buttons: Query<(
        Entity,
        &ActionButton,
        &mut Tooltip,
        Has<InteractionDisabled>,
    )>,
    parents: Query<&ChildOf>,
    headers: Query<(), With<PanelHeader>>,
    mut commands: Commands,
) {
    if !status.is_changed() {
        return;
    }
    for (e, b, mut tip, disabled) in &mut buttons {
        let in_header = parents.get(e).is_ok_and(|p| headers.contains(p.parent()));
        let Some(def) = panels.get(b.panel).filter(|_| in_header) else {
            continue;
        };
        let Some(a) = def.header_actions.iter().find(|a| a.id == b.action) else {
            continue;
        };
        let enabled = status
            .action_enabled(b.panel, b.action)
            .unwrap_or(a.enabled);
        if enabled == !disabled {
            continue;
        }
        if enabled {
            commands.entity(e).remove::<InteractionDisabled>();
            tip.detail = None;
        } else {
            commands.entity(e).insert(InteractionDisabled);
            tip.detail = Some(body::NOT_YET.into());
        }
    }
}

fn slot_buttons(
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
        apply_slots
            .in_set(ShellSystems::Layout)
            .after(super::regions::apply_layout),
    )
    // After the panels have written their views.
    .add_systems(
        Update,
        (rebuild_slots, (slot_counts, header_action_states))
            .chain()
            .in_set(ShellSystems::Readouts),
    )
    .add_observer(on_slot_action)
    .add_observer(body::on_action_button)
    .add_observer(body::on_reconnect)
    .add_observer(slot_splitter::on_slot_drag_start)
    .add_observer(slot_splitter::on_slot_drag)
    .add_observer(slot_splitter::on_slot_drag_end);
}

#[cfg(test)]
mod tests;
