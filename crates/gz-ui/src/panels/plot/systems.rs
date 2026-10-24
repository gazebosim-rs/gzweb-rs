//! The Plot panel's systems: the clock, recording and subscriptions
//! (always on), its status in [`PanelStatus`], and, while it shows, the
//! charts, legend, crosshair and picker in place (DECISIONS.md D-100).

use bevy::input_focus::InputFocus;
use bevy::picking::hover::Hovered;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::text::EditableText;
use bevy::ui::{Checked, RelativeCursorPosition};
use bevy::ui_widgets::{Activate, ValueChange};
use gz_bevy::connection::{GzCommand, GzPublish, GzStatus, SimStats};
use gz_bevy::save::save_text;
use gz_bevy::topics::{GzSubscriptions, GzTopics, Holder};
use gz_core::fields::{FieldPath, Message};

use super::model::{
    span_label, tick_label, ticks, value_label, Chart, Picker, PlotData, PlotState, SimClock,
    Target, CHART_HZ, COLORS, PLOT_RATE,
};
use super::raster::{trace, Canvas};
use super::view::{
    pick_row, spawn_chart, spawn_picker, tip_line, y_label, ChartHover, ChartList, Crosshair,
    HoverDot, HoverTip, LegendPart, LegendRemove, LegendRow, PickRow, PickValue, PickerButton,
    PickerFilter, PickerRoot, PickerRows, PickerStatus, PickerTarget, PlotArea, PlotNotice,
    WindowChip, XLabel, XRow, YAxis, AXIS_CHAR_W, GUTTER_W, LINE_W,
};
use super::{PlotCharts, PlotPick, PlotStart, ADD, ADD_FIELD, EXPORT, PAUSE, PLOT};
use crate::panels::{PanelAction, PanelActionActivated, PanelView};
use crate::shell::{
    CurrentPhase, PanelStatus, Phase, SetPanelStatus, ShownPanels, Slot, UiCommand,
};
use crate::theme::{ColorRole, Ink, Theme, SPACE};
use crate::widgets::layout::set_shown;
use crate::widgets::text::set_text;
use crate::widgets::{BadgeKind, SelectChanged};

/// What holds the series' topics ([`GzSubscriptions`]).
pub(crate) const HOLDER: Holder = "plot";
/// What holds the picker's topic, while it shows.
pub(crate) const PICK_HOLDER: Holder = "plot_pick";

/// How often the picker's values and the legend's redraw.
const VALUES_HZ: f64 = 5.0;
/// How long Export CSV's notice shows.
const NOTICE_SECS: f64 = 6.0;
/// How often the topic list is asked for again while the picker lists
/// topics.
const REFRESH_SECS: f64 = 2.0;

/// The empty state's message (§3.3.1).
pub(crate) const EMPTY: &str = "Add a field from Topics or the inspector";

/// The charts the list was built for: each chart's id and title, and its
/// series' ids and colours.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct BuiltCharts(Vec<BuiltChart>);

/// A chart's id and title, and its series' ids and colours.
type BuiltChart = (u32, String, Vec<(u32, usize)>);

/// The picker's head: its topic, the charts it can add to, its target.
type PickerHead = (Option<String>, Vec<(u32, String)>, Target);

/// What the picker was built for: its head (topic, targets, target) and
/// rows.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct BuiltPicker {
    head: Option<PickerHead>,
    rows: Option<Vec<PickRow>>,
}

/// A chart as last drawn: what it was drawn for, and the mapping the
/// crosshair needs.
#[derive(Component, Clone, Debug, Default, PartialEq)]
pub(crate) struct ChartFrame {
    key: Option<DrawKey>,
    pub t0: f64,
    pub t1: f64,
    pub lo: f64,
    pub hi: f64,
    /// The plot's physical size and its scale factor.
    pub size: UVec2,
    pub scale: f32,
    labels: Vec<(String, i32)>,
}

impl ChartFrame {
    /// The vertical padding that keeps a line at the range's edge whole,
    /// in physical pixels.
    fn pad(&self) -> f32 {
        LINE_W * self.scale / 2.0 + 1.0
    }

    /// A value's y, in physical pixels.
    #[allow(clippy::cast_possible_truncation)]
    fn y(&self, v: f64) -> f32 {
        #[allow(clippy::cast_precision_loss)]
        let h = self.size.y as f32 - 2.0 * self.pad();
        self.pad() + ((self.hi - v) / (self.hi - self.lo)) as f32 * h
    }

    /// A time's x, in physical pixels.
    #[allow(clippy::cast_possible_truncation)]
    fn x(&self, t: f64) -> f32 {
        #[allow(clippy::cast_precision_loss)]
        let w = self.size.x as f32;
        ((t - self.t0) / (self.t1 - self.t0)) as f32 * w
    }
}

/// What a chart's pixels depend on: its size, window and theme, and each
/// series' samples up to the window's end (how many, the oldest's time:
/// samples are only added at the end, dropped from the front or cleared).
#[derive(Clone, Debug, PartialEq, Eq)]
struct DrawKey {
    size: UVec2,
    t0: u64,
    t1: u64,
    samples: Vec<(usize, u64)>,
    dark: bool,
}

impl DrawKey {
    fn samples(chart: &Chart, t1: f64) -> Vec<(usize, u64)> {
        chart
            .series
            .iter()
            .map(|s| {
                // With the one after the end, which the line runs to.
                let n = (s.samples.partition_point(|p| p.0 <= t1) + 1).min(s.samples.len());
                (n, s.samples.front().map_or(0, |p| p.0.to_bits()))
            })
            .collect()
    }
}

/// The empty state, with Add field… while live, until there is a chart
/// or the picker opens (§3.3.1).
pub(super) fn view(live: bool, content: bool) -> PanelView {
    if content {
        return PanelView::Ready;
    }
    let add = PanelAction::new(ADD_FIELD, "Add field…");
    PanelView::empty(EMPTY).action(if live {
        add
    } else {
        add.unavailable("Not connected")
    })
}

/// The clock takes each new stats report.
pub(super) fn track_clock(
    time: Res<Time<Real>>,
    status: Option<Res<GzStatus>>,
    mut last: Local<Option<SimStats>>,
    mut clock: ResMut<SimClock>,
) {
    let Some(sim) = status.filter(|s| s.is_changed()).and_then(|s| s.sim) else {
        return;
    };
    if *last != Some(sim) {
        clock.update(&sim, time.elapsed_secs_f64());
        *last = Some(sim);
    }
}

/// A [`PlotStart`]: opens the panel at once (in the second slot when
/// Topics opens too), and charts each entry's fields once the server
/// lists its topic.
pub(super) fn apply_start(
    start: Option<ResMut<PlotStart>>,
    echo: Option<Res<crate::panels::TopicsStart>>,
    topics: Res<GzTopics>,
    mut data: ResMut<PlotData>,
    mut state: ResMut<PlotState>,
    mut ui: MessageWriter<UiCommand>,
    mut commands: Commands,
) {
    let Some(mut start) = start else {
        return;
    };
    if start.is_added() {
        // Under Topics when it opens too (`--echo`).
        let slot = if echo.is_some() {
            Slot::Secondary
        } else {
            Slot::Primary
        };
        ui.write(UiCommand::OpenPanel { panel: PLOT, slot });
    }
    let start = start.bypass_change_detection();
    start.charts.retain(|(topic, fields)| {
        let Some(info) = topics.get(topic) else {
            return true;
        };
        let mut target = Target::New;
        for f in fields {
            match f.parse::<FieldPath>() {
                Ok(path) => {
                    let id = data.add(topic, &info.msg_type, path, target);
                    target = Target::Chart(id);
                }
                Err(e) => warn!("--plot {topic}: {e}"),
            }
        }
        false
    });
    if let Some(topic) = start.pick.take() {
        state.picker = Some(Picker::on(Some(topic), Target::New));
    }
    if start.charts.is_empty() {
        commands.remove_resource::<PlotStart>();
    }
}

/// Topics' Plot field opens the picker on its topic, and the panel in
/// the second slot unless it already shows.
pub(super) fn read_picks(
    mut picks: MessageReader<PlotPick>,
    hosts: Res<ShownPanels>,
    mut state: ResMut<PlotState>,
    mut ui: MessageWriter<UiCommand>,
) {
    for pick in picks.read() {
        state.picker = Some(Picker::on(Some(pick.0.clone()), Target::New));
        if !hosts.is_shown(PLOT) {
            ui.write(UiCommand::OpenPanel {
                panel: PLOT,
                slot: Slot::Secondary,
            });
        }
    }
}

/// Records each message of a plotted topic into its series, at its
/// stamp or the clock's time, and keeps the picked topic's numbers (at
/// most [`VALUES_HZ`] times a second). A reset (the reported sim time
/// went back) drops the samples and lets go of Pause.
#[allow(clippy::too_many_arguments)]
pub(super) fn record(
    time: Res<Time<Real>>,
    clock: Res<SimClock>,
    topics: Res<GzTopics>,
    mut publishes: MessageReader<GzPublish>,
    mut data: ResMut<PlotData>,
    mut state: ResMut<PlotState>,
    mut resets: Local<u32>,
    mut picked: Local<PickedMessage>,
) {
    let real = time.elapsed_secs_f64();
    if *resets != clock.resets {
        *resets = clock.resets;
        if data.has_samples() {
            data.clear_samples();
        }
        if state.paused_at.is_some() {
            state.paused_at = None;
        }
    }
    let plotted: Vec<String> = data.topics().into_iter().map(str::to_string).collect();
    let pick = state.picker.as_ref().and_then(|p| p.topic.clone());
    if picked.topic != pick {
        *picked = PickedMessage {
            topic: pick.clone(),
            ..default()
        };
    }
    for p in publishes.read() {
        let plot = plotted.contains(&p.topic);
        let is_pick = pick.as_ref() == Some(&p.topic);
        if !plot && !is_pick {
            continue;
        }
        let msg_type = match p.msg_type.as_str() {
            "" => topics.get(&p.topic).map_or("", |t| t.msg_type.as_str()),
            t => t,
        };
        if is_pick {
            picked.newest = Some((msg_type.to_string(), p.payload.clone()));
        }
        if !plot {
            continue;
        }
        match Message::decode(msg_type, &p.payload) {
            Ok(m) => {
                let t = clock.time_of(m.stamp(), real).unwrap_or(real);
                data.record(&p.topic, t, |path| m.number(path));
            }
            Err(_) => data.record(&p.topic, real, |_| None),
        }
    }
    if real - picked.read_at < 1.0 / VALUES_HZ {
        return;
    }
    if let Some((msg_type, payload)) = picked.newest.take() {
        if let Some(picker) = state.picker.as_mut() {
            match Message::decode(&msg_type, &payload) {
                Ok(m) => {
                    picker.numbers = Some(m.numbers());
                    picker.error = None;
                }
                Err(e) => picker.error = Some(e.to_string()),
            }
        }
        picked.read_at = real;
    }
}

/// The picked topic's newest message not yet read into the picker, and
/// when it last was.
#[derive(Default)]
pub(super) struct PickedMessage {
    topic: Option<String>,
    newest: Option<(String, Vec<u8>)>,
    read_at: f64,
}

/// A series whose topic leaves the server's list goes missing (§3.3.1).
pub(super) fn mark_listed(topics: Res<GzTopics>, mut data: ResMut<PlotData>) {
    if topics.is_changed() && topics.replies > 0 {
        let listed = |t: &str| topics.get(t).is_some();
        if data.series().any(|s| s.missing == listed(&s.topic)) {
            data.mark_listed(listed);
        }
    }
}

/// The view, the header's count ("2 charts"), the badge, and the actions:
/// adding needs a live world, Export CSV a sample, Pause a chart.
pub(super) fn plot_status(
    phase: Res<CurrentPhase>,
    data: Res<PlotData>,
    mut state: ResMut<PlotState>,
    mut status: ResMut<PanelStatus>,
    mut charts: ResMut<PlotCharts>,
) {
    if !phase.is_changed() && !data.is_changed() && !state.is_changed() {
        return;
    }
    let live = phase.0 == Phase::Live;
    let n = data.charts.len();
    // Nothing to pause: a new chart starts moving.
    if n == 0 && state.paused_at.is_some() {
        state.paused_at = None;
    }
    status.set_view(PLOT, view(live, n > 0 || state.picker.is_some()));
    let count = (n > 0).then(|| format!("{n} chart{}", if n == 1 { "" } else { "s" }));
    status.set_count(PLOT, count);
    #[allow(clippy::cast_possible_truncation)]
    let n32 = n as u32;
    charts.set_if_neq(PlotCharts(n32));
    status.set_badge(PLOT, (n > 0).then_some((BadgeKind::Count, n32)));
    status.set_action_enabled(PLOT, ADD, live);
    status.set_action_enabled(PLOT, ADD_FIELD, live);
    status.set_action_enabled(PLOT, EXPORT, data.has_samples());
    status.set_action_enabled(PLOT, PAUSE, n > 0);
    status.set_action_on(PLOT, PAUSE, state.paused_at.is_some());
}

/// Holds every series' topic (recording goes on while the panel is
/// closed), and the picker's topic while the picker shows on a live
/// world.
pub(super) fn hold_series(
    data: Res<PlotData>,
    state: Res<PlotState>,
    hosts: Res<ShownPanels>,
    phase: Res<CurrentPhase>,
    mut subs: ResMut<GzSubscriptions>,
) {
    let want: Vec<&str> = data.topics();
    sync_holds(&mut subs, HOLDER, &want, Some(PLOT_RATE));
    let live = phase.0 == Phase::Live && hosts.is_shown(PLOT);
    let pick = state.picker.as_ref().and_then(|p| p.topic.as_deref());
    let pick: Vec<&str> = pick.filter(|_| live).into_iter().collect();
    sync_holds(&mut subs, PICK_HOLDER, &pick, Some(PLOT_RATE));
}

/// Makes `holder` hold exactly `want`.
fn sync_holds(
    subs: &mut ResMut<GzSubscriptions>,
    holder: Holder,
    want: &[&str],
    rate: Option<u32>,
) {
    let held: Vec<String> = subs
        .bypass_change_detection()
        .held_by(holder)
        .map(str::to_string)
        .collect();
    if held.len() == want.len() && want.iter().all(|w| held.iter().any(|h| h == w)) {
        return;
    }
    for topic in held.iter().filter(|h| !want.contains(&h.as_str())) {
        subs.release(holder, topic);
    }
    for topic in want.iter().filter(|w| !held.iter().any(|h| h == *w)) {
        subs.hold(holder, topic, rate);
    }
}

/// Asks for the topic list every [`REFRESH_SECS`] while the picker lists
/// topics on a live world.
pub(super) fn refresh_topics(
    time: Res<Time<Real>>,
    phase: Res<CurrentPhase>,
    state: Res<PlotState>,
    hosts: Res<ShownPanels>,
    mut last: Local<Option<f64>>,
    mut out: MessageWriter<GzCommand>,
) {
    let listing = state.picker.as_ref().is_some_and(|p| p.topic.is_none());
    if phase.0 != Phase::Live || !listing || !hosts.is_shown(PLOT) {
        *last = None;
        return;
    }
    let now = time.elapsed_secs_f64();
    let since = *last.get_or_insert(now);
    if now - since >= REFRESH_SECS {
        out.write(GzCommand::RefreshTopics);
        *last = Some(now);
    }
}

/// Rebuilds the charts when they or their series change; each chart gets
/// a new image to draw into.
pub(super) fn sync_charts(
    theme: Res<Theme>,
    data: Res<PlotData>,
    lists: Query<(Entity, Ref<ChartList>)>,
    mut built: ResMut<BuiltCharts>,
    mut images: ResMut<Assets<Image>>,
    mut commands: Commands,
) {
    let Ok((list, added)) = lists.single() else {
        return;
    };
    let key: Vec<_> = data
        .charts
        .iter()
        .map(|c| {
            let series = c.series.iter().map(|s| (s.id, s.color)).collect();
            (c.id, c.title.clone(), series)
        })
        .collect();
    if !added.is_added() && key == built.0 {
        return;
    }
    if added.is_added() {
        // The body's builder spawned them.
        built.0 = key;
        return;
    }
    commands.entity(list).despawn_children();
    let charts = data.charts.clone();
    let handles: Vec<Handle<Image>> = charts
        .iter()
        .map(|_| images.add(super::view::blank()))
        .collect();
    commands.entity(list).with_children(|l| {
        for (chart, image) in charts.iter().zip(handles) {
            spawn_chart(l, &theme, chart, image);
        }
    });
    built.0 = key;
}

/// The legend's values, at most [`VALUES_HZ`] times a second, and each
/// row's look: ⚠ and `danger` while its field no longer exists.
pub(super) fn legend_states(
    time: Res<Time<Real>>,
    data: Res<PlotData>,
    mut parts: Query<(&LegendPart, &mut Node, Option<&mut Text>, Option<&mut Ink>)>,
    added: Query<(), Added<LegendRow>>,
    mut drawn_at: Local<f64>,
    mut pending: Local<bool>,
) {
    let now = time.elapsed_secs_f64();
    *pending |= data.is_changed();
    let values = (*pending && now - *drawn_at >= 1.0 / VALUES_HZ) || !added.is_empty();
    if !data.is_changed() && !values {
        return;
    }
    if values {
        *drawn_at = now;
        *pending = false;
    }
    let series: HashMap<u32, (bool, Option<f64>)> = data
        .series()
        .map(|s| (s.id, (s.missing, s.last())))
        .collect();
    for (part, mut node, text, ink) in &mut parts {
        let (LegendPart::Swatch(id)
        | LegendPart::Warn(id)
        | LegendPart::Label(id)
        | LegendPart::Value(id)) = *part;
        let Some(&(missing, last)) = series.get(&id) else {
            continue;
        };
        match part {
            LegendPart::Swatch(_) => set_shown(&mut node, !missing),
            LegendPart::Warn(_) => set_shown(&mut node, missing),
            LegendPart::Label(_) => {
                if let Some(mut ink) = ink {
                    let want = if missing {
                        ColorRole::Danger
                    } else {
                        ColorRole::Text
                    };
                    if ink.0 != want {
                        ink.0 = want;
                    }
                }
            }
            LegendPart::Value(_) if values => {
                if let Some(mut text) = text {
                    let want = match last {
                        Some(v) if !missing => value_label(v),
                        _ => "—".to_string(),
                    };
                    set_text(&mut text, &want);
                }
            }
            LegendPart::Value(_) => {}
        }
    }
}

/// A legend row's remove button shows, over its value, while the row is
/// hovered or the button has focus.
pub(super) fn legend_hover(
    rows: Query<(&LegendRow, &Hovered)>,
    mut removes: Query<(Entity, &LegendRemove, &mut Visibility)>,
    mut values: Query<(&LegendPart, &mut Visibility), Without<LegendRemove>>,
    focus: Res<InputFocus>,
) {
    let hovered: Vec<u32> = rows
        .iter()
        .filter(|(_, h)| h.get())
        .map(|(r, _)| r.0)
        .collect();
    let mut shown = vec![];
    for (e, remove, mut vis) in &mut removes {
        let on = hovered.contains(&remove.0) || focus.get() == Some(e);
        if on {
            shown.push(remove.0);
        }
        vis.set_if_neq(if on {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
    for (part, mut vis) in &mut values {
        if let LegendPart::Value(id) = *part {
            vis.set_if_neq(if shown.contains(&id) {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            });
        }
    }
}

/// Where the window ends: where it was paused, else the clock, else the
/// newest sample.
fn window_end(state: &PlotState, clock: &SimClock, data: &PlotData, real: f64) -> f64 {
    let newest = || {
        data.series()
            .filter_map(|s| s.samples.back().map(|p| p.0))
            .reduce(f64::max)
    };
    state
        .paused_at
        .or_else(|| clock.now(real))
        .or_else(newest)
        .unwrap_or(0.0)
}

/// A chart's window: the window's span back from `end`, or for All its
/// first sample (at least a second).
fn chart_window(chart: &Chart, state: &PlotState, end: f64) -> (f64, f64) {
    let t0 = match state.window.secs() {
        Some(s) => end - s,
        None => chart.span().map_or(end - 10.0, |(t0, _)| t0).min(end - 1.0),
    };
    (t0, end)
}

/// What [`draw_charts`] reads.
#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct DrawSources<'w> {
    time: Res<'w, Time<Real>>,
    theme: Res<'w, Theme>,
    data: Res<'w, PlotData>,
    state: Res<'w, PlotState>,
    clock: Res<'w, SimClock>,
}

/// Draws each chart at its physical size, at most [`CHART_HZ`] times a
/// second and only when what it shows changed: gridlines in `line_soft`
/// at the y ticks, the baseline (0, or the lowest tick) in `line`, and
/// each series' line; then its axis labels.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_charts(
    src: DrawSources,
    mut areas: Query<(&PlotArea, &ComputedNode, &mut ChartFrame)>,
    axes: Query<(Entity, &YAxis)>,
    mut xrows: Query<(&XRow, &mut Node), Without<PlotArea>>,
    mut xlabels: Query<(&XLabel, &mut Text)>,
    mut images: ResMut<Assets<Image>>,
    mut drawn_at: Local<f64>,
    mut commands: Commands,
) {
    let real = src.time.elapsed_secs_f64();
    if real - *drawn_at < 1.0 / CHART_HZ {
        return;
    }
    *drawn_at = real;
    let end = window_end(&src.state, &src.clock, &src.data, real);
    for (area, computed, mut frame) in &mut areas {
        let Some(chart) = src.data.charts.iter().find(|c| c.id == area.chart) else {
            continue;
        };
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let size = computed.size().round().as_uvec2();
        if size.x == 0 || size.y == 0 {
            continue;
        }
        let (t0, t1) = chart_window(chart, &src.state, end);
        let key = DrawKey {
            size,
            t0: t0.to_bits(),
            t1: t1.to_bits(),
            samples: DrawKey::samples(chart, t1),
            dark: src.theme.is_dark,
        };
        if frame.key.as_ref() == Some(&key) {
            continue;
        }
        let (lo, hi) = chart.range(t0, t1).unwrap_or((0.0, 1.0));
        let (step, ys) = ticks(lo, hi, 3);
        let scale = 1.0 / computed.inverse_scale_factor;
        *frame = ChartFrame {
            key: Some(key),
            t0,
            t1,
            lo: ys[0],
            hi: ys[ys.len() - 1],
            size,
            scale,
            labels: frame.labels.clone(),
        };
        let image = draw(&frame, chart, &ys, step, &src.theme);
        if let Some(mut img) = images.get_mut(&area.image) {
            *img = image;
        }
        let labels: Vec<(String, i32)> = ys
            .iter()
            .map(|&v| {
                #[allow(clippy::cast_possible_truncation)]
                let top = (frame.y(v) / scale).round() as i32;
                (tick_label(v, step), top)
            })
            .collect();
        if labels != frame.labels {
            let chars = labels
                .iter()
                .map(|(l, _)| l.chars().count())
                .max()
                .unwrap_or(0);
            #[allow(clippy::cast_precision_loss)]
            let gutter = (chars as f32 * AXIS_CHAR_W + SPACE[3]).max(GUTTER_W);
            for (e, axis) in &axes {
                if axis.0 == chart.id {
                    commands.entity(e).insert(Node {
                        width: px(gutter),
                        flex_shrink: 0.0,
                        align_self: AlignSelf::Stretch,
                        ..default()
                    });
                    commands.entity(e).despawn_children();
                    commands.entity(e).with_children(|a| {
                        for (label, top) in &labels {
                            #[allow(clippy::cast_precision_loss)]
                            a.spawn(y_label(label.clone(), *top as f32));
                        }
                    });
                }
            }
            for (row, mut node) in &mut xrows {
                if row.0 == chart.id {
                    node.margin.left = px(gutter);
                }
            }
            frame.labels = labels;
        }
        for (label, mut text) in &mut xlabels {
            if let XLabel::Start(id) = *label {
                if id == chart.id {
                    set_text(&mut text, &span_label(t1 - t0));
                }
            }
        }
    }
}

/// A chart's image (§3.3): gridlines, baseline, and its series' lines in
/// their colours, 2 px wide.
fn draw(frame: &ChartFrame, chart: &Chart, ys: &[f64], step: f64, theme: &Theme) -> Image {
    let (w, h) = (frame.size.x as usize, frame.size.y as usize);
    let mut canvas = Canvas::new(w, h);
    let hair = frame.scale.round().max(1.0);
    let zero = ys.iter().position(|v| v.abs() < step * 1e-6).unwrap_or(0);
    for (i, &v) in ys.iter().enumerate() {
        let role = if i == zero {
            ColorRole::Line
        } else {
            ColorRole::LineSoft
        };
        // On a pixel row's centre (or edge, for an even width), so it is
        // crisp.
        let y = (frame.y(v) - hair / 2.0).round() + hair / 2.0;
        canvas.hline(y, hair, theme.color(role).to_linear().to_f32_array());
    }
    for s in &chart.series {
        let color = theme.color(COLORS[s.color]).to_linear().to_f32_array();
        let lines = trace(&s.samples, frame.t0, frame.t1, |t, v| {
            Vec2::new(frame.x(t), frame.y(v))
        });
        for line in lines {
            canvas.polyline(&line, LINE_W * frame.scale, color);
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: frame.size.x,
            height: frame.size.y,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        canvas.rgba8(),
        TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::default(),
    );
    image.sampler = bevy::image::ImageSampler::nearest();
    image
}

/// The pointer over a plot, from its [`RelativeCursorPosition`]: only
/// when that reading changes, so a hover set otherwise stays.
pub(super) fn hover_from_cursor(mut areas: Query<(&RelativeCursorPosition, &mut ChartHover)>) {
    for (cursor, mut hover) in &mut areas {
        let at = cursor.normalized.filter(|_| cursor.cursor_over);
        if hover.from != at {
            hover.from = at;
            hover.x = at.map(|p| p.x + 0.5).filter(|x| (0.0..=1.0).contains(x));
        }
    }
}

/// The crosshair (§3.3: a live crosshair tooltip on hover): a line at
/// the pointer, a dot on each series' line, and a readout of the time and
/// each series' value there, beside the pointer.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(super) fn hover_readout(
    data: Res<PlotData>,
    areas: Query<(&PlotArea, &ChartHover, &ChartFrame)>,
    mut crosshairs: Query<(&Crosshair, &mut Node), (Without<HoverDot>, Without<HoverTip>)>,
    mut dots: Query<(&HoverDot, &mut Node), (Without<Crosshair>, Without<HoverTip>)>,
    mut tips: Query<(Entity, &HoverTip, &mut Node), (Without<Crosshair>, Without<HoverDot>)>,
    mut shown: Local<HashMap<u32, Vec<String>>>,
    mut commands: Commands,
) {
    for (area, hover, frame) in &areas {
        let Some(chart) = data.charts.iter().find(|c| c.id == area.chart) else {
            continue;
        };
        let at = hover.x.filter(|_| frame.key.is_some());
        let w = frame.size.x as f32 / frame.scale;
        for (c, mut node) in &mut crosshairs {
            if c.0 == chart.id {
                set_shown(&mut node, at.is_some());
                if let Some(x) = at {
                    node.left = px((x * w).round());
                }
            }
        }
        let t = at.map(|x| frame.t0 + f64::from(x) * (frame.t1 - frame.t0));
        let mut lines = vec![];
        for s in &chart.series {
            let v = t.and_then(|t| s.value_at(t)).filter(|v| v.is_finite());
            for (d, mut node) in &mut dots {
                if d.0 == s.id {
                    set_shown(&mut node, v.is_some());
                    if let (Some(x), Some(v)) = (at, v) {
                        node.left = px(x * w - 3.5);
                        node.top = px(frame.y(v) / frame.scale - 3.5);
                    }
                }
            }
            if t.is_some() {
                lines.push((
                    Some(COLORS[s.color]),
                    v.map_or("—".to_string(), value_label),
                ));
            }
        }
        for (e, tip, mut node) in &mut tips {
            if tip.0 != chart.id {
                continue;
            }
            set_shown(&mut node, at.is_some());
            let (Some(x), Some(t)) = (at, t) else {
                shown.remove(&chart.id);
                continue;
            };
            let (left, right) = if x > 0.5 {
                (Val::Auto, px((1.0 - x) * w + 8.0))
            } else {
                (px(x * w + 8.0), Val::Auto)
            };
            node.left = left;
            node.right = right;
            let ago = frame.t1 - t;
            let mut texts = vec![if ago < 0.005 {
                "now".to_string()
            } else {
                format!("-{} s", value_label(ago))
            }];
            texts.extend(lines.iter().map(|(_, v)| v.clone()));
            if shown.get(&chart.id) != Some(&texts) {
                commands.entity(e).despawn_children();
                let mut colors = vec![None];
                colors.extend(lines.iter().map(|(c, _)| *c));
                commands.entity(e).with_children(|tip| {
                    for (c, text) in colors.into_iter().zip(texts.iter()) {
                        tip.spawn(tip_line(c, text.clone()));
                    }
                });
                shown.insert(chart.id, texts);
            }
        }
    }
}

/// The chips show the state's window.
pub(super) fn chip_states(
    state: Res<PlotState>,
    chips: Query<(Entity, &WindowChip, Has<Checked>)>,
    added: Query<(), Added<WindowChip>>,
    mut commands: Commands,
) {
    if !state.is_changed() && added.is_empty() {
        return;
    }
    for (e, chip, checked) in &chips {
        let want = chip.0 == state.window;
        if want != checked {
            if want {
                commands.entity(e).insert(Checked);
            } else {
                commands.entity(e).remove::<Checked>();
            }
        }
    }
}

/// Rebuilds the picker when it opens, closes, changes topic or target,
/// or the charts it can add to change.
pub(super) fn sync_picker(
    theme: Res<Theme>,
    state: Res<PlotState>,
    data: Res<PlotData>,
    mut roots: Query<(Entity, Ref<PickerRoot>, &mut Node)>,
    mut built: ResMut<BuiltPicker>,
    mut commands: Commands,
) {
    let Ok((root, added, mut node)) = roots.single_mut() else {
        return;
    };
    let head = state.picker.as_ref().map(|p| {
        let targets = data
            .charts
            .iter()
            .filter(|c| !c.is_full())
            .map(|c| (c.id, c.title.clone()))
            .collect();
        (p.topic.clone(), targets, p.target)
    });
    if !added.is_added() && head == built.head {
        return;
    }
    set_shown(&mut node, head.is_some());
    if !added.is_added() {
        commands.entity(root).despawn_children();
        if let Some(p) = &state.picker {
            let charts = data.charts.clone();
            let p = p.clone();
            let t = theme.clone();
            commands
                .entity(root)
                .with_children(|r| spawn_picker(r, &t, &p, &charts));
        }
    }
    built.head = head;
    built.rows = None;
}

/// The picker's filter field's text is its filter.
pub(super) fn picker_filter(
    inputs: Query<(&EditableText, &ChildOf), Changed<EditableText>>,
    fields: Query<(), With<PickerFilter>>,
    mut state: ResMut<PlotState>,
) {
    for (text, parent) in &inputs {
        if !fields.contains(parent.parent()) {
            continue;
        }
        let value = text.value().to_string();
        if let Some(p) = state.picker.as_mut().filter(|p| p.filter != value) {
            p.filter = value;
        }
    }
}

/// The picker's rows: the server's topics, or the picked topic's
/// numbers, that pass the filter; and its status line while there are
/// none.
pub(super) fn sync_picker_rows(
    theme: Res<Theme>,
    state: Res<PlotState>,
    topics: Res<GzTopics>,
    parents: Query<Entity, With<PickerRows>>,
    mut status: Query<(&mut Text, &mut Node), With<PickerStatus>>,
    mut built: ResMut<BuiltPicker>,
    mut commands: Commands,
) {
    let (Some(p), Ok(parent)) = (&state.picker, parents.single()) else {
        return;
    };
    if !state.is_changed() && !topics.is_changed() && built.rows.is_some() {
        return;
    }
    let mut rows: Vec<(PickRow, String, String)> = vec![];
    let note = match &p.topic {
        None => {
            for t in topics.list.iter().filter(|t| p.passes(&t.name)) {
                let short = t.msg_type.rsplit('.').next().unwrap_or_default();
                rows.push((PickRow::Topic(t.name.clone()), t.name.clone(), short.into()));
            }
            match (rows.is_empty(), topics.list.is_empty()) {
                (true, true) => "No topics yet",
                (true, false) => "No topics match",
                _ => "",
            }
        }
        Some(_) => match (&p.error, &p.numbers) {
            (Some(_), _) => "Can't read this topic's messages",
            (None, None) => "Waiting for first message…",
            (None, Some(numbers)) => {
                for (path, v) in numbers {
                    let label = path.to_string();
                    if p.passes(&label) {
                        rows.push((PickRow::Field(path.clone()), label, value_label(*v)));
                    }
                }
                match (rows.is_empty(), numbers.is_empty()) {
                    (true, true) => "No numbers in this message",
                    (true, false) => "No fields match",
                    _ => "",
                }
            }
        },
    };
    for (mut text, mut node) in &mut status {
        set_text(&mut text, note);
        set_shown(&mut node, !note.is_empty());
    }
    let key: Vec<PickRow> = rows.iter().map(|r| r.0.clone()).collect();
    if built.rows.as_ref() == Some(&key) {
        return;
    }
    commands.entity(parent).despawn_children();
    commands.entity(parent).with_children(|list| {
        for (row, label, detail) in rows {
            list.spawn(pick_row(&theme, row, label, detail));
        }
    });
    built.rows = Some(key);
}

/// The picker's field values, in place.
pub(super) fn picker_values(state: Res<PlotState>, mut values: Query<(&PickValue, &mut Text)>) {
    if !state.is_changed() {
        return;
    }
    let Some(numbers) = state.picker.as_ref().and_then(|p| p.numbers.as_ref()) else {
        return;
    };
    for (value, mut text) in &mut values {
        let Some(path) = &value.0 else {
            continue;
        };
        if let Some((_, v)) = numbers.iter().find(|(p, _)| p == path) {
            set_text(&mut text, &value_label(*v));
        }
    }
}

/// Export CSV's notice, for [`NOTICE_SECS`].
pub(super) fn notice(
    time: Res<Time<Real>>,
    mut state: ResMut<PlotState>,
    mut notices: Query<(&mut Text, &mut Node), With<PlotNotice>>,
) {
    let now = time.elapsed_secs_f64();
    if state
        .notice
        .as_ref()
        .is_some_and(|(_, at)| now - at >= NOTICE_SECS)
    {
        state.notice = None;
    }
    if !state.is_changed() {
        return;
    }
    let text = state.notice.as_ref().map(|(t, _)| t.as_str());
    for (mut t, mut node) in &mut notices {
        set_text(&mut t, text.unwrap_or_default());
        set_shown(&mut node, text.is_some());
    }
}

/// A chip picks the window.
pub(super) fn on_window_chip(
    ev: On<ValueChange<Entity>>,
    chips: Query<&WindowChip>,
    mut state: ResMut<PlotState>,
) {
    if let Ok(chip) = chips.get(ev.value) {
        if state.window != chip.0 {
            state.window = chip.0;
        }
    }
}

/// Add field… and + open the picker on the topics; Pause freezes the
/// charts where they are (or lets them move again); Export CSV saves
/// every sample.
pub(super) fn on_action(
    ev: On<PanelActionActivated>,
    time: Res<Time<Real>>,
    clock: Res<SimClock>,
    data: Res<PlotData>,
    mut state: ResMut<PlotState>,
) {
    if ev.panel != PLOT {
        return;
    }
    let real = time.elapsed_secs_f64();
    match ev.action {
        ADD | ADD_FIELD => {
            if state.picker.as_ref().is_none_or(|p| p.topic.is_some()) {
                state.picker = Some(Picker::on(None, Target::New));
            }
        }
        PAUSE => {
            state.paused_at = match state.paused_at {
                Some(_) => None,
                None => Some(window_end(&state, &clock, &data, real)),
            };
        }
        EXPORT => {
            let end = window_end(&state, &clock, &data, real);
            let name = format!("plot-{end:.0}s.csv");
            let said = match save_text(&name, "text/csv", &data.csv()) {
                Ok(at) if cfg!(target_arch = "wasm32") => format!("Downloaded {at}"),
                Ok(at) => format!("Saved {at}"),
                Err(e) => format!("Couldn't save the CSV: {e}"),
            };
            state.notice = Some((said, real));
        }
        _ => {}
    }
}

/// A legend row's remove button removes its series (and its chart, with
/// the last).
pub(super) fn on_remove(
    ev: On<Activate>,
    removes: Query<&LegendRemove>,
    mut data: ResMut<PlotData>,
) {
    if let Ok(remove) = removes.get(ev.entity) {
        data.remove(remove.0);
    }
}

/// A topic row lists its fields; a field row adds it to the picker's
/// target, which then becomes that chart, so the next field joins it.
pub(super) fn on_pick_row(
    ev: On<Activate>,
    rows: Query<&PickRow>,
    topics: Res<GzTopics>,
    mut state: ResMut<PlotState>,
    mut data: ResMut<PlotData>,
) {
    let (Ok(row), Some(picker)) = (rows.get(ev.entity), state.picker.as_mut()) else {
        return;
    };
    match row {
        PickRow::Topic(name) => {
            *picker = Picker::on(Some(name.clone()), picker.target);
        }
        PickRow::Field(path) => {
            let Some(topic) = picker.topic.clone() else {
                return;
            };
            let msg_type = topics.get(&topic).map_or("", |t| t.msg_type.as_str());
            let id = data.add(&topic, msg_type, path.clone(), picker.target);
            picker.target = Target::Chart(id);
        }
    }
}

/// The picker's Back lists the topics again; Close closes it.
pub(super) fn on_picker_button(
    ev: On<Activate>,
    buttons: Query<&PickerButton>,
    mut state: ResMut<PlotState>,
) {
    match buttons.get(ev.entity) {
        Ok(PickerButton::Back) => {
            if let Some(p) = state.picker.as_mut() {
                *p = Picker::on(None, p.target);
            }
        }
        Ok(PickerButton::Close) => state.picker = None,
        Err(_) => {}
    }
}

/// The picker's "Add to" select picks where the next field goes.
pub(super) fn on_target(
    ev: On<SelectChanged>,
    targets: Query<&PickerTarget>,
    mut state: ResMut<PlotState>,
) {
    let (Ok(targets), Some(picker)) = (targets.get(ev.entity), state.picker.as_mut()) else {
        return;
    };
    if let Some(t) = targets.0.get(ev.index) {
        picker.target = *t;
    }
}
