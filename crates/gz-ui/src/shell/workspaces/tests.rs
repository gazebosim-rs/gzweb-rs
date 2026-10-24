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
