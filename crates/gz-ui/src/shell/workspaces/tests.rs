//! Workspaces: switching, what a switch keeps, the defaults, saving,
//! the store, the switcher and the palette.

use bevy::input::keyboard::KeyCode;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use gz_bevy::view::{SnapView, ViewCamera, ViewId};

use super::*;
use crate::shell::status_bar::readouts::StatusNote;
use crate::shell::status_bar::{SwitcherItem, WorkspaceName};
use crate::test_support::frames;
use crate::test_support::shell::*;
use crate::theme::visuals::Active;

fn workspaces(app: &App) -> &Workspaces {
    app.world().resource::<Workspaces>()
}

fn alt(app: &mut App, digit: KeyCode) {
    chord(app, &[KeyCode::AltLeft, digit]);
    frames(app, 3);
}

fn button_name(app: &mut App) -> String {
    let world = app.world_mut();
    world
        .query_filtered::<&Text, With<WorkspaceName>>()
        .single(world)
        .unwrap()
        .0
        .clone()
}

fn preset(app: &App) -> LayoutPreset {
    app.world().resource::<ViewLayout>().preset
}

fn camera(app: &mut App, id: u8) -> Option<(ViewCamera, Transform, Projection)> {
    let world = app.world_mut();
    world
        .query::<(&ViewId, &ViewCamera, &Transform, &Projection)>()
        .iter(world)
        .find(|(v, ..)| v.0 == id)
        .map(|(_, c, t, p)| (c.clone(), *t, p.clone()))
}

/// Alt+1–4 switch: each workspace's default the first time (§8's table),
/// then as it was left; the status bar names the current one.
#[test]
fn alt_keys_switch_and_a_switch_keeps_what_it_leaves() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 3);
    assert_eq!(workspaces(&app).current, 0);
    assert_eq!(button_name(&mut app), "Build");
    let start = layout(&app);

    // Run: no sidebar, no inspector, one view.
    alt(&mut app, KeyCode::Digit2);
    assert_eq!(workspaces(&app).current, 1);
    assert_eq!(button_name(&mut app), "Run");
    let l = layout(&app);
    assert!(!l.sidebar_open && !l.inspector_open);
    assert_eq!(preset(&app), LayoutPreset::Single);
    // Changed: the inspector opens.
    app.world_mut().resource_mut::<ShellLayout>().inspector_open = true;
    frames(&mut app, 1);

    // Debug: Topics over Plot, the inspector.
    alt(&mut app, KeyCode::Digit3);
    let l = layout(&app);
    assert_eq!(l.primary.map(|p| p.name()), Some("topics"));
    assert_eq!(l.secondary.map(|p| p.name()), Some("plot"));
    assert!(l.sidebar_open && l.inspector_open);

    // Review: Plot alone.
    alt(&mut app, KeyCode::Digit4);
    let l = layout(&app);
    assert_eq!(l.primary.map(|p| p.name()), Some("plot"));
    assert!(l.secondary.is_none() && !l.inspector_open);

    // Back to Run: as it was left, inspector open.
    alt(&mut app, KeyCode::Digit2);
    assert!(layout(&app).inspector_open);
    // Back to Build: as the app started.
    alt(&mut app, KeyCode::Digit1);
    let l = layout(&app);
    assert_eq!(
        (l.sidebar_open, l.inspector_open, l.primary),
        (start.sidebar_open, start.inspector_open, start.primary)
    );
    // The current one again: nothing.
    alt(&mut app, KeyCode::Digit1);
    assert_eq!(workspaces(&app).current, 0);
}

/// Build's default: a perspective and a top ortho view, side by side.
#[test]
fn builds_default_has_a_top_view() {
    let mut app = shell_app((1440, 900), 1.0);
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::from_millis(100),
    ));
    frames(&mut app, 3);
    app.world_mut().resource_mut::<Workspaces>().current = 1;
    alt(&mut app, KeyCode::Digit1);
    // The snap animates in.
    frames(&mut app, 10);
    assert_eq!(preset(&app), LayoutPreset::TwoColumns);
    let (top, ..) = camera(&mut app, 1).expect("a second view");
    assert_eq!(top.snapped, Some(SnapView::Top));
}

/// A view's camera and overlays come back as they were left.
#[test]
fn a_views_camera_comes_back() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 3);
    {
        let world = app.world_mut();
        let (mut t, mut v, mut set) = world
            .query_filtered::<(&mut Transform, &mut ViewCamera, &mut OverlaySet), With<MainCamera>>(
            )
            .single_mut(world)
            .unwrap();
        *t = Transform::from_xyz(3.0, 4.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y);
        v.pivot = Vec3::new(0.5, 0.0, 0.0);
        set.set(Overlay::Joints, true);
    }
    let (_, before, _) = camera(&mut app, 0).unwrap();
    alt(&mut app, KeyCode::Digit2);
    {
        let world = app.world_mut();
        let (mut t, mut set) = world
            .query_filtered::<(&mut Transform, &mut OverlaySet), With<MainCamera>>()
            .single_mut(world)
            .unwrap();
        *t = Transform::from_xyz(-9.0, 1.0, 0.0);
        set.set(Overlay::Joints, false);
    }
    alt(&mut app, KeyCode::Digit1);
    frames(&mut app, 2);
    let (v, t, _) = camera(&mut app, 0).unwrap();
    assert!(t.translation.distance(before.translation) < 1e-4, "{t:?}");
    assert!(v.pivot.distance(Vec3::new(0.5, 0.0, 0.0)) < 1e-4);
    let world = app.world_mut();
    let set = world
        .query_filtered::<&OverlaySet, With<MainCamera>>()
        .single(world)
        .unwrap();
    assert!(set.is_on(Overlay::Joints));
}

fn stored(app: &App) -> Option<WorkspaceFile> {
    match &app.world().resource::<WorkspaceStore>().at {
        StoreAt::Memory(Some(text)) => Some(serde_json::from_str(text).unwrap()),
        _ => None,
    }
}

fn with_store(text: Option<&str>, theme: bool) -> App {
    let mut app = shell_app((1440, 900), 1.0);
    app.insert_resource(WorkspaceStore {
        at: StoreAt::Memory(text.map(str::to_string)),
        theme,
    });
    frames(&mut app, 3);
    app
}

/// A switch writes the current workspace and the theme; Save writes the
/// layout too, saying so.
#[test]
fn switching_and_saving_persist() {
    let mut app = with_store(None, true);
    assert_eq!(stored(&app), None, "nothing to write yet");
    alt(&mut app, KeyCode::Digit3);
    let file = stored(&app).expect("written");
    assert_eq!(
        (file.current.as_str(), file.theme.as_deref()),
        ("debug", Some("light"))
    );
    assert!(file.workspaces.is_empty());
    app.world_mut().write_message(WorkspaceCommand::Save);
    frames(&mut app, 2);
    let file = stored(&app).unwrap();
    let debug = &file.workspaces["debug"];
    assert_eq!(debug.primary.as_deref(), Some("topics"));
    assert_eq!(debug.views.len(), 1);
    let note = app.world().resource::<StatusNote>();
    assert_eq!(
        note.0.as_ref().map(|n| n.0.as_str()),
        Some("Saved the Debug layout")
    );
    // The theme, when it changes.
    app.world_mut()
        .resource_mut::<Theme>()
        .set_mode(ThemeMode::Dark);
    frames(&mut app, 2);
    assert_eq!(stored(&app).unwrap().theme.as_deref(), Some("dark"));
}

/// The app starts in the workspace it was left in, laid out as saved,
/// with the saved theme (unless a theme was asked for); a file it can't
/// read leaves the defaults.
#[test]
fn the_store_is_read_at_start() {
    let mut saved = WorkspaceLayout::default_for(1);
    saved.inspector_open = true;
    let file = WorkspaceFile {
        version: 1,
        current: "run".into(),
        theme: Some("dark".into()),
        workspaces: [("run".to_string(), saved)].into_iter().collect(),
        custom: vec![],
    };
    let text = serde_json::to_string(&file).unwrap();
    let app = with_store(Some(&text), true);
    assert_eq!(workspaces(&app).current, 1);
    let l = layout(&app);
    assert!(!l.sidebar_open && l.inspector_open);
    assert_eq!(app.world().resource::<Theme>().mode, ThemeMode::Dark);
    let app = with_store(Some(&text), false);
    assert_eq!(app.world().resource::<Theme>().mode, ThemeMode::Light);

    let app = with_store(Some("{not json"), true);
    assert_eq!(workspaces(&app).current, 0);
}

/// The switcher marks the current workspace and switches.
#[test]
fn the_switcher_switches() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 3);
    let item = |app: &mut App, which: SwitcherItem| {
        let world = app.world_mut();
        world
            .query::<(Entity, &SwitcherItem, Has<Active>)>()
            .iter(world)
            .find(|(_, i, _)| **i == which)
            .map(|(e, _, a)| (e, a))
            .unwrap()
    };
    assert!(item(&mut app, SwitcherItem::Switch(0)).1);
    let (run, _) = item(&mut app, SwitcherItem::Switch(1));
    app.world_mut().trigger(Activate { entity: run });
    frames(&mut app, 3);
    assert_eq!(workspaces(&app).current, 1);
    assert!(item(&mut app, SwitcherItem::Switch(1)).1);
    assert!(!item(&mut app, SwitcherItem::Switch(0)).1);
}

/// Saved layouts read back from the file's text.
#[test]
fn layouts_round_trip_through_json() {
    for i in 0..4 {
        let w = WorkspaceLayout::default_for(i);
        let text = serde_json::to_string(&w).unwrap();
        assert_eq!(serde_json::from_str::<WorkspaceLayout>(&text).unwrap(), w);
    }
    assert_eq!(snap_view("top"), Some(SnapView::Top));
    assert_eq!(snap_view("nope"), None);
}

fn start_with(text: Option<&str>, theme: bool, before: impl FnOnce(&mut App)) -> App {
    let mut app = shell_app((1440, 900), 1.0);
    app.insert_resource(WorkspaceStore {
        at: StoreAt::Memory(text.map(str::to_string)),
        theme,
    });
    before(&mut app);
    frames(&mut app, 3);
    app
}

fn file_text(current: &str, version: u32, saved: &[(&str, WorkspaceLayout)]) -> String {
    serde_json::to_string(&WorkspaceFile {
        version,
        current: current.into(),
        theme: Some("dark".into()),
        workspaces: saved
            .iter()
            .map(|(id, w)| (id.to_string(), w.clone()))
            .collect(),
        custom: vec![],
    })
    .unwrap()
}

/// A file it can't parse is kept apart, and the next write is whole: the
/// saved layouts aren't lost to a typo, nor the file to a crash.
#[test]
fn a_file_it_cant_parse_is_kept_apart() {
    let dir = std::env::temp_dir().join(format!("gz-ui-workspaces-{}", std::process::id()));
    let path = dir.join(WORKSPACE_FILE);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(&path, "{oops").unwrap();
    let mut app = shell_app((1440, 900), 1.0);
    app.insert_resource(WorkspaceStore {
        at: StoreAt::File(path.clone()),
        theme: true,
    });
    frames(&mut app, 3);
    assert_eq!(workspaces(&app).current, 0);
    let bad = dir.join(format!("{WORKSPACE_FILE}.bad"));
    assert_eq!(std::fs::read_to_string(&bad).unwrap(), "{oops");
    alt(&mut app, KeyCode::Digit2);
    let file: WorkspaceFile =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(file.current, "run");
    assert!(!dir.join(format!("{WORKSPACE_FILE}.part")).exists());
    std::fs::remove_dir_all(&dir).unwrap();
    // In memory too: it's written again after a bad one.
    let mut app = start_with(Some("{not json"), true, |_| {});
    alt(&mut app, KeyCode::Digit3);
    assert_eq!(stored(&app).unwrap().current, "debug");
}

/// A newer gzweb's file is read as far as it can be and never written
/// over; workspaces this build doesn't know are kept when it writes.
#[test]
fn newer_files_and_unknown_workspaces_are_kept() {
    let text = file_text("run", FILE_VERSION + 1, &[]);
    let mut app = start_with(Some(&text), true, |_| {});
    assert_eq!(workspaces(&app).current, 1);
    alt(&mut app, KeyCode::Digit3);
    match &app.world().resource::<WorkspaceStore>().at {
        StoreAt::Memory(Some(t)) => assert_eq!(t, &text, "not written over"),
        other => panic!("{other:?}"),
    }

    let custom = WorkspaceLayout::default_for(3);
    let text = file_text("build", FILE_VERSION, &[("custom", custom.clone())]);
    let mut app = start_with(Some(&text), true, |_| {});
    alt(&mut app, KeyCode::Digit2);
    let file = stored(&app).unwrap();
    assert_eq!(file.current, "run");
    assert_eq!(file.workspaces.get("custom"), Some(&custom));
}

/// A theme asked for (`--theme`) isn't saved: the file keeps its own.
#[test]
fn a_theme_asked_for_isnt_saved() {
    let text = file_text("build", FILE_VERSION, &[]);
    let mut app = start_with(Some(&text), false, |app| {
        app.world_mut()
            .resource_mut::<Theme>()
            .set_mode(ThemeMode::Light);
    });
    alt(&mut app, KeyCode::Digit2);
    assert_eq!(stored(&app).unwrap().theme.as_deref(), Some("dark"));
    let mut app = start_with(None, false, |_| {});
    alt(&mut app, KeyCode::Digit2);
    assert_eq!(stored(&app).unwrap().theme, None);
}

/// Left in a workspace never saved, the app starts in its default (Build's
/// is the app's start).
#[test]
fn an_unsaved_workspace_starts_as_its_default() {
    let text = file_text("debug", FILE_VERSION, &[]);
    let app = start_with(Some(&text), true, |_| {});
    assert_eq!(workspaces(&app).current, 2);
    let l = layout(&app);
    assert_eq!(l.primary.map(|p| p.name()), Some("topics"));
    assert_eq!(l.secondary.map(|p| p.name()), Some("plot"));
}

/// The start flags go over the saved layout.
#[test]
fn start_flags_go_over_the_saved_layout() {
    let mut run = WorkspaceLayout::default_for(1);
    run.inspector_open = true;
    let text = file_text("run", FILE_VERSION, &[("run", run)]);
    let mut app = start_with(Some(&text), true, |app| {
        app.insert_resource(crate::shell::StartLayout(LayoutPreset::TwoColumns));
        app.insert_resource(crate::shell::StartPopOuts(vec!["console".into()]));
    });
    frames(&mut app, 2);
    assert_eq!(preset(&app), LayoutPreset::TwoColumns);
    let l = layout(&app);
    assert!(l.inspector_open, "the rest as saved");
    assert_eq!(
        l.popouts.iter().map(|p| p.panel.name()).collect::<Vec<_>>(),
        ["console"]
    );
}

/// A view's grid as saved stays when the scene's grid flag first comes;
/// later ones apply.
#[test]
fn a_saved_grid_outlasts_the_scenes_flag() {
    let mut run = WorkspaceLayout::default_for(1);
    run.views = vec![SavedView {
        overlays: Some(vec![]),
        ..default()
    }];
    let text = file_text("run", FILE_VERSION, &[("run", run)]);
    let mut app = start_with(Some(&text), true, |_| {});
    let grid = |app: &mut App| {
        let world = app.world_mut();
        world
            .query_filtered::<&OverlaySet, With<MainCamera>>()
            .single(world)
            .unwrap()
            .is_on(Overlay::Grid)
    };
    assert!(!grid(&mut app));
    app.world_mut()
        .resource_mut::<gz_bevy::render::GzGrid>()
        .visible = true;
    frames(&mut app, 2);
    assert!(!grid(&mut app), "the scene's first flag leaves it");
    app.world_mut()
        .resource_mut::<gz_bevy::render::GzGrid>()
        .set_changed();
    frames(&mut app, 2);
    assert!(grid(&mut app), "a later one applies");
}

/// Alt+1–4 wait while an edit is under way.
#[test]
fn alt_keys_wait_for_an_edit() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 3);
    let scrub = app
        .world_mut()
        .spawn(crate::widgets::number_field::Scrub {
            value: 0.0,
            moved: false,
        })
        .id();
    alt(&mut app, KeyCode::Digit2);
    assert_eq!(workspaces(&app).current, 0);
    app.world_mut().despawn(scrub);
    alt(&mut app, KeyCode::Digit2);
    assert_eq!(workspaces(&app).current, 1);
}

/// A view's axis view comes back with it.
#[test]
fn a_snapped_view_comes_back_snapped() {
    let mut app = shell_app((1440, 900), 1.0);
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::from_millis(100),
    ));
    frames(&mut app, 3);
    let view = {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<MainCamera>>()
            .single(world)
            .unwrap()
    };
    app.world_mut().write_message(ViewCommand {
        view,
        op: ViewOp::Snap(SnapView::Front),
    });
    // Mid-animation: the switch keeps where it ends.
    frames(&mut app, 1);
    alt(&mut app, KeyCode::Digit2);
    alt(&mut app, KeyCode::Digit1);
    frames(&mut app, 2);
    let (v, ..) = camera(&mut app, 0).unwrap();
    assert_eq!(v.snapped, Some(SnapView::Front));
}

/// The user's workspaces read back in their order with their names (one
/// the file holds without a name goes by its id), and the app starts in
/// one.
#[test]
fn the_users_workspaces_read_back() {
    let mut mine = WorkspaceLayout::default_for(2);
    mine.inspector_open = false;
    let file = WorkspaceFile {
        version: FILE_VERSION,
        current: "user_2".into(),
        theme: None,
        workspaces: [
            ("user_2".to_string(), mine),
            ("user_1".to_string(), WorkspaceLayout::default_for(1)),
            ("other".to_string(), WorkspaceLayout::default_for(3)),
        ]
        .into_iter()
        .collect(),
        custom: vec![
            CustomName {
                id: "user_2".into(),
                name: "Mine".into(),
            },
            CustomName {
                id: "user_1".into(),
                name: "Older".into(),
            },
        ],
    };
    let app = start_with(Some(&serde_json::to_string(&file).unwrap()), true, |_| {});
    let w = workspaces(&app);
    let names: Vec<&str> = w.list().iter().map(|w| w.name.as_str()).collect();
    assert_eq!(
        names,
        ["Build", "Run", "Debug", "Review", "Mine", "Older", "other"]
    );
    assert_eq!(w.current, 4);
    let l = layout(&app);
    assert_eq!(l.primary.map(|p| p.name()), Some("topics"));
    assert!(!l.inspector_open);
}

/// Unsaved, Build's layout as saved is the app's start once start-up ends,
/// so a first start isn't modified; a saved Build keeps its own.
#[test]
fn builds_start_is_its_layout_until_saved() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 3);
    assert!(
        app.world().resource::<CurrentLayout>().modified,
        "§8's default"
    );
    app.world_mut().resource_mut::<ShellLayout>().startup = false;
    frames(&mut app, 3);
    let current = app.world().resource::<CurrentLayout>();
    assert!(!current.modified);
    assert_eq!(current.reset, Some(true), "Reset gives §8's default");
    assert_eq!(workspaces(&app).saved(0).preset, "single");
}

/// A popped-out Workspaces panel isn't part of a layout.
#[test]
fn the_workspaces_panel_isnt_saved() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 3);
    app.world_mut()
        .write_message(crate::shell::UiCommand::PopOut(WORKSPACES_PANEL));
    frames(&mut app, 3);
    assert_eq!(layout(&app).popouts.len(), 1);
    app.world_mut().write_message(WorkspaceCommand::Save);
    frames(&mut app, 2);
    assert!(workspaces(&app).saved(0).popouts.is_empty());
    // A switch keeps it open.
    alt(&mut app, KeyCode::Digit2);
    assert_eq!(layout(&app).popouts.len(), 1);
}
