//! The Console panel: its rules, and the panel through the shell.

use bevy::a11y::AccessibilityNode;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui_widgets::ValueChange;
use gz_bevy::connection::{GzCommand, GzServiceReply, GzStatus};
use gz_bevy::messages::{MessageLevel, SystemMessage, SystemMessages};
use gz_bevy::topics::GzTopics;
use gz_core::msgs;

use super::model::*;
use super::view::{ChipChoice, CommandLine, ConsoleRow, ConsoleRows, NoMatches};
use super::{ConsoleUnread, CLEAR, CONSOLE, PAUSE};
use crate::panels::{PanelActionActivated, PanelView, TopicsStart};
use crate::shell::sidebar::body::BodyView;
use crate::shell::{PanelStatus, Slot, UiCommand};
use crate::test_support::frames;
use crate::test_support::shell::*;
use crate::widgets::text_field::TextFieldInput;
use crate::widgets::{BadgeKind, TextSubmit};

/// The badge counts errors (with the warnings) in `danger`, else the
/// warnings, else hides; writing the same badge again changes nothing.
#[test]
fn the_console_badge_counts_errors_then_warnings() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    let badge = |app: &mut App, warnings, errors| {
        *app.world_mut().resource_mut::<ConsoleUnread>() = ConsoleUnread { warnings, errors };
        frames(app, 1);
        app.world().resource::<PanelStatus>().badge(CONSOLE)
    };
    assert_eq!(badge(&mut app, 2, 1), Some((BadgeKind::Error, 3)));
    assert_eq!(badge(&mut app, 2, 0), Some((BadgeKind::Warning, 2)));
    assert_eq!(badge(&mut app, 0, 0), None);
    let tick = app.world().resource_ref::<PanelStatus>().last_changed();
    badge(&mut app, 0, 0);
    assert_eq!(
        app.world().resource_ref::<PanelStatus>().last_changed(),
        tick,
        "an unchanged badge isn't written"
    );
}

fn line(seq: u64, level: MessageLevel, text: &str, source: &str) -> SystemMessage {
    SystemMessage {
        level,
        text: text.into(),
        source: source.into(),
        clock: "12:04:31".into(),
        seq,
        ..default()
    }
}

/// A chip takes its level; the filter matches the text or the source in
/// any case; the source reads as the module or the crate; the count names
/// the problems; the lines come newest first, the command line's among
/// the log's, after Clear and up to a pause.
#[test]
fn lines_chips_filter_and_labels() {
    assert!(LevelChip::All.takes(MessageLevel::Debug));
    assert!(LevelChip::Warn.takes(MessageLevel::Warning));
    assert!(!LevelChip::Error.takes(MessageLevel::Warning));
    assert!(LevelChip::Debug.takes(MessageLevel::Debug));
    assert_eq!(LevelChip::Info.label(), "Info");
    let m = line(
        1,
        MessageLevel::Warning,
        "Mesh not found",
        "gz_bevy::assets",
    );
    let state = |filter: &str, chip| ConsoleState {
        filter: filter.into(),
        chip,
        ..default()
    };
    assert!(matches(&m, &state("", LevelChip::All)));
    assert!(matches(&m, &state("MESH", LevelChip::Warn)));
    assert!(matches(&m, &state("assets", LevelChip::All)));
    assert!(!matches(&m, &state("pallet", LevelChip::All)));
    assert!(!matches(&m, &state("", LevelChip::Info)));
    assert_eq!(source_label("gz_bevy::connection"), "connection");
    assert_eq!(source_label("gz_ui"), "gz-ui");
    assert_eq!(source_label("console"), "console");

    let warn = line(1, MessageLevel::Warning, "a", "x");
    let err = line(2, MessageLevel::Error, "b", "x");
    let info = line(3, MessageLevel::Info, "c", "x");
    assert_eq!(count_label(&[&info]), None);
    assert_eq!(count_label(&[&warn, &info]).as_deref(), Some("1 warn"));
    assert_eq!(count_label(&[&err]).as_deref(), Some("1 err"));
    assert_eq!(
        count_label(&[&warn, &warn, &err]).as_deref(),
        Some("2 warn · 1 err")
    );

    let mut log = SystemMessages::default();
    for m in [warn, err, info] {
        log.push(m);
    }
    let mut output = ConsoleOutput::default();
    output.say(&mut log, MessageLevel::Info, "› help", 0.0);
    let text = |s: &ConsoleState| -> Vec<String> {
        lines(&log, &output, s)
            .iter()
            .map(|m| m.text.clone())
            .collect()
    };
    assert_eq!(text(&ConsoleState::default()), ["› help", "c", "b", "a"]);
    let later = ConsoleState {
        cleared_through: 1,
        paused_at: Some(3),
        ..default()
    };
    assert_eq!(text(&later), ["c", "b"]);
}

/// The panel open on a live world, with these lines in the log.
fn console_with(lines: &[(MessageLevel, &str)]) -> App {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    go_live(&mut app, false);
    for (level, text) in lines {
        push(&mut app, *level, text);
    }
    app.world_mut().write_message(UiCommand::OpenPanel {
        panel: CONSOLE,
        slot: Slot::Primary,
    });
    frames(&mut app, 3);
    app
}

fn push(app: &mut App, level: MessageLevel, text: &str) {
    app.world_mut()
        .resource_mut::<SystemMessages>()
        .push(SystemMessage {
            level,
            text: text.into(),
            source: "gz_bevy::connection".into(),
            clock: "12:04:31".into(),
            ..default()
        });
}

fn view(app: &mut App) -> PanelView {
    let world = app.world_mut();
    world
        .query::<(&BodyView, &crate::panels::PanelBody)>()
        .iter(world)
        .find(|(_, b)| b.0 == CONSOLE)
        .unwrap()
        .0
         .0
        .clone()
}

/// The rows' messages, top to bottom.
fn rows(app: &mut App) -> Vec<String> {
    let world = app.world_mut();
    let Ok(parent) = world
        .query_filtered::<Entity, With<ConsoleRows>>()
        .single(world)
    else {
        return vec![];
    };
    let Some(children) = world.get::<Children>(parent) else {
        return vec![];
    };
    children
        .iter()
        .filter(|c| world.get::<ConsoleRow>(*c).is_some())
        .filter_map(|c| world.get::<AccessibilityNode>(c))
        .filter_map(|a| a.label().map(str::to_string))
        .collect()
}

fn act(app: &mut App, action: crate::panels::ActionId) {
    let e = app.world_mut().spawn_empty().id();
    app.world_mut().trigger(PanelActionActivated {
        entity: e,
        panel: CONSOLE,
        action,
    });
    frames(app, 2);
}

fn chip(app: &mut App, which: LevelChip) {
    let world = app.world_mut();
    let e = world
        .query::<(Entity, &ChipChoice)>()
        .iter(world)
        .find(|(_, c)| c.0 == which)
        .unwrap()
        .0;
    let group = world.get::<ChildOf>(e).unwrap().parent();
    world.trigger(ValueChange {
        source: group,
        value: e,
        is_final: true,
    });
    frames(app, 2);
}

/// No lines says "No messages"; the log's lines show newest first with
/// their `source · time`, the header counts the problems; a chip and the
/// filter narrow them ("No messages match" when none do); a new line goes
/// on top; Pause holds new lines back until Resume; Clear empties it.
#[test]
fn the_log_lists_filters_pauses_and_clears() {
    let mut app = console_with(&[]);
    assert_eq!(view(&mut app), PanelView::empty("No messages"));
    let mut app = console_with(&[
        (MessageLevel::Info, "Connected to localhost:9002"),
        (MessageLevel::Warning, "Mesh for crate_01 not found"),
        (MessageLevel::Error, "websocket closed"),
    ]);
    assert_eq!(view(&mut app), PanelView::Ready);
    assert_eq!(
        rows(&mut app),
        [
            "websocket closed",
            "Mesh for crate_01 not found",
            "Connected to localhost:9002"
        ]
    );
    assert!(all_text(&mut app).contains("connection · 12:04:31"));
    let count = |app: &App| {
        app.world()
            .resource::<PanelStatus>()
            .count(CONSOLE)
            .map(str::to_string)
    };
    assert_eq!(count(&app).as_deref(), Some("1 warn · 1 err"));

    chip(&mut app, LevelChip::Warn);
    assert_eq!(rows(&mut app), ["Mesh for crate_01 not found"]);
    chip(&mut app, LevelChip::Debug);
    assert!(rows(&mut app).is_empty());
    let none = find::<NoMatches>(&mut app);
    assert!(displayed(&app, none));
    chip(&mut app, LevelChip::All);
    app.world_mut().resource_mut::<ConsoleState>().filter = "LOCALHOST".into();
    frames(&mut app, 2);
    assert_eq!(rows(&mut app), ["Connected to localhost:9002"]);
    app.world_mut()
        .resource_mut::<ConsoleState>()
        .filter
        .clear();
    frames(&mut app, 2);

    push(&mut app, MessageLevel::Info, "Reconnected");
    frames(&mut app, 2);
    assert_eq!(rows(&mut app)[0], "Reconnected");
    assert_eq!(rows(&mut app).len(), 4);

    act(&mut app, PAUSE);
    assert!(app
        .world()
        .resource::<PanelStatus>()
        .action_on(CONSOLE, PAUSE));
    push(&mut app, MessageLevel::Error, "held back");
    frames(&mut app, 2);
    assert_eq!(rows(&mut app)[0], "Reconnected", "paused");
    act(&mut app, PAUSE);
    assert_eq!(rows(&mut app)[0], "held back", "resumed");

    act(&mut app, CLEAR);
    assert_eq!(view(&mut app), PanelView::empty("No messages"));
    assert_eq!(count(&app), None);
    assert!(!app
        .world()
        .resource::<PanelStatus>()
        .action_enabled_or(CONSOLE, CLEAR, true));
}

/// Problems while the Console is closed count on its badge; showing it
/// reads them.
#[test]
fn unseen_problems_count_until_the_console_shows() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    push(&mut app, MessageLevel::Warning, "a");
    push(&mut app, MessageLevel::Error, "b");
    push(&mut app, MessageLevel::Info, "c");
    frames(&mut app, 2);
    assert_eq!(
        *app.world().resource::<ConsoleUnread>(),
        ConsoleUnread {
            warnings: 1,
            errors: 1
        }
    );
    assert_eq!(
        app.world().resource::<PanelStatus>().badge(CONSOLE),
        Some((BadgeKind::Error, 2))
    );
    app.world_mut().write_message(UiCommand::OpenPanel {
        panel: CONSOLE,
        slot: Slot::Primary,
    });
    frames(&mut app, 3);
    assert_eq!(app.world().resource::<PanelStatus>().badge(CONSOLE), None);
}

/// Runs `line` in the command line.
fn submit(app: &mut App, line: &str) {
    let e = find::<CommandLine>(app);
    app.world_mut().trigger(TextSubmit {
        entity: e,
        value: line.into(),
    });
    frames(app, 1);
}

/// The command line's lines so far, oldest first; then forgets them.
fn said(app: &mut App) -> Vec<(MessageLevel, String)> {
    let mut out = app.world_mut().resource_mut::<ConsoleOutput>();
    std::mem::take(&mut out.lines)
        .into_iter()
        .map(|m| (m.level, m.text))
        .collect()
}

fn command_text(app: &mut App) -> String {
    let world = app.world_mut();
    let line = world
        .query_filtered::<Entity, With<CommandLine>>()
        .single(world)
        .unwrap();
    world
        .query_filtered::<(&EditableText, &ChildOf), With<TextFieldInput>>()
        .iter(world)
        .find(|(_, p)| p.parent() == line)
        .map(|(t, _)| t.value().to_string())
        .unwrap()
}

/// `topic -l`, `-i` and `-e`; help; a mistake says how; a line runs once
/// and empties the field; ↑ brings it back.
#[test]
fn the_command_line_reads_topics() {
    let mut app = console_with(&[]);
    let mut topics = GzTopics::default();
    topics.set(vec![
        ("/clock".into(), "gz.msgs.Clock".into()),
        ("/stats".into(), "gz.msgs.WorldStatistics".into()),
    ]);
    *app.world_mut().resource_mut::<GzTopics>() = topics;
    frames(&mut app, 1);
    said(&mut app);
    submit(&mut app, "topic -l");
    frames(&mut app, 2);
    assert_eq!(rows(&mut app)[..2], ["/clock\n/stats", "› topic -l"]);
    assert_eq!(
        said(&mut app),
        [
            (MessageLevel::Info, "› topic -l".to_string()),
            (MessageLevel::Info, "/clock\n/stats".to_string())
        ]
    );
    assert_eq!(command_text(&mut app), "");

    submit(&mut app, "gz topic -i -t /clock");
    assert_eq!(said(&mut app)[1].1, "/clock: gz.msgs.Clock");
    submit(&mut app, "topic -i -t /nope");
    assert_eq!(
        said(&mut app)[1],
        (MessageLevel::Error, "No topic /nope".into())
    );
    submit(&mut app, "topic -e -t /stats");
    assert_eq!(said(&mut app)[1].1, "Echoing /stats in Topics");
    frames(&mut app, 2);
    // Topics opens beside the Console, echoing it.
    let l = layout(&app);
    assert_eq!(
        (l.primary, l.secondary),
        (Some(CONSOLE), Some(crate::panels::TOPICS))
    );
    assert!(app.world().get_resource::<TopicsStart>().is_none());
    submit(&mut app, "model -l");
    assert_eq!(said(&mut app)[1].0, MessageLevel::Error);
    submit(&mut app, "help");
    assert!(said(&mut app)[1].1.contains("topic -l"));
    submit(&mut app, "   ");
    assert!(said(&mut app).is_empty(), "nothing to run");

    // ↑ walks back through what was typed; ↓ past the newest empties.
    let input = {
        let world = app.world_mut();
        let line = world
            .query_filtered::<Entity, With<CommandLine>>()
            .single(world)
            .unwrap();
        world
            .query_filtered::<(Entity, &ChildOf), With<TextFieldInput>>()
            .iter(world)
            .find(|(_, p)| p.parent() == line)
            .unwrap()
            .0
    };
    focused_key(&mut app, input, KeyCode::ArrowUp);
    assert_eq!(command_text(&mut app), "help");
    focused_key(&mut app, input, KeyCode::ArrowUp);
    assert_eq!(command_text(&mut app), "model -l");
    focused_key(&mut app, input, KeyCode::ArrowDown);
    assert_eq!(command_text(&mut app), "help");
    focused_key(&mut app, input, KeyCode::ArrowDown);
    assert_eq!(command_text(&mut app), "");
}

/// A service call sends its request and shows its reply, or says it was
/// not found or got no reply in time; a message needs a server that takes
/// binary requests; a publication needs one that publishes; nothing runs
/// while not connected.
#[test]
fn the_command_line_calls_services_and_publishes() {
    use prost::Message as _;
    let mut app = console_with(&[]);
    sent(&mut app);
    said(&mut app);
    submit(
        &mut app,
        "service -s /world/w/scene/info --reqtype gz.msgs.Empty --reptype gz.msgs.Scene",
    );
    let requests: Vec<_> = sent(&mut app)
        .into_iter()
        .filter(|c| matches!(c, GzCommand::Request { .. }))
        .collect();
    assert_eq!(
        requests,
        [GzCommand::Request {
            service: "/world/w/scene/info".into(),
            msg_type: "gz.msgs.Empty".into(),
            payload: vec![],
        }]
    );
    let reply = msgs::Boolean {
        data: true,
        ..default()
    }
    .encode_to_vec();
    app.world_mut().write_message(GzServiceReply {
        service: "/world/w/scene/info".into(),
        msg_type: "gz.msgs.Boolean".into(),
        payload: reply,
    });
    frames(&mut app, 1);
    let lines = said(&mut app);
    assert_eq!(lines[1].1, "Calling /world/w/scene/info…");
    assert_eq!(
        lines[2],
        (
            MessageLevel::Info,
            "/world/w/scene/info replied gz.msgs.Boolean\ndata: true".into()
        )
    );

    submit(
        &mut app,
        "service -s /nope --reqtype gz.msgs.Empty --reptype gz.msgs.Boolean",
    );
    let missing = msgs::StringMsg {
        data: "service_not_found".into(),
        ..default()
    }
    .encode_to_vec();
    app.world_mut().write_message(GzServiceReply {
        service: "/nope".into(),
        msg_type: "gz.msgs.StringMsg".into(),
        payload: missing,
    });
    frames(&mut app, 1);
    assert_eq!(
        said(&mut app)[2],
        (MessageLevel::Error, "/nope: service not found".into())
    );

    submit(
        &mut app,
        "service -s /slow --reqtype gz.msgs.Empty --reptype gz.msgs.Boolean --timeout 0",
    );
    frames(&mut app, 1);
    assert_eq!(
        said(&mut app)[2],
        (MessageLevel::Error, "/slow: no reply in time".into())
    );

    // A message to send needs binary requests.
    app.world_mut().resource_mut::<GzStatus>().edits = Some(false);
    submit(
        &mut app,
        "service -s /world/w/control --reqtype gz.msgs.WorldControl --reptype gz.msgs.Boolean --req 'pause: true'",
    );
    assert_eq!(said(&mut app)[1].0, MessageLevel::Error);
    app.world_mut().resource_mut::<GzStatus>().edits = Some(true);
    sent(&mut app);
    submit(
        &mut app,
        "service -s /world/w/control --reqtype gz.msgs.WorldControl --reptype gz.msgs.Boolean --req 'pause: true'",
    );
    let sent_now = sent(&mut app);
    let payload = sent_now.iter().find_map(|c| match c {
        GzCommand::Request { payload, .. } => Some(payload.clone()),
        _ => None,
    });
    let control = msgs::WorldControl::decode(payload.unwrap().as_slice()).unwrap();
    assert!(control.pause);
    submit(
        &mut app,
        "service -s /a --reqtype gz.msgs.Empty --reptype gz.msgs.Boolean --req 'nope: 1'",
    );
    assert!(said(&mut app)
        .iter()
        .any(|(_, t)| t.starts_with("can't read the message")));
    submit(&mut app, "service -l");
    assert_eq!(said(&mut app)[1].0, MessageLevel::Error);

    // Publishing needs a server that publishes.
    let publish = "topic -t /cmd_vel -m gz.msgs.Twist -p 'linear { x: 0.5 }'";
    app.world_mut().resource_mut::<GzStatus>().publishes = Some(false);
    submit(&mut app, publish);
    assert_eq!(
        said(&mut app)[1],
        (
            MessageLevel::Error,
            "This server doesn't take publications".into()
        )
    );
    app.world_mut().resource_mut::<GzStatus>().publishes = Some(true);
    sent(&mut app);
    submit(&mut app, publish);
    assert_eq!(said(&mut app)[1].1, "Published a gz.msgs.Twist on /cmd_vel");
    let twist = sent(&mut app).into_iter().find_map(|c| match c {
        GzCommand::Publish { topic, payload, .. } if topic == "/cmd_vel" => Some(payload),
        _ => None,
    });
    assert_eq!(
        msgs::Twist::decode(twist.unwrap().as_slice())
            .unwrap()
            .linear
            .unwrap()
            .x,
        0.5
    );
    submit(&mut app, "topic -t /cmd_vel -m gz.msgs.Twist -p 'nope: 1'");
    assert_eq!(said(&mut app)[1].0, MessageLevel::Error);

    // Not connected: nothing runs but help.
    app.world_mut().resource_mut::<GzStatus>().status = gz_core::transport::Status::Disconnected;
    frames(&mut app, 2);
    submit(&mut app, "topic -l");
    assert_eq!(
        said(&mut app)[1],
        (MessageLevel::Error, "Not connected".into())
    );
}
