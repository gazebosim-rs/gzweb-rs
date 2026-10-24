//! The Plot panel's data and rules, as plain data and functions: charts
//! and their series, the time window, the simulation clock, axis ticks,
//! value labels and CSV (DECISIONS.md D-100).

use std::collections::VecDeque;
use std::fmt::Write as _;

use bevy::prelude::*;
use gz_bevy::connection::SimStats;
use gz_core::fields::FieldPath;

use crate::theme::ColorRole;

/// The series colours, in their fixed order (§3.3): a chart holds one
/// series of each.
pub(crate) const COLORS: [ColorRole; 4] = [
    ColorRole::Chart1,
    ColorRole::Chart2,
    ColorRole::Chart3,
    ColorRole::Chart4,
];

/// How many series a chart holds: one per colour.
pub(crate) const MAX_SERIES: usize = COLORS.len();

/// How many samples a series keeps (about 11 minutes at
/// [`PLOT_RATE`]); "All" shows these.
pub(crate) const MAX_SAMPLES: usize = 20_000;

/// The rate a series asks its topic for (the server throttles to it).
pub(crate) const PLOT_RATE: u32 = 30;

/// How often a chart redraws at most.
pub(crate) const CHART_HZ: f64 = 30.0;

/// How far the reported sim time may go back before the samples are
/// dropped (a reset).
const RESET_SECS: f64 = 0.5;

/// How far ahead of the last stats the clock runs while they don't come
/// (they come five times a second).
const MAX_AHEAD_SECS: f64 = 0.5;

/// A stamp further than this from the clock isn't sim time (a publisher
/// that stamps wall time); the clock's time is used.
const STAMP_SLACK_SECS: f64 = 10.0;

/// The time window chips (§3.3), one at a time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum TimeWindow {
    #[default]
    Secs10,
    Secs30,
    Mins2,
    /// Every sample kept.
    All,
}

impl TimeWindow {
    pub(crate) const ALL: [TimeWindow; 4] = [
        TimeWindow::Secs10,
        TimeWindow::Secs30,
        TimeWindow::Mins2,
        TimeWindow::All,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            TimeWindow::Secs10 => "10 s",
            TimeWindow::Secs30 => "30 s",
            TimeWindow::Mins2 => "2 min",
            TimeWindow::All => "All",
        }
    }

    /// Its span in seconds; `None` for All.
    pub(crate) fn secs(self) -> Option<f64> {
        match self {
            TimeWindow::Secs10 => Some(10.0),
            TimeWindow::Secs30 => Some(30.0),
            TimeWindow::Mins2 => Some(120.0),
            TimeWindow::All => None,
        }
    }
}

/// One field of one topic, over time.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Series {
    pub id: u32,
    pub topic: String,
    pub path: FieldPath,
    /// Its colour's index in [`COLORS`], kept while others are removed.
    pub color: usize,
    /// (sim seconds, value), oldest first, at most [`MAX_SAMPLES`].
    pub samples: VecDeque<(f64, f64)>,
    /// The field no longer exists: the topic left the server's list, or
    /// its message has no such field (§3.3.1).
    pub missing: bool,
}

impl Series {
    /// Its legend label: the topic's last name and the field
    /// ("odometry.twist.linear.x").
    pub(crate) fn label(&self) -> String {
        let short = self.topic.rsplit('/').find(|s| !s.is_empty());
        format!("{}.{}", short.unwrap_or(&self.topic), self.path)
    }

    pub(crate) fn last(&self) -> Option<f64> {
        self.samples.back().map(|s| s.1)
    }

    /// The value at time `t`: the last sample at or before it.
    pub(crate) fn value_at(&self, t: f64) -> Option<f64> {
        let n = self.samples.partition_point(|s| s.0 <= t);
        n.checked_sub(1).map(|i| self.samples[i].1)
    }

    fn push(&mut self, t: f64, v: f64) {
        // A sample older than the last (a late stamp) would fold the line
        // back on itself; it goes in at the last time instead.
        let t = self.samples.back().map_or(t, |last| t.max(last.0));
        if self.samples.len() == MAX_SAMPLES {
            self.samples.pop_front();
        }
        self.samples.push_back((t, v));
    }
}

/// A chart: up to [`MAX_SERIES`] series on one y axis.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Chart {
    pub id: u32,
    pub title: String,
    /// Its first series' unit, if known ("m/s").
    pub unit: &'static str,
    pub series: Vec<Series>,
}

impl Chart {
    pub(crate) fn is_full(&self) -> bool {
        self.series.len() >= MAX_SERIES
    }

    /// The first colour no series of it has.
    fn free_color(&self) -> usize {
        (0..MAX_SERIES)
            .find(|c| self.series.iter().all(|s| s.color != *c))
            .unwrap_or(0)
    }

    /// The time span its samples cover, if it has any.
    pub(crate) fn span(&self) -> Option<(f64, f64)> {
        let first = self.series.iter().filter_map(|s| s.samples.front());
        let last = self.series.iter().filter_map(|s| s.samples.back());
        let t0 = first.map(|s| s.0).reduce(f64::min)?;
        let t1 = last.map(|s| s.0).reduce(f64::max)?;
        Some((t0, t1))
    }

    /// The range of its finite values between `t0` and `t1` (and the
    /// sample before `t0`, whose line enters the window).
    pub(crate) fn range(&self, t0: f64, t1: f64) -> Option<(f64, f64)> {
        let mut out: Option<(f64, f64)> = None;
        for s in &self.series {
            let start = s.samples.partition_point(|p| p.0 < t0).saturating_sub(1);
            for &(t, v) in s.samples.range(start..) {
                if t > t1 {
                    break;
                }
                if v.is_finite() {
                    out = Some(out.map_or((v, v), |(lo, hi)| (lo.min(v), hi.max(v))));
                }
            }
        }
        out
    }
}

/// Where an added field goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Target {
    /// A chart of its own.
    #[default]
    New,
    /// The chart with this id.
    Chart(u32),
}

/// The charts and their samples: always on, so the series record while
/// the panel is closed (PR #30 second review, item 6).
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct PlotData {
    pub charts: Vec<Chart>,
    next: u32,
}

impl PlotData {
    fn next_id(&mut self) -> u32 {
        self.next += 1;
        self.next
    }

    /// Adds `topic`'s field `path` (of type `msg_type`) to `target`; a
    /// full or missing chart takes a new one. A field the target already
    /// shows isn't added again. Returns the chart's id.
    pub(crate) fn add(
        &mut self,
        topic: &str,
        msg_type: &str,
        path: FieldPath,
        target: Target,
    ) -> u32 {
        let has = |c: &Chart| c.series.iter().any(|s| s.topic == topic && s.path == path);
        if let Target::Chart(id) = target {
            if let Some(c) = self.charts.iter().find(|c| c.id == id && has(c)) {
                return c.id;
            }
        }
        let at = match target {
            Target::Chart(id) => self.charts.iter().position(|c| c.id == id && !c.is_full()),
            Target::New => None,
        };
        let at = match at {
            Some(i) => i,
            None => {
                let id = self.next_id();
                self.charts.push(Chart {
                    id,
                    title: path.to_string(),
                    unit: unit(msg_type, &path),
                    series: vec![],
                });
                self.charts.len() - 1
            }
        };
        let id = self.next_id();
        let chart = &mut self.charts[at];
        let color = chart.free_color();
        chart.series.push(Series {
            id,
            topic: topic.to_string(),
            path,
            color,
            samples: VecDeque::new(),
            missing: false,
        });
        chart.id
    }

    /// Removes series `id`, and its chart once it has none.
    pub(crate) fn remove(&mut self, id: u32) {
        for chart in &mut self.charts {
            chart.series.retain(|s| s.id != id);
        }
        self.charts.retain(|c| !c.series.is_empty());
    }

    /// The topics the series read, each once.
    pub(crate) fn topics(&self) -> Vec<&str> {
        let mut out: Vec<&str> = vec![];
        for s in self.series() {
            if !out.contains(&s.topic.as_str()) {
                out.push(&s.topic);
            }
        }
        out
    }

    pub(crate) fn series(&self) -> impl Iterator<Item = &Series> {
        self.charts.iter().flat_map(|c| &c.series)
    }

    fn series_mut(&mut self) -> impl Iterator<Item = &mut Series> {
        self.charts.iter_mut().flat_map(|c| &mut c.series)
    }

    /// Records `topic`'s message at time `t`: each series of it takes its
    /// field's value, or goes missing when the message has none.
    pub(crate) fn record(
        &mut self,
        topic: &str,
        t: f64,
        mut value: impl FnMut(&FieldPath) -> Option<f64>,
    ) {
        for s in self.series_mut().filter(|s| s.topic == topic) {
            match value(&s.path) {
                Some(v) => {
                    s.push(t, v);
                    s.missing = false;
                }
                None => s.missing = true,
            }
        }
    }

    /// Marks the series of topics no longer listed (`listed` says) as
    /// missing, and those listed again as not.
    pub(crate) fn mark_listed(&mut self, listed: impl Fn(&str) -> bool) {
        for s in self.series_mut() {
            let gone = !listed(&s.topic);
            if gone {
                s.missing = true;
            } else if s.missing && s.samples.is_empty() {
                // Listed again; its next message says whether the field is
                // there.
                s.missing = false;
            }
        }
    }

    /// Drops every sample (the world was reset).
    pub(crate) fn clear_samples(&mut self) {
        for s in self.series_mut() {
            s.samples.clear();
        }
    }

    pub(crate) fn has_samples(&self) -> bool {
        self.series().any(|s| !s.samples.is_empty())
    }

    /// Every sample as CSV: one row a sample, by chart and series.
    pub(crate) fn csv(&self) -> String {
        let mut out = String::from("chart,series,topic,field,sim_time,value\n");
        for chart in &self.charts {
            for s in &chart.series {
                for (t, v) in &s.samples {
                    let _ = writeln!(
                        out,
                        "{},{},{},{},{t},{v}",
                        csv_cell(&chart.title),
                        csv_cell(&s.label()),
                        csv_cell(&s.topic),
                        csv_cell(&s.path.to_string()),
                    );
                }
            }
        }
        out
    }
}

/// A CSV cell: quoted when it holds a comma, a quote or a line break.
fn csv_cell(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// A field's unit, where its message type says it (SI, as Gazebo
/// publishes); "" otherwise.
pub(crate) fn unit(msg_type: &str, path: &FieldPath) -> &'static str {
    let short = msg_type.rsplit('.').next().unwrap_or_default();
    let path = path.to_string();
    let under = |prefix: &str| path.starts_with(prefix);
    match short {
        "Twist" if under("linear.") => "m/s",
        "Twist" if under("angular.") => "rad/s",
        "Odometry" if under("twist.linear.") => "m/s",
        "Odometry" if under("twist.angular.") => "rad/s",
        "Odometry" if under("pose.position.") => "m",
        "Pose" if under("position.") => "m",
        "IMU" if under("linear_acceleration.") => "m/s²",
        "IMU" if under("angular_velocity.") => "rad/s",
        "LaserScan" if under("ranges[") => "m",
        "Altimeter" if under("vertical_position") => "m",
        "Altimeter" if under("vertical_velocity") => "m/s",
        _ => "",
    }
}

/// The simulation's clock between stats: the last sim time reported, run
/// on at the real-time factor while the world runs (at most
/// [`MAX_AHEAD_SECS`] past it), frozen while paused.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct SimClock {
    /// The last stats' sim time, and the real time they came.
    at: Option<(f64, f64)>,
    rtf: f64,
    paused: bool,
    /// How many times the reported sim time went back (a reset).
    pub resets: u32,
}

impl SimClock {
    /// New stats came at real time `real`; a sim time more than
    /// [`RESET_SECS`] before the last reported one counts a reset (the
    /// reported times, not the run-on clock, so a stalled server isn't
    /// one).
    pub(crate) fn update(&mut self, stats: &SimStats, real: f64) {
        if self
            .at
            .is_some_and(|(last, _)| stats.sim_time < last - RESET_SECS)
        {
            self.resets += 1;
        }
        self.at = Some((stats.sim_time, real));
        self.rtf = stats.real_time_factor.clamp(0.0, 100.0);
        self.paused = stats.paused;
    }

    /// The sim time at real time `real`, once stats have come.
    pub(crate) fn now(&self, real: f64) -> Option<f64> {
        let (sim, at) = self.at?;
        if self.paused {
            return Some(sim);
        }
        let ahead = ((real - at) * self.rtf).clamp(0.0, MAX_AHEAD_SECS);
        Some(sim + ahead)
    }

    /// A message's time: its stamp when it is sim time (near the clock),
    /// else the clock's; the stamp alone before stats come.
    pub(crate) fn time_of(&self, stamp: Option<f64>, real: f64) -> Option<f64> {
        match (stamp, self.now(real)) {
            (Some(s), Some(now)) if (s - now).abs() <= STAMP_SLACK_SECS => Some(s),
            (_, Some(now)) => Some(now),
            (stamp, None) => stamp,
        }
    }
}

/// What the user set; the body is built from it.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct PlotState {
    pub window: TimeWindow,
    /// Paused at this sim time: the charts stop moving (recording goes
    /// on).
    pub paused_at: Option<f64>,
    /// The field picker, while open.
    pub picker: Option<Picker>,
    /// What Export CSV said, and the real time it said it.
    pub notice: Option<(String, f64)>,
}

/// The field picker (Add field…): a topic, then one of its numbers.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Picker {
    /// The topic whose fields it lists; `None` lists the topics.
    pub topic: Option<String>,
    /// The filter text, over topics or fields.
    pub filter: String,
    pub target: Target,
    /// The topic's newest numbers, once a message has come.
    pub numbers: Option<Vec<(FieldPath, f64)>>,
    /// The topic's message couldn't be read: why.
    pub error: Option<String>,
}

impl Picker {
    /// Open on `topic`'s fields, or on the topics.
    pub(crate) fn on(topic: Option<String>, target: Target) -> Self {
        Self {
            topic,
            target,
            ..default()
        }
    }

    /// Whether `text` passes the filter (containing it, ignoring case).
    pub(crate) fn passes(&self, text: &str) -> bool {
        let f = self.filter.trim().to_lowercase();
        f.is_empty() || text.to_lowercase().contains(&f)
    }
}

/// A range narrower than this (times its magnitude, at least 1) is flat:
/// a solver's noise around a constant, not a signal.
const FLAT: f64 = 1e-9;

/// About `target` ticks covering `lo..hi`: the step a 1, 2 or 5 times a
/// power of ten, and the ticks from the step at or below `lo` to the one
/// at or above `hi`. A flat range is widened around its value.
pub(crate) fn ticks(lo: f64, hi: f64, target: usize) -> (f64, Vec<f64>) {
    let (lo, hi) = if hi - lo > FLAT * hi.abs().max(lo.abs()).max(1.0) {
        (lo, hi)
    } else {
        let mid = (lo + hi) / 2.0;
        let mid = if mid.abs() < FLAT { 0.0 } else { mid };
        let pad = if mid == 0.0 { 1.0 } else { mid.abs() * 0.1 };
        (mid - pad, mid + pad)
    };
    #[allow(clippy::cast_precision_loss)]
    let raw = (hi - lo) / target.max(1) as f64;
    let mag = 10f64.powf(raw.log10().floor());
    let step = [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|m| m * mag)
        .find(|s| *s >= raw * 0.999)
        .unwrap_or(10.0 * mag);
    let first = (lo / step).floor();
    let last = (hi / step).ceil();
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let n = (last - first) as usize;
    #[allow(clippy::cast_precision_loss)]
    let out = (0..=n).map(|i| (first + i as f64) * step).collect();
    (step, out)
}

/// A tick's label at `step`: as many decimals as the step needs ("0.2",
/// "1500", "0.325001"; e-notation below 1e-4), and zero as "0".
pub(crate) fn tick_label(v: f64, step: f64) -> String {
    if v.abs() < step * 1e-6 {
        return "0".into();
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let decimals = (-step.log10().floor()).clamp(0.0, 12.0) as usize;
    // Tiny values read in e-notation, with the digits the step needs.
    if v.abs() < 1e-4 {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let digits = (v.abs().log10().floor() - step.log10().floor()).clamp(0.0, 6.0) as usize;
        return format!("{v:.digits$e}");
    }
    format!("{v:.decimals$}")
}

/// A live value's label: three significant digits ("0.352", "3.52",
/// "471"), whole numbers from 1000 (and any whole number, a counter's or
/// a stamp's), and e-notation for other values from a million.
pub(crate) fn value_label(v: f64) -> String {
    if !v.is_finite() {
        return if v.is_nan() {
            "NaN".into()
        } else if v > 0.0 {
            "∞".into()
        } else {
            "-∞".into()
        };
    }
    // Below this, a value is a solver's rounding, not a reading.
    let v = if v.abs() < 1e-12 { 0.0 } else { v };
    let mag = v.abs();
    if v.fract() == 0.0 && (1.0..1e15).contains(&mag) {
        return format!("{v:.0}");
    }
    if mag >= 1e6 || (mag > 0.0 && mag < 1e-4) {
        return format!("{v:.2e}");
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let decimals = if mag == 0.0 {
        2
    } else {
        (2.0 - mag.log10().floor()).clamp(0.0, 6.0) as usize
    };
    format!("{v:.decimals$}")
}

/// The x axis's left label: how far back the window starts ("-10 s",
/// "-2 min").
pub(crate) fn span_label(secs: f64) -> String {
    let short = |v: f64| {
        if (v - v.round()).abs() < 0.05 {
            format!("{v:.0}")
        } else {
            format!("{v:.1}")
        }
    };
    if secs >= 120.0 {
        format!("-{} min", short(secs / 60.0))
    } else {
        format!("-{} s", short(secs))
    }
}
