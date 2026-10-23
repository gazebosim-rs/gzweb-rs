//! The sidebar (spec §3.3): a primary and an optional secondary slot with a
//! row splitter between them (§3.3.2), each a [`PanelHost`] holding one
//! panel: a section header with its title, count, header actions and slot
//! buttons, a body with its content or its empty, loading or error state
//! (§3.3.1), and the panel's footer, if it has one. The panels register
//! themselves ([`PanelRegistry`]) and write their views into
//! [`PanelStatus`]; the hosts rebuild only what changed (DECISIONS.md
//! D-58, D-98).

pub(crate) mod body;
pub(crate) mod footer;
pub(crate) mod slot_splitter;

use bevy::ecs::spawn::SpawnWith;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Activate;
use gz_bevy::view::input::InputMap;

use super::commands::UiCommand;
use super::host::PanelHost;
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
use footer::{action_tooltip, FooterButton, PanelFooter};
use slot_splitter::SlotSplitter;

/// A sidebar slot's node, a [`PanelHost`].
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
        PanelHost::default(),
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

/// A panel's section header in a host: rebuilt when its panel changes,
/// never for the body's sake. In a slot, when the number of slots changes
/// only its slot buttons are (`two`: what they were built for); its count
/// changes in place ([`slot_counts`]).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PanelHeader {
    pub panel: PanelId,
    pub two: bool,
}

/// What the hosts are built from: the layout (the slot buttons), the
/// registered panels, what they wrote into [`PanelStatus`], the theme and
/// the input map.
#[derive(SystemParam)]
pub(crate) struct SlotSources<'w> {
    layout: Res<'w, ShellLayout>,
    panels: Res<'w, PanelRegistry>,
    status: Res<'w, PanelStatus>,
    theme: Res<'w, Theme>,
    map: Option<Res<'w, InputMap>>,
}

impl SlotSources<'_> {
    /// Whether what the hosts are built from may have changed since the
    /// system's last run (besides the hosts themselves).
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

/// Keeps each host's header, body and footer in step with the panel it
/// holds and the panel's view, rebuilding as little as it can
/// (DECISIONS.md D-58, D-98): a header and footer when the host's panel
/// changes, only a slot's buttons when the number of slots does (so a
/// focused header action keeps focus); a body only when its panel's view
/// changes. A body whose panel moved to another host (a swap) moves with
/// it, and a second slot opening or closing leaves the first slot's body
/// alone, so its entities (M4's tree) survive. The body sits between the
/// header and the footer.
#[allow(clippy::too_many_arguments)]
pub(crate) fn rebuild_hosts(
    sources: SlotSources,
    hosts: Query<(Entity, &PanelHost, Option<&SlotView>)>,
    changed: Query<(), Changed<PanelHost>>,
    headers: Query<(Entity, &PanelHeader, &ChildOf)>,
    footers: Query<(Entity, &ChildOf), With<PanelFooter>>,
    slot_actions: Query<(Entity, &ChildOf), With<SlotAction>>,
    bodies: Query<(Entity, &PanelBody, &BodyView, &ChildOf)>,
    mut commands: Commands,
) {
    if !sources.changed() && changed.is_empty() {
        return;
    }
    let two = sources.layout.secondary.is_some();
    // The hosts' bodies not claimed by a host below are despawned.
    let mut spare: Vec<_> = bodies
        .iter()
        .filter(|b| hosts.contains(b.3.parent()))
        .collect();
    for (host_e, host, slot) in &hosts {
        let def = host.panel.and_then(|p| sources.panels.get(p));
        let want = def.map(|d| PanelHeader { panel: d.id, two });
        let header = headers.iter().find(|h| h.2.parent() == host_e);
        match (header, def, want) {
            (Some((_, have, _)), _, Some(want)) if *have == want => {}
            (Some((e, have, _)), _, Some(want)) if have.panel == want.panel => {
                // Only the number of slots changed: new slot buttons.
                for (b, _) in slot_actions.iter().filter(|b| b.1.parent() == e) {
                    commands.entity(b).despawn();
                }
                let (t, map) = (&sources.theme, sources.map());
                commands.entity(e).insert(want);
                if let Some(slot) = slot {
                    commands
                        .entity(e)
                        .with_children(|h| slot_buttons(h, t, &map, slot.0, two));
                }
            }
            (header, def, want) => {
                if let Some((e, ..)) = header {
                    commands.entity(e).despawn();
                }
                for (f, _) in footers.iter().filter(|f| f.1.parent() == host_e) {
                    commands.entity(f).despawn();
                }
                if let (Some(def), Some(want)) = (def, want) {
                    let slot = slot.map(|s| s.0);
                    let h = spawn_header(&mut commands, &sources, def, slot, want);
                    commands.entity(host_e).insert_children(0, &[h]);
                    let t = &sources.theme;
                    if let Some(f) = footer::spawn_footer(&mut commands, t, &sources.status, def) {
                        commands.entity(host_e).add_child(f);
                    }
                }
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
                if parent.parent() != host_e {
                    commands.entity(host_e).insert_children(1, &[body]);
                }
            }
            None => {
                let body = body::spawn_body(&mut commands, &sources.theme, def, view);
                commands.entity(host_e).insert_children(1, &[body]);
            }
        }
    }
    for (body, ..) in spare {
        commands.entity(body).despawn();
    }
}

/// A host's section header: the panel's title and count, its header
/// actions, then a slot's buttons.
fn spawn_header(
    commands: &mut Commands,
    sources: &SlotSources,
    def: &PanelDef,
    slot: Option<Slot>,
    header: PanelHeader,
) -> Entity {
    let t = &sources.theme;
    let map = sources.map();
    let count = sources.status.count(def.id).map(str::to_string);
    commands
        .spawn((section_header(t, def.title, count), header))
        .with_children(|h| {
            for a in &def.header_actions {
                header_action(h, t, &map, &sources.status, def.id, a);
            }
            if let Some(slot) = slot {
                slot_buttons(h, t, &map, slot, header.two);
            }
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

/// A header action's button (with its [`ActionButton`]): its enabled
/// state follows [`PanelStatus`] in place ([`header_action_states`]).
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct HeaderButton;

/// A panel's header action: an icon button that triggers
/// [`PanelActionActivated`](crate::panels::PanelActionActivated), enabled
/// as the panel's status says.
fn header_action(
    h: &mut ChildSpawnerCommands,
    theme: &Theme,
    map: &InputMap,
    status: &PanelStatus,
    panel: PanelId,
    a: &HeaderAction,
) {
    let enabled = status.header_action_enabled(panel, a);
    let mut tip = Tooltip::new(a.tooltip);
    if let Some(b) = a.binding {
        tip = tip.binding(map, b);
    }
    if !enabled {
        tip = tip.detail(a.why.unwrap_or(body::NOT_YET));
    }
    let mut e = h.spawn((
        icon_button(theme, a.icon, IconButtonSize::Small, tip),
        ActionButton {
            panel,
            action: a.id,
        },
        HeaderButton,
    ));
    if !enabled {
        e.insert(InteractionDisabled);
    }
}

/// The header and footer actions' buttons, with whether each is disabled
/// and in a footer.
type ActionButtons<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static ActionButton,
        &'static mut Tooltip,
        Has<InteractionDisabled>,
        Has<FooterButton>,
    ),
    Or<(With<HeaderButton>, With<FooterButton>)>,
>;

/// Each header and footer action's button follows its panel's
/// [`PanelStatus`] in place: dimmed, saying why and inert while disabled.
pub(crate) fn action_states(
    status: Res<PanelStatus>,
    panels: Res<PanelRegistry>,
    mut buttons: ActionButtons,
    mut commands: Commands,
) {
    if !status.is_changed() {
        return;
    }
    for (e, b, mut tip, disabled, in_footer) in &mut buttons {
        let Some(def) = panels.get(b.panel) else {
            continue;
        };
        let Some((registered, why)) = def.action(b.action) else {
            continue;
        };
        let enabled = status.action_enabled_or(b.panel, b.action, registered);
        if enabled == !disabled {
            continue;
        }
        if enabled {
            commands.entity(e).remove::<InteractionDisabled>();
        } else {
            commands.entity(e).insert(InteractionDisabled);
        }
        if in_footer {
            let label = tip.title.clone();
            *tip = action_tooltip(&label, enabled, why);
        } else {
            tip.detail = (!enabled).then(|| why.unwrap_or(body::NOT_YET).to_string());
        }
    }
}

/// Writes which panel each slot holds, and whether it shows (the sidebar
/// is open), into its [`PanelHost`].
pub(crate) fn host_slots(layout: Res<ShellLayout>, mut slots: Query<(&SlotView, &mut PanelHost)>) {
    if !layout.is_changed() {
        return;
    }
    for (slot, mut host) in &mut slots {
        let panel = layout.slot(slot.0);
        host.set_if_neq(PanelHost {
            panel,
            shown: layout.sidebar_open && panel.is_some(),
        });
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
        (apply_slots, (host_slots, super::host::track_hosts).chain())
            .in_set(ShellSystems::Layout)
            .after(super::regions::apply_layout),
    )
    // After the panels have written their views, before the readouts.
    .add_systems(
        Update,
        (rebuild_hosts, slot_counts, action_states)
            .chain()
            .in_set(ShellSystems::Slots),
    )
    .add_systems(
        Update,
        slot_splitter::slot_splitter_ranges.in_set(ShellSystems::Readouts),
    )
    .add_observer(on_slot_action)
    .add_observer(body::on_action_button)
    .add_observer(body::on_reconnect)
    .add_observer(slot_splitter::on_slots_resized)
    .add_observer(slot_splitter::on_slots_reset);
}

#[cfg(test)]
mod tests;
