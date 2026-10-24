//! The command palette's systems (DECISIONS.md D-107): what it lists, how
//! it opens and closes, the query, the keys, and running a result.

use bevy::a11y::AccessibilityNode;
use bevy::ecs::system::SystemParam;
use bevy::input::keyboard::KeyboardInput;
use bevy::input::ButtonState;
use bevy::input_focus::{FocusCause, FocusedInput, InputFocus};
use bevy::picking::events::PointerPress;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Activate;
use gz_bevy::connection::GzStatus;
use gz_bevy::edit::edits_allowed;
use gz_bevy::overlays::{Overlay, OverlayCommand};
use gz_bevy::spawn::GzEntity;
use gz_bevy::topics::GzTopics;
use gz_bevy::undo::UndoStack;
use gz_bevy::view::controls::RunBinding;
use gz_bevy::view::input::InputMap;
use gz_bevy::view::Selection;
use gz_core::scene::NodeKind;

use super::model::{gazebo_names, rank, step, Group, Item, Run};
use super::view::{
    group_label, no_results, palette, result_row, PaletteBackdrop, PaletteField, PaletteRoot,
    PaletteRow, PaletteRows,
};
use super::{PaletteOpen, PaletteState, StartPalette};
use crate::assets::icons::IconId;
use crate::panels::{PanelActionActivated, PanelRegistry, TopicsStart};
use crate::shell::sidebar::body::ActionButton;
use crate::shell::top_bar::search::SearchTrigger;
use crate::shell::top_bar::tools::Tool;
use crate::shell::top_bar::transport::Transport;
use crate::shell::{CurrentPhase, PanelStatus, Phase, ShellLayout, ShownPanels, Slot, UiCommand};
use crate::theme::visuals::Active;
use crate::theme::Theme;
use crate::widgets::shortcut::shortcut_label;
use crate::widgets::text_field::{set_field_text, TextFieldInput};
use crate::widgets::TextSubmit;

/// The commands the palette lists from `shortcuts.json`, in this order:
/// those a key or a click can do now. (Held keys, the gizmo's axis
/// constraints, Teleop's keys and what M8b and M8c bring wait.)
const COMMANDS: &[&str] = &[
    "sim.play_pause",
    "sim.step",
    "sim.reset",
    "edit.undo",
    "edit.redo",
    "tool.navigate",
    "tool.box_select",
    "tool.translate",
    "tool.rotate",
    "view.focus",
    "view.home",
    "view.projection",
    "view.top",
    "view.bottom",
    "view.front",
    "view.back",
    "view.right",
    "view.left",
    "view.follow",
    "view.track",
    "view.maximize",
    "view.split_right",
    "view.split_down",
    "view.close",
    "overlay.grid",
    "overlay.collisions",
    "overlay.inertia",
    "overlay.joints",
    "ui.sidebar",
    "ui.inspector",
];

/// The Settings group's commands.
const SETTINGS: &[&str] = &["ui.theme_light", "ui.theme_dark", "ui.theme_system"];

/// A command's icon.
fn command_icon(id: &str) -> IconId {
    match id {
        "sim.play_pause" => IconId::Play,
        "sim.step" => IconId::Step,
        "sim.reset" => IconId::Reset,
        "edit.undo" => IconId::Undo,
        "edit.redo" => IconId::Redo,
        "view.focus" => IconId::Focus,
        "view.home" => IconId::Home,
        "view.projection" => IconId::ProjOrtho,
        "view.follow" => IconId::Follow,
        "view.track" => IconId::Track,
        "view.maximize" => IconId::Maximize,
        "view.split_right" | "view.split_down" | "ui.sidebar" | "ui.inspector" => {
            IconId::LayoutSplit
        }
        "view.close" => IconId::Close,
        "overlay.grid" => IconId::OverlayGrid,
        "overlay.collisions" => IconId::OverlayCollision,
        "overlay.inertia" => IconId::OverlayInertia,
        "overlay.joints" => IconId::OverlayJoints,
        "ui.theme_light" | "ui.theme_dark" | "ui.theme_system" => IconId::Settings,
        id => Tool::ALL
            .into_iter()
            .find(|t| t.binding() == id)
            .map_or(IconId::Search, Tool::icon),
    }
}

/// What the palette's items are read from.
#[derive(SystemParam)]
pub(crate) struct Sources<'w, 's> {
    map: Option<Res<'w, InputMap>>,
    panels: Res<'w, PanelRegistry>,
    status: Res<'w, PanelStatus>,
    hosts: Res<'w, ShownPanels>,
    layout: Res<'w, ShellLayout>,
    phase: Res<'w, CurrentPhase>,
    gz: Option<Res<'w, GzStatus>>,
    undo: Option<Res<'w, UndoStack>>,
    topics: Option<Res<'w, GzTopics>>,
    entities: Query<'w, 's, (Entity, &'static GzEntity)>,
    new_entities: Query<'w, 's, (), Added<GzEntity>>,
    gone: RemovedComponents<'w, 's, GzEntity>,
    /// What the items read of the connection's status, last look.
    seen_gz: Local<'s, Option<GzKey>>,
}

/// What the items read of [`GzStatus`]: whether the sim is there and
/// paused, and what the server takes. (The stats change every message;
/// these rarely.)
type GzKey = (Option<bool>, Option<bool>, Option<bool>);

impl Sources<'_, '_> {
    /// What the items come from changed since the last look: the
    /// connection, the world's entities, the topics, the undo stack, the
    /// panels' state or the layout.
    fn changed(&mut self) -> bool {
        let gone = self.gone.read().count() > 0;
        let gz = self
            .gz
            .as_deref()
            .map(|s| (s.sim.as_ref().map(|x| x.paused), s.edits, s.publishes));
        let gz_changed = std::mem::replace(&mut *self.seen_gz, gz) != gz;
        gone || gz_changed
            || self.phase.is_changed()
            || self.topics.as_ref().is_some_and(Res::is_changed)
            || self.undo.as_ref().is_some_and(Res::is_changed)
            || self.status.is_changed()
            || self.hosts.is_changed()
            || self.layout.is_changed()
            || !self.new_entities.is_empty()
    }

    /// Why the command `id` can't run now, if it can't.
    fn why_not(&self, id: &str) -> Option<&'static str> {
        let live = self.phase.0 == Phase::Live;
        let sim = self.gz.as_deref().and_then(|s| s.sim.as_ref());
        let transport = match id {
            "sim.play_pause" => Some(Transport::PlayPause),
            "sim.step" => Some(Transport::Step),
            "sim.reset" => Some(Transport::Reset),
            _ => None,
        };
        if let Some(t) = transport {
            let why = match (live, sim) {
                (false, _) => "Not connected",
                (true, None) => "Waiting for the world's clock",
                (true, Some(_)) => "Not while running",
            };
            return (!t.enabled(self.phase.0, sim)).then_some(why);
        }
        let undo = self.undo.as_deref();
        match id {
            "edit.undo" if undo.and_then(UndoStack::undo_label).is_none() => {
                Some("Nothing to undo")
            }
            "edit.redo" if undo.and_then(UndoStack::redo_label).is_none() => {
                Some("Nothing to redo")
            }
            "tool.translate" | "tool.rotate" if !edits_allowed(self.gz.as_deref()) => {
                Some("This server doesn't take edits")
            }
            // The view layout works offline; looking at the world doesn't.
            "view.maximize" | "view.split_right" | "view.split_down" | "view.close" => None,
            id if !live && (id.starts_with("view.") || id.starts_with("overlay.")) => {
                Some("Not connected")
            }
            _ => None,
        }
    }

    /// A `shortcuts.json` command, if the map has it: its label (Undo and
    /// Redo naming what they'd do), keys, icon and Gazebo names.
    fn command(&self, group: Group, id: &'static str) -> Option<Item> {
        let map = self.map.as_deref()?;
        let b = map.binding(id)?;
        let undo = self.undo.as_deref();
        let name = match id {
            "edit.undo" => undo
                .and_then(UndoStack::undo_label)
                .map_or_else(|| b.label.clone(), |l| format!("Undo {l}")),
            "edit.redo" => undo
                .and_then(UndoStack::redo_label)
                .map_or_else(|| b.label.clone(), |l| format!("Redo {l}")),
            _ => b.label.clone(),
        };
        Some(Item {
            group,
            icon: command_icon(id),
            name,
            hint: String::new(),
            shortcut: shortcut_label(map, id),
            keywords: gazebo_names(id).to_vec(),
            run: Run::Binding(id.to_string()),
            disabled: self.why_not(id),
        })
    }

    /// Everything the palette can list now.
    pub(crate) fn items(&self) -> Vec<Item> {
        let mut out: Vec<Item> = COMMANDS
            .iter()
            .filter_map(|id| self.command(Group::Commands, id))
            .collect();
        let shortcut = |b: Option<&str>| {
            b.and_then(|b| self.map.as_deref().and_then(|m| shortcut_label(m, b)))
        };
        // The overlays without a key (§7.3).
        for o in [Overlay::CenterOfMass, Overlay::Frames, Overlay::Wireframe] {
            out.push(Item {
                group: Group::Commands,
                icon: IconId::Overlays,
                name: format!("Toggle {}", o.label().to_lowercase()),
                hint: String::new(),
                shortcut: None,
                keywords: gazebo_names("overlays").to_vec(),
                run: Run::Overlay(o),
                disabled: (self.phase.0 != Phase::Live).then_some("Not connected"),
            });
        }
        // The shown panels' own actions.
        for def in self.panels.iter().filter(|d| self.hosts.is_shown(d.id)) {
            for a in &def.header_actions {
                let (icon, label) = a.look(self.status.action_on(def.id, a.id));
                let enabled = self.status.header_action_enabled(def.id, a);
                out.push(Item {
                    group: Group::Commands,
                    icon,
                    name: format!("{}: {label}", def.title),
                    hint: String::new(),
                    shortcut: shortcut(a.binding),
                    keywords: vec![],
                    run: Run::PanelAction {
                        panel: def.id,
                        action: a.id,
                    },
                    disabled: (!enabled).then_some(a.why.unwrap_or("Not available now")),
                });
            }
            for a in &def.footer {
                let enabled = self.status.action_enabled_or(def.id, a.id, a.enabled);
                out.push(Item {
                    group: Group::Commands,
                    icon: def.icon,
                    name: format!("{}: {}", def.title, a.label.trim_end_matches('…')),
                    hint: String::new(),
                    shortcut: None,
                    keywords: vec![],
                    run: Run::PanelAction {
                        panel: def.id,
                        action: a.id,
                    },
                    disabled: (!enabled).then_some(a.why.unwrap_or("Not available now")),
                });
            }
        }
        // The world's models, links and lights.
        for (e, g) in &self.entities {
            let icon = match g.kind {
                NodeKind::Model => IconId::ShapeBox,
                NodeKind::Link => IconId::ShapeCylinder,
                NodeKind::Light => IconId::LightPoint,
                NodeKind::Visual => continue,
            };
            out.push(Item {
                group: Group::Entities,
                icon,
                name: g.name.clone(),
                // Its path, or its kind where that is only its name.
                hint: if g.path == g.name {
                    g.kind.as_str().to_string()
                } else {
                    g.path.clone()
                },
                shortcut: None,
                keywords: vec![],
                run: Run::Select(e),
                disabled: None,
            });
        }
        if let Some(topics) = self
            .topics
            .as_deref()
            .filter(|_| self.phase.0 == Phase::Live)
        {
            for t in &topics.list {
                out.push(Item {
                    group: Group::Topics,
                    icon: IconId::ActivityTopics,
                    name: t.name.clone(),
                    hint: t.msg_type.clone(),
                    shortcut: None,
                    keywords: vec![],
                    run: Run::Echo(t.name.clone()),
                    disabled: None,
                });
            }
        }
        // Each panel opens, or pops out.
        for def in self.panels.iter() {
            let words = gazebo_names(def.id.name()).to_vec();
            out.push(Item {
                group: Group::Panels,
                icon: def.icon,
                name: def.title.to_string(),
                hint: "Open in the sidebar".into(),
                shortcut: shortcut(def.binding),
                keywords: words.clone(),
                run: Run::Ui(UiCommand::OpenPanel {
                    panel: def.id,
                    slot: Slot::Primary,
                }),
                disabled: None,
            });
            out.push(Item {
                group: Group::Panels,
                icon: IconId::PopOut,
                name: format!("Pop out {}", def.title),
                hint: "A window over the viewport".into(),
                shortcut: None,
                keywords: words,
                run: Run::Ui(UiCommand::PopOut(def.id)),
                disabled: self
                    .layout
                    .is_popped(def.id)
                    .then_some("Already popped out"),
            });
        }
        out.extend(
            SETTINGS
                .iter()
                .filter_map(|id| self.command(Group::Settings, id)),
        );
        out
    }
}

/// The palette opens and closes with [`PaletteOpen`]: open, it starts
/// with an empty query and takes the keys; closed, focus goes back where
/// it was.
pub(crate) fn sync_palette(
    open: Res<PaletteOpen>,
    roots: Query<Entity, With<PaletteRoot>>,
    mut state: ResMut<PaletteState>,
    mut focus: ResMut<InputFocus>,
    mut commands: Commands,
) {
    if !open.is_changed() {
        return;
    }
    let shown = roots.iter().next();
    match (open.0, shown) {
        (true, None) => {
            commands.spawn(palette());
            *state = PaletteState {
                return_focus: focus.get(),
                ..default()
            };
            state.dirty = true;
        }
        (false, Some(root)) => {
            commands.entity(root).despawn();
            match state.return_focus.take() {
                Some(e) => focus.set(e, FocusCause::Navigated),
                None => focus.clear(),
            }
        }
        _ => {}
    }
}

/// A [`StartPalette`]: opens the palette, then types its query once the
/// field exists.
pub(crate) fn apply_start(
    start: Option<Res<StartPalette>>,
    open: Res<PaletteOpen>,
    fields: Query<(), With<PaletteField>>,
    mut inputs: Query<(&ChildOf, &mut EditableText), With<TextFieldInput>>,
    mut out: MessageWriter<UiCommand>,
    mut commands: Commands,
) {
    let Some(start) = start else {
        return;
    };
    if !open.0 {
        out.write(UiCommand::OpenPalette);
        return;
    }
    for (parent, mut text) in &mut inputs {
        if fields.contains(parent.parent()) {
            set_field_text(&mut text, &start.0);
            commands.remove_resource::<StartPalette>();
        }
    }
}

/// The field takes the keys once it exists.
pub(crate) fn focus_field(
    fields: Query<(Entity, &ChildOf), Added<TextFieldInput>>,
    roots: Query<(), With<PaletteField>>,
    mut focus: ResMut<InputFocus>,
) {
    for (e, parent) in &fields {
        if roots.contains(parent.parent()) {
            focus.set(e, FocusCause::Navigated);
        }
    }
}

/// While the palette is open its field keeps the keys: a press on its
/// card that runs nothing (a label, a dimmed row, the padding) or on its
/// rows hands them back, so typed keys never reach the world behind it.
pub(crate) fn keep_focus(
    open: Res<PaletteOpen>,
    fields: Query<(Entity, &ChildOf), With<TextFieldInput>>,
    roots: Query<(), With<PaletteField>>,
    mut focus: ResMut<InputFocus>,
) {
    if !open.0 {
        return;
    }
    let field = fields
        .iter()
        .find(|(_, p)| roots.contains(p.parent()))
        .map(|(e, _)| e);
    if let Some(field) = field.filter(|f| focus.get() != Some(*f)) {
        focus.set(field, FocusCause::Navigated);
    }
}

/// After ↑ or ↓, the caret goes back to the end of the query.
pub(crate) fn caret_to_end(
    mut state: ResMut<PaletteState>,
    mut inputs: Query<(&ChildOf, &mut EditableText), With<TextFieldInput>>,
    fields: Query<(), With<PaletteField>>,
) {
    if !state.caret_end {
        return;
    }
    state.caret_end = false;
    for (parent, mut text) in &mut inputs {
        if fields.contains(parent.parent()) {
            text.queue_edit(bevy::text::TextEdit::TextEnd(false));
        }
    }
}

/// The field's text is the query.
pub(crate) fn query_text(
    inputs: Query<(&EditableText, &ChildOf), Changed<EditableText>>,
    fields: Query<(), With<PaletteField>>,
    mut state: ResMut<PaletteState>,
) {
    for (text, parent) in &inputs {
        if fields.contains(parent.parent()) {
            let value = text.value().to_string();
            if state.query != value {
                state.query = value;
                state.dirty = true;
            }
        }
    }
}

/// A new query (or the palette opening) ranks the items again and
/// chooses the first available result; so does a change to what they
/// come from (a world loading while it's open), which keeps a result
/// chosen with ↑ or ↓ where it still is.
pub(crate) fn refresh_results(
    open: Res<PaletteOpen>,
    mut sources: Sources,
    mut state: ResMut<PaletteState>,
) {
    let changed = sources.changed();
    if !open.0 || !(state.dirty || changed) {
        return;
    }
    let new_query = std::mem::take(&mut state.dirty);
    let results = rank(sources.items(), &state.query);
    if !new_query && results == state.results {
        return;
    }
    if new_query {
        state.moved = false;
    }
    let kept = state
        .selected
        .and_then(|i| state.results.get(i))
        .filter(|_| state.moved)
        .and_then(|was| results.iter().position(|r| r.run == was.run))
        .filter(|i| results[*i].disabled.is_none());
    state.selected = kept.or_else(|| step(&results, None, 1));
    state.results = results;
    state.built = false;
}

/// The rows follow the results; the chosen one shows chosen and stays in
/// view.
pub(crate) fn sync_rows(
    theme: Res<Theme>,
    mut state: ResMut<PaletteState>,
    lists: Query<Entity, With<PaletteRows>>,
    mut rows: Query<(Entity, &PaletteRow, Has<Active>, &mut AccessibilityNode)>,
    mut scrolls: Query<&mut ScrollPosition, With<PaletteRows>>,
    mut commands: Commands,
) {
    let Ok(list) = lists.single() else {
        return;
    };
    if !state.built {
        state.built = true;
        // New results start at the top ([`reveal_selected`] then brings a
        // chosen row further down into view).
        for mut scroll in &mut scrolls {
            if scroll.y != 0.0 {
                scroll.y = 0.0;
            }
        }
        commands.entity(list).despawn_children();
        let t = theme.clone();
        let results = state.results.clone();
        let selected = state.selected;
        commands.entity(list).with_children(|l| {
            if results.is_empty() {
                l.spawn(no_results());
            }
            let mut group = None;
            for (i, item) in results.iter().enumerate() {
                if group != Some(item.group) {
                    group = Some(item.group);
                    l.spawn(group_label(item.group.label()));
                }
                let mut row = l.spawn(result_row(&t, i, item));
                if item.disabled.is_some() {
                    row.insert(InteractionDisabled);
                }
                if selected == Some(i) {
                    row.insert(Active);
                }
            }
        });
        return;
    }
    for (e, row, active, mut access) in &mut rows {
        let want = state.selected == Some(row.0);
        if want != active {
            if want {
                commands.entity(e).insert(Active);
            } else {
                commands.entity(e).remove::<Active>();
            }
        }
        // The chosen result, to a screen reader (focus stays in the field).
        if access.is_selected() != Some(want) {
            access.set_selected(want);
        }
    }
}

/// Keeps the chosen row in the list's view, once the rows are laid out
/// (rows built this frame have no size yet: it tries again).
pub(crate) fn reveal_selected(
    state: Res<PaletteState>,
    mut lists: Query<(&mut ScrollPosition, &ComputedNode), With<PaletteRows>>,
    rows: Query<(&PaletteRow, &ComputedNode, &UiGlobalTransform)>,
    list_at: Query<&UiGlobalTransform, With<PaletteRows>>,
    mut pending: Local<bool>,
) {
    if !state.is_changed() && !*pending {
        return;
    }
    *pending = true;
    let (Ok((mut scroll, list)), Ok(at)) = (lists.single_mut(), list_at.single()) else {
        return;
    };
    let Some((_, node, row_at)) = rows.iter().find(|(r, ..)| Some(r.0) == state.selected) else {
        *pending = state.selected.is_some() && !state.results.is_empty();
        return;
    };
    if node.size().y == 0.0 || list.size().y == 0.0 {
        return;
    }
    *pending = false;
    let scale = list.inverse_scale_factor();
    let half = list.size().y * scale / 2.0;
    let row_y = (row_at.translation.y - at.translation.y) * scale;
    let row_h = node.size().y * scale;
    let (top, bottom) = (row_y - row_h / 2.0, row_y + row_h / 2.0);
    if top < -half {
        scroll.y += top + half;
    } else if bottom > half {
        scroll.y += bottom - half;
    }
}

/// ↑ and ↓ choose the result above or below (the available ones,
/// wrapping); Esc closes the palette.
pub(crate) fn on_field_key(
    mut ev: On<FocusedInput<KeyboardInput>>,
    inputs: Query<&ChildOf, With<TextFieldInput>>,
    fields: Query<(), With<PaletteField>>,
    mut state: ResMut<PaletteState>,
    mut out: MessageWriter<UiCommand>,
) {
    if ev.input.state != ButtonState::Pressed || ev.event_target() != ev.focused_entity {
        return;
    }
    let ours = inputs
        .get(ev.focused_entity)
        .is_ok_and(|p| fields.contains(p.parent()));
    if !ours {
        return;
    }
    let by = match ev.input.key_code {
        KeyCode::ArrowDown => 1,
        KeyCode::ArrowUp => -1,
        KeyCode::Escape => {
            ev.propagate(false);
            out.write(UiCommand::ClosePalette);
            return;
        }
        _ => return,
    };
    ev.propagate(false);
    // The field's own arrow handling moves its caret: back to the end.
    state.caret_end = true;
    let next = step(&state.results, state.selected, by);
    if state.selected != next {
        state.selected = next;
        state.moved = true;
    }
}

/// What running a result writes.
#[derive(SystemParam)]
pub(crate) struct Runner<'w, 's> {
    ui: MessageWriter<'w, UiCommand>,
    bindings: MessageWriter<'w, RunBinding>,
    overlays: MessageWriter<'w, OverlayCommand>,
    selection: ResMut<'w, Selection>,
    buttons: Query<'w, 's, (Entity, &'static ActionButton)>,
    commands: Commands<'w, 's>,
}

impl Runner<'_, '_> {
    /// Runs `item` and closes the palette (an unavailable one, or none,
    /// does nothing).
    fn run(&mut self, item: Option<&Item>) {
        // Nothing to run (no results, or only unavailable ones): it stays.
        let Some(item) = item.filter(|i| i.disabled.is_none()) else {
            return;
        };
        self.ui.write(UiCommand::ClosePalette);
        match &item.run {
            Run::Binding(id) => {
                self.bindings.write(RunBinding(id.clone()));
            }
            Run::Ui(cmd) => {
                self.ui.write(cmd.clone());
            }
            Run::PanelAction { panel, action } => {
                let entity = self
                    .buttons
                    .iter()
                    .find(|(_, b)| b.panel == *panel && b.action == *action)
                    .map_or(Entity::PLACEHOLDER, |(e, _)| e);
                self.commands.trigger(PanelActionActivated {
                    entity,
                    panel: *panel,
                    action: *action,
                });
            }
            Run::Select(e) => {
                *self.selection = Selection {
                    entities: vec![*e],
                    parts: vec![],
                };
                self.bindings.write(RunBinding("view.focus".into()));
            }
            Run::Overlay(o) => {
                self.overlays.write(OverlayCommand::toggle(*o));
            }
            Run::Echo(topic) => {
                self.commands.insert_resource(TopicsStart {
                    echo: topic.clone(),
                    beside: None,
                });
            }
        }
    }
}

/// Enter runs the chosen result.
pub(crate) fn on_submit(
    ev: On<TextSubmit>,
    fields: Query<(), With<PaletteField>>,
    state: Res<PaletteState>,
    mut runner: Runner,
) {
    if fields.contains(ev.entity) {
        let item = state.selected.and_then(|i| state.results.get(i));
        runner.run(item);
    }
}

/// A click on a result runs it.
pub(crate) fn on_row(
    ev: On<Activate>,
    rows: Query<&PaletteRow>,
    state: Res<PaletteState>,
    mut runner: Runner,
) {
    if let Ok(row) = rows.get(ev.entity) {
        runner.run(state.results.get(row.0));
    }
}

/// A press outside the card closes the palette.
pub(crate) fn on_backdrop(
    ev: On<PointerPress>,
    backdrops: Query<(), With<PaletteBackdrop>>,
    mut out: MessageWriter<UiCommand>,
) {
    if backdrops.contains(ev.entity) {
        out.write(UiCommand::ClosePalette);
    }
}

/// The top bar's search trigger opens the palette.
pub(crate) fn on_trigger(
    ev: On<Activate>,
    triggers: Query<(), With<SearchTrigger>>,
    mut out: MessageWriter<UiCommand>,
) {
    if triggers.contains(ev.entity) {
        out.write(UiCommand::OpenPalette);
    }
}
