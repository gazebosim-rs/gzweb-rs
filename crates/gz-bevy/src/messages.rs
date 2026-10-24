//! Client-side system messages (spec §3.6 item 5, §7.5; `.agent/UI_PLAN.md`
//! §3 item 3; DECISIONS.md D-68, D-105): what the client logs (the
//! connection closing, a service failing, an asset that didn't load, a bad
//! message, the server's abilities), kept for the status bar's message
//! chip, the Console panel and, later, the message tray.
//!
//! The Gazebo websocket server forwards no log, so these are the client's
//! own: [`log_layer`] is a `tracing` layer the app installs through
//! Bevy's `LogPlugin::custom_layer`; it passes the events of this
//! project's crates (`gz_*`, `gzweb`) that the log filter lets through
//! (info and up, by default) to [`SystemMessages`] each frame. Without it
//! the list stays empty.

use std::collections::VecDeque;

use bevy::log::tracing::field::{Field, Visit};
use bevy::log::tracing::{Event, Level, Subscriber};
use bevy::log::tracing_subscriber::layer::{Context, Layer};
use bevy::log::BoxedLayer;
use bevy::prelude::*;
use crossbeam_channel::{Receiver, Sender};

/// How bad a message is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum MessageLevel {
    Debug,
    #[default]
    Info,
    Warning,
    Error,
}

/// One message.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SystemMessage {
    pub level: MessageLevel,
    pub text: String,
    /// Where it came from: the logging module (`gz_bevy::connection`).
    pub source: String,
    /// When, in real seconds since start.
    pub at: f64,
    /// And on the local clock (`12:04:31`).
    pub clock: String,
    /// Its place in the order messages came in, from 1 ([`SystemMessages::push`]
    /// numbers it).
    pub seq: u64,
}

/// The messages so far, oldest first, at most [`SystemMessages::CAP`].
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct SystemMessages {
    pub list: VecDeque<SystemMessage>,
    /// Warnings and errors since start (the ones dropped from the list
    /// included).
    pub warnings: u32,
    pub errors: u32,
    /// The last number given out ([`SystemMessages::next_seq`]).
    pub last_seq: u64,
}

impl SystemMessages {
    /// The most kept.
    pub const CAP: usize = 500;

    /// Keeps `message`, numbered next.
    pub fn push(&mut self, mut message: SystemMessage) {
        match message.level {
            MessageLevel::Warning => self.warnings += 1,
            MessageLevel::Error => self.errors += 1,
            MessageLevel::Info | MessageLevel::Debug => {}
        }
        message.seq = self.next_seq();
        if self.list.len() == Self::CAP {
            self.list.pop_front();
        }
        self.list.push_back(message);
    }

    /// The next number in the order messages come in: the Console numbers
    /// its own lines from it too, so they interleave with these.
    pub fn next_seq(&mut self) -> u64 {
        self.last_seq += 1;
        self.last_seq
    }

    /// The latest warning or error.
    pub fn latest_problem(&self) -> Option<&SystemMessage> {
        self.list
            .iter()
            .rev()
            .find(|m| m.level >= MessageLevel::Warning)
    }
}

/// What the log layer caught, waiting for [`drain_log`].
#[derive(Resource)]
pub struct LogInbox(Receiver<(MessageLevel, String, String)>);

/// Passes this project's messages on.
struct MessageLayer(Sender<(MessageLevel, String, String)>);

/// The text of an event's `message` field.
#[derive(Default)]
struct MessageText(String);

impl Visit for MessageText {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.0 = value.to_string();
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{value:?}");
        }
    }
}

/// An event from one of this project's crates, by its level (trace is
/// left out).
fn ours(level: &Level, target: &str) -> Option<MessageLevel> {
    if !["gz_", "gzweb"].iter().any(|p| target.starts_with(p)) {
        return None;
    }
    match *level {
        Level::ERROR => Some(MessageLevel::Error),
        Level::WARN => Some(MessageLevel::Warning),
        Level::INFO => Some(MessageLevel::Info),
        Level::DEBUG => Some(MessageLevel::Debug),
        _ => None,
    }
}

/// The local wall-clock time, `HH:MM:SS`.
pub fn clock_now() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        let d = js_sys::Date::new_0();
        format!(
            "{:02}:{:02}:{:02}",
            d.get_hours(),
            d.get_minutes(),
            d.get_seconds()
        )
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        chrono::Local::now().format("%H:%M:%S").to_string()
    }
}

impl<S: Subscriber> Layer<S> for MessageLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let meta = event.metadata();
        if let Some(level) = ours(meta.level(), meta.target()) {
            let mut text = MessageText::default();
            event.record(&mut text);
            let _ = self.0.send((level, text.0, meta.target().to_string()));
        }
    }
}

/// The layer for `LogPlugin::custom_layer`: inserts [`LogInbox`] and
/// returns the layer that feeds it.
pub fn log_layer(app: &mut App) -> Option<BoxedLayer> {
    let (tx, rx) = crossbeam_channel::unbounded();
    app.insert_resource(LogInbox(rx));
    Some(Box::new(MessageLayer(tx)))
}

/// Moves what the layer caught into [`SystemMessages`].
pub fn drain_log(
    inbox: Option<Res<LogInbox>>,
    time: Res<Time<Real>>,
    mut messages: ResMut<SystemMessages>,
) {
    let Some(inbox) = inbox else { return };
    for (level, text, source) in inbox.0.try_iter() {
        messages.push(SystemMessage {
            level,
            text,
            source,
            at: time.elapsed_secs_f64(),
            clock: clock_now(),
            seq: 0,
        });
    }
}

#[cfg(test)]
mod tests {
    use bevy::log::tracing_subscriber::layer::SubscriberExt;

    use super::*;

    /// The layer passes this project's messages, not others' or trace;
    /// the list keeps the newest, numbered, and counts the warnings and
    /// errors.
    #[test]
    fn the_log_layer_keeps_our_warnings_and_errors() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<SystemMessages>()
            .add_systems(Update, drain_log);
        let layer = log_layer(&mut app).unwrap();
        let subscriber = bevy::log::tracing_subscriber::registry().with(layer);
        bevy::log::tracing::subscriber::with_default(subscriber, || {
            bevy::log::tracing::warn!(target: "gz_bevy::connection", "closed: {}", 1006);
            bevy::log::tracing::error!(target: "gzweb", "boom");
            bevy::log::tracing::warn!(target: "wgpu_core", "not ours");
            bevy::log::tracing::info!(target: "gz_bevy::connection", "connected");
            bevy::log::tracing::debug!(target: "gz_ui", "detail");
            bevy::log::tracing::trace!(target: "gz_ui", "too low");
        });
        app.update();
        let m = app.world().resource::<SystemMessages>();
        let got: Vec<(MessageLevel, &str, &str)> = m
            .list
            .iter()
            .map(|x| (x.level, x.text.as_str(), x.source.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                (MessageLevel::Warning, "closed: 1006", "gz_bevy::connection"),
                (MessageLevel::Error, "boom", "gzweb"),
                (MessageLevel::Info, "connected", "gz_bevy::connection"),
                (MessageLevel::Debug, "detail", "gz_ui"),
            ]
        );
        assert_eq!((m.warnings, m.errors), (1, 1));
        let seqs: Vec<u64> = m.list.iter().map(|x| x.seq).collect();
        assert_eq!(seqs, [1, 2, 3, 4]);
        let clock = &m.list[0].clock;
        assert_eq!(clock.len(), 8, "{clock}");
        assert_eq!(&clock[2..3], ":");
        assert_eq!(m.latest_problem().unwrap().text, "boom");

        let mut many = SystemMessages::default();
        for i in 0..SystemMessages::CAP + 2 {
            many.push(SystemMessage {
                text: i.to_string(),
                ..default()
            });
        }
        assert_eq!(many.list.len(), SystemMessages::CAP);
        assert_eq!(many.list[0].text, "2");
        assert!(many.latest_problem().is_none());
    }
}
