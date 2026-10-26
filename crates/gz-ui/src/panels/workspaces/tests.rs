//! The Workspaces panel: its cards, summary, buttons and name row, and
//! what they do to the workspaces.

use bevy::input::keyboard::KeyCode;
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Activate;

use super::view::{
    detail_of, note_of, Card, CardDelete, CurrentHeader, IoItem, LayoutButton, NameButton,
    NameField, NameRow, SummaryValue,
};
use super::{PanelState, NEW};
use crate::panels::PanelActionActivated;
use crate::shell::status_bar::{SwitcherItem, WorkspaceName};
use crate::shell::workspaces::{
    exported, NameWorkspace, StoreAt, WorkspaceFile, WorkspaceLayout, WorkspaceStore,
};
use crate::shell::{
    CurrentLayout, ShellLayout, Slot, UiCommand, WorkspaceCommand, Workspaces, WORKSPACES_PANEL,
};
use crate::test_support::shell::*;
use crate::test_support::{descendants, frames};
use crate::theme::visuals::Active;
use crate::widgets::section::SectionCount;
use crate::widgets::TextSubmit;

/// The shell, past start-up, with the Workspaces panel open.
fn panel_app() -> App {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    // Start-up lasts 30 frames and a real second.
    app.world_mut().resource_mut::<ShellLayout>().startup = false;
    frames(&mut app, 2);
    app.world_mut().write_message(UiCommand::OpenPanel {
        panel: WORKSPACES_PANEL,
        slot: Slot::Primary,
    });
    frames(&mut app, 4);
    app
}

fn workspaces(app: &App) -> &Workspaces {
    app.world().resource::<Workspaces>()
}

fn current(app: &App) -> &CurrentLayout {
    app.world().resource::<CurrentLayout>()
}

/// The texts under `e`, joined by " | ".
fn texts(app: &mut App, e: Entity) -> String {
    let world = app.world_mut();
    descendants(world, e)
        .into_iter()
        .filter_map(|d| world.get::<Text>(d).map(|t| t.0.clone()))
        .collect::<Vec<_>>()
        .join(" | ")
}

/// The cards, in order: (index, its texts, current).
fn cards(app: &mut App) -> Vec<(usize, String, bool)> {
    let found: Vec<(usize, Entity, bool)> = {
        let world = app.world_mut();
        world
            .query::<(Entity, &Card, Has<Active>)>()
            .iter(world)
            .map(|(e, c, a)| (c.0, e, a))
            .collect()
    };
    let mut out: Vec<_> = found
        .into_iter()
        .map(|(i, e, a)| (i, texts(app, e), a))
        .collect();
    out.sort_by_key(|c| c.0);
    out
}

/// The entity of the first `C` that `pick` accepts.
fn pick<C: Component>(app: &mut App, pick: impl Fn(&C) -> bool) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &C)>()
        .iter(world)
        .find(|(_, c)| pick(c))
        .unwrap()
        .0
}

fn value(app: &mut App, which: SummaryValue) -> String {
    let world = app.world_mut();
    world
        .query::<(&SummaryValue, &Text)>()
        .iter(world)
        .find(|(v, _)| **v == which)
        .map(|(_, t)| t.0.clone())
        .unwrap()
}

fn header(app: &mut App) -> String {
    let world = app.world_mut();
    let children: Vec<Entity> = world
        .query_filtered::<&Children, With<CurrentHeader>>()
        .single(world)
        .unwrap()
        .to_vec();
    children
        .into_iter()
        .find(|c| world.get::<SectionCount>(*c).is_some())
        .and_then(|c| world.get::<Text>(c))
        .map(|t| t.0.clone())
        .unwrap()
}

/// A layout button and whether it's enabled.
fn button(app: &mut App, which: LayoutButton) -> (Entity, bool) {
    let e = pick(app, |b: &LayoutButton| *b == which);
    (e, app.world().get::<InteractionDisabled>(e).is_none())
}

fn activate(app: &mut App, entity: Entity) {
    app.world_mut().trigger(Activate { entity });
    frames(app, 3);
}

fn naming(app: &App) -> bool {
    app.world().resource::<PanelState>().naming
}

/// The cards: §8's four, the current one marked, each with what it shows
/// and its keys; the summary names the layout under the panel, which
/// showing the panel doesn't change.
#[test]
fn the_panel_shows_the_workspaces_and_the_layout() {
    let mut app = panel_app();
    let c = cards(&mut app);
    assert_eq!(c.len(), 4);
    assert!(c[0].1.contains("Build | scene · inspector"), "{c:?}");
    assert!(c[0].1.contains("Alt+1"), "{c:?}");
    assert!(c[0].2 && !c[1].2);
    assert!(c[1].1.contains("Run | viewport only"), "{c:?}");
    assert!(
        c[2].1.contains("Debug | topics · plot · inspector"),
        "{c:?}"
    );
    assert!(c[3].1.contains("Review | plot"), "{c:?}");
    // The Workspaces panel stands in for Scene: not modified.
    assert_eq!(layout(&app).primary, Some(WORKSPACES_PANEL));
    assert!(!current(&app).modified);
    assert_eq!(header(&mut app), "Build");
    assert_eq!(value(&mut app, SummaryValue::Sidebar), "Scene · 300 px");
    assert_eq!(value(&mut app, SummaryValue::Inspector), "shown · 300 px");
    assert_eq!(value(&mut app, SummaryValue::Viewports), "1 · Perspective");
    assert_eq!(value(&mut app, SummaryValue::Floating), "none");
    assert_eq!(value(&mut app, SummaryValue::Theme), "light");
    let (save, _) = button(&mut app, LayoutButton::Save);
    assert_eq!(texts(&mut app, save), "Save to Build");
    // Reset gives §8's default.
    assert_eq!(current(&app).reset, Some(true));
}

/// A change shows "modified" (on the card too); Reset puts the layout as
/// saved back, then §8's default, then has nothing to do.
#[test]
fn reset_puts_back_the_saved_layout_then_the_default() {
    let mut app = panel_app();
    app.world_mut().resource_mut::<ShellLayout>().inspector_open = false;
    frames(&mut app, 3);
    assert!(current(&app).modified);
    assert_eq!(header(&mut app), "Build · modified");
    assert!(cards(&mut app)[0].1.contains("modified"));
    assert_eq!(value(&mut app, SummaryValue::Inspector), "collapsed");
    let (reset, enabled) = button(&mut app, LayoutButton::Reset);
    assert!(enabled);
    activate(&mut app, reset);
    assert!(layout(&app).inspector_open, "as saved");
    assert!(!current(&app).modified);
    // Again: §8's default, two views; then nothing left to reset.
    let (reset, _) = button(&mut app, LayoutButton::Reset);
    activate(&mut app, reset);
    frames(&mut app, 2);
    assert_eq!(
        value(&mut app, SummaryValue::Viewports),
        "2 · Perspective, Top"
    );
    assert_eq!(
        layout(&app).primary,
        Some(WORKSPACES_PANEL),
        "the panel stays"
    );
    let (_, enabled) = button(&mut app, LayoutButton::Reset);
    assert!(!enabled);
    assert_eq!(current(&app).reset, None);
}

/// A card switches, and the panel stays where it is, over the next
/// workspace's layout.
#[test]
fn a_card_switches_and_the_panel_stays() {
    let mut app = panel_app();
    let run = pick(&mut app, |c: &Card| c.0 == 1);
    activate(&mut app, run);
    assert_eq!(workspaces(&app).current, 1);
    let l = layout(&app);
    assert_eq!(l.primary, Some(WORKSPACES_PANEL));
    assert!(l.sidebar_open, "the panel keeps the sidebar open");
    assert!(!l.inspector_open, "Run's");
    assert_eq!(value(&mut app, SummaryValue::Sidebar), "collapsed");
    assert!(!current(&app).modified);
    let c = cards(&mut app);
    assert!(c[1].2 && !c[0].2);
    // Back to Build: Scene under the panel, as it was.
    let build = pick(&mut app, |c: &Card| c.0 == 0);
    activate(&mut app, build);
    assert_eq!(value(&mut app, SummaryValue::Sidebar), "Scene · 300 px");
}

/// Save as new… opens the name row with the keys; Enter saves a workspace
/// by that name and switches to it; its card has Delete, the switcher
/// lists it, and the file keeps it.
#[test]
fn save_as_new_names_a_workspace_of_the_users() {
    let mut app = panel_app();
    app.insert_resource(WorkspaceStore {
        at: StoreAt::Memory(None),
        theme: true,
    });
    let (new, _) = button(&mut app, LayoutButton::SaveAsNew);
    activate(&mut app, new);
    assert!(naming(&app));
    let row = find::<NameRow>(&mut app);
    assert!(displayed(&app, row));
    let field = find::<NameField>(&mut app);
    let input = focused(&app).expect("the name field has the keys");
    assert!(descendants(app.world(), field).contains(&input));
    // An empty name does nothing.
    app.world_mut().trigger(TextSubmit {
        entity: field,
        value: "  ".into(),
    });
    frames(&mut app, 2);
    assert!(naming(&app));
    app.world_mut().trigger(TextSubmit {
        entity: field,
        value: "Teleop".into(),
    });
    frames(&mut app, 4);
    assert!(!naming(&app));
    let w = workspaces(&app);
    assert_eq!(w.list().len(), 5);
    assert_eq!((w.current, w.name()), (4, "Teleop"));
    assert_eq!(header(&mut app), "Teleop");
    let delete = pick(&mut app, |d: &CardDelete| d.0 == 4);
    assert!(app.world().get_entity(delete).is_ok());
    let name = {
        let world = app.world_mut();
        world
            .query_filtered::<&Text, With<WorkspaceName>>()
            .single(world)
            .unwrap()
            .0
            .clone()
    };
    assert_eq!(name, "Teleop");
    let in_switcher = {
        let world = app.world_mut();
        world
            .query::<(&SwitcherItem, Has<Active>)>()
            .iter(world)
            .any(|(i, a)| *i == SwitcherItem::Switch(4) && a)
    };
    assert!(in_switcher, "listed and marked");
    let file = match &app.world().resource::<WorkspaceStore>().at {
        StoreAt::Memory(Some(t)) => serde_json::from_str::<WorkspaceFile>(t).unwrap(),
        other => panic!("{other:?}"),
    };
    assert_eq!(file.current, "user_1");
    assert_eq!(file.custom[0].name, "Teleop");
    assert!(file.workspaces.contains_key("user_1"));
    // The same name again is "teleop 2".
    app.world_mut()
        .write_message(WorkspaceCommand::SaveAsNew("teleop".into()));
    frames(&mut app, 2);
    assert_eq!(workspaces(&app).name(), "teleop 2");
}

/// Esc and Cancel close the name row; the header's + opens it.
#[test]
fn the_name_row_closes() {
    let mut app = panel_app();
    app.world_mut().trigger(PanelActionActivated {
        entity: Entity::PLACEHOLDER,
        panel: WORKSPACES_PANEL,
        action: NEW,
    });
    frames(&mut app, 3);
    assert!(naming(&app));
    let input = focused(&app).unwrap();
    focused_key(&mut app, input, KeyCode::Escape);
    frames(&mut app, 2);
    assert!(!naming(&app));
    let row = find::<NameRow>(&mut app);
    assert!(!displayed(&app, row));
    app.world_mut().resource_mut::<PanelState>().naming = true;
    frames(&mut app, 2);
    let cancel = pick(&mut app, |b: &NameButton| *b == NameButton::Cancel);
    activate(&mut app, cancel);
    assert!(!naming(&app));
    assert_eq!(workspaces(&app).list().len(), 4);
}

/// Delete drops one of the user's; deleting the current one goes back to
/// Build. The built-in ones can't be deleted.
#[test]
fn delete_drops_a_users_workspace() {
    let mut app = panel_app();
    app.world_mut()
        .write_message(WorkspaceCommand::SaveAsNew("Mine".into()));
    frames(&mut app, 4);
    assert_eq!(workspaces(&app).current, 4);
    let delete = pick(&mut app, |d: &CardDelete| d.0 == 4);
    activate(&mut app, delete);
    let w = workspaces(&app);
    assert_eq!((w.list().len(), w.current), (4, 0));
    assert_eq!(cards(&mut app).len(), 4);
    app.world_mut().write_message(WorkspaceCommand::Delete(1));
    frames(&mut app, 2);
    assert_eq!(workspaces(&app).list().len(), 4);
}

/// The switcher's Manage… opens the panel; ⌘K's Save as new opens it
/// naming.
#[test]
fn manage_and_the_palette_open_the_panel() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 3);
    let manage = pick(&mut app, |i: &SwitcherItem| *i == SwitcherItem::Manage);
    activate(&mut app, manage);
    assert_eq!(layout(&app).primary, Some(WORKSPACES_PANEL));
    assert!(!naming(&app));
    app.insert_resource(NameWorkspace);
    frames(&mut app, 3);
    assert!(naming(&app));
}

/// A store it couldn't read at start is the panel's error state (§3.3.1):
/// the footer says so, and where the file was kept, over the note.
#[test]
fn the_footer_says_the_store_couldnt_be_read() {
    use super::view::LoadFailed;
    use crate::shell::workspaces::LOAD_FAILED;

    let mut app = panel_app();
    let world = app.world_mut();
    assert_eq!(world.query::<&LoadFailed>().iter(world).count(), 0);
    let mut app = shell_app((1440, 900), 1.0);
    app.insert_resource(WorkspaceStore {
        at: StoreAt::Memory(Some("{oops".into())),
        theme: true,
    });
    frames(&mut app, 2);
    app.world_mut().write_message(UiCommand::OpenPanel {
        panel: WORKSPACES_PANEL,
        slot: Slot::Primary,
    });
    frames(&mut app, 4);
    let line = find::<LoadFailed>(&mut app);
    let text = texts(&mut app, line);
    assert!(
        text.starts_with(&format!("{LOAD_FAILED}: it's kept as memory (")),
        "{text}"
    );
    let access = app
        .world()
        .get::<bevy::a11y::AccessibilityNode>(line)
        .unwrap();
    assert_eq!(access.role(), accesskit::Role::Alert);
    assert_eq!(access.label(), Some(text.as_str()));
    assert!(all_text(&mut app).contains("Layouts are kept for this session only"));
}

/// The footer says where the layouts are kept; Import waits for a file
/// picker.
#[test]
fn the_footer_says_where_the_layouts_are_kept() {
    let mut app = panel_app();
    assert!(all_text(&mut app).contains("Layouts are kept for this session only"));
    let file = WorkspaceStore {
        at: StoreAt::File("/home/me/.config/gz/ui/workspaces.json".into()),
        theme: true,
    };
    assert_eq!(
        note_of(Some(&file), false),
        "Layouts are kept in /home/me/.config/gz/ui/workspaces.json"
    );
    assert_eq!(
        note_of(Some(&file), true),
        "Layouts are kept for this session only: /home/me/.config/gz/ui/workspaces.json is left as it is"
    );
    let browser = WorkspaceStore {
        at: StoreAt::Browser,
        theme: true,
    };
    assert_eq!(
        note_of(Some(&browser), false),
        "Layouts are kept in this browser's storage"
    );
    let import = pick(&mut app, |i: &IoItem| *i == IoItem::Import);
    assert!(app.world().get::<InteractionDisabled>(import).is_some());
}

/// Export writes every workspace's layout and the user's names, which
/// read back.
#[test]
fn exported_layouts_read_back() {
    let mut app = panel_app();
    app.world_mut()
        .write_message(WorkspaceCommand::SaveAsNew("Mine".into()));
    frames(&mut app, 4);
    let file: WorkspaceFile = serde_json::from_str(&exported(workspaces(&app))).unwrap();
    assert_eq!(file.workspaces.len(), 5);
    assert_eq!(file.theme, None);
    assert_eq!(file.custom[0].name, "Mine");
    assert_eq!(file.workspaces["run"], WorkspaceLayout::default_for(1));
}

/// What a card says a layout shows.
#[test]
fn details_name_what_shows() {
    let app = shell_app((1440, 900), 1.0);
    let panels = app.world().resource::<crate::panels::PanelRegistry>();
    let d = |i| detail_of(&WorkspaceLayout::default_for(i), panels);
    assert_eq!(d(0), "scene · inspector · 2 views");
    assert_eq!(d(1), "viewport only");
    assert_eq!(d(2), "topics · plot · inspector");
    assert_eq!(d(3), "plot");
}

fn alt(app: &mut App, digit: KeyCode) {
    chord(app, &[KeyCode::AltLeft, digit]);
    frames(app, 3);
}

/// Swapping the slots moves what the panel stands in for with it: Debug's
/// Topics, now under Plot, never "Plot + Plot".
#[test]
fn a_swap_carries_what_the_panel_stands_in_for() {
    let mut app = panel_app();
    alt(&mut app, KeyCode::Digit3);
    assert_eq!(
        value(&mut app, SummaryValue::Sidebar),
        "Topics + Plot · 300 px"
    );
    app.world_mut().write_message(UiCommand::SwapSlots);
    frames(&mut app, 3);
    let l = layout(&app);
    assert_eq!(l.secondary, Some(WORKSPACES_PANEL));
    assert_eq!(
        value(&mut app, SummaryValue::Sidebar),
        "Plot + Topics · 300 px"
    );
    let now = current(&app).layout.clone().unwrap();
    assert_eq!(
        (now.primary.as_deref(), now.secondary.as_deref()),
        (Some("plot"), Some("topics"))
    );
}

/// In a closed sidebar the panel isn't showing: the layout says closed,
/// and a switch lays out the next workspace as it is (Run's: no sidebar).
#[test]
fn a_closed_sidebar_lets_the_panel_go() {
    let mut app = panel_app();
    app.world_mut().write_message(UiCommand::ToggleSidebar);
    frames(&mut app, 3);
    assert!(!layout(&app).sidebar_open);
    assert!(!current(&app).layout.as_ref().unwrap().sidebar_open);
    alt(&mut app, KeyCode::Digit2);
    let l = layout(&app);
    assert!(!l.sidebar_open);
    assert_ne!(l.primary, Some(WORKSPACES_PANEL));
}

/// The user's workspaces stay before the switcher's separator, however
/// many there are.
#[test]
fn the_switcher_keeps_the_users_workspaces_in_order() {
    let mut app = panel_app();
    for name in ["A", "B", "C"] {
        app.world_mut()
            .write_message(WorkspaceCommand::SaveAsNew(name.into()));
        frames(&mut app, 3);
    }
    let order: Vec<String> = {
        let world = app.world_mut();
        let sep = world
            .query_filtered::<Entity, With<crate::shell::status_bar::SwitcherSeparator>>()
            .single(world)
            .unwrap();
        let popup = world.get::<ChildOf>(sep).unwrap().parent();
        world
            .get::<Children>(popup)
            .unwrap()
            .iter()
            .map(|c| {
                if c == sep {
                    "|".to_string()
                } else {
                    match world.get::<SwitcherItem>(c) {
                        Some(SwitcherItem::Switch(i)) => i.to_string(),
                        Some(SwitcherItem::Save) => "save".into(),
                        Some(SwitcherItem::Manage) => "manage".into(),
                        None => "?".into(),
                    }
                }
            })
            .collect()
    };
    assert_eq!(
        order,
        ["0", "1", "2", "3", "4", "5", "6", "|", "save", "manage"]
    );
}

/// A card is respawned only when its look changes: a divider's drag
/// doesn't, and a switch only marks; a card with the keys keeps them.
#[test]
fn cards_change_in_place() {
    let mut app = panel_app();
    let ids = |app: &mut App| {
        let world = app.world_mut();
        let mut c: Vec<(usize, Entity)> = world
            .query::<(Entity, &Card)>()
            .iter(world)
            .map(|(e, c)| (c.0, e))
            .collect();
        c.sort();
        c
    };
    alt(&mut app, KeyCode::Digit1);
    app.world_mut().write_message(WorkspaceCommand::Reset);
    frames(&mut app, 3);
    let step = |app: &mut App, by: f32| {
        let mut views = app
            .world_mut()
            .resource_mut::<crate::shell::viewport::views::ViewLayout>();
        let s = views.shares();
        views.set_shares([s[0] + by, s[1]]);
    };
    // The first step makes Build "modified"; the next ones (within a
    // twentieth) redraw nothing.
    step(&mut app, 0.01);
    frames(&mut app, 3);
    let before = ids(&mut app);
    for _ in 0..3 {
        step(&mut app, 0.002);
        frames(&mut app, 1);
    }
    frames(&mut app, 2);
    assert_eq!(ids(&mut app), before, "a drag's steps redraw nothing");
    // Tab, then Enter on Run: the card keeps the keys and gets the mark.
    let run = before[1].1;
    app.world_mut()
        .resource_mut::<bevy::input_focus::InputFocus>()
        .set(run, bevy::input_focus::FocusCause::Navigated);
    activate(&mut app, run);
    assert_eq!(workspaces(&app).current, 1);
    let after = ids(&mut app);
    let run = after[1].1;
    assert_eq!(focused(&app), Some(run));
    assert!(app.world().get::<Active>(run).is_some());
}

/// Save's tip names the workspace it saves to, whatever it was before.
#[test]
fn saves_tip_follows_the_workspace() {
    let mut app = panel_app();
    app.world_mut()
        .write_message(WorkspaceCommand::SaveAsNew("A long name".into()));
    frames(&mut app, 3);
    let tip = |app: &mut App| {
        let (save, _) = button(app, LayoutButton::Save);
        app.world()
            .get::<crate::widgets::Tooltip>(save)
            .unwrap()
            .title
            .clone()
    };
    assert_eq!(tip(&mut app), "Save to A long name");
    let (save, _) = button(&mut app, LayoutButton::Save);
    assert_eq!(texts(&mut app, save), "Save");
    alt(&mut app, KeyCode::Digit1);
    assert_eq!(tip(&mut app), "Save to Build");
    let (save, _) = button(&mut app, LayoutButton::Save);
    assert_eq!(texts(&mut app, save), "Save to Build");
}

/// Raising a window reorders them, which isn't a change.
#[test]
fn raising_a_window_isnt_a_change() {
    let mut app = panel_app();
    for p in [crate::panels::PLOT, crate::panels::CONSOLE] {
        app.world_mut().write_message(UiCommand::PopOut(p));
        frames(&mut app, 3);
    }
    app.world_mut().write_message(WorkspaceCommand::Save);
    frames(&mut app, 3);
    assert!(!current(&app).modified);
    app.world_mut()
        .resource_mut::<ShellLayout>()
        .raise(crate::panels::PLOT);
    frames(&mut app, 3);
    assert!(!current(&app).modified);
}

/// Reset to the default is kept: Build saved as its default, so it starts
/// so next time; a name the file repeats is made unique.
#[test]
fn a_reset_default_is_kept_and_names_are_unique() {
    let mut app = panel_app();
    app.insert_resource(WorkspaceStore {
        at: StoreAt::Memory(None),
        theme: true,
    });
    app.world_mut().write_message(WorkspaceCommand::Reset);
    frames(&mut app, 3);
    let file = match &app.world().resource::<WorkspaceStore>().at {
        StoreAt::Memory(Some(t)) => serde_json::from_str::<WorkspaceFile>(t).unwrap(),
        other => panic!("{other:?}"),
    };
    assert_eq!(file.workspaces["build"], WorkspaceLayout::default_for(0));
    let note = app
        .world()
        .resource::<crate::shell::status_bar::readouts::StatusNote>();
    assert_eq!(
        note.0.as_ref().map(|n| n.0.as_str()),
        Some("Reset Build to its default")
    );

    let file = WorkspaceFile {
        version: 1,
        current: "build".into(),
        theme: None,
        workspaces: [("user_1".to_string(), WorkspaceLayout::default_for(1))]
            .into_iter()
            .collect(),
        custom: vec![crate::shell::workspaces::CustomName {
            id: "user_1".into(),
            name: "Build".into(),
        }],
    };
    let mut app = shell_app((1440, 900), 1.0);
    app.insert_resource(WorkspaceStore {
        at: StoreAt::Memory(Some(serde_json::to_string(&file).unwrap())),
        theme: true,
    });
    frames(&mut app, 3);
    assert_eq!(workspaces(&app).list()[4].name, "Build 2");
}
