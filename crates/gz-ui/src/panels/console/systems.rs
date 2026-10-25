//! The Console panel's systems: its view, count and actions, the unread
//! badge, the rows in place, the filter and chips, and the command line
//! (DECISIONS.md D-105).

use bevy::ecs::system::SystemParam;
use bevy::input::keyboard::KeyboardInput;
use bevy::input::ButtonState;
use bevy::input_focus::FocusedInput;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::Checked;
use bevy::ui_widgets::ValueChange;
use gz_bevy::connection::{GzCommand, GzServiceReply, GzStatus};
use gz_bevy::edit::{edits_allowed, too_long};
use gz_bevy::messages::{MessageLevel, SystemMessages};
use gz_bevy::topics::GzTopics;
use gz_core::cli::{self, GzCli};
use gz_core::control::service_not_found;
use gz_core::echo::{encode_text, message_text};

use super::model::{count_label, lines, matches, ConsoleOutput, ConsoleState};
use super::view::{
    line_row, ChipChoice, CommandLine, ConsoleFilter, ConsoleRow, ConsoleRows, NoMatches,
};
use super::{ConsoleStart, ConsoleUnread, CLEAR, CONSOLE, PAUSE};
use crate::panels::{PanelActionActivated, PanelView, TopicsStart};
use crate::shell::{
    CurrentPhase, PanelStatus, Phase, SetPanelStatus, ShownPanels, Slot, UiCommand,
};
use crate::widgets::layout::set_shown;
use crate::widgets::text_field::{set_field_text, TextFieldInput};
use crate::widgets::{BadgeKind, TextSubmit};

/// The empty state (§3.3.1).
pub(crate) const NO_MESSAGES: &str = "No messages";

/// A [`ConsoleStart`]: opens the panel.
pub(super) fn apply_start(
    start: Option<Res<ConsoleStart>>,
    mut ui: MessageWriter<UiCommand>,
    mut commands: Commands,
) {
    if start.is_some() {
        ui.write(UiCommand::OpenPanel {
            panel: CONSOLE,
            slot: Slot::Primary,
        });
        commands.remove_resource::<ConsoleStart>();
    }
}

/// The activity badge: errors (with the warnings in the count) in
/// `danger`, else warnings in `warn_badge`.
pub(super) fn unread_badge(unread: Res<ConsoleUnread>, mut status: ResMut<PanelStatus>) {
    if !unread.is_changed() {
        return;
    }
    let ConsoleUnread { warnings, errors } = *unread;
    let badge = if errors > 0 {
        Some((BadgeKind::Error, errors + warnings))
    } else if warnings > 0 {
        Some((BadgeKind::Warning, warnings))
    } else {
        None
    };
    status.set_badge(CONSOLE, badge);
}

/// The log's warnings and errors not seen yet: while the Console shows,
/// all are seen; Clear drops them too.
pub(super) fn unread(
    hosts: Res<ShownPanels>,
    log: Res<SystemMessages>,
    mut state: ResMut<ConsoleState>,
    mut unread: ResMut<ConsoleUnread>,
) {
    if !log.is_changed() && !hosts.is_changed() && !state.is_changed() {
        return;
    }
    let counted = (log.warnings, log.errors);
    if hosts.is_shown(CONSOLE) && state.seen != counted {
        state.seen = counted;
    }
    unread.set_if_neq(ConsoleUnread {
        warnings: counted.0.saturating_sub(state.seen.0),
        errors: counted.1.saturating_sub(state.seen.1),
    });
}

/// The view ("No messages" with none), the header's count ("2 warn · 1
/// err"), Clear (with something to clear) and Pause's look.
pub(super) fn console_status(
    log: Res<SystemMessages>,
    output: Res<ConsoleOutput>,
    state: Res<ConsoleState>,
    mut status: ResMut<PanelStatus>,
) {
    if !log.is_changed() && !output.is_changed() && !state.is_changed() {
        return;
    }
    let all = lines(&log, &output, &state);
    let view = if all.is_empty() {
        PanelView::empty(NO_MESSAGES)
    } else {
        PanelView::Ready
    };
    status.set_view(CONSOLE, view);
    status.set_count(CONSOLE, count_label(&all));
    status.set_action_enabled(CONSOLE, CLEAR, !all.is_empty());
    status.set_action_on(CONSOLE, PAUSE, state.paused_at.is_some());
}

/// What the rows were built for (Clear and Pause) and the lines they
/// hold, newest first.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct BuiltRows {
    key: Option<(u64, u64, Option<u64>)>,
    seqs: Vec<u64>,
}

/// The rows follow the lines: each held line has a row, shown while it
/// matches the chip and the filter (so typing in the filter only shows
/// and hides rows); a new line's row goes on top, a line the log let go
/// of loses its row, and Clear or Pause builds them again.
#[allow(clippy::too_many_arguments)]
pub(super) fn sync_rows(
    log: Res<SystemMessages>,
    output: Res<ConsoleOutput>,
    state: Res<ConsoleState>,
    parents: Query<(Entity, Ref<ConsoleRows>)>,
    mut rows: Query<(Entity, &ConsoleRow, &mut Node), Without<NoMatches>>,
    mut none: Query<&mut Node, With<NoMatches>>,
    mut built: ResMut<BuiltRows>,
    mut commands: Commands,
) {
    let Ok((parent, added)) = parents.single() else {
        return;
    };
    let fresh = added.is_added();
    if !fresh && !log.is_changed() && !output.is_changed() && !state.is_changed() {
        return;
    }
    let all = lines(&log, &output, &state);
    let key = Some((state.cleared_through, state.output_cleared, state.paused_at));
    let seqs: Vec<u64> = all.iter().map(|m| m.seq).collect();
    let by_seq: HashMap<u64, bool> = all.iter().map(|m| (m.seq, matches(m, &state))).collect();
    if fresh || built.key != key {
        commands.entity(parent).despawn_children();
        commands.entity(parent).with_children(|p| {
            for m in &all {
                p.spawn(line_row(m, by_seq[&m.seq]));
            }
        });
    } else {
        // Rows the log let go of go; the others show as they match.
        for (e, row, mut node) in &mut rows {
            match by_seq.get(&row.0) {
                Some(shown) => set_shown(&mut node, *shown),
                None => commands.entity(e).despawn(),
            }
        }
        // New lines are newer than every built one: they go on top.
        let newest = built.seqs.first().copied().unwrap_or(0);
        let new: Vec<Entity> = all
            .iter()
            .take_while(|m| m.seq > newest)
            .map(|m| commands.spawn(line_row(m, by_seq[&m.seq])).id())
            .collect();
        if !new.is_empty() {
            commands.entity(parent).insert_children(0, &new);
        }
    }
    let any_shown = by_seq.values().any(|s| *s);
    for mut node in &mut none {
        set_shown(&mut node, !all.is_empty() && !any_shown);
    }
    built.set_if_neq(BuiltRows { key, seqs });
}

/// The filter field's text is the state's filter.
pub(super) fn filter_text(
    inputs: Query<(&EditableText, &ChildOf), Changed<EditableText>>,
    fields: Query<(), With<ConsoleFilter>>,
    mut state: ResMut<ConsoleState>,
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
    state: Res<ConsoleState>,
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

/// A chip picks the level.
pub(super) fn on_chip(
    ev: On<ValueChange<Entity>>,
    chips: Query<&ChipChoice>,
    mut state: ResMut<ConsoleState>,
) {
    if let Ok(chip) = chips.get(ev.value) {
        if state.chip != chip.0 {
            state.chip = chip.0;
        }
    }
}

/// Clear drops the lines so far from the panel; Pause holds the lines
/// after now back until Resume.
pub(super) fn on_action(
    ev: On<PanelActionActivated>,
    log: Res<SystemMessages>,
    mut state: ResMut<ConsoleState>,
) {
    if ev.panel != CONSOLE {
        return;
    }
    if ev.action == CLEAR {
        state.cleared_through = state.paused_at.unwrap_or(log.last_seq);
        state.output_cleared = log.last_seq;
        state.seen = (log.warnings, log.errors);
    } else if ev.action == PAUSE {
        state.paused_at = match state.paused_at {
            Some(_) => None,
            None => Some(log.last_seq),
        };
    }
}

/// How long after a call times out its reply is still taken for that
/// call's (and dropped).
const LATE_SECS: f64 = 30.0;

/// A service call waiting for its reply.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Pending {
    service: String,
    rep_type: String,
    /// When it times out, in real seconds.
    deadline: f64,
}

/// The command line's service calls waiting for replies, and where ↑ and
/// ↓ are in its history.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct CommandState {
    pub(crate) pending: Vec<Pending>,
    recall: Option<usize>,
    /// A line ↑ or ↓ brought back, for its field, set by [`apply_recall`]
    /// after the text input's own handling of the key.
    recalled: Option<(Entity, String)>,
    /// Services whose call timed out, until when: a reply that comes in
    /// that time is the late one, not the next call's.
    late: Vec<(String, f64)>,
}

/// What a command reads and writes.
#[derive(SystemParam)]
pub(super) struct Run<'w> {
    time: Res<'w, Time<Real>>,
    phase: Res<'w, CurrentPhase>,
    status: Option<Res<'w, GzStatus>>,
    topics: Res<'w, GzTopics>,
    log: ResMut<'w, SystemMessages>,
    output: ResMut<'w, ConsoleOutput>,
    calls: ResMut<'w, CommandState>,
    out: MessageWriter<'w, GzCommand>,
}

impl Run<'_> {
    fn say(&mut self, level: MessageLevel, text: impl Into<String>) {
        let at = self.time.elapsed_secs_f64();
        self.output.say(&mut self.log, level, text, at);
    }

    fn fail(&mut self, why: impl Into<String>) {
        self.say(MessageLevel::Error, why);
    }

    /// Runs `line`, saying what it did; a topic to echo in Topics comes
    /// back.
    fn run(&mut self, line: &str) -> Option<String> {
        self.say(MessageLevel::Info, format!("› {line}"));
        let command = match cli::parse(line) {
            Ok(c) => c,
            Err(e) => {
                self.fail(e.to_string());
                return None;
            }
        };
        if command != GzCli::Help && self.phase.0 != Phase::Live {
            self.fail("Not connected");
            return None;
        }
        match command {
            GzCli::Help => self.say(MessageLevel::Info, cli::HELP),
            GzCli::TopicList => {
                let names: Vec<&str> = self.topics.list.iter().map(|t| t.name.as_str()).collect();
                let text = if names.is_empty() {
                    "No topics".to_string()
                } else {
                    names.join("\n")
                };
                self.say(MessageLevel::Info, text);
            }
            GzCli::TopicInfo { topic } => match self.topics.get(&topic) {
                Some(t) => {
                    let text = format!("{topic}: {}", t.msg_type);
                    self.say(MessageLevel::Info, text);
                }
                None => self.fail(format!("No topic {topic}")),
            },
            GzCli::TopicEcho { topic } => {
                self.say(MessageLevel::Info, format!("Echoing {topic} in Topics"));
                return Some(topic);
            }
            GzCli::TopicPub {
                topic,
                msg_type,
                text,
            } => self.publish(topic, msg_type, &text),
            GzCli::ServiceList => self.fail("The websocket server doesn't list services"),
            GzCli::ServiceCall {
                service,
                req_type,
                rep_type,
                timeout_ms,
                req,
            } => self.call(service, req_type, rep_type, timeout_ms, &req),
        }
        None
    }

    fn publish(&mut self, topic: String, msg_type: String, text: &str) {
        if self.status.as_deref().and_then(|s| s.publishes) != Some(true) {
            self.fail("This server doesn't take publications");
            return;
        }
        match encode_text(&msg_type, text) {
            Ok(payload) => {
                let said = format!("Published a {msg_type} on {topic}");
                let publish = GzCommand::Publish {
                    topic,
                    msg_type,
                    payload,
                };
                if let Some(why) = too_long(&publish) {
                    return self.fail(format!("Too long to send: {why}"));
                }
                self.out.write(publish);
                self.say(MessageLevel::Info, said);
            }
            Err(e) => self.fail(e.to_string()),
        }
    }

    fn call(
        &mut self,
        service: String,
        req_type: String,
        rep_type: String,
        timeout_ms: u32,
        req: &str,
    ) {
        let payload = match encode_text(&req_type, req) {
            Ok(p) => p,
            Err(e) => return self.fail(e.to_string()),
        };
        // A stock server mangles a request's bytes; an empty one has none
        // to mangle.
        if !payload.is_empty() && !edits_allowed(self.status.as_deref()) {
            return self.fail("This server mangles a request's message; only an empty one works");
        }
        let request = GzCommand::Request {
            service: service.clone(),
            msg_type: req_type,
            payload,
        };
        if let Some(why) = too_long(&request) {
            return self.fail(format!("Too long to send: {why}"));
        }
        self.say(MessageLevel::Info, format!("Calling {service}…"));
        self.out.write(request);
        let deadline = self.time.elapsed_secs_f64() + f64::from(timeout_ms) / 1000.0;
        self.calls.pending.push(Pending {
            service,
            rep_type,
            deadline,
        });
    }
}

/// Enter in the command line runs its command, keeps it for ↑, and
/// empties the line.
pub(super) fn on_submit(
    ev: On<TextSubmit>,
    lines: Query<(), With<CommandLine>>,
    mut inputs: Query<(&ChildOf, &mut EditableText), With<TextFieldInput>>,
    mut run: Run,
    mut commands: Commands,
) {
    if !lines.contains(ev.entity) {
        return;
    }
    let line = ev.value.trim().to_string();
    if line.is_empty() {
        return;
    }
    if run.output.history.last() != Some(&line) {
        run.output.history.push(line.clone());
    }
    run.calls.recall = None;
    if let Some(topic) = run.run(&line) {
        commands.insert_resource(TopicsStart {
            echo: topic,
            beside: Some(CONSOLE),
        });
    }
    for (parent, mut text) in &mut inputs {
        if parent.parent() == ev.entity {
            set_field_text(&mut text, "");
        }
    }
}

/// ↑ and ↓ in the command line walk its history; past the newest, the
/// line empties.
pub(super) fn on_history_key(
    mut ev: On<FocusedInput<KeyboardInput>>,
    lines: Query<(), With<CommandLine>>,
    inputs: Query<&ChildOf, With<TextFieldInput>>,
    output: Res<ConsoleOutput>,
    mut calls: ResMut<CommandState>,
) {
    if ev.input.state != ButtonState::Pressed || ev.event_target() != ev.focused_entity {
        return;
    }
    let Ok(parent) = inputs.get(ev.focused_entity) else {
        return;
    };
    if !lines.contains(parent.parent()) {
        return;
    }
    let n = output.history.len();
    let next = match (ev.input.key_code, calls.recall) {
        (KeyCode::ArrowUp, None) if n > 0 => Some(n - 1),
        (KeyCode::ArrowUp, Some(i)) => Some(i.saturating_sub(1)),
        (KeyCode::ArrowDown, Some(i)) if i + 1 < n => Some(i + 1),
        (KeyCode::ArrowDown, Some(_)) => None,
        _ => return,
    };
    ev.propagate(false);
    calls.recall = next;
    let line = next.map_or("", |i| output.history[i].as_str());
    calls.recalled = Some((ev.focused_entity, line.to_string()));
}

/// Puts the recalled line in its field, the caret at its end: after the
/// text input's observer has queued its own move for the arrow.
pub(super) fn apply_recall(
    mut calls: ResMut<CommandState>,
    mut inputs: Query<&mut EditableText, With<TextFieldInput>>,
) {
    let Some((field, line)) = calls.recalled.take() else {
        return;
    };
    if let Ok(mut text) = inputs.get_mut(field) {
        set_field_text(&mut text, &line);
        // Even when the text is the same: the arrow moved the caret.
        text.queue_edit(bevy::text::TextEdit::TextEnd(false));
    }
}

/// A reply to the command line's service call says what came back; a
/// call with no reply in time, or whose connection dropped, says so.
pub(super) fn service_replies(
    mut replies: MessageReader<GzServiceReply>,
    time: Res<Time<Real>>,
    status: Option<Res<GzStatus>>,
    mut log: ResMut<SystemMessages>,
    mut output: ResMut<ConsoleOutput>,
    mut calls: ResMut<CommandState>,
) {
    let now = time.elapsed_secs_f64();
    calls.late.retain(|(_, until)| *until > now);
    for reply in replies.read() {
        // A timed-out call's reply, come late: dropped.
        if let Some(j) = calls.late.iter().position(|(s, _)| *s == reply.service) {
            calls.late.remove(j);
            continue;
        }
        let Some(i) = calls
            .pending
            .iter()
            .position(|p| p.service == reply.service)
        else {
            continue;
        };
        let call = calls.pending.remove(i);
        let not_found = service_not_found(&reply.msg_type, &reply.payload);
        let msg_type = if reply.msg_type.is_empty() {
            call.rep_type.as_str()
        } else {
            reply.msg_type.as_str()
        };
        let (level, text) = if not_found {
            (
                MessageLevel::Error,
                format!("{}: service not found", call.service),
            )
        } else {
            match message_text(msg_type, &reply.payload) {
                Ok(text) => (
                    MessageLevel::Info,
                    format!("{} replied {msg_type}\n{}", call.service, text.trim_end())
                        .trim_end()
                        .to_string(),
                ),
                Err(e) => (MessageLevel::Error, format!("{}: {e}", call.service)),
            }
        };
        output.say(&mut log, level, text, now);
    }
    // A dropped connection ends the calls still waiting, after this
    // frame's replies (the old server's last): the next server's replies
    // aren't theirs, nor the late ones'.
    let ready = status
        .as_deref()
        .is_some_and(|s| s.status == gz_core::transport::Status::Ready);
    if !ready && (!calls.pending.is_empty() || !calls.late.is_empty()) {
        calls.late.clear();
        for p in std::mem::take(&mut calls.pending) {
            let text = format!("{}: the connection dropped", p.service);
            output.say(&mut log, MessageLevel::Error, text, now);
        }
    }
    if !calls.pending.iter().any(|p| p.deadline <= now) {
        return;
    }
    let (late, waiting): (Vec<Pending>, Vec<Pending>) = std::mem::take(&mut calls.pending)
        .into_iter()
        .partition(|p| p.deadline <= now);
    calls.pending = waiting;
    for p in late {
        calls.late.push((p.service.clone(), now + LATE_SECS));
        let text = format!("{}: no reply in time", p.service);
        output.say(&mut log, MessageLevel::Error, text, now);
    }
}
