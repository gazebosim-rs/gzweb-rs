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
