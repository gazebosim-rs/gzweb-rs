//! The status bar's own parts (its readouts are in the shell's tests).

use gz_bevy::view::input::{InputMap, Platform};

use super::workspace_tooltip;

/// The workspace button's shortcut comes from the input map.
#[test]
fn the_workspace_tooltip_reads_the_input_map() {
    let mut map = InputMap::builtin();
    map.platform = Platform::Linux;
    assert_eq!(workspace_tooltip(&map).shortcut.as_deref(), Some("Alt+1–4"));
    map.platform = Platform::Mac;
    assert_eq!(workspace_tooltip(&map).shortcut.as_deref(), Some("Opt+1–4"));
}

/// Item 5: the latest warning or error as a chip, counted by kind; none
/// before the first.
#[test]
fn the_message_chip_shows_the_latest_problem() {
    use bevy::prelude::*;
    use gz_bevy::messages::{MessageLevel, SystemMessage, SystemMessages};

    use super::readouts::MessageChip;
    use crate::test_support::frames;
    use crate::test_support::shell::{all_text, shell_app};
    use crate::widgets::{StatusKind, Tooltip};

    let mut app = shell_app((1440, 900), 1.0);
    app.init_resource::<SystemMessages>();
    frames(&mut app, 2);
    let chip = |app: &mut App| {
        let world = app.world_mut();
        world
            .query::<(&MessageChip, &Tooltip)>()
            .iter(world)
            .map(|(c, t)| (c.clone(), t.detail.clone()))
            .collect::<Vec<_>>()
    };
    assert!(chip(&mut app).is_empty());
    let push = |app: &mut App, level, text: &str| {
        app.world_mut()
            .resource_mut::<SystemMessages>()
            .push(SystemMessage {
                level,
                text: text.into(),
                source: "gz_bevy::assets".into(),
                ..default()
            });
        frames(app, 2);
    };
    push(&mut app, MessageLevel::Info, "connected");
    assert!(chip(&mut app).is_empty());
    push(
        &mut app,
        MessageLevel::Warning,
        "Mesh for crate_01 not found",
    );
    push(
        &mut app,
        MessageLevel::Warning,
        "Mesh for crate_02 not found",
    );
    let shown = chip(&mut app);
    assert_eq!(shown.len(), 1);
    assert_eq!(
        shown[0].0,
        MessageChip {
            kind: StatusKind::Warning,
            text: "Mesh for crate_02 not found".into(),
            count: "2 warnings".into(),
        }
    );
    assert_eq!(shown[0].1.as_deref(), Some("gz_bevy::assets"));
    assert!(all_text(&mut app).contains("2 warnings"));
    push(&mut app, MessageLevel::Error, "websocket closed");
    let shown = chip(&mut app);
    assert_eq!(shown.len(), 1);
    assert_eq!(
        (shown[0].0.kind, shown[0].0.count.as_str()),
        (StatusKind::Error, "1 error")
    );
}

/// The problem chip opens the message tray: the warnings and errors,
/// newest first, with their counts; Clear drops them (and the chip), Open
/// console opens the Console. Either closes it.
#[test]
fn the_problem_chip_opens_the_message_tray() {
    use bevy::prelude::*;
    use bevy::ui_widgets::Activate;
    use gz_bevy::messages::{MessageLevel, SystemMessage, SystemMessages};

    use super::readouts::MessageChip;
    use super::tray::{TrayButton, TrayCounts, TrayRow};
    use crate::test_support::frames;
    use crate::test_support::shell::{layout, shell_app};
    use crate::widgets::menu::MenuPopupOf;

    let mut app = shell_app((1440, 900), 1.0);
    app.init_resource::<SystemMessages>();
    frames(&mut app, 2);
    for (level, text) in [
        (MessageLevel::Warning, "slow"),
        (MessageLevel::Info, "fine"),
        (MessageLevel::Error, "boom"),
    ] {
        app.world_mut()
            .resource_mut::<SystemMessages>()
            .push(SystemMessage {
                level,
                text: text.into(),
                ..default()
            });
    }
    frames(&mut app, 2);
    let find = |app: &mut App| {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<MessageChip>>()
            .iter(world)
            .next()
    };
    let tray_shown = |app: &mut App| {
        let world = app.world_mut();
        world
            .query_filtered::<&Visibility, With<MenuPopupOf>>()
            .iter(world)
            .any(|v| *v == Visibility::Visible)
    };
    let chip = find(&mut app).expect("a chip");
    app.world_mut().trigger(Activate { entity: chip });
    frames(&mut app, 3);
    assert!(tray_shown(&mut app));
    let rows: Vec<u64> = {
        let world = app.world_mut();
        let mut rows: Vec<(Entity, u64)> = world
            .query::<(Entity, &TrayRow)>()
            .iter(world)
            .map(|(e, r)| (e, r.0))
            .collect();
        rows.sort_by_key(|(e, _)| e.index());
        rows.into_iter().map(|(_, s)| s).collect()
    };
    assert_eq!(rows, [3, 1], "the error, then the warning");
    let counts = {
        let world = app.world_mut();
        world
            .query_filtered::<&Text, With<TrayCounts>>()
            .single(world)
            .unwrap()
            .0
            .clone()
    };
    assert_eq!(counts, "1 warn · 1 err");
    let button = |app: &mut App, which: TrayButton| {
        let world = app.world_mut();
        world
            .query::<(Entity, &TrayButton)>()
            .iter(world)
            .find(|(_, b)| **b == which)
            .unwrap()
            .0
    };
    let open = button(&mut app, TrayButton::OpenConsole);
    app.world_mut().trigger(Activate { entity: open });
    frames(&mut app, 3);
    assert!(!tray_shown(&mut app));
    assert_eq!(layout(&app).primary.map(|p| p.name()), Some("console"));
    // Clear: the tray and the chip go; the Console's totals stay.
    let chip = find(&mut app).unwrap();
    app.world_mut().trigger(Activate { entity: chip });
    frames(&mut app, 3);
    let clear = button(&mut app, TrayButton::Clear);
    app.world_mut().trigger(Activate { entity: clear });
    frames(&mut app, 3);
    assert!(find(&mut app).is_none());
    let m = app.world().resource::<SystemMessages>();
    assert_eq!((m.warnings, m.errors), (1, 1));
    assert_eq!(m.problems().count(), 0);

    // Open while a note replaces its chip, it still closes.
    app.world_mut()
        .resource_mut::<SystemMessages>()
        .push(SystemMessage {
            level: MessageLevel::Warning,
            text: "again".into(),
            ..default()
        });
    frames(&mut app, 2);
    let chip = find(&mut app).unwrap();
    app.world_mut().trigger(Activate { entity: chip });
    frames(&mut app, 3);
    assert!(tray_shown(&mut app));
    app.world_mut()
        .resource_mut::<super::readouts::StatusNote>()
        .show("Saved", 1.0e9);
    frames(&mut app, 2);
    let popup = {
        let world = app.world_mut();
        world
            .query_filtered::<(Entity, &Visibility), With<MenuPopupOf>>()
            .iter(world)
            .find(|(_, v)| **v == Visibility::Visible)
            .unwrap()
            .0
    };
    app.world_mut().trigger(bevy::ui_widgets::MenuEvent {
        source: popup,
        action: bevy::ui_widgets::MenuAction::CloseAll,
    });
    frames(&mut app, 2);
    assert!(!tray_shown(&mut app));
}
