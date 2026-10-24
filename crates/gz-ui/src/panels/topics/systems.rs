//! The Topics panel's systems: its status in [`PanelStatus`], the topic
//! list's refresh, the echo's subscription and text, and the body's rows
//! and echo in place (DECISIONS.md D-99).

use bevy::clipboard::Clipboard;
use bevy::ecs::system::SystemParam;
use bevy::input::keyboard::KeyboardInput;
use bevy::input::ButtonState;
use bevy::input_focus::FocusedInput;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::Checked;
use bevy::ui_widgets::{Activate, ValueChange};
use gz_bevy::clipboard::clipboard_available;
use gz_bevy::connection::{GzCommand, GzPublish};
use gz_bevy::topics::{GzSubscriptions, GzTopics, GzTraffic, Holder};
use gz_core::echo::message_text;

use super::model::{clip_lines, rate_label, Echo, TopicChip, TopicsState, ECHO_HZ};
use super::view::{
    topic_row, ChipChoice, EchoClose, EchoPart, EchoSection, NoMatches, RowRate, TopicFilter,
    TopicList, TopicRow, TopicRows,
};
use super::{TopicsStart, COPY, PLOT_FIELD, TOPICS};
use crate::panels::{PanelActionActivated, PanelRegistry, PanelView, PlotPick, PLOT};
use crate::shell::{
    CurrentPhase, PanelStatus, Phase, SetPanelStatus, ShownPanels, Slot, UiCommand,
};
use crate::theme::visuals::Active;
use crate::theme::{Theme, SPACE};
use crate::widgets::layout::set_shown;
use crate::widgets::text::set_text;

/// What holds the echoed topic ([`GzSubscriptions`]).
pub(crate) const HOLDER: Holder = "topics_echo";

/// How often the topic list is asked for again while the panel shows.
pub(crate) const REFRESH_SECS: f64 = 2.0;

/// The list's scroll offset, kept across a respawned body.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct TopicsScroll(pub f32);

/// The topics the list's rows were built for, in order.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct BuiltRows(pub Vec<String>);

/// "No topics yet" once live with none, the list once there are topics,
/// skeleton rows while loading, else "Not connected" (§3.3.1).
pub(super) fn view(phase: Phase, topics: usize) -> PanelView {
    match phase {
        Phase::Live if topics == 0 => PanelView::empty("No topics yet"),
        Phase::Live => PanelView::Ready,
        Phase::Loading => PanelView::Loading,
        _ => PanelView::empty("Not connected"),
    }
}

/// The view, the header's count ("12 topics"), Copy, which needs a
/// message, and Plot field, which needs an echoed topic and the Plot
/// panel.
pub(super) fn topics_status(
    phase: Res<CurrentPhase>,
    topics: Res<GzTopics>,
    echo: Res<Echo>,
    panels: Res<PanelRegistry>,
    mut status: ResMut<PanelStatus>,
) {
    if !phase.is_changed() && !topics.is_changed() && !echo.is_changed() {
        return;
    }
    let n = topics.list.len();
    status.set_view(TOPICS, view(phase.0, n));
    let count = (phase.0 == Phase::Live && n > 0)
        .then(|| format!("{n} topic{}", if n == 1 { "" } else { "s" }));
    status.set_count(TOPICS, count);
    let copy = echo.full.is_some() && clipboard_available();
    status.set_action_enabled(TOPICS, COPY, copy);
    let plot = echo.topic.is_some() && panels.get(PLOT).is_some();
    status.set_action_enabled(TOPICS, PLOT_FIELD, plot);
}

/// A [`TopicsStart`]: opens the panel at once, and echoes its topic once
/// the server lists it.
pub(crate) fn apply_start(
    start: Option<Res<TopicsStart>>,
    topics: Res<GzTopics>,
    mut echo: ResMut<Echo>,
    mut ui: MessageWriter<UiCommand>,
    mut commands: Commands,
) {
    let Some(start) = start else {
        return;
    };
    if start.is_added() {
        ui.write(UiCommand::OpenPanel {
            panel: TOPICS,
            slot: Slot::Primary,
        });
    }
    if topics.get(&start.echo).is_some() {
        echo_topic(&mut echo, &topics, &start.echo);
        commands.remove_resource::<TopicsStart>();
    }
}

/// Asks for the topic list every [`REFRESH_SECS`] while the panel shows
/// and the world is live, so topics that start or end show.
pub(super) fn refresh_topics(
    time: Res<Time<Real>>,
    phase: Res<CurrentPhase>,
    mut last: Local<Option<f64>>,
    mut out: MessageWriter<GzCommand>,
) {
    let now = time.elapsed_secs_f64();
    if phase.0 != Phase::Live {
        *last = None;
        return;
    }
    // The handshake's list is fresh when the world goes live.
    let since = *last.get_or_insert(now);
    if now - since >= REFRESH_SECS {
        out.write(GzCommand::RefreshTopics);
        *last = Some(now);
    }
}

/// Holds the echoed topic while the panel shows and the world is live,
/// and lets go otherwise (PR #30 second review, item 6: an echo
/// subscribes only while it can be seen).
pub(super) fn hold_echo(
    hosts: Res<ShownPanels>,
    phase: Res<CurrentPhase>,
    echo: Res<Echo>,
    mut subs: ResMut<GzSubscriptions>,
) {
    let live = phase.0 == Phase::Live && hosts.is_shown(TOPICS);
    let want = echo.topic.as_deref().filter(|_| live);
    if subs.bypass_change_detection().held_by(HOLDER).eq(want) {
        return;
    }
    let held: Vec<String> = subs.held_by(HOLDER).map(str::to_string).collect();
    for topic in &held {
        subs.release(HOLDER, topic);
    }
    if let Some(topic) = want {
        subs.hold(HOLDER, topic, None);
    }
}

/// Keeps the echoed topic's newest message and redraws its text at most
/// [`ECHO_HZ`] times a second; marks the topic ended when it leaves the
/// server's list (§3.3.1: "Topic ended", keeping the last message).
pub(super) fn read_echo(
    time: Res<Time<Real>>,
    topics: Res<GzTopics>,
    mut publishes: MessageReader<GzPublish>,
    mut echo: ResMut<Echo>,
) {
    let Some(topic) = echo.topic.clone() else {
        publishes.clear();
        return;
    };
    for p in publishes.read().filter(|p| p.topic == topic) {
        let e = echo.bypass_change_detection();
        e.pending = Some(p.payload.clone());
        if !p.msg_type.is_empty() {
            e.msg_type.clone_from(&p.msg_type);
        }
    }
    if topics.is_changed() && topics.replies > 0 {
        let ended = topics.get(&topic).is_none();
        if echo.ended != ended {
            echo.ended = ended;
        }
    }
    let now = time.elapsed_secs_f64();
    let due = echo.text.is_none() || now - echo.drawn_at >= 1.0 / ECHO_HZ;
    if !due {
        return;
    }
    let Some(payload) = echo.bypass_change_detection().pending.take() else {
        return;
    };
    let (text, full) = match message_text(&echo.msg_type, &payload) {
        Ok(text) => (clip_lines(&text), Some(text)),
        Err(e) => (e.to_string(), None),
    };
    echo.text = Some(text);
    echo.full = full;
    echo.drawn_at = now;
}

/// What the rows and the echo are drawn from.
#[derive(SystemParam)]
pub(super) struct RowSources<'w> {
    theme: Res<'w, Theme>,
    state: Res<'w, TopicsState>,
    topics: Res<'w, GzTopics>,
    traffic: Res<'w, GzTraffic>,
    echo: Res<'w, Echo>,
}

/// Rebuilds the list's rows when the topics it shows change (a refresh,
/// the filter, a chip, a topic going quiet), and shows "No topics match"
/// while a filter hides them all.
pub(super) fn sync_rows(
    src: RowSources,
    lists: Query<(Entity, Ref<TopicRows>)>,
    mut no_matches: Query<&mut Node, With<NoMatches>>,
    mut built: ResMut<BuiltRows>,
    mut measured: Local<u64>,
    mut commands: Commands,
) {
    let Ok((list, added)) = lists.single() else {
        return;
    };
    // Rates change which topics are quiet, and only the Quiet chip lists
    // by rate.
    let rates = std::mem::replace(&mut *measured, src.traffic.measured) != src.traffic.measured
        && src.state.chip == TopicChip::Quiet;
    if !added.is_added() && !src.topics.is_changed() && !src.state.is_changed() && !rates {
        return;
    }
    let visible = src.state.visible(&src.topics, &src.traffic);
    if !added.is_added() && visible == built.0 {
        return;
    }
    commands.entity(list).despawn_children();
    commands.entity(list).with_children(|rows| {
        for name in &visible {
            let rate = rate_label(src.traffic.rate(name));
            let mut row = rows.spawn(topic_row(&src.theme, name, rate));
            if src.echo.is(name) {
                row.insert(Active);
            }
        }
    });
    let none = visible.is_empty() && src.state.filtering();
    for mut node in &mut no_matches {
        set_shown(&mut node, none);
    }
    built.0 = visible;
}

/// The rows' rates and the echoed row's active look, in place.
pub(super) fn row_states(
    src: RowSources,
    mut rates: Query<(&RowRate, &mut Text)>,
    added: Query<(), Added<RowRate>>,
    rows: Query<(Entity, &TopicRow, Has<Active>)>,
    mut measured: Local<u64>,
    mut commands: Commands,
) {
    let fresh = std::mem::replace(&mut *measured, src.traffic.measured) != src.traffic.measured;
    if fresh || !added.is_empty() {
        for (r, mut text) in &mut rates {
            set_text(&mut text, &rate_label(src.traffic.rate(&r.0)));
        }
    }
    if src.echo.is_changed() {
        for (e, row, active) in &rows {
            let want = src.echo.is(&row.0);
            if want != active {
                if want {
                    commands.entity(e).insert(Active);
                } else {
                    commands.entity(e).remove::<Active>();
                }
            }
        }
    }
}

/// The echo section follows [`Echo`]: shown while a topic is echoed, its
/// title, type and rate, its status line and its message.
pub(super) fn echo_section(
    src: RowSources,
    mut sections: Query<&mut Node, With<EchoSection>>,
    mut parts: Query<(&EchoPart, &mut Text, &mut Node), Without<EchoSection>>,
    added: Query<(), Added<EchoSection>>,
    mut measured: Local<u64>,
) {
    let fresh = std::mem::replace(&mut *measured, src.traffic.measured) != src.traffic.measured;
    if !src.echo.is_changed() && !fresh && added.is_empty() {
        return;
    }
    let echo = &*src.echo;
    for mut node in &mut sections {
        set_shown(&mut node, echo.topic.is_some());
    }
    let Some(topic) = &echo.topic else {
        return;
    };
    let rate = rate_label(src.traffic.rate(topic));
    let status = match (&echo.text, echo.ended) {
        (_, true) => "Topic ended",
        (None, false) => "Waiting for first message…",
        (Some(_), false) => "",
    };
    let meta = format!("{} · {rate}", echo.msg_type);
    for (part, mut text, mut node) in &mut parts {
        let want = match part {
            EchoPart::Title => topic.as_str(),
            EchoPart::Meta => meta.as_str(),
            EchoPart::Status => status,
            EchoPart::Message => echo.text.as_deref().unwrap_or_default(),
        };
        set_text(&mut text, want);
        if *part == EchoPart::Status {
            set_shown(&mut node, !status.is_empty());
        }
    }
}

/// The filter field's text is the list's filter.
pub(super) fn filter_text(
    inputs: Query<(&EditableText, &ChildOf), Changed<EditableText>>,
    fields: Query<(), With<TopicFilter>>,
    mut state: ResMut<TopicsState>,
) {
    for (text, parent) in &inputs {
        if fields.contains(parent.parent()) {
            let value = text.value().to_string();
            if state.filter != value {
                state.filter = value;
            }
        }
    }
}

/// The chips show the state's chip.
pub(super) fn chip_states(
    state: Res<TopicsState>,
    chips: Query<(Entity, &ChipChoice, Has<Checked>)>,
    added: Query<(), Added<ChipChoice>>,
    mut commands: Commands,
) {
    if !state.is_changed() && added.is_empty() {
        return;
    }
    for (e, chip, checked) in &chips {
        let want = chip.0 == state.chip;
        if want != checked {
            if want {
                commands.entity(e).insert(Checked);
            } else {
                commands.entity(e).remove::<Checked>();
            }
        }
    }
}

/// Keeps the list's scroll offset for a respawned body.
pub(super) fn keep_scroll(
    lists: Query<&ScrollPosition, (With<TopicList>, Changed<ScrollPosition>)>,
    mut scroll: ResMut<TopicsScroll>,
) {
    for s in &lists {
        scroll.set_if_neq(TopicsScroll(s.y));
    }
}

/// A chip picks the list's kind.
pub(super) fn on_chip(
    ev: On<ValueChange<Entity>>,
    chips: Query<&ChipChoice>,
    mut state: ResMut<TopicsState>,
) {
    if let Ok(chip) = chips.get(ev.value) {
        if state.chip != chip.0 {
            state.chip = chip.0;
        }
    }
}

/// Echoes `name`, unless it already is.
fn echo_topic(echo: &mut Echo, topics: &GzTopics, name: &str) {
    if !echo.is(name) {
        let msg_type = topics.get(name).map(|t| t.msg_type.as_str());
        echo.start(name, msg_type.unwrap_or_default());
    }
}

/// A row's click echoes its topic.
pub(super) fn on_row(
    ev: On<Activate>,
    rows: Query<&TopicRow>,
    topics: Res<GzTopics>,
    mut echo: ResMut<Echo>,
) {
    if let Ok(row) = rows.get(ev.entity) {
        echo_topic(&mut echo, &topics, &row.0);
    }
}

/// The echo header's close stops the echo.
pub(super) fn on_echo_close(
    ev: On<Activate>,
    closes: Query<(), With<EchoClose>>,
    mut echo: ResMut<Echo>,
) {
    if closes.contains(ev.entity) {
        *echo = Echo::default();
    }
}

/// With the list focused, ↑ and ↓ echo the topic above or below the
/// echoed one, Home and End the first and last, and Esc stops the echo.
pub(super) fn on_list_key(
    mut ev: On<FocusedInput<KeyboardInput>>,
    mut lists: Query<(&mut ScrollPosition, &ComputedNode), With<TopicList>>,
    theme: Res<Theme>,
    built: Res<BuiltRows>,
    topics: Res<GzTopics>,
    mut echo: ResMut<Echo>,
) {
    let Ok((mut scroll, list)) = lists.get_mut(ev.focused_entity) else {
        return;
    };
    if ev.input.state != ButtonState::Pressed {
        return;
    }
    let rows = &built.0;
    let at = echo
        .topic
        .as_ref()
        .and_then(|t| rows.iter().position(|r| r == t));
    let last = rows.len().checked_sub(1);
    let target = match (ev.input.key_code, at, last) {
        (KeyCode::Escape, ..) if echo.topic.is_some() => None,
        (_, _, None) => return,
        (KeyCode::ArrowDown, None, _) | (KeyCode::Home, ..) => Some(0),
        (KeyCode::ArrowUp, None, Some(l)) | (KeyCode::End, _, Some(l)) => Some(l),
        (KeyCode::ArrowDown, Some(i), Some(l)) => Some((i + 1).min(l)),
        (KeyCode::ArrowUp, Some(i), _) => Some(i.saturating_sub(1)),
        _ => return,
    };
    ev.propagate(false);
    match target {
        Some(i) => {
            echo_topic(&mut echo, &topics, &rows[i]);
            reveal_row(&mut scroll, list, theme.s.tree_row_h, i);
        }
        None => *echo = Echo::default(),
    }
}

/// Scrolls the list just enough to show row `i` (rows are `row_h` tall,
/// under the list's padding).
#[allow(clippy::cast_precision_loss)]
fn reveal_row(scroll: &mut ScrollPosition, list: &ComputedNode, row_h: f32, i: usize) {
    let pad = SPACE[4];
    let view = list.size().y * list.inverse_scale_factor;
    let top = pad + i as f32 * row_h;
    let y = if top < scroll.y + pad {
        top - pad
    } else if top + row_h > scroll.y + view - pad {
        top + row_h + pad - view
    } else {
        return;
    };
    scroll.y = y.max(0.0);
}

/// Copy puts the echoed message on the clipboard; Plot field opens the
/// Plot panel's picker on the echoed topic.
pub(super) fn on_action(
    ev: On<PanelActionActivated>,
    echo: Res<Echo>,
    clipboard: Option<ResMut<Clipboard>>,
    mut picks: MessageWriter<PlotPick>,
) {
    if ev.panel != TOPICS {
        return;
    }
    if ev.action == PLOT_FIELD {
        if let Some(topic) = &echo.topic {
            picks.write(PlotPick(topic.clone()));
        }
        return;
    }
    if ev.action != COPY {
        return;
    }
    let (Some(text), Some(mut clipboard)) = (&echo.full, clipboard) else {
        return;
    };
    if let Err(e) = clipboard.set_text(text.as_str()) {
        warn!("copy the message: {e:?}");
    }
}
