//! The Plot panel: its rules, its pixels, and the panel through the
//! shell.

use std::collections::VecDeque;
use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::{Activate, ValueChange};
use gz_bevy::connection::{GzCommand, GzPublish, GzStatus};
use gz_bevy::topics::GzTopics;
use gz_core::fields::FieldPath;
use gz_core::msgs;
use prost::Message as _;

use super::model::*;
use super::raster::{trace, Canvas};
use super::systems::{ChartFrame, EMPTY};
use super::view::{
    ChartHover, HoverTip, LegendPart, LegendRemove, PickRow, PickerButton, PickerStatus, PlotArea,
    PlotNotice, WindowChip, YAxis,
};
use super::{PlotCharts, PlotPick, PlotStart, ADD, ADD_FIELD, EXPORT, PAUSE, PLOT, POP_OUT};
use crate::assets::icons::Icon;
use crate::panels::{ActionId, PanelActionActivated, PanelView};
use crate::shell::sidebar::body::{ActionButton, BodyView};
use crate::shell::{PanelStatus, ShellLayout, Slot, UiCommand};
use crate::test_support::frames;
use crate::test_support::shell::*;
use crate::widgets::{BadgeKind, Tooltip};

fn path(s: &str) -> FieldPath {
    s.parse().unwrap()
}

/// About 3 ticks on 1, 2 or 5 steps, covering the range; a flat range
/// widens; labels take the step's decimals.
#[test]
fn ticks_cover_the_range_on_round_steps() {
    assert_eq!(ticks(0.0, 0.4, 3), (0.2, vec![0.0, 0.2, 0.4]));
    let (step, t) = ticks(0.0, 4.71, 3);
    assert_eq!((step, t.first(), t.last()), (2.0, Some(&0.0), Some(&6.0)));
    let (step, t) = ticks(-1.3, 0.2, 3);
    assert_eq!(step, 0.5);
    assert_eq!((t[0], t[t.len() - 1]), (-1.5, 0.5));
    let (_, t) = ticks(1.0, 1.0, 3);
    assert!(t[0] < 1.0 && t[t.len() - 1] > 1.0, "{t:?}");
    let (_, t) = ticks(0.0, 0.0, 3);
    assert!(t[0] < 0.0 && t[t.len() - 1] > 0.0, "{t:?}");
    assert_eq!(tick_label(0.2, 0.2), "0.2");
    assert_eq!(tick_label(0.0, 0.2), "0");
    assert_eq!(tick_label(-0.0, 0.5), "0");
    assert_eq!(tick_label(-0.5, 0.5), "-0.5");
    assert_eq!(tick_label(1500.0, 500.0), "1500");
    assert_eq!(tick_label(0.05, 0.05), "0.05");
    assert_eq!(tick_label(2e-6, 1e-6), "2.0e-6");
    // Noise around zero is flat: one unit either side.
    let (_, t) = ticks(0.0, 1.6e-14, 3);
    assert_eq!((t[0], t[t.len() - 1]), (-1.0, 1.0));
}

/// Live values at three significant digits; whole numbers from 1000,
/// e-notation for the very large and small; numerical zero is 0.
#[test]
fn values_read_at_three_digits() {
    assert_eq!(value_label(0.352), "0.352");
    assert_eq!(value_label(0.4), "0.400");
    assert_eq!(value_label(3.52), "3.52");
    assert_eq!(value_label(-47.123), "-47.1");
    assert_eq!(value_label(1234.5), "1234");
    assert_eq!(value_label(0.0), "0.00");
    assert_eq!(value_label(6.4e-19), "0.00");
    assert_eq!(value_label(2.5e-5), "2.50e-5");
    assert_eq!(value_label(3.5e7 + 0.5), "3.50e7");
    assert_eq!(value_label(976_000_000.0), "976000000");
    assert_eq!(value_label(3.0), "3");
    assert_eq!(value_label(f64::NAN), "NaN");
    assert_eq!(value_label(f64::INFINITY), "∞");
    assert_eq!(value_label(f64::NEG_INFINITY), "-∞");
    assert_eq!(span_label(10.0), "-10 s");
    assert_eq!(span_label(2.5), "-2.5 s");
    assert_eq!(span_label(120.0), "-2 min");
    assert_eq!(span_label(150.0), "-2.5 min");
}

/// Units come from the message type where Gazebo's are known.
#[test]
fn units_come_from_known_fields() {
    assert_eq!(unit("gz.msgs.Twist", &path("linear.x")), "m/s");
    assert_eq!(unit("gz.msgs.Odometry", &path("twist.angular.z")), "rad/s");
    assert_eq!(unit("gz.msgs.Odometry", &path("pose.position.y")), "m");
    assert_eq!(unit("gz.msgs.IMU", &path("linear_acceleration.z")), "m/s²");
    assert_eq!(unit("gz.msgs.LaserScan", &path("ranges[3]")), "m");
    assert_eq!(unit("gz.msgs.Odometry", &path("pose.orientation.w")), "");
    assert_eq!(
        unit("gz.msgs.WorldStatistics", &path("real_time_factor")),
        ""
    );
}

/// A chart takes up to four series, each the first colour free (kept
/// when another is removed); a full or missing target takes a new chart;
/// removing a chart's last series removes it; a field is in a chart once.
#[test]
fn charts_hold_four_series_in_fixed_colours() {
    let mut data = PlotData::default();
    let a = data.add(
        "/odom",
        "gz.msgs.Odometry",
        path("twist.linear.x"),
        Target::New,
    );
    assert_eq!(data.charts[0].title, "twist.linear.x");
    assert_eq!(data.charts[0].unit, "m/s");
    for f in ["twist.linear.y", "twist.linear.z", "twist.angular.z"] {
        assert_eq!(data.add("/odom", "", path(f), Target::Chart(a)), a);
    }
    assert!(data.charts[0].is_full());
    let colors: Vec<usize> = data.charts[0].series.iter().map(|s| s.color).collect();
    assert_eq!(colors, [0, 1, 2, 3]);
    let b = data.add("/odom", "", path("pose.position.x"), Target::Chart(a));
    assert_ne!(a, b, "a full chart takes a new one");
    assert_eq!(
        data.add("/x", "", path("y"), Target::Chart(999)),
        data.charts[2].id
    );

    let second = data.charts[0].series[1].id;
    data.remove(second);
    let colors: Vec<usize> = data.charts[0].series.iter().map(|s| s.color).collect();
    assert_eq!(colors, [0, 2, 3], "the others keep theirs");
    data.add("/odom", "", path("twist.angular.x"), Target::Chart(a));
    assert_eq!(data.charts[0].series[3].color, 1, "the free colour");
    data.add("/odom", "", path("twist.angular.x"), Target::Chart(a));
    assert_eq!(data.charts[0].series.len(), 4, "once");

    let only = data.charts[1].series[0].id;
    data.remove(only);
    assert_eq!(data.charts.len(), 2);
    assert_eq!(data.topics(), ["/odom", "/x"]);
}

/// Samples record by topic and path; a missing field marks the series;
/// a late sample doesn't fold the line; values read at a time; the span
/// and range cover the samples; a reset clears them; CSV quotes cells.
#[test]
fn series_record_and_read() {
    let mut data = PlotData::default();
    let c = data.add("/odom", "", path("a"), Target::New);
    data.add("/odom", "", path("b"), Target::Chart(c));
    data.record("/odom", 1.0, |p| (p.to_string() == "a").then_some(1.5));
    let s = &data.charts[0].series;
    assert_eq!((s[0].last(), s[0].missing), (Some(1.5), false));
    assert_eq!((s[1].last(), s[1].missing), (None, true));
    data.record("/odom", 2.0, |_| Some(-2.0));
    data.record("/odom", 1.5, |_| Some(4.0));
    let s = &data.charts[0].series[0];
    assert_eq!(s.samples, [(1.0, 1.5), (2.0, -2.0), (2.0, 4.0)]);
    assert_eq!(s.value_at(0.5), None);
    assert_eq!(s.value_at(1.7), Some(1.5));
    assert_eq!(s.value_at(9.0), Some(4.0));
    assert!(!data.charts[0].series[1].missing, "the field came");
    let chart = &data.charts[0];
    assert_eq!(chart.span(), Some((1.0, 2.0)));
    assert_eq!(
        chart.range(1.2, 3.0),
        Some((-2.0, 4.0)),
        "with the sample before"
    );
    assert_eq!(chart.range(5.0, 6.0), Some((4.0, 4.0)));

    data.record("/other", 3.0, |_| Some(1.0));
    let csv = data.csv();
    assert!(
        csv.starts_with("chart,series,topic,field,sim_time,value\n"),
        "{csv}"
    );
    assert!(csv.contains("a,odom.a,/odom,a,1,1.5\n"), "{csv}");
    assert_eq!(csv.lines().count(), 1 + 3 + 2);

    data.mark_listed(|t| t != "/odom");
    assert!(data.series().all(|s| s.missing));
    data.clear_samples();
    assert!(!data.has_samples());
    data.mark_listed(|_| true);
    assert!(data.series().all(|s| !s.missing), "listed again");

    let mut data = PlotData::default();
    data.add("/a,b", "", path("x"), Target::New);
    data.record("/a,b", 0.0, |_| Some(1.0));
    assert!(data.csv().contains("\"/a,b\""), "{}", data.csv());
}

/// A series keeps its newest samples.
#[test]
fn a_series_keeps_its_newest_samples() {
    let mut data = PlotData::default();
    data.add("/t", "", path("x"), Target::New);
    for i in 0..MAX_SAMPLES + 5 {
        #[allow(clippy::cast_precision_loss)]
        data.record("/t", i as f64, |_| Some(0.0));
    }
    let s = &data.charts[0].series[0];
    assert_eq!(s.samples.len(), MAX_SAMPLES);
    assert_eq!(s.samples.front().map(|p| p.0), Some(5.0));
}

/// The clock runs on from the last stats at their real-time factor, at
/// most a second ahead; frozen while paused; a message's stamp is its
/// time when near the clock.
#[test]
fn the_clock_runs_between_stats() {
    let mut clock = SimClock::default();
    assert_eq!(clock.now(1.0), None);
    assert_eq!(clock.time_of(Some(5.0), 1.0), Some(5.0), "the stamp alone");
    assert_eq!(clock.time_of(None, 1.0), None);
    let mut stats = stats(false);
    stats.sim_time = 100.0;
    stats.real_time_factor = 0.5;
    clock.update(&stats, 10.0);
    assert_eq!(clock.now(10.0), Some(100.0));
    assert_eq!(clock.now(11.0), Some(100.5));
    assert_eq!(clock.now(20.0), Some(101.0), "at most a second ahead");
    assert_eq!(clock.time_of(Some(99.0), 10.0), Some(99.0));
    assert_eq!(clock.time_of(Some(1e9), 10.0), Some(100.0), "wall time");
    assert_eq!(clock.time_of(None, 10.0), Some(100.0));
    stats.paused = true;
    clock.update(&stats, 10.0);
    assert_eq!(clock.now(15.0), Some(100.0));
    assert!(SimClock::went_back(10.0, 9.0));
    assert!(!SimClock::went_back(10.0, 9.8));
}

/// A sparse series traces each sample; a dense one each column's first,
/// extremes and last; the samples just outside the window are kept; a
/// non-finite value breaks the line.
#[test]
fn a_trace_keeps_the_shape() {
    let px = |t: f64, v: f64| Vec2::new(t as f32, -(v as f32));
    let s: VecDeque<(f64, f64)> = [(0.0, 0.0), (1.0, 1.0), (2.0, 0.0), (3.0, 1.0)].into();
    let lines = trace(&s, 1.5, 2.5, px);
    assert_eq!(
        lines,
        [vec![
            Vec2::new(1.0, -1.0),
            Vec2::new(2.0, 0.0),
            Vec2::new(3.0, -1.0)
        ]]
    );
    let dense: VecDeque<(f64, f64)> =
        [(0.1, 0.0), (0.2, 5.0), (0.3, -5.0), (0.4, 1.0), (1.2, 2.0)].into();
    let lines = trace(&dense, 0.0, 2.0, px);
    assert_eq!(
        lines[0],
        [
            Vec2::new(0.1, 0.0),
            Vec2::new(0.2, -5.0),
            Vec2::new(0.3, 5.0),
            Vec2::new(0.4, -1.0),
            Vec2::new(1.2, -2.0)
        ]
    );
    let gap: VecDeque<(f64, f64)> = [(0.0, 1.0), (1.0, f64::NAN), (2.0, 1.0), (3.0, 2.0)].into();
    let lines = trace(&gap, 0.0, 3.0, px);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[1].len(), 2);
}

/// A line covers the pixels it passes through fully and fades at its
/// edge; a hairline covers its row; the bytes are sRGB, straight alpha.
#[test]
fn the_canvas_draws_smooth_lines() {
    let mut c = Canvas::new(20, 10);
    let white = [1.0; 4];
    c.polyline(&[Vec2::new(2.0, 5.0), Vec2::new(18.0, 5.0)], 2.0, white);
    let alpha = |c: &Canvas, x: usize, y: usize| c.rgba8()[(y * c.w + x) * 4 + 3];
    assert_eq!(alpha(&c, 10, 4), 255);
    assert_eq!(alpha(&c, 10, 5), 255);
    assert_eq!(alpha(&c, 10, 7), 0);
    let edge = alpha(&c, 1, 4);
    assert!(edge > 0 && edge < 255, "{edge}");
    // A polyline blends once: its joint is no darker than its run.
    let mut d = Canvas::new(20, 10);
    let half = [1.0, 1.0, 1.0, 0.5];
    d.polyline(
        &[
            Vec2::new(2.0, 5.0),
            Vec2::new(10.0, 5.0),
            Vec2::new(18.0, 5.0),
        ],
        2.0,
        half,
    );
    assert_eq!(alpha(&d, 10, 4), alpha(&d, 6, 4));
    d.polyline(
        &[Vec2::new(-50.0, -50.0), Vec2::new(-40.0, -40.0)],
        2.0,
        half,
    );
    d.polyline(&[Vec2::new(3.0, 1.0)], 2.0, half);
    assert!(alpha(&d, 3, 1) > 0, "a single point is a dot");

    let mut h = Canvas::new(4, 4);
    h.hline(1.5, 1.0, [0.5, 0.0, 0.0, 1.0]);
    let bytes = h.rgba8();
    assert_eq!(&bytes[16..20], &[188, 0, 0, 255], "row 1, sRGB");
    assert_eq!(&bytes[..4], &[0, 0, 0, 0]);
    let mut h = Canvas::new(4, 4);
    h.hline(1.0, 1.0, [1.0; 4]);
    let a = |c: &Canvas, y: usize| c.rgba8()[y * 16 + 3];
    assert_eq!((a(&h, 0), a(&h, 1)), (128, 128), "half a pixel each");
}

// The panel through the shell.

const ODOM: &str = "/model/v/odometry";

fn topics(app: &mut App, list: &[(&str, &str)]) {
    let pairs = list
        .iter()
        .map(|(t, m)| (t.to_string(), m.to_string()))
        .collect();
    app.world_mut().resource_mut::<GzTopics>().set(pairs);
    frames(app, 2);
}

fn open(app: &mut App) {
    app.world_mut().write_message(UiCommand::OpenPanel {
        panel: PLOT,
        slot: Slot::Primary,
    });
    frames(app, 2);
}

fn live_plot() -> App {
    let mut app = shell_app((1440, 900), 1.0);
    // Frames 50 ms apart, so the throttled redraws come.
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        50,
    )));
    frames(&mut app, 2);
    open(&mut app);
    go_live(&mut app, false);
    topics(
        &mut app,
        &[
            (ODOM, "gz.msgs.Odometry"),
            ("/stats", "gz.msgs.WorldStatistics"),
        ],
    );
    sent(&mut app);
    app
}

fn view(app: &mut App) -> PanelView {
    let world = app.world_mut();
    world
        .query::<(&BodyView, &crate::panels::PanelBody)>()
        .iter(world)
        .find(|(_, b)| b.0 == PLOT)
        .unwrap()
        .0
         .0
        .clone()
}

/// The panel's action buttons: (action, disabled).
fn actions(app: &mut App) -> Vec<(ActionId, bool)> {
    let world = app.world_mut();
    let mut a: Vec<_> = world
        .query::<(&ActionButton, Has<InteractionDisabled>)>()
        .iter(world)
        .filter(|(b, _)| b.panel == PLOT)
        .map(|(b, d)| (b.action, d))
        .collect();
    a.sort_by_key(|a| a.0);
    a.dedup();
    a
}

fn act(app: &mut App, action: ActionId) {
    let entity = app.world_mut().spawn_empty().id();
    app.world_mut().trigger(PanelActionActivated {
        entity,
        panel: PLOT,
        action,
    });
    frames(app, 2);
}

fn odometry(x: f64, stamp: Option<f64>) -> GzPublish {
    let header = stamp.map(|t| msgs::Header {
        stamp: Some(msgs::Time {
            sec: t as i64,
            nsec: ((t.fract()) * 1e9) as i32,
        }),
        ..Default::default()
    });
    GzPublish {
        topic: ODOM.into(),
        msg_type: "gz.msgs.Odometry".into(),
        payload: msgs::Odometry {
            header,
            twist: Some(msgs::Twist {
                linear: Some(msgs::Vector3d {
                    x,
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
        .encode_to_vec(),
    }
}

fn publish(app: &mut App, p: GzPublish) {
    app.world_mut().write_message(p);
    frames(app, 2);
}

fn pick_rows(app: &mut App) -> Vec<PickRow> {
    let world = app.world_mut();
    world.query::<&PickRow>().iter(world).cloned().collect()
}

fn pick(app: &mut App, row: &PickRow) {
    let e = {
        let world = app.world_mut();
        world
            .query::<(Entity, &PickRow)>()
            .iter(world)
            .find(|(_, r)| *r == row)
            .unwrap()
            .0
    };
    app.world_mut().trigger(Activate { entity: e });
    frames(app, 2);
}

fn data(app: &App) -> &PlotData {
    app.world().resource::<PlotData>()
}

fn picker_status(app: &mut App) -> Option<String> {
    let world = app.world_mut();
    let (text, node) = world
        .query_filtered::<(&Text, &Node), With<PickerStatus>>()
        .single(world)
        .ok()?;
    (node.display != Display::None).then(|| text.0.clone())
}

/// Empty until a chart, with Add field… while live; the header's +, the
/// footer's Add field… and Pause wait for the world or a chart; Export
/// for a sample; Pop out for M7e.
#[test]
fn the_plot_panel_starts_empty() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    open(&mut app);
    let PanelView::Empty {
        message,
        actions: a,
    } = view(&mut app)
    else {
        panic!("{:?}", view(&mut app));
    };
    assert_eq!(message, EMPTY);
    assert!(!a[0].enabled);
    let mut want = vec![
        (ADD, true),
        (ADD_FIELD, true),
        (PAUSE, true),
        (EXPORT, true),
        (POP_OUT, true),
    ];
    want.sort_by_key(|a| a.0);
    assert_eq!(actions(&mut app), want);
    go_live(&mut app, false);
    let PanelView::Empty { actions: a, .. } = view(&mut app) else {
        panic!();
    };
    assert!(a[0].enabled);
    let en = actions(&mut app);
    assert!(
        en.contains(&(ADD, false)) && en.contains(&(ADD_FIELD, false)),
        "{en:?}"
    );
    assert!(
        en.contains(&(PAUSE, true)) && en.contains(&(EXPORT, true)),
        "{en:?}"
    );
}

/// Add field… opens the picker on the topics; a topic lists its numbers
/// once a message comes (held meanwhile); a field charts it and the next
/// joins that chart; Back lists the topics again, Close closes it.
#[test]
fn the_picker_adds_a_topics_field() {
    let mut app = live_plot();
    act(&mut app, ADD_FIELD);
    assert_eq!(view(&mut app), PanelView::Ready);
    assert_eq!(
        pick_rows(&mut app),
        [PickRow::Topic(ODOM.into()), PickRow::Topic("/stats".into())]
    );
    pick(&mut app, &PickRow::Topic(ODOM.into()));
    assert_eq!(
        picker_status(&mut app).as_deref(),
        Some("Waiting for first message…")
    );
    let subs = sent(&mut app);
    assert!(
        subs.contains(&GzCommand::Subscribe(ODOM.into())),
        "{subs:?}"
    );
    publish(&mut app, odometry(0.352, None));
    frames(&mut app, 12);
    let rows = pick_rows(&mut app);
    assert!(
        rows.contains(&PickRow::Field(path("twist.linear.x"))),
        "{rows:?}"
    );
    assert_eq!(picker_status(&mut app), None);

    pick(&mut app, &PickRow::Field(path("twist.linear.x")));
    pick(&mut app, &PickRow::Field(path("twist.angular.z")));
    assert_eq!(data(&app).charts.len(), 1);
    assert_eq!(data(&app).charts[0].series.len(), 2);
    assert_eq!(app.world().resource::<PlotCharts>().0, 1);
    let status = app.world().resource::<PanelStatus>();
    assert_eq!(status.count(PLOT), Some("1 chart"));
    assert_eq!(status.badge(PLOT), Some((BadgeKind::Count, 1)));

    let back = find_button(&mut app, PickerButton::Back);
    app.world_mut().trigger(Activate { entity: back });
    frames(&mut app, 2);
    assert_eq!(pick_rows(&mut app).len(), 2);
    let close = find_button(&mut app, PickerButton::Close);
    app.world_mut().trigger(Activate { entity: close });
    frames(&mut app, 2);
    assert!(app.world().resource::<PlotState>().picker.is_none());
    assert!(pick_rows(&mut app).is_empty());
}

fn find_button(app: &mut App, which: PickerButton) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &PickerButton)>()
        .iter(world)
        .find(|(_, b)| **b == which)
        .unwrap()
        .0
}

/// A plotted app: one chart of the odometry's forward speed.
fn plotted() -> App {
    let mut app = live_plot();
    app.world_mut().resource_mut::<PlotData>().add(
        ODOM,
        "gz.msgs.Odometry",
        path("twist.linear.x"),
        Target::New,
    );
    frames(&mut app, 2);
    app
}

/// A series holds its topic at the plot rate (closed too), records each
/// message at its stamp, and its legend shows the newest value; the
/// chart draws at its size with y labels.
#[test]
fn a_series_records_and_draws() {
    let mut app = plotted();
    let subs = sent(&mut app);
    assert!(
        subs.contains(&GzCommand::Subscribe(ODOM.into())),
        "{subs:?}"
    );
    assert!(
        subs.contains(&GzCommand::Throttle(ODOM.into(), PLOT_RATE)),
        "{subs:?}"
    );
    publish(&mut app, odometry(0.1, Some(134.0)));
    publish(&mut app, odometry(0.4, Some(134.25)));
    frames(&mut app, 10);
    let s = &data(&app).charts[0].series[0];
    assert_eq!(s.samples, [(134.0, 0.1), (134.25, 0.4)]);
    assert!(all_text(&mut app).contains("0.400"));
    assert!(all_text(&mut app).contains("odometry.twist.linear.x"));

    let world = app.world_mut();
    let (area, frame) = world
        .query::<(&PlotArea, &ChartFrame)>()
        .single(world)
        .unwrap();
    let handle = area.image.clone();
    assert!(frame.size.x > 100 && frame.size.y > 100, "{:?}", frame.size);
    let size = frame.size;
    let image = world.resource::<Assets<Image>>().get(&handle).unwrap();
    assert_eq!(image.size(), size);
    let labels: Vec<String> = {
        let axis = world
            .query::<(&YAxis, &Children)>()
            .single(world)
            .unwrap()
            .1
            .to_vec();
        axis.iter()
            .filter_map(|c| world.get::<Text>(*c).map(|t| t.0.clone()))
            .collect()
    };
    assert_eq!(labels, ["0.1", "0.2", "0.3", "0.4"]);
    assert!(all_text(&mut app).contains("-10 s"));

    // Closed, it keeps recording.
    app.world_mut().write_message(UiCommand::ToggleSidebar);
    frames(&mut app, 2);
    publish(&mut app, odometry(0.5, Some(134.4)));
    assert_eq!(data(&app).charts[0].series[0].samples.len(), 3);
    assert!(!sent(&mut app).contains(&GzCommand::Unsubscribe(ODOM.into())));
}

/// The crosshair reads each series at the pointer's time.
#[test]
fn the_crosshair_reads_the_values() {
    let mut app = plotted();
    publish(&mut app, odometry(0.25, Some(134.0)));
    frames(&mut app, 10);
    let area = find::<PlotArea>(&mut app);
    app.world_mut().get_mut::<ChartHover>(area).unwrap().x = Some(0.999);
    frames(&mut app, 2);
    let tip = find::<HoverTip>(&mut app);
    assert!(displayed(&app, tip));
    let text = all_text(&mut app);
    assert!(text.contains("0.250"), "{text}");
    app.world_mut().get_mut::<ChartHover>(area).unwrap().x = None;
    frames(&mut app, 2);
    assert!(!displayed(&app, tip));
}

/// A topic leaving the list marks its series with ⚠ in `danger`; the
/// remove button removes it and its chart.
#[test]
fn a_gone_field_warns_and_a_series_can_go() {
    let mut app = plotted();
    topics(&mut app, &[("/stats", "gz.msgs.WorldStatistics")]);
    frames(&mut app, 10);
    assert!(data(&app).charts[0].series[0].missing);
    let world = app.world_mut();
    let warn = world
        .query::<(&LegendPart, &Node)>()
        .iter(world)
        .find(|(p, _)| matches!(p, LegendPart::Warn(_)))
        .map(|(_, n)| n.display);
    assert_eq!(warn, Some(Display::Flex));
    let ink = world
        .query::<(&LegendPart, &crate::theme::Ink)>()
        .iter(world)
        .find(|(p, _)| matches!(p, LegendPart::Label(_)))
        .map(|(_, i)| i.0);
    assert_eq!(ink, Some(crate::theme::ColorRole::Danger));

    let remove = find::<LegendRemove>(&mut app);
    app.world_mut().trigger(Activate { entity: remove });
    frames(&mut app, 2);
    assert!(data(&app).charts.is_empty());
    assert!(matches!(view(&mut app), PanelView::Empty { .. }));
    assert!(sent(&mut app).contains(&GzCommand::Unsubscribe(ODOM.into())));
}

/// Pause freezes the window and shows Resume; the chips pick the window.
#[test]
fn pause_and_window() {
    let mut app = plotted();
    act(&mut app, PAUSE);
    let paused = app.world().resource::<PlotState>().paused_at;
    assert!(paused.is_some());
    let world = app.world_mut();
    let (tip, icon) = world
        .query::<(&ActionButton, &Tooltip, &Children)>()
        .iter(world)
        .find(|(b, ..)| b.action == PAUSE)
        .map(|(_, t, c)| (t.title.clone(), c[0]))
        .unwrap();
    assert_eq!(tip, "Resume charts");
    assert_eq!(
        world.get::<Icon>(icon).unwrap().id,
        crate::assets::icons::IconId::Play
    );
    act(&mut app, PAUSE);
    assert_eq!(app.world().resource::<PlotState>().paused_at, None);

    let chips: Vec<(Entity, TimeWindow)> = {
        let world = app.world_mut();
        world
            .query::<(Entity, &WindowChip)>()
            .iter(world)
            .map(|(e, c)| (e, c.0))
            .collect()
    };
    let group = app.world().get::<ChildOf>(chips[0].0).unwrap().parent();
    let (all, _) = chips.iter().find(|(_, w)| *w == TimeWindow::All).unwrap();
    app.world_mut().trigger(ValueChange {
        source: group,
        value: *all,
        is_final: true,
    });
    frames(&mut app, 2);
    assert_eq!(app.world().resource::<PlotState>().window, TimeWindow::All);
}

/// Export CSV saves every sample and says where.
#[test]
fn export_saves_a_csv() {
    let mut app = plotted();
    publish(&mut app, odometry(0.1, Some(134.0)));
    assert!(actions(&mut app).contains(&(EXPORT, false)));
    act(&mut app, EXPORT);
    let notice = app
        .world()
        .resource::<PlotState>()
        .notice
        .clone()
        .unwrap()
        .0;
    let file = notice.strip_prefix("Saved ").expect(&notice).to_string();
    let csv = std::fs::read_to_string(&file).unwrap();
    std::fs::remove_file(&file).unwrap();
    assert!(
        csv.contains("/model/v/odometry,twist.linear.x,134,0.1"),
        "{csv}"
    );
    let n = find::<PlotNotice>(&mut app);
    assert!(displayed(&app, n));
}

/// Topics' Plot field opens the picker on its topic in the second slot.
#[test]
fn a_pick_opens_the_picker_in_the_second_slot() {
    let mut app = live_plot();
    app.world_mut().write_message(UiCommand::OpenPanel {
        panel: crate::panels::TOPICS,
        slot: Slot::Primary,
    });
    frames(&mut app, 2);
    app.world_mut().write_message(PlotPick(ODOM.into()));
    frames(&mut app, 3);
    let layout = app.world().resource::<ShellLayout>();
    assert_eq!(
        (layout.primary, layout.secondary),
        (Some(crate::panels::TOPICS), Some(PLOT))
    );
    let picker = app.world().resource::<PlotState>().picker.clone().unwrap();
    assert_eq!(picker.topic.as_deref(), Some(ODOM));
}

/// A start charts its fields once the server lists the topic.
#[test]
fn a_start_charts_its_fields() {
    let mut app = shell_app((1440, 900), 1.0);
    app.insert_resource(PlotStart {
        charts: vec![(
            ODOM.into(),
            vec![
                "twist.linear.x".into(),
                "bad..path".into(),
                "pose.position.x".into(),
            ],
        )],
        pick: Some(ODOM.into()),
    });
    frames(&mut app, 2);
    assert_eq!(app.world().resource::<ShellLayout>().primary, Some(PLOT));
    go_live(&mut app, false);
    assert!(data(&app).charts.is_empty());
    topics(&mut app, &[(ODOM, "gz.msgs.Odometry")]);
    assert_eq!(data(&app).charts.len(), 1);
    assert_eq!(data(&app).charts[0].series.len(), 2);
    assert!(!app.world().contains_resource::<PlotStart>());
    let picker = app.world().resource::<PlotState>().picker.clone();
    assert_eq!(picker.and_then(|p| p.topic).as_deref(), Some(ODOM));
}

/// The world's clock going back (a reset) drops the samples.
#[test]
fn a_reset_drops_the_samples() {
    let mut app = plotted();
    publish(&mut app, odometry(0.1, None));
    assert!(data(&app).has_samples());
    let mut s = stats(false);
    s.sim_time = 1.0;
    app.world_mut().resource_mut::<GzStatus>().sim = Some(s);
    frames(&mut app, 2);
    assert!(!data(&app).has_samples());
}
