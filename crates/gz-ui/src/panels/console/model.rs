//! The Console panel's data and rules: its lines (the client's log and the
//! command line's own output), the level chips, the filter, pause and
//! clear (DECISIONS.md D-105).

use std::collections::VecDeque;

use bevy::prelude::*;
use gz_bevy::messages::{MessageLevel, SystemMessage, SystemMessages};

/// The level chips (§3.3), one at a time: All, or one level.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum LevelChip {
    #[default]
    All,
    Error,
    Warn,
    Info,
    Debug,
}

impl LevelChip {
    pub(crate) const ALL: [LevelChip; 5] = [
        LevelChip::All,
        LevelChip::Error,
        LevelChip::Warn,
        LevelChip::Info,
        LevelChip::Debug,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            LevelChip::All => "All",
            LevelChip::Error => "Error",
            LevelChip::Warn => "Warn",
            LevelChip::Info => "Info",
            LevelChip::Debug => "Debug",
        }
    }

    /// A line of `level` shows under this chip.
    pub(crate) fn takes(self, level: MessageLevel) -> bool {
        match self {
            LevelChip::All => true,
            LevelChip::Error => level == MessageLevel::Error,
            LevelChip::Warn => level == MessageLevel::Warning,
            LevelChip::Info => level == MessageLevel::Info,
            LevelChip::Debug => level == MessageLevel::Debug,
        }
    }
}

/// What the user set.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct ConsoleState {
    pub filter: String,
    pub chip: LevelChip,
    /// Paused: the lines after this one wait (they are still kept).
    pub paused_at: Option<u64>,
    /// Clear: the log's lines up to this one are gone from the panel (the
    /// status bar's chip keeps its own). While paused that's the pause's
    /// line, so the held-back ones still come on Resume.
    pub cleared_through: u64,
    /// And the command line's lines up to this one (all of them at the
    /// time: Pause doesn't hold them back).
    pub output_cleared: u64,
    /// How many warnings and errors the log had counted when they were
    /// last seen (the Console showed) or cleared: the ones since are
    /// unread, even those the list has let go.
    pub seen: (u32, u32),
}

/// The command line's own lines: what was typed, and the results. They
/// stay out of [`SystemMessages`], so a mistyped command doesn't show in
/// the status bar.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct ConsoleOutput {
    pub lines: VecDeque<SystemMessage>,
    /// What was typed, oldest first, for ↑ and ↓.
    pub history: Vec<String>,
}

impl ConsoleOutput {
    /// The most kept, like [`SystemMessages::CAP`].
    pub(crate) const CAP: usize = SystemMessages::CAP;

    /// Keeps a line from the command line, numbered among the log's.
    pub(crate) fn say(
        &mut self,
        log: &mut SystemMessages,
        level: MessageLevel,
        text: impl Into<String>,
        at: f64,
    ) {
        if self.lines.len() == Self::CAP {
            self.lines.pop_front();
        }
        self.lines.push_back(SystemMessage {
            level,
            text: text.into(),
            source: SOURCE.into(),
            at,
            clock: gz_bevy::messages::clock_now(),
            seq: log.next_seq(),
        });
    }
}

/// The command line's lines' source.
pub(crate) const SOURCE: &str = "console";

/// The lines the panel holds (since Clear; the log's up to a pause),
/// newest first.
pub(crate) fn lines<'a>(
    log: &'a SystemMessages,
    output: &'a ConsoleOutput,
    state: &ConsoleState,
) -> Vec<&'a SystemMessage> {
    let mut all: Vec<&SystemMessage> = log
        .list
        .iter()
        .chain(&output.lines)
        .filter(|m| {
            if m.source == SOURCE {
                // Pause holds the log back, not the command line's answers.
                m.seq > state.output_cleared
            } else {
                m.seq > state.cleared_through && state.paused_at.is_none_or(|p| m.seq <= p)
            }
        })
        .collect();
    all.sort_by_key(|m| std::cmp::Reverse(m.seq));
    all
}

/// `m` shows under the chip and the filter (in its text or its source,
/// any case).
pub(crate) fn matches(m: &SystemMessage, state: &ConsoleState) -> bool {
    if !state.chip.takes(m.level) {
        return false;
    }
    let needle = state.filter.trim().to_lowercase();
    needle.is_empty()
        || m.text.to_lowercase().contains(&needle)
        || m.source.to_lowercase().contains(&needle)
}

/// A line's source as the row shows it: the logging module's last part
/// (`gz_bevy::connection` reads `connection`), a crate's name with dashes
/// (`gzweb`, `gz-ui`).
pub(crate) fn source_label(source: &str) -> String {
    match source.rsplit_once("::") {
        Some((_, last)) => last.to_string(),
        None => source.replace('_', "-"),
    }
}

/// The header's count: "2 warn · 1 err", of the lines the panel holds;
/// none without problems.
pub(crate) fn count_label(lines: &[&SystemMessage]) -> Option<String> {
    let n = |level| lines.iter().filter(|m| m.level == level).count();
    let (warn, err) = (n(MessageLevel::Warning), n(MessageLevel::Error));
    match (warn, err) {
        (0, 0) => None,
        (w, 0) => Some(format!("{w} warn")),
        (0, e) => Some(format!("{e} err")),
        (w, e) => Some(format!("{w} warn · {e} err")),
    }
}
