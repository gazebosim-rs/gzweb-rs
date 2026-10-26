//! The Workspaces panel's systems: the cards and the summary follow the
//! workspaces and the layout; the cards switch, the buttons save, save as
//! new and reset, the name row names a new workspace, Delete drops one of
//! the user's, and Export writes the layouts.

use bevy::input::keyboard::KeyboardInput;
use bevy::input::ButtonState;
use bevy::input_focus::{FocusCause, FocusedInput, InputFocus};
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Activate;
use gz_bevy::view::input::InputMap;

use super::view::{
    card_look, note_of, spawn_card, Card, CardDelete, CardLook, Cards, CurrentHeader, IoItem,
    LayoutButton, LoadFailed, LoadFailedText, NameButton, NameField, NameRow, Spawned, StoreNote,
    SummaryValue,
};
use super::{PanelState, NEW};
use crate::panels::{PanelActionActivated, PanelRegistry};
use crate::shell::status_bar::readouts::StatusNote;
use crate::shell::workspaces::{exported, NameWorkspace, Persisted, WorkspaceStore};
use crate::shell::{
    CurrentLayout, Slot, UiCommand, WorkspaceCommand, Workspaces, WORKSPACES_PANEL,
};
use crate::theme::visuals::{Active, Content};
use crate::theme::{Theme, ThemeMode};
use crate::widgets::layout::set_shown;
use crate::widgets::section::SectionCount;
use crate::widgets::text::set_text;
use crate::widgets::text_field::{set_field_text, TextFieldInput};
use crate::widgets::{TextSubmit, Tooltip};

/// A [`WorkspacesStart`](super::WorkspacesStart): opens the panel.
pub(super) fn apply_start(
    start: Option<Res<super::WorkspacesStart>>,
    mut ui: MessageWriter<UiCommand>,
    mut commands: Commands,
) {
    if start.is_some() {
        ui.write(UiCommand::OpenPanel {
            panel: WORKSPACES_PANEL,
            slot: Slot::Primary,
        });
        commands.remove_resource::<super::WorkspacesStart>();
    }
}

/// The longest name "Save to …" holds.
const SAVE_NAME_MAX: usize = 8;

/// Save as new asked for from ⌘K: the name row opens.
pub(super) fn start_naming(
    asked: Option<Res<NameWorkspace>>,
    mut state: ResMut<PanelState>,
    mut commands: Commands,
) {
    if asked.is_some() {
        state.naming = true;
        commands.remove_resource::<NameWorkspace>();
    }
}

/// The cards follow the workspaces and the current layout: a card whose
/// look changed is respawned in its place (only its mark, when that's all
/// that changed), and one that had the keys keeps them.
#[allow(clippy::too_many_arguments)]
pub(super) fn sync_cards(
    workspaces: Res<Workspaces>,
    current: Res<CurrentLayout>,
    panels: Res<PanelRegistry>,
    map: Option<Res<InputMap>>,
    theme: Res<Theme>,
    cards: Query<(Entity, Option<&Children>), With<Cards>>,
    added: Query<(), Added<Cards>>,
    mut focus: ResMut<InputFocus>,
    mut shown: Local<Vec<(CardLook, Spawned)>>,
    mut commands: Commands,
) {
    let Ok((parent, children)) = cards.single() else {
        return;
    };
    let fresh = !added.is_empty();
    if !fresh && !workspaces.is_changed() && !current.is_changed() && !theme.is_changed() {
        return;
    }
    let looks: Vec<CardLook> = (0..workspaces.list().len())
        .map(|i| {
            card_look(
                &workspaces,
                i,
                current.layout.as_ref(),
                current.modified,
                &panels,
                map.as_deref(),
            )
        })
        .collect();
    let focused = focus.get();
    let all = fresh || theme.is_changed() || shown.len() != looks.len();
    if all {
        for &c in children.into_iter().flatten() {
            commands.entity(c).despawn();
        }
        shown.clear();
    }
    let mut next = Vec::with_capacity(looks.len());
    for (i, look) in looks.into_iter().enumerate() {
        let was = shown.get(i).cloned();
        let spawned = match was {
            Some((old, spawned)) if old == look => spawned,
            Some((old, spawned))
                if CardLook {
                    current: look.current,
                    ..old.clone()
                } == look =>
            {
                let mut c = commands.entity(spawned.card);
                if look.current {
                    c.insert(Active);
                } else {
                    c.remove::<Active>();
                }
                spawned
            }
            was => {
                if let Some((_, old)) = was {
                    commands.entity(old.item).despawn();
                }
                let spawned = spawn_card(&mut commands, &theme, i, look.clone());
                commands.entity(parent).insert_children(i, &[spawned.item]);
                if focused == Some(was.map_or(Entity::PLACEHOLDER, |(_, s)| s.card)) {
                    focus.set(spawned.card, FocusCause::Navigated);
                }
                spawned
            }
        };
        next.push((look, spawned));
    }
    *shown = next;
}

/// The summary's values, the section's "Build · modified", and the
/// buttons: "Save to Build", and Reset only with something to put back.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn sync_summary(
    workspaces: Res<Workspaces>,
    current: Res<CurrentLayout>,
    panels: Res<PanelRegistry>,
    theme: Res<Theme>,
    mut values: Query<(&SummaryValue, &mut Text)>,
    headers: Query<&Children, With<CurrentHeader>>,
    mut counts: Query<&mut Text, (With<SectionCount>, Without<SummaryValue>)>,
    buttons: Query<(Entity, &LayoutButton, &Children, Has<InteractionDisabled>)>,
    mut labels: Query<&mut Text, (With<Content>, Without<SummaryValue>, Without<SectionCount>)>,
    added: Query<(), Added<SummaryValue>>,
    mut buttons_said: Local<Option<(String, Option<bool>)>>,
    mut commands: Commands,
) {
    let fresh = !added.is_empty();
    if !fresh && !workspaces.is_changed() && !current.is_changed() && !theme.is_changed() {
        return;
    }
    let Some(l) = current.layout.as_ref() else {
        return;
    };
    let title = |name: &str| {
        panels
            .iter()
            .find(|d| d.id.name() == name)
            .map_or(name.to_string(), |d| d.title.to_string())
    };
    let px = |w: f32| format!("{} px", w.round());
    for (v, mut t) in &mut values {
        let s = match v {
            SummaryValue::Sidebar => match (&l.primary, l.sidebar_open) {
                (Some(p), true) => {
                    let mut names = title(p);
                    if let Some(s) = &l.secondary {
                        names = format!("{names} + {}", title(s));
                    }
                    format!("{names} · {}", px(l.sidebar_w))
                }
                _ => "collapsed".into(),
            },
            SummaryValue::Inspector => {
                if l.inspector_open {
                    format!("shown · {}", px(l.inspector_w))
                } else {
                    "collapsed".into()
                }
            }
            SummaryValue::Viewports => {
                format!("{} · {}", current.views.len(), current.views.join(", "))
            }
            SummaryValue::Floating => {
                if l.popouts.is_empty() {
                    "none".into()
                } else {
                    l.popouts
                        .iter()
                        .map(|p| title(&p.panel))
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            }
            SummaryValue::Theme => {
                let shade = if theme.is_dark { "dark" } else { "light" };
                if theme.mode == ThemeMode::System {
                    format!("{shade} · follows OS")
                } else {
                    shade.into()
                }
            }
        };
        set_text(&mut t, &s);
    }
    let name = workspaces.name();
    let head = if current.modified {
        format!("{name} · modified")
    } else {
        name.to_string()
    };
    for children in &headers {
        for &c in children {
            if let Ok(mut t) = counts.get_mut(c) {
                set_text(&mut t, &head);
            }
        }
    }
    // The buttons only when what they say changes (a tooltip put back
    // while it shows would flicker).
    let key = (name.to_string(), current.reset);
    if !fresh && buttons_said.as_ref() == Some(&key) {
        return;
    }
    *buttons_said = Some(key);
    for (e, b, children, disabled) in &buttons {
        let save_tip = Some(Tooltip::new(format!("Save to {name}")));
        let (label, enabled, tip) = match b {
            // A long name would crowd the row: "Save", named in the tip.
            LayoutButton::Save if name.chars().count() <= SAVE_NAME_MAX => {
                (format!("Save to {name}"), true, save_tip)
            }
            LayoutButton::Save => ("Save".to_string(), true, save_tip),
            LayoutButton::SaveAsNew => ("Save as new…".to_string(), true, None),
            LayoutButton::Reset => (
                "Reset".to_string(),
                current.reset.is_some(),
                Some(match current.reset {
                    Some(true) => Tooltip::new("Reset").detail("Back to its default layout"),
                    Some(false) => Tooltip::new("Reset").detail("Back to the layout as saved"),
                    None => {
                        Tooltip::new("Reset").detail("Nothing to reset: the layout is as saved")
                    }
                }),
            ),
        };
        for &c in children {
            if let Ok(mut t) = labels.get_mut(c) {
                set_text(&mut t, &label);
            }
        }
        let mut ec = commands.entity(e);
        if enabled && disabled {
            ec.remove::<InteractionDisabled>();
        } else if !enabled && !disabled {
            ec.insert(InteractionDisabled);
        }
        if let Some(tip) = tip {
            ec.insert(tip);
        }
    }
}

/// The name row shows while naming; it opens empty.
pub(super) fn sync_naming(
    state: Res<PanelState>,
    mut rows: Query<&mut Node, With<NameRow>>,
    names: Query<(), With<NameField>>,
    mut inputs: Query<(&mut EditableText, &ChildOf), With<TextFieldInput>>,
    added: Query<(), Added<NameRow>>,
) {
    if !state.is_changed() && added.is_empty() {
        return;
    }
    for mut node in &mut rows {
        set_shown(&mut node, state.naming);
    }
    if state.naming {
        for (mut t, p) in &mut inputs {
            if names.contains(p.parent()) {
                set_field_text(&mut t, "");
            }
        }
    }
}

/// Opening the name row gives its field the keys.
pub(super) fn focus_name(
    state: Res<PanelState>,
    fields: Query<(Entity, &ChildOf), With<TextFieldInput>>,
    names: Query<(), With<NameField>>,
    added: Query<(), Added<NameRow>>,
    mut focus: ResMut<InputFocus>,
) {
    if !state.naming || (!state.is_changed() && added.is_empty()) {
        return;
    }
    if let Some((e, _)) = fields.iter().find(|(_, p)| names.contains(p.parent())) {
        focus.set(e, FocusCause::Navigated);
    }
}

/// The header's + names a new workspace.
pub(super) fn on_action(ev: On<PanelActionActivated>, mut state: ResMut<PanelState>) {
    if ev.panel == WORKSPACES_PANEL && ev.action == NEW {
        state.naming = true;
    }
}

/// A card switches to its workspace; its Delete drops it.
pub(super) fn on_card(
    ev: On<Activate>,
    cards: Query<&Card>,
    deletes: Query<&CardDelete>,
    mut out: MessageWriter<WorkspaceCommand>,
) {
    if let Ok(Card(i)) = cards.get(ev.entity) {
        out.write(WorkspaceCommand::Switch(*i));
    } else if let Ok(CardDelete(i)) = deletes.get(ev.entity) {
        out.write(WorkspaceCommand::Delete(*i));
    }
}

/// Save to …, Save as new… (the name row) and Reset.
pub(super) fn on_layout_button(
    ev: On<Activate>,
    buttons: Query<&LayoutButton, Without<InteractionDisabled>>,
    mut state: ResMut<PanelState>,
    mut out: MessageWriter<WorkspaceCommand>,
) {
    match buttons.get(ev.entity) {
        Ok(LayoutButton::Save) => {
            out.write(WorkspaceCommand::Save);
        }
        Ok(LayoutButton::SaveAsNew) => state.naming = true,
        Ok(LayoutButton::Reset) => {
            out.write(WorkspaceCommand::Reset);
        }
        Err(_) => {}
    }
}

/// The name field's text.
fn typed_name(
    names: &Query<(), With<NameField>>,
    inputs: &Query<(&EditableText, &ChildOf), With<TextFieldInput>>,
) -> String {
    inputs
        .iter()
        .find(|(_, p)| names.contains(p.parent()))
        .map(|(t, _)| t.value().to_string())
        .unwrap_or_default()
}

/// Saves the layout as a new workspace named `name` and closes the row; an
/// empty name leaves it open.
fn save_new(name: &str, state: &mut PanelState, out: &mut MessageWriter<WorkspaceCommand>) {
    if name.trim().is_empty() {
        return;
    }
    out.write(WorkspaceCommand::SaveAsNew(name.trim().to_string()));
    state.naming = false;
}

/// Cancel closes the name row; Save saves.
pub(super) fn on_name_button(
    ev: On<Activate>,
    buttons: Query<&NameButton>,
    names: Query<(), With<NameField>>,
    inputs: Query<(&EditableText, &ChildOf), With<TextFieldInput>>,
    mut state: ResMut<PanelState>,
    mut out: MessageWriter<WorkspaceCommand>,
) {
    match buttons.get(ev.entity) {
        Ok(NameButton::Cancel) => state.naming = false,
        Ok(NameButton::Save) => save_new(&typed_name(&names, &inputs), &mut state, &mut out),
        Err(_) => {}
    }
}

/// Enter in the name field saves.
pub(super) fn on_name_submit(
    ev: On<TextSubmit>,
    names: Query<(), With<NameField>>,
    mut state: ResMut<PanelState>,
    mut out: MessageWriter<WorkspaceCommand>,
) {
    if names.contains(ev.entity) {
        save_new(&ev.value, &mut state, &mut out);
    }
}

/// Esc in the name row closes it.
pub(super) fn on_name_key(
    ev: On<FocusedInput<KeyboardInput>>,
    rows: Query<(), With<NameRow>>,
    parents: Query<&ChildOf>,
    mut state: ResMut<PanelState>,
) {
    if ev.input.state != ButtonState::Pressed || ev.input.key_code != KeyCode::Escape {
        return;
    }
    let inside = parents
        .iter_ancestors(ev.focused_entity)
        .any(|a| rows.contains(a));
    if inside && state.naming {
        state.naming = false;
    }
}

/// Export layouts… saves them as `gz-workspaces.json` (downloaded in the
/// browser), saying where; Import is unavailable.
pub(super) fn on_io_item(
    ev: On<Activate>,
    items: Query<&IoItem, Without<InteractionDisabled>>,
    workspaces: Res<Workspaces>,
    time: Res<Time<Real>>,
    mut note: ResMut<StatusNote>,
) {
    if items.get(ev.entity) != Ok(&IoItem::Export) {
        return;
    }
    match gz_bevy::save::save_text(
        "gz-workspaces.json",
        "application/json",
        &exported(&workspaces),
    ) {
        Ok(at) => note.show(
            format!("Exported the layouts to {at}"),
            time.elapsed_secs_f64(),
        ),
        Err(e) => warn!("Couldn't export the layouts: {e}"),
    }
}

/// The footer's failure line and note follow the store: the failure shows
/// from the read that found it until a good file is written over it, and
/// a footer built before the store was read catches up.
#[allow(clippy::type_complexity)]
pub(super) fn sync_store(
    persisted: Res<Persisted>,
    store: Option<Res<WorkspaceStore>>,
    added: Query<(), Or<(Added<LoadFailed>, Added<StoreNote>)>>,
    mut lines: Query<(&mut Node, &mut bevy::a11y::AccessibilityNode), With<LoadFailed>>,
    mut texts: Query<&mut Text, (With<LoadFailedText>, Without<StoreNote>)>,
    mut notes: Query<&mut Text, (With<StoreNote>, Without<LoadFailedText>)>,
) {
    let store_changed = store.as_ref().is_some_and(Res::is_changed);
    if !persisted.is_changed() && !store_changed && added.is_empty() {
        return;
    }
    let why = persisted.failed();
    for (mut node, mut access) in &mut lines {
        set_shown(&mut node, why.is_some());
        match why {
            Some(why) => {
                if access.is_hidden() || access.label() != Some(why) {
                    access.clear_hidden();
                    access.set_label(why);
                }
            }
            None => {
                if !access.is_hidden() {
                    access.set_hidden();
                }
            }
        }
    }
    for mut text in &mut texts {
        set_text(&mut text, why.unwrap_or_default());
    }
    let note = note_of(store.as_deref(), persisted.read_only());
    for mut text in &mut notes {
        set_text(&mut text, &note);
    }
}
