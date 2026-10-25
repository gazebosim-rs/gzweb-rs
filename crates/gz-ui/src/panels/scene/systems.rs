//! The Scene panel's systems: its view, count and header actions written
//! into the shell's [`PanelStatus`]; the tree's model rebuilt from the
//! scene; and, while it shows, its rows, their pool, the chips, the
//! keyboard cursor and the answers to its rows' asks.

use std::collections::HashMap;

use bevy::a11y::AccessibilityNode;
use bevy::ecs::system::SystemParam;
use bevy::input::keyboard::KeyboardInput;
use bevy::input::ButtonState;
use bevy::input_focus::{FocusedInput, InputFocus, InputFocusVisible};
use bevy::picking::events::PointerPress;
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::Checked;
use bevy::ui_widgets::{Activate, ValueChange};
use gz_bevy::connection::GzStatus;
use gz_bevy::overlays::extras::{SdfExtras, SdfWanted};
use gz_bevy::spawn::{GzEntity, GzIndex, GzSensors};
use gz_bevy::view::editing::EditingModel;
use gz_bevy::view::input::{InputMap, KeyMods, Platform};
use gz_bevy::view::{select, select_parts};
use gz_bevy::view::{SelectMode, Selection};
use gz_core::scene::NodeKind;

use super::model::{ModelBuilder, NodeKey, Row, TreeModel, TreeScroll, TreeState};
use super::view::{
    expose, first_row, pool_size, spawn_slot, ChipChoice, ChipRow, FilterButton, FilterField,
    NoMatches, TreeContent, TreeList, TreeSlot, TreeSources,
};
use super::{COLLAPSE_ALL, EXPAND_ALL, NEW_WORLD, OPEN_WORLD, SCENE};
use crate::panels::{PanelAction, PanelActionActivated, PanelView};
use crate::shell::{panel_shown, world_name, CurrentPhase, PanelStatus, Phase, SetPanelStatus};
use crate::theme::visuals::Active;
use crate::theme::{ColorRole, Ring};
use crate::widgets::layout::set_shown;
use crate::widgets::{EntityKind, TreeRowExpand, TreeRowSpec, TreeRowVisibility};

/// An entity came, or a link's sensors changed.
type StructureChanged = Or<(Added<GzEntity>, Changed<GzSensors>)>;
/// The list's content node.
type ContentOnly = (With<TreeContent>, Without<TreeSlot>);
/// A pooled row: what it shows, its node and parent, and whether it shows
/// a selected entity.
type PooledRow = (
    Entity,
    &'static mut TreeSlot,
    &'static mut TreeRowSpec,
    &'static mut Node,
    &'static ChildOf,
    Has<Active>,
);
/// The "No entities match" line.
type NoMatchesOnly = (With<NoMatches>, Without<TreeSlot>, Without<TreeContent>);

/// The rows the tree shows now, top to bottom (from the model and the
/// state; recomputed when either changes).
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct TreeRows(pub(crate) Vec<Row>);

/// Ready once the world is loaded; skeleton rows while the first entity
/// snapshot arrives; the error with Reconnect once it's lost; else "No
/// world loaded" with the world actions (disabled until the World menu,
/// M8).
pub(super) fn view(phase: Phase) -> PanelView {
    match phase {
        Phase::Live => PanelView::Ready,
        Phase::Loading => PanelView::Loading,
        Phase::Lost { .. } => {
            PanelView::error("Lost connection to server").action(PanelAction::reconnect())
        }
        Phase::Idle | Phase::Connecting => PanelView::empty("No world loaded")
            .action(PanelAction::new(OPEN_WORLD, "Open world…").disabled())
            .action(PanelAction::new(NEW_WORLD, "New world").disabled()),
    }
}

/// Writes the view as the connection changes.
pub(super) fn scene_view(phase: Res<CurrentPhase>, mut status: ResMut<PanelStatus>) {
    if phase.is_changed() {
        status.set_view(SCENE, view(phase.0));
    }
}

/// Writes the header's count: the tree's entities once the world is
/// loaded.
pub(super) fn entity_count(
    phase: Res<CurrentPhase>,
    model: Res<TreeModel>,
    mut status: ResMut<PanelStatus>,
) {
    if !phase.is_changed() && !model.is_changed() {
        return;
    }
    let count = (phase.0 == Phase::Live).then(|| {
        let n = model.entities();
        format!("{n} {}", if n == 1 { "entity" } else { "entities" })
    });
    status.set_count(SCENE, count);
}

/// Expand all and Collapse all work while a world with a tree is loaded.
pub(super) fn header_actions(
    phase: Res<CurrentPhase>,
    model: Res<TreeModel>,
    mut status: ResMut<PanelStatus>,
) {
    if !phase.is_changed() && !model.is_changed() {
        return;
    }
    let on = phase.0 == Phase::Live && model.branches().next().is_some();
    status.set_action_enabled(SCENE, EXPAND_ALL, on);
    status.set_action_enabled(SCENE, COLLAPSE_ALL, on);
}

/// What the tree's model is built from: the scene's entities and the
/// world's SDFormat (joints).
#[derive(SystemParam)]
pub(super) struct SceneSources<'w, 's> {
    status: Option<Res<'w, GzStatus>>,
    index: Option<Res<'w, GzIndex>>,
    extras: Option<Res<'w, SdfExtras>>,
    entities: Query<'w, 's, (&'static GzEntity, Option<&'static GzSensors>)>,
    children: Query<'w, 's, &'static Children>,
    added: Query<'w, 's, (), StructureChanged>,
    removed: RemovedComponents<'w, 's, GzEntity>,
    /// The world the tree was last built for (`GzStatus` changes with every
    /// stats message; only its world matters here).
    world: Local<'s, Option<String>>,
}

impl SceneSources<'_, '_> {
    /// Entities came or went, or the world or its SDFormat changed.
    fn changed(&mut self) -> bool {
        let removed = self.removed.read().count() > 0;
        let world = self.status.as_ref().and_then(|s| s.world.as_deref());
        let new_world = world != self.world.as_deref();
        if new_world {
            *self.world = world.map(str::to_owned);
        }
        removed
            || new_world
            || !self.added.is_empty()
            || self.extras.as_ref().is_some_and(Res::is_changed)
            || self.index.as_ref().is_some_and(Res::is_changed)
    }

    /// The tree (DECISIONS.md D-66): the world, then the root's entities
    /// depth first; links' sensors after their children, models' joints
    /// after theirs. Empty until a scene has loaded.
    fn build(&self) -> TreeModel {
        let index = self.index.as_deref();
        let Some(root) = index.filter(|i| i.scene.is_some()).and_then(|i| i.root) else {
            return TreeModel::default();
        };
        let name = world_name(self.status.as_deref(), index).unwrap_or_else(|| "world".into());
        let joints = self.joints();
        let mut b = ModelBuilder::default();
        b.open(NodeKey::World, &name, EntityKind::World, None);
        for &e in self.children.get(root).into_iter().flatten() {
            self.add(&mut b, e, &joints);
        }
        b.finish()
    }

    /// The joints by the path of the model that declares them (a joint
    /// whose child is a nested model's link is its declaring model's).
    fn joints(&self) -> HashMap<&str, Vec<&str>> {
        let mut out: HashMap<&str, Vec<&str>> = HashMap::new();
        if let Some(extras) = self.extras.as_ref().and_then(|x| x.extras.as_ref()) {
            for le in extras.links.values() {
                for j in &le.joints {
                    out.entry(j.model.as_str())
                        .or_default()
                        .push(j.name.as_str());
                }
            }
        }
        for v in out.values_mut() {
            v.sort_unstable();
        }
        out
    }

    fn add(&self, b: &mut ModelBuilder, e: Entity, joints: &HashMap<&str, Vec<&str>>) {
        let Ok((gz, sensors)) = self.entities.get(e) else {
            return;
        };
        let kind = match gz.kind {
            NodeKind::Model => EntityKind::Model,
            NodeKind::Link => EntityKind::Link,
            NodeKind::Light => EntityKind::Light,
            NodeKind::Visual => return,
        };
        b.open(NodeKey::Entity(gz.path.clone()), &gz.name, kind, Some(e));
        for &c in self.children.get(e).into_iter().flatten() {
            self.add(b, c, joints);
        }
        for s in sensors.into_iter().flat_map(|s| &s.0) {
            let key = NodeKey::Sensor(gz.path.clone(), s.name.clone());
            b.leaf(key, &s.name, EntityKind::Sensor, None);
        }
        if kind == EntityKind::Model {
            for &j in joints.get(gz.path.as_str()).into_iter().flatten() {
                let key = NodeKey::Joint(gz.path.clone(), j.to_string());
                b.leaf(key, j, EntityKind::Joint, None);
            }
        }
        b.close();
    }
}

/// Rebuilds the tree's model when the scene's structure changes. It runs
/// while the panel is closed too: the header's count comes from it.
pub(super) fn rebuild_model(mut sources: SceneSources, mut model: ResMut<TreeModel>) {
    if sources.changed() {
        let built = sources.build();
        model.set_if_neq(built);
    }
}

/// Entering a model's editing opens its row (§6.6).
pub(super) fn expand_edited(
    editing: Option<Res<EditingModel>>,
    model: Res<TreeModel>,
    mut state: ResMut<TreeState>,
) {
    let Some(editing) = editing.filter(|e| e.is_changed()) else {
        return;
    };
    let Some(edited) = editing.model() else {
        return;
    };
    if let Some(n) = model.nodes.iter().find(|n| n.entity == Some(edited)) {
        if !state.expanded.contains(&n.key) {
            state.expanded.insert(n.key.clone());
        }
    }
}

/// A [`SceneTreeStart`](super::SceneTreeStart), once there is a tree: the
/// filter on the first one; every row open, as rows arrive, until the tree
/// has more than the world (a world that starts empty gets its models
/// later), then it is removed.
pub(super) fn apply_start(
    start: Option<ResMut<super::SceneTreeStart>>,
    model: Res<TreeModel>,
    mut state: ResMut<TreeState>,
    inputs: Query<(Entity, &ChildOf), With<EditableText>>,
    fields: Query<(), With<FilterField>>,
    mut commands: Commands,
) {
    let Some(mut start) = start.filter(|_| !model.nodes.is_empty()) else {
        return;
    };
    if start.expand_all && (model.is_changed() || start.is_added()) {
        state.expanded.extend(model.branches().cloned());
    }
    if !start.filter.is_empty() {
        state.filter = std::mem::take(&mut start.filter);
        // A body built before the tree shows the filter too.
        for (e, parent) in &inputs {
            if fields.contains(parent.parent()) {
                commands
                    .entity(e)
                    .insert(EditableText::new(state.filter.as_str()));
            }
        }
    }
    let waiting = start.expand_all && model.branches().all(|k| *k == NodeKey::World);
    if !waiting {
        commands.remove_resource::<super::SceneTreeStart>();
    }
}

/// The world's SDFormat is wanted while the tree shows (its joints) or
/// an entity is inspected.
pub(super) fn want_sdf(
    hosts: Option<Res<crate::shell::ShownPanels>>,
    inspected: Option<Res<crate::inspectors::Inspected>>,
    wanted: Option<ResMut<SdfWanted>>,
) {
    use crate::inspectors::InspectKind;
    if let Some(mut wanted) = wanted {
        // The tree's joints, and the inspectors' definitions (flags,
        // inertials, sensor outputs, joint limits).
        let inspecting =
            inspected.is_some_and(|i| !matches!(i.kind, InspectKind::World | InspectKind::Multi));
        let shown = panel_shown(SCENE)(hosts);
        gz_bevy::overlays::extras::want_sdf(&mut wanted, "scene", shown || inspecting);
    }
}

/// The filter field's text is the tree's filter; a new filter opens every
/// path to a match again and scrolls to the top.
pub(super) fn filter_text(
    inputs: Query<(&EditableText, &ChildOf), Changed<EditableText>>,
    fields: Query<(), With<FilterField>>,
    mut state: ResMut<TreeState>,
    mut lists: Query<&mut ScrollPosition, With<TreeList>>,
) {
    for (text, parent) in &inputs {
        if !fields.contains(parent.parent()) {
            continue;
        }
        let value = text.value().to_string();
        if state.filter != value {
            state.filter = value;
            state.filter_collapsed.clear();
            for mut s in &mut lists {
                s.y = 0.0;
            }
        }
    }
}

/// The rows, when the model or the state changes.
pub(super) fn compute_rows(
    model: Res<TreeModel>,
    state: Res<TreeState>,
    mut rows: ResMut<TreeRows>,
) {
    if model.is_changed() || state.is_changed() {
        let next = TreeRows(model.rows(&state));
        rows.set_if_neq(next);
    }
}

/// The pool and the list it scrolls in.
#[derive(SystemParam)]
pub(super) struct Pool<'w, 's> {
    lists: Query<
        'w,
        's,
        (
            Entity,
            Ref<'static, ScrollPosition>,
            Ref<'static, ComputedNode>,
        ),
        With<TreeList>,
    >,
    contents: Query<'w, 's, (Entity, &'static mut Node), ContentOnly>,
    slots: Query<'w, 's, PooledRow>,
    no_matches: Query<'w, 's, &'static mut Node, NoMatchesOnly>,
    shown: Query<'w, 's, (), (With<GzEntity>, Changed<Visibility>)>,
    access: Query<'w, 's, &'static mut AccessibilityNode, With<TreeSlot>>,
}

/// Places the pooled rows over what is in view (DECISIONS.md D-66): the
/// content is as tall as every row, and each pooled row shows the row at
/// its offset, recycled by writing its spec, so the tree costs what a
/// screenful does. Runs when the rows, the scroll, the list's size or an
/// entity's visibility change; keeps the scroll for a rebuilt body.
pub(super) fn place_rows(
    src: TreeSources,
    mut pool: Pool,
    mut scroll: ResMut<TreeScroll>,
    mut commands: Commands,
) {
    let Ok((_, pos, computed)) = pool.lists.single() else {
        return;
    };
    let visibility = !pool.shown.is_empty();
    // The model too: a row's name or kind can change with its place.
    let editing = src.editing.as_ref().is_some_and(Res::is_changed);
    let repaint = src.rows.is_changed()
        || src.model.is_changed()
        || src.selection.is_changed()
        || visibility
        || editing;
    if !repaint && !pos.is_changed() && !computed.is_changed() {
        return;
    }
    let row_h = src.row_h();
    let rows = &src.rows.0;
    // Where layout will put the list this frame: the offset clamped to
    // the content (as tall as the rows, plus the list's padding).
    let view_h = computed.size().y * computed.inverse_scale_factor();
    let pad = 2.0 * crate::theme::SPACE[4];
    let max = (rows.len() as f32 * row_h + pad - view_h).max(0.0);
    let y = pos.y.clamp(0.0, max);
    scroll.set_if_neq(TreeScroll(y));
    for (_, mut node) in &mut pool.contents {
        let h = px(rows.len() as f32 * row_h);
        if node.height != h {
            node.height = h;
        }
    }
    for mut node in &mut pool.no_matches {
        set_shown(&mut node, rows.is_empty() && src.state.filtering());
    }
    let first = first_row(y, row_h);
    let want = pool_size(view_h, row_h);
    let Ok((content, _)) = pool.contents.single() else {
        return;
    };
    // In entity order, so a row stays in its pooled row while scrolling
    // doesn't move it.
    let mut ours: Vec<Entity> = pool
        .slots
        .iter()
        .filter(|s| s.4.parent() == content)
        .map(|s| s.0)
        .collect();
    ours.sort_unstable();
    let mut k = 0;
    for e in ours {
        let Ok((_, mut slot, mut spec, mut node, _, active)) = pool.slots.get_mut(e) else {
            continue;
        };
        let i = first + k;
        k += 1;
        let shown = i < rows.len() && k <= want.max(1);
        let at = shown.then_some(i);
        slot.set_if_neq(TreeSlot(at));
        set_shown(&mut node, shown);
        if let Ok(mut access) = pool.access.get_mut(e) {
            if access.0.is_hidden() == shown {
                expose(&mut access, shown);
            }
        }
        if let Some(i) = at {
            let top = px(i as f32 * row_h);
            if node.top != top {
                node.top = top;
            }
            spec.set_if_neq(src.spec(rows[i]));
        }
        let selected = at.is_some_and(|i| src.selected(rows[i]));
        if selected != active {
            if selected {
                commands.entity(e).insert(Active);
            } else {
                commands.entity(e).remove::<Active>();
            }
        }
    }
    // Grow the pool to cover the view.
    if k < want {
        let t = src.theme.clone();
        commands.entity(content).with_children(|c| {
            for i in (first + k..rows.len()).take(want - k) {
                spawn_slot(c, &t, Some(i), src.spec(rows[i]), src.selected(rows[i]));
            }
        });
    }
}

/// The type chips and the filter button follow the state.
pub(super) fn chip_states(
    state: Res<TreeState>,
    chips: Query<(Entity, &ChipChoice, Has<Checked>)>,
    added: Query<(), Added<ChipChoice>>,
    mut rows: Query<&mut Node, With<ChipRow>>,
    buttons: Query<(Entity, Has<Active>), With<FilterButton>>,
    mut commands: Commands,
) {
    if !state.is_changed() && added.is_empty() {
        return;
    }
    for (e, chip, checked) in &chips {
        let want = chip.0 == state.chip;
        if want != checked {
            if want {
                commands.entity(e).insert(Checked);
            } else {
                commands.entity(e).remove::<Checked>();
            }
        }
    }
    for mut node in &mut rows {
        set_shown(&mut node, state.chips_shown);
    }
    let filtering = state.chip != super::model::Chip::All;
    for (e, active) in &buttons {
        if filtering != active {
            if filtering {
                commands.entity(e).insert(Active);
            } else {
                commands.entity(e).remove::<Active>();
            }
        }
    }
}

/// The keyboard cursor's row shows the focus ring while the tree has
/// keyboard focus (§5: focus-visible).
pub(super) fn cursor_ring(
    focus: Res<InputFocus>,
    visible: Res<InputFocusVisible>,
    state: Res<TreeState>,
    model: Res<TreeModel>,
    rows: Res<TreeRows>,
    lists: Query<(), With<TreeList>>,
    mut slots: Query<(Ref<TreeSlot>, &mut Ring)>,
) {
    let on = visible.0 && focus.get().is_some_and(|f| lists.contains(f));
    let cursor = state.cursor.as_ref();
    let all = focus.is_changed() || visible.is_changed() || state.is_changed() || rows.is_changed();
    for (slot, mut ring) in &mut slots {
        if !all && !slot.is_changed() {
            continue;
        }
        let here = on
            && slot
                .0
                .and_then(|i| rows.0.get(i))
                .is_some_and(|r| Some(&model.node(*r).key) == cursor);
        ring.set_if_neq(Ring(if here {
            ColorRole::Accent
        } else {
            ColorRole::Clear
        }));
    }
}

/// The row a pooled row shows.
fn row_of(slots: &Query<&TreeSlot>, rows: &TreeRows, e: Entity) -> Option<(usize, Row)> {
    let i = slots.get(e).ok()?.0?;
    rows.0.get(i).map(|r| (i, *r))
}

/// A caret: expand or collapse (while filtering, close or reopen a path).
pub(super) fn on_expand(
    ev: On<TreeRowExpand>,
    slots: Query<&TreeSlot>,
    rows: Res<TreeRows>,
    model: Res<TreeModel>,
    mut state: ResMut<TreeState>,
) {
    let Some((_, row)) = row_of(&slots, &rows, ev.entity) else {
        return;
    };
    let key = model.node(row).key.clone();
    let filtering = state.filtering();
    let set = if filtering {
        &mut state.filter_collapsed
    } else {
        &mut state.expanded
    };
    // Filtering, the set holds the closed paths.
    if ev.expanded != filtering {
        set.insert(key);
    } else {
        set.remove(&key);
    }
}

/// An eye: the panel owns the entity's visibility (the row follows).
pub(super) fn on_visibility(
    ev: On<TreeRowVisibility>,
    slots: Query<&TreeSlot>,
    rows: Res<TreeRows>,
    model: Res<TreeModel>,
    mut visibility: Query<&mut Visibility>,
    mut records: MessageWriter<gz_bevy::undo::Record>,
) {
    let Some((_, row)) = row_of(&slots, &rows, ev.entity) else {
        return;
    };
    let Some(entity) = model.node(row).entity else {
        return;
    };
    let Ok(mut v) = visibility.get_mut(entity) else {
        return;
    };
    let after = if ev.hidden {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    };
    let before = *v;
    if before == after {
        return;
    }
    *v = after;
    records.write(gz_bevy::undo::Record {
        verb: if ev.hidden { "hide" } else { "show" },
        named: true,
        changes: vec![gz_bevy::undo::Change::Visibility {
            entity,
            before,
            after,
        }],
        scrub: None,
        open: false,
    });
}

/// A right press on a row opens its entity's context menu (§7.9; the
/// world, joints and sensors have none), selecting it first unless it is.
pub(super) fn on_row_context(
    mut ev: On<PointerPress>,
    slots: Query<&TreeSlot>,
    rows: Res<TreeRows>,
    model: Res<TreeModel>,
    mut selection: ResMut<Selection>,
    mut out: MessageWriter<gz_bevy::view::EntityContext>,
) {
    if ev.button != PointerButton::Secondary {
        return;
    }
    let Some((_, row)) = row_of(&slots, &rows, ev.event_target()) else {
        return;
    };
    ev.propagate(false);
    if let Some(entity) = model.node(row).entity {
        crate::shell::context_menu::tree_context(entity, &mut selection, &mut out);
    }
}

/// A row click moves the tree's cursor there and selects it
/// ([`select_row`]); a right or middle click doesn't (a right press opens
/// the context menu, [`on_row_context`]).
#[allow(clippy::too_many_arguments)]
pub(super) fn on_row_activate(
    ev: On<Activate>,
    slots: Query<&TreeSlot>,
    rows: Res<TreeRows>,
    model: Res<TreeModel>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    map: Option<Res<InputMap>>,
    mut state: ResMut<TreeState>,
    mut selection: ResMut<Selection>,
    (editing, parents): (Option<Res<EditingModel>>, Query<&ChildOf>),
) {
    let others = [MouseButton::Right, MouseButton::Middle];
    if mouse.any_pressed(others) || mouse.any_just_released(others) {
        return;
    }
    // Editing a model, the rows outside it are inert (§6.6).
    let found = row_of(&slots, &rows, ev.entity)
        .filter(|(i, _)| super::view::in_edited(&model, rows.0[*i], editing.as_deref(), &parents));
    if let Some((i, _)) = found {
        let platform = map.map_or(Platform::current(), |m| m.platform);
        let held = KeyMods::held(&keys);
        select_row(
            i,
            &rows.0,
            &model,
            &mut state,
            &mut selection.reborrow(),
            held,
            platform,
        );
    }
}

/// Moves the cursor to row `i` and selects (spec §5.7, §6.4): the row's
/// model, link, light, joint or sensor, or nothing for the world (its
/// inspector). Shift selects the shown rows from the anchor (the last row
/// picked without Shift) to this one; ⌘/Ctrl toggles the row.
fn select_row(
    i: usize,
    rows: &[Row],
    model: &TreeModel,
    state: &mut TreeState,
    selection: &mut Mut<Selection>,
    held: KeyMods,
    platform: Platform,
) {
    let node = model.node(rows[i]);
    let key = node.key.clone();
    if state.cursor.as_ref() != Some(&key) {
        state.cursor = Some(key.clone());
    }
    let toggle = held.mod_key(platform);
    let anchor = state
        .anchor
        .as_ref()
        .and_then(|a| rows.iter().position(|r| &model.node(*r).key == a));
    let mode = SelectMode::from_keys(false, toggle);
    match (
        anchor.filter(|_| held.shift),
        node.key == NodeKey::World,
        node.entity,
    ) {
        (Some(a), _, _) => {
            let range = &rows[a.min(i)..=a.max(i)];
            let entities: Vec<Entity> =
                range.iter().filter_map(|r| model.node(*r).entity).collect();
            let parts: Vec<_> = range.iter().filter_map(|r| model.part(*r)).collect();
            select(selection, SelectMode::Replace, &entities);
            select_parts(selection, SelectMode::Add, &parts);
            return;
        }
        (None, true, _) if !toggle => select(selection, SelectMode::Replace, &[]),
        (None, false, Some(e)) => select(selection, mode, &[e]),
        (None, false, None) => {
            if let Some(part) = model.part(rows[i]) {
                select_parts(selection, mode, &[part]);
            }
        }
        _ => {}
    }
    if state.anchor.as_ref() != Some(&key) {
        state.anchor = Some(key);
    }
}

/// A type chip.
pub(super) fn on_chip(
    ev: On<ValueChange<Entity>>,
    chips: Query<&ChipChoice>,
    mut state: ResMut<TreeState>,
    mut lists: Query<&mut ScrollPosition, With<TreeList>>,
) {
    if let Ok(chip) = chips.get(ev.value) {
        if state.chip != chip.0 {
            state.chip = chip.0;
            state.filter_collapsed.clear();
            for mut s in &mut lists {
                s.y = 0.0;
            }
        }
    }
}

/// The filter button shows or hides the type chips.
pub(super) fn on_filter_button(
    ev: On<Activate>,
    buttons: Query<(), With<FilterButton>>,
    mut state: ResMut<TreeState>,
) {
    if buttons.contains(ev.entity) {
        state.chips_shown = !state.chips_shown;
    }
}

/// The header's Expand all and Collapse all (the world stays open).
pub(super) fn on_header_action(
    ev: On<PanelActionActivated>,
    model: Res<TreeModel>,
    mut state: ResMut<TreeState>,
) {
    if ev.panel != SCENE {
        return;
    }
    if ev.action == EXPAND_ALL {
        state.expanded = model.branches().cloned().collect();
        state.filter_collapsed.clear();
    } else if ev.action == COLLAPSE_ALL {
        state.expanded.clear();
        state.expanded.insert(NodeKey::World);
        if state.filtering() {
            state.filter_collapsed = model.branches().cloned().collect();
            state.filter_collapsed.remove(&NodeKey::World);
        }
    }
}

/// Arrow keys in the focused tree (§6.5): up and down move the cursor,
/// right opens its row or goes to its first child, left closes it or goes
/// to its parent; Home, End and the page keys jump; Enter and Space select
/// the cursor's row (with Shift and ⌘/Ctrl, as a click). The view scrolls
/// to keep the cursor in sight.
#[allow(clippy::too_many_arguments)]
pub(super) fn on_tree_key(
    mut ev: On<FocusedInput<KeyboardInput>>,
    mut lists: Query<(&ComputedNode, &mut ScrollPosition), With<TreeList>>,
    rows: Res<TreeRows>,
    model: Res<TreeModel>,
    theme: Res<crate::theme::Theme>,
    keys: Res<ButtonInput<KeyCode>>,
    map: Option<Res<InputMap>>,
    mut state: ResMut<TreeState>,
    mut selection: ResMut<Selection>,
) {
    let Ok((computed, mut scroll)) = lists.get_mut(ev.focused_entity) else {
        return;
    };
    if ev.input.state != ButtonState::Pressed || rows.0.is_empty() {
        return;
    }
    let rows = &rows.0;
    let at = state
        .cursor
        .as_ref()
        .and_then(|k| rows.iter().position(|r| &model.node(*r).key == k));
    let row_h = theme.s.tree_row_h;
    let page = ((computed.size().y * computed.inverse_scale_factor()) / row_h).floor() as usize;
    let last = rows.len() - 1;
    let target = match (ev.input.key_code, at) {
        (KeyCode::ArrowDown, None) | (KeyCode::Home, _) => Some(0),
        (KeyCode::ArrowUp, None) | (KeyCode::End, _) => Some(last),
        (KeyCode::ArrowDown, Some(i)) => Some((i + 1).min(last)),
        (KeyCode::ArrowUp, Some(i)) => Some(i.saturating_sub(1)),
        (KeyCode::PageDown, i) => Some((i.unwrap_or(0) + page.max(1)).min(last)),
        (KeyCode::PageUp, i) => Some(i.unwrap_or(0).saturating_sub(page.max(1))),
        (KeyCode::ArrowRight, Some(i)) => match rows[i].expanded {
            Some(false) => {
                toggle(&mut state, &model, rows[i], true);
                None
            }
            Some(true) => Some((i + 1).min(last)),
            None => None,
        },
        (KeyCode::ArrowLeft, Some(i)) => match rows[i].expanded {
            Some(true) => {
                toggle(&mut state, &model, rows[i], false);
                None
            }
            _ => model
                .parent(rows[i].node)
                .and_then(|p| rows.iter().position(|r| r.node == p)),
        },
        (KeyCode::Enter | KeyCode::Space, Some(i)) => {
            let platform = map.map_or(Platform::current(), |m| m.platform);
            let held = KeyMods::held(&keys);
            select_row(
                i,
                rows,
                &model,
                &mut state,
                &mut selection.reborrow(),
                held,
                platform,
            );
            None
        }
        _ => return,
    };
    ev.propagate(false);
    let Some(t) = target else {
        return;
    };
    let key = model.node(rows[t]).key.clone();
    if state.cursor.as_ref() != Some(&key) {
        state.cursor = Some(key);
    }
    // Keep the cursor in view (the list's padding included).
    let view_h = computed.size().y * computed.inverse_scale_factor();
    let pad = crate::theme::SPACE[4];
    let top = t as f32 * row_h;
    if top < scroll.y {
        scroll.y = top;
    } else if top + row_h + 2.0 * pad > scroll.y + view_h {
        scroll.y = top + row_h + 2.0 * pad - view_h;
    }
}

/// Opens or closes `row` (while filtering, its path).
fn toggle(state: &mut TreeState, model: &TreeModel, row: Row, open: bool) {
    let key = model.node(row).key.clone();
    if state.filtering() {
        if open {
            state.filter_collapsed.remove(&key);
        } else {
            state.filter_collapsed.insert(key);
        }
    } else if open {
        state.expanded.insert(key);
    } else {
        state.expanded.remove(&key);
    }
}

/// The entity of the row under the pointer, which the views outline
/// faintly (spec §6.4); none once no row is (or the tree is gone).
pub(super) fn hover_outline(
    slots: Query<(&TreeSlot, &bevy::picking::hover::Hovered)>,
    rows: Res<TreeRows>,
    model: Res<TreeModel>,
    mut hovered: ResMut<gz_bevy::view::outline::Hovered>,
) {
    let entity = slots
        .iter()
        .find(|(_, h)| h.get())
        .and_then(|(slot, _)| slot.0)
        .and_then(|i| rows.0.get(i))
        .and_then(|r| model.node(*r).entity);
    hovered.set_if_neq(gz_bevy::view::outline::Hovered(entity));
}
