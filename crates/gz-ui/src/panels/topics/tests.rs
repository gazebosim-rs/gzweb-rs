//! The Topics panel through the shell.

use std::time::Duration;

use crate::widgets::clipboard::CopyText;
use bevy::input::keyboard::KeyCode;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::{Activate, ValueChange};
use gz_bevy::connection::{GzCommand, GzPublish, GzStatus};
use gz_bevy::topics::{GzTopics, GzTraffic};
use gz_core::msgs;
use gz_core::transport::Status;
use prost::Message as _;

use super::model::{
    clip_lines, is_model, is_sensor, rate_label, Echo, TopicChip, ECHO_LINES, ECHO_LINE_CHARS,
};
use super::view::{ChipChoice, EchoPart, EchoSection, NoMatches, TopicList, TopicRow};
use super::{
    COPY, NO_PLOT, NO_PUBLISH, NO_SERVICES, PLOT_FIELD, PUBLISH, PUBLISH_FOOTER, SERVICES, TOPICS,
};
use crate::panels::{ActionId, PanelView};
use crate::shell::sidebar::body::{ActionButton, BodyView, NOT_YET};
use crate::shell::{Slot, UiCommand};
use crate::test_support::frames;
use crate::test_support::shell::*;
use crate::widgets::Tooltip;
use gz_bevy::topics::TopicInfo;

fn view(app: &mut App) -> PanelView {
    let world = app.world_mut();
    world
        .query::<(&BodyView, &crate::panels::PanelBody)>()
        .iter(world)
        .find(|(_, b)| b.0 == TOPICS)
        .unwrap()
        .0
         .0
        .clone()
}

/// Each of the panel's action buttons: (action, disabled, tooltip detail).
fn actions(app: &mut App) -> Vec<(ActionId, bool, Option<String>)> {
    let world = app.world_mut();
    let mut a: Vec<_> = world
        .query::<(&ActionButton, Has<InteractionDisabled>, &Tooltip)>()
        .iter(world)
        .filter(|(b, ..)| b.panel == TOPICS)
        .map(|(b, d, t)| (b.action, d, t.detail.clone()))
        .collect();
    a.sort_by_key(|a| a.0);
    a
}

fn open(app: &mut App) {
    app.world_mut().write_message(UiCommand::OpenPanel {
        panel: TOPICS,
        slot: Slot::Primary,
    });
    frames(app, 2);
}

/// The world's topics, as a server lists them.
const LIST: [(&str, &str); 5] = [
    ("/clock", "gz.msgs.Clock"),
    ("/model/robot/cmd_vel", "gz.msgs.Twist"),
    ("/stats", "gz.msgs.WorldStatistics"),
    (
        "/world/w/model/box/link/l/sensor/cam/image",
        "gz.msgs.Image",
    ),
    ("/imu", "gz.msgs.IMU"),
];

fn set_topics(app: &mut App, list: &[(&str, &str)]) {
    let pairs = list
        .iter()
        .map(|(t, m)| (t.to_string(), m.to_string()))
        .collect();
    app.world_mut().resource_mut::<GzTopics>().set(pairs);
    frames(app, 2);
}

/// The panel live and open, listing [`LIST`].
fn live_topics() -> App {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    open(&mut app);
    go_live(&mut app, false);
    set_topics(&mut app, &LIST);
    app
}

fn rows(app: &mut App) -> Vec<String> {
    let world = app.world_mut();
    world
        .query::<&TopicRow>()
        .iter(world)
        .map(|r| r.0.clone())
        .collect()
}

fn row(app: &mut App, name: &str) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &TopicRow)>()
        .iter(world)
        .find(|(_, r)| r.0 == name)
        .unwrap()
        .0
}

fn echo_part(app: &mut App, part: EchoPart) -> String {
    let world = app.world_mut();
    world
        .query::<(&EchoPart, &Text)>()
        .iter(world)
        .find(|(p, _)| **p == part)
        .unwrap()
        .1
         .0
        .clone()
}

fn shown<C: Component>(app: &mut App) -> bool {
    let world = app.world_mut();
    world
        .query_filtered::<&Node, With<C>>()
        .iter(world)
        .all(|n| n.display != Display::None)
}

fn chip(app: &mut App, chip: TopicChip) {
    let world = app.world_mut();
    let e = world
        .query::<(Entity, &ChipChoice)>()
        .iter(world)
        .find(|(_, c)| c.0 == chip)
        .unwrap()
        .0;
    world.trigger(ValueChange {
        source: e,
        value: e,
        is_final: true,
    });
    frames(app, 2);
}

/// Types `text` into the filter field.
fn type_filter(app: &mut App, text: &str) {
    let field = find::<super::view::TopicFilter>(app);
    let input = crate::test_support::descendants(app.world(), field)
        .into_iter()
        .find(|e| app.world().get::<bevy::text::EditableText>(*e).is_some())
        .unwrap();
    app.world_mut()
        .entity_mut(input)
        .insert(bevy::text::EditableText::new(text));
    frames(app, 2);
}

fn subscriptions(app: &mut App) -> Vec<GzCommand> {
    sent(app)
        .into_iter()
        .filter(|c| matches!(c, GzCommand::Subscribe(_) | GzCommand::Unsubscribe(_)))
        .collect()
}

/// "Not connected", skeleton rows while loading, "No topics yet" once
/// live with none, then the list, counted. Publish and Services say why
/// they're unavailable; the footer is there in every state: Plot field
/// waits for Plot, Copy for a message, Publish… for a server that takes
/// one.
#[test]
fn the_topics_panel_follows_the_connection() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    open(&mut app);
    assert_eq!(view(&mut app), PanelView::empty("Not connected"));
    let no = |why: &str| Some(why.to_string());
    let mut want = vec![
        (PUBLISH, true, no(NO_PUBLISH)),
        (SERVICES, true, no(NO_SERVICES)),
        (PLOT_FIELD, true, no(NO_PLOT)),
        (COPY, true, no(NOT_YET)),
        (PUBLISH_FOOTER, true, no(NO_PUBLISH)),
    ];
    want.sort_by_key(|a| a.0);
    assert_eq!(actions(&mut app), want);
    app.world_mut().resource_mut::<GzStatus>().status = Status::Connected;
    frames(&mut app, 2);
    assert_eq!(view(&mut app), PanelView::Loading);
    go_live(&mut app, false);
    assert_eq!(view(&mut app), PanelView::empty("No topics yet"));
    set_topics(&mut app, &LIST);
    assert_eq!(view(&mut app), PanelView::Ready);
    assert!(all_text(&mut app).contains("5 topics"));
    assert_eq!(rows(&mut app).len(), 5);
    assert!(!shown::<EchoSection>(&mut app), "nothing echoed yet");
}

/// The list is sorted by name; the filter matches names ignoring case;
/// the chips pick sensors (by type or path), models (by path) and quiet
/// topics (no rate, or none over the last second); "No topics match"
/// shows while they hide every topic.
#[test]
fn the_list_filters_by_name_and_kind() {
    let mut app = live_topics();
    assert_eq!(
        rows(&mut app),
        [
            "/clock",
            "/imu",
            "/model/robot/cmd_vel",
            "/stats",
            "/world/w/model/box/link/l/sensor/cam/image"
        ]
    );
    type_filter(&mut app, "MODEL");
    assert_eq!(
        rows(&mut app),
        [
            "/model/robot/cmd_vel",
            "/world/w/model/box/link/l/sensor/cam/image"
        ]
    );
    app.world_mut()
        .resource_mut::<super::model::TopicsState>()
        .filter
        .clear();
    chip(&mut app, TopicChip::Sensors);
    assert_eq!(
        rows(&mut app),
        ["/imu", "/world/w/model/box/link/l/sensor/cam/image"]
    );
    chip(&mut app, TopicChip::Models);
    assert_eq!(
        rows(&mut app),
        [
            "/model/robot/cmd_vel",
            "/world/w/model/box/link/l/sensor/cam/image"
        ]
    );
    {
        let mut traffic = app.world_mut().resource_mut::<GzTraffic>();
        for i in 0..5 {
            traffic.arrived("/clock", i as f64 * 0.1);
        }
        traffic.arrived("/stats", 0.0);
        traffic.measure(1.0);
        traffic.measure(2.0);
        traffic.arrived("/clock", 2.5);
        traffic.measure(3.0);
    }
    chip(&mut app, TopicChip::Quiet);
    assert!(!rows(&mut app).contains(&"/clock".to_string()));
    assert!(
        rows(&mut app).contains(&"/stats".to_string()),
        "silent since"
    );
    assert!(!shown::<NoMatches>(&mut app));
    app.world_mut()
        .resource_mut::<super::model::TopicsState>()
        .filter = "nothing".into();
    frames(&mut app, 2);
    assert!(rows(&mut app).is_empty());
    assert!(shown::<NoMatches>(&mut app));

    let t = |name: &str, msg_type: &str| TopicInfo {
        name: name.into(),
        msg_type: msg_type.into(),
    };
    assert!(is_sensor(&t("/lidar", "gz.msgs.LaserScan")));
    assert!(!is_sensor(&t("/clock", "gz.msgs.Clock")));
    assert!(is_model(&t("/model/x/odometry", "gz.msgs.Odometry")));
    assert!(!is_model(&t("/clock", "gz.msgs.Clock")));
}

/// A row's click echoes its topic: it is subscribed while the panel
/// shows, its newest message shows as text and Copy copies it; collapsing
/// the sidebar lets go, and opening it again holds it again. A topic that
/// leaves the server's list says it ended and keeps its message; the
/// close button stops the echo.
#[test]
fn a_row_echoes_its_topic_while_the_panel_shows() {
    let mut app = live_topics();
    sent(&mut app);
    let stats = "/stats".to_string();
    let r = row(&mut app, &stats);
    app.world_mut().trigger(Activate { entity: r });
    frames(&mut app, 2);
    assert_eq!(
        subscriptions(&mut app),
        [GzCommand::Subscribe(stats.clone())]
    );
    assert!(shown::<EchoSection>(&mut app));
    assert_eq!(echo_part(&mut app, EchoPart::Title), stats);
    assert_eq!(
        echo_part(&mut app, EchoPart::Meta),
        "gz.msgs.WorldStatistics · —"
    );
    assert_eq!(
        echo_part(&mut app, EchoPart::Status),
        "Waiting for first message…"
    );
    let disabled =
        |app: &mut App, id: ActionId| actions(app).into_iter().find(|a| a.0 == id).unwrap().1;
    assert!(disabled(&mut app, COPY));

    let msg = msgs::WorldStatistics {
        iterations: 42,
        ..Default::default()
    };
    app.world_mut().write_message(GzPublish {
        topic: stats.clone(),
        msg_type: "gz.msgs.WorldStatistics".into(),
        payload: msg.encode_to_vec(),
    });
    frames(&mut app, 2);
    assert_eq!(echo_part(&mut app, EchoPart::Message), "iterations: 42");
    assert!(!disabled(&mut app, COPY));
    let copy = {
        let world = app.world_mut();
        world
            .query::<(Entity, &ActionButton)>()
            .iter(world)
            .find(|(_, b)| b.action == COPY)
            .unwrap()
            .0
    };
    app.world_mut().trigger(Activate { entity: copy });
    frames(&mut app, 1);
    let copied: Vec<CopyText> = app
        .world_mut()
        .resource_mut::<Messages<CopyText>>()
        .drain()
        .collect();
    assert_eq!(copied, [CopyText("iterations: 42".into())]);

    app.world_mut().write_message(UiCommand::ToggleSidebar);
    frames(&mut app, 2);
    assert_eq!(
        subscriptions(&mut app),
        [GzCommand::Unsubscribe(stats.clone())],
        "hidden: let go"
    );
    app.world_mut().write_message(UiCommand::ToggleSidebar);
    frames(&mut app, 2);
    assert_eq!(
        subscriptions(&mut app),
        [GzCommand::Subscribe(stats.clone())]
    );

    set_topics(&mut app, &LIST[..2]);
    assert_eq!(echo_part(&mut app, EchoPart::Status), "Topic ended");
    assert_eq!(echo_part(&mut app, EchoPart::Message), "iterations: 42");

    let close = find::<super::view::EchoClose>(&mut app);
    app.world_mut().trigger(Activate { entity: close });
    frames(&mut app, 2);
    assert!(!shown::<EchoSection>(&mut app));
    assert_eq!(subscriptions(&mut app), [GzCommand::Unsubscribe(stats)]);
}

/// With the list focused, ↓ echoes the first topic, then the next; End
/// the last, ↑ the one before, Home the first; Esc stops the echo. The
/// keys stay with the list.
#[test]
fn the_list_keys_move_the_echo() {
    let mut app = live_topics();
    let list = find::<TopicList>(&mut app);
    let echoed = |app: &mut App| app.world().resource::<Echo>().topic.clone();
    focused_key(&mut app, list, KeyCode::ArrowDown);
    assert_eq!(echoed(&mut app).as_deref(), Some("/clock"));
    assert!(!app
        .world()
        .resource::<ButtonInput<KeyCode>>()
        .pressed(KeyCode::ArrowDown));
    focused_key(&mut app, list, KeyCode::ArrowDown);
    assert_eq!(echoed(&mut app).as_deref(), Some("/imu"));
    focused_key(&mut app, list, KeyCode::End);
    assert_eq!(
        echoed(&mut app).as_deref(),
        Some("/world/w/model/box/link/l/sensor/cam/image")
    );
    focused_key(&mut app, list, KeyCode::ArrowUp);
    assert_eq!(echoed(&mut app).as_deref(), Some("/stats"));
    focused_key(&mut app, list, KeyCode::Home);
    assert_eq!(echoed(&mut app).as_deref(), Some("/clock"));
    focused_key(&mut app, list, KeyCode::Escape);
    assert_eq!(echoed(&mut app), None);
}

/// Each row shows its topic's rate, and the echo's header its type and
/// rate; the list is asked for again every two seconds while live.
#[test]
fn rates_show_and_the_list_refreshes() {
    let mut app = live_topics();
    // Frames 10 ms apart, so the rate measured holds while it's read.
    let step = |app: &mut App, ms| {
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            ms,
        )));
    };
    step(&mut app, 10);
    frames(&mut app, 1);
    let now = app.world().resource::<Time<Real>>().elapsed_secs_f64();
    {
        let mut traffic = app.world_mut().resource_mut::<GzTraffic>();
        for i in 0..50 {
            traffic.arrived("/clock", now - 1.0 + i as f64 * 0.02);
        }
        traffic.measure(now);
    }
    let r = row(&mut app, "/clock");
    app.world_mut().trigger(Activate { entity: r });
    frames(&mut app, 2);
    let rate = {
        let world = app.world_mut();
        world
            .query::<(&super::view::RowRate, &Text)>()
            .iter(world)
            .find(|(r, _)| r.0 == "/clock")
            .unwrap()
            .1
             .0
            .clone()
    };
    assert_eq!(rate, "50 Hz");
    assert_eq!(echo_part(&mut app, EchoPart::Meta), "gz.msgs.Clock · 50 Hz");
    step(&mut app, 500);
    sent(&mut app);
    frames(&mut app, 5);
    let refreshes = sent(&mut app)
        .into_iter()
        .filter(|c| *c == GzCommand::RefreshTopics)
        .count();
    assert_eq!(refreshes, 1, "one in 2.5 s");
}

/// `gzweb --echo`: the panel opens at start and echoes the topic once
/// the server lists it.
#[test]
fn a_start_echo_opens_the_panel_and_waits_for_the_topic() {
    let mut app = shell_app((1440, 900), 1.0);
    app.insert_resource(super::TopicsStart {
        echo: "/stats".into(),
        beside: None,
    });
    frames(&mut app, 2);
    let layout = app.world().resource::<crate::shell::ShellLayout>();
    assert_eq!(layout.primary, Some(TOPICS));
    go_live(&mut app, false);
    assert!(app.world().contains_resource::<super::TopicsStart>());
    set_topics(&mut app, &LIST);
    assert!(app.world().resource::<Echo>().is("/stats"));
    assert!(!app.world().contains_resource::<super::TopicsStart>());
}

/// A message the client can't read says why in place of its text.
#[test]
fn an_unreadable_message_says_why() {
    let mut app = live_topics();
    set_topics(&mut app, &[("/odd", "gz.msgs.NotAType")]);
    let r = row(&mut app, "/odd");
    app.world_mut().trigger(Activate { entity: r });
    frames(&mut app, 2);
    app.world_mut().write_message(GzPublish {
        topic: "/odd".into(),
        msg_type: String::new(),
        payload: vec![1, 2],
    });
    frames(&mut app, 2);
    assert_eq!(
        echo_part(&mut app, EchoPart::Message),
        "unknown message type gz.msgs.NotAType"
    );
}

/// Rate labels, and a long message cut to its first lines.
#[test]
fn labels_and_long_messages() {
    assert_eq!(rate_label(None), "—");
    assert_eq!(rate_label(Some(0.0)), "0 Hz");
    assert_eq!(rate_label(Some(2.54)), "2.5 Hz");
    assert_eq!(rate_label(Some(999.6)), "1000 Hz");
    let long: String = (0..ECHO_LINES + 3).map(|i| format!("{i}\n")).collect();
    let cut = clip_lines(&long);
    assert_eq!(cut.lines().count(), ECHO_LINES + 1);
    assert!(cut.ends_with("… 3 more lines"));
    assert_eq!(clip_lines("a\nb\n"), "a\nb");
    let wide = format!("ranges: [{}]", "1, ".repeat(ECHO_LINE_CHARS));
    let cut = clip_lines(&wide);
    let more = wide.chars().count() - ECHO_LINE_CHARS;
    assert!(
        cut.ends_with(&format!(" … {more} more characters")),
        "{cut}"
    );
    assert_eq!(cut.lines().count(), 1);
}

/// The echo shows a long message clipped; Copy copies all of it.
#[test]
fn copy_copies_the_whole_message() {
    let mut app = live_topics();
    set_topics(&mut app, &[("/poses", "gz.msgs.Pose_V")]);
    let r = row(&mut app, "/poses");
    app.world_mut().trigger(Activate { entity: r });
    frames(&mut app, 2);
    let poses = msgs::PoseV {
        pose: (0..100)
            .map(|i| msgs::Pose {
                name: format!("p{i}"),
                position: Some(msgs::Vector3d {
                    x: 1.0,
                    ..Default::default()
                }),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    };
    app.world_mut().write_message(GzPublish {
        topic: "/poses".into(),
        msg_type: "gz.msgs.Pose_V".into(),
        payload: poses.encode_to_vec(),
    });
    frames(&mut app, 2);
    assert!(echo_part(&mut app, EchoPart::Message).ends_with("more lines"));
    let copy = {
        let world = app.world_mut();
        world
            .query::<(Entity, &ActionButton)>()
            .iter(world)
            .find(|(_, b)| b.action == COPY)
            .unwrap()
            .0
    };
    app.world_mut().trigger(Activate { entity: copy });
    frames(&mut app, 1);
    let copied = app
        .world_mut()
        .resource_mut::<Messages<CopyText>>()
        .drain()
        .next()
        .unwrap()
        .0;
    assert!(
        copied.lines().count() > ECHO_LINES,
        "{}",
        copied.lines().count()
    );
    assert!(copied.contains("name: \"p99\""));
    assert!(!copied.contains("more lines"));
}

/// A key move scrolls the list to show the echoed row.
#[test]
fn a_key_move_shows_its_row() {
    let mut app = live_topics();
    let names: Vec<(String, &str)> = (0..100)
        .map(|i| (format!("/t{i:03}"), "gz.msgs.Clock"))
        .collect();
    let list: Vec<(&str, &str)> = names.iter().map(|(n, t)| (n.as_str(), *t)).collect();
    set_topics(&mut app, &list);
    frames(&mut app, 2);
    let list = find::<TopicList>(&mut app);
    let y = |app: &mut App| app.world().get::<ScrollPosition>(list).unwrap().y;
    assert_eq!(y(&mut app), 0.0);
    focused_key(&mut app, list, KeyCode::End);
    frames(&mut app, 2);
    // Again, now the echo section has taken its share of the height.
    focused_key(&mut app, list, KeyCode::End);
    frames(&mut app, 2);
    let end = y(&mut app);
    assert!(end > 1000.0, "{end}");
    focused_key(&mut app, list, KeyCode::ArrowUp);
    frames(&mut app, 2);
    assert_eq!(y(&mut app), end, "still in view: no scroll");
    focused_key(&mut app, list, KeyCode::Home);
    frames(&mut app, 2);
    assert_eq!(y(&mut app), 0.0);
}

/// Plot field, once a topic is echoed, opens the Plot panel's picker on
/// it in the second slot.
#[test]
fn plot_field_picks_the_echoed_topic() {
    let mut app = live_topics();
    let disabled = |app: &mut App| {
        actions(app)
            .into_iter()
            .find(|a| a.0 == PLOT_FIELD)
            .unwrap()
            .1
    };
    assert!(disabled(&mut app));
    let r = row(&mut app, "/stats");
    app.world_mut().trigger(Activate { entity: r });
    frames(&mut app, 2);
    assert!(!disabled(&mut app));
    let plot = {
        let world = app.world_mut();
        world
            .query::<(Entity, &ActionButton)>()
            .iter(world)
            .find(|(_, b)| b.action == PLOT_FIELD)
            .unwrap()
            .0
    };
    app.world_mut().trigger(Activate { entity: plot });
    frames(&mut app, 3);
    let layout = app.world().resource::<crate::shell::ShellLayout>();
    assert_eq!(
        (layout.primary, layout.secondary),
        (Some(TOPICS), Some(crate::panels::PLOT))
    );
}
