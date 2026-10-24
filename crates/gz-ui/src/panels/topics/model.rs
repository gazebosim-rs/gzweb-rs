//! The Topics panel's state and rules, as plain data and functions: the
//! chips and the filter, rate labels, and the echo (DECISIONS.md D-99).

use bevy::prelude::*;
use gz_bevy::topics::{GzTopics, GzTraffic, TopicInfo};

/// The topic chips (§3.3), one at a time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum TopicChip {
    #[default]
    All,
    /// A sensor's topics: a sensor message type, or a path through a
    /// sensor.
    Sensors,
    /// A model's topics: a path through a model.
    Models,
    /// No message in the last second, or none seen: the client measures
    /// only the topics it receives (D-99).
    Quiet,
}

impl TopicChip {
    pub(crate) const ALL: [TopicChip; 4] = [
        TopicChip::All,
        TopicChip::Sensors,
        TopicChip::Models,
        TopicChip::Quiet,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            TopicChip::All => "All",
            TopicChip::Sensors => "Sensors",
            TopicChip::Models => "Models",
            TopicChip::Quiet => "Quiet",
        }
    }
}

/// The message types Gazebo's sensors publish.
const SENSOR_TYPES: [&str; 16] = [
    "Image",
    "CameraInfo",
    "LaserScan",
    "PointCloudPacked",
    "IMU",
    "Magnetometer",
    "Altimeter",
    "AirPressureSensor",
    "AirSpeedSensor",
    "FluidPressure",
    "NavSat",
    "Contacts",
    "Wrench",
    "LogicalCameraImage",
    "Odometry",
    "BatteryState",
];

/// Whether `topic` is a sensor's.
pub(crate) fn is_sensor(topic: &TopicInfo) -> bool {
    let short = topic.msg_type.rsplit('.').next().unwrap_or_default();
    SENSOR_TYPES.contains(&short) || topic.name.contains("/sensor/")
}

/// Whether `topic` is a model's.
pub(crate) fn is_model(topic: &TopicInfo) -> bool {
    topic.name.contains("/model/")
}

/// Whether `topic` has been quiet: no rate measured, or none over the
/// last second.
pub(crate) fn is_quiet(rate: Option<f32>) -> bool {
    rate.is_none_or(|r| r <= 0.0)
}

/// What the user set; the body is built from it.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct TopicsState {
    /// The filter text: names that contain it, ignoring case.
    pub filter: String,
    pub chip: TopicChip,
}

impl TopicsState {
    /// Whether `topic` passes the filter and the chip.
    pub(crate) fn shows(&self, topic: &TopicInfo, rate: Option<f32>) -> bool {
        let needle = self.filter.trim().to_lowercase();
        let named = needle.is_empty() || topic.name.to_lowercase().contains(&needle);
        named
            && match self.chip {
                TopicChip::All => true,
                TopicChip::Sensors => is_sensor(topic),
                TopicChip::Models => is_model(topic),
                TopicChip::Quiet => is_quiet(rate),
            }
    }

    /// The topics the list shows, by name.
    pub(crate) fn visible(&self, topics: &GzTopics, traffic: &GzTraffic) -> Vec<String> {
        topics
            .list
            .iter()
            .filter(|t| self.shows(t, traffic.rate(&t.name)))
            .map(|t| t.name.clone())
            .collect()
    }

    /// Whether a filter or a chip narrows the list.
    pub(crate) fn filtering(&self) -> bool {
        !self.filter.trim().is_empty() || self.chip != TopicChip::All
    }
}

/// A rate for a list row or the echo's header: "—" before one is
/// measured.
pub(crate) fn rate_label(rate: Option<f32>) -> String {
    match rate {
        None => "—".into(),
        Some(r) if r <= 0.0 => "0 Hz".into(),
        Some(r) if r < 10.0 => format!("{r:.1} Hz"),
        Some(r) => format!("{r:.0} Hz"),
    }
}

/// The most lines of a message the echo shows.
pub(crate) const ECHO_LINES: usize = 400;

/// How often the echo's text is redrawn, at most (a second's worth of a
/// fast topic is unreadable anyway).
pub(crate) const ECHO_HZ: f64 = 10.0;

/// The longest line the echo shows: a long repeated field prints on one
/// line ("ranges: [ … ]").
pub(crate) const ECHO_LINE_CHARS: usize = 2000;

/// `text` cut to [`ECHO_LINES`] lines of at most [`ECHO_LINE_CHARS`]
/// characters, saying what it left out.
pub(crate) fn clip_lines(text: &str) -> String {
    let total = text.lines().count();
    let mut out = String::new();
    for (i, line) in text.lines().take(ECHO_LINES).enumerate() {
        if i > 0 {
            out.push('\n');
        }
        match line.char_indices().nth(ECHO_LINE_CHARS) {
            Some((cut, _)) => {
                let more = line[cut..].chars().count();
                out.push_str(&line[..cut]);
                out.push_str(&format!(" … {more} more characters"));
            }
            None => out.push_str(line),
        }
    }
    if total > ECHO_LINES {
        out.push_str(&format!("\n… {} more lines", total - ECHO_LINES));
    }
    out.trim_end().to_string()
}

/// The echoed topic: what it last said, as text.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct Echo {
    /// The topic echoed, if any.
    pub topic: Option<String>,
    /// Its message type, from the topic list.
    pub msg_type: String,
    /// The last message as text, once one arrived, clipped to show
    /// ([`clip_lines`]).
    pub text: Option<String>,
    /// The same, whole: what Copy copies.
    pub full: Option<String>,
    /// The topic left the server's list; the last message stays.
    pub ended: bool,
    /// The newest message not yet drawn, and when the text was last drawn
    /// (`Time<Real>` seconds).
    pub pending: Option<Vec<u8>>,
    pub drawn_at: f64,
}

impl Echo {
    /// Echoes `topic` (of `msg_type`) from its next message.
    pub(crate) fn start(&mut self, topic: &str, msg_type: &str) {
        *self = Echo {
            topic: Some(topic.to_string()),
            msg_type: msg_type.to_string(),
            ..default()
        };
    }

    pub(crate) fn is(&self, topic: &str) -> bool {
        self.topic.as_deref() == Some(topic)
    }
}
