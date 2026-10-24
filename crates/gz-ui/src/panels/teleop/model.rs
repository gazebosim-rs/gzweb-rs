//! The Teleop panel's data and rules: what can be driven, how, and the
//! command (DECISIONS.md D-104).

use bevy::platform::collections::HashMap;
use bevy::prelude::*;

/// How often an armed Teleop publishes its command.
pub(crate) const PUBLISH_HZ: f64 = 10.0;

/// A gamepad stick's dead zone.
pub(crate) const DEADZONE: f32 = 0.12;

/// The ways to drive (§3.3), one at a time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum DriveMode {
    #[default]
    Keyboard,
    Sliders,
    Gamepad,
}

impl DriveMode {
    pub(crate) const ALL: [DriveMode; 3] =
        [DriveMode::Keyboard, DriveMode::Sliders, DriveMode::Gamepad];

    pub(crate) fn label(self) -> &'static str {
        match self {
            DriveMode::Keyboard => "Keyboard",
            DriveMode::Sliders => "Sliders",
            DriveMode::Gamepad => "Gamepad",
        }
    }
}

/// A model's joint that takes position commands.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct JointTarget {
    pub joint: String,
    pub topic: String,
    /// Its limits, where the SDFormat sets finite ones.
    pub lower: f64,
    pub upper: f64,
}

/// Something to drive: a model and the topic its drive takes
/// `gz.msgs.Twist` on, and its commandable joints.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DriveTarget {
    pub model: String,
    pub topic: String,
    pub joints: Vec<JointTarget>,
}

impl DriveTarget {
    /// Its select label: the model and the topic, under the model's own
    /// prefix ("amr_01 · cmd_vel", "car · /drive").
    pub(crate) fn label(&self) -> String {
        if self.model.is_empty() {
            return self.topic.clone();
        }
        let own = format!("/model/{}/", self.model);
        let topic = self.topic.strip_prefix(&own).unwrap_or(&self.topic);
        format!("{} · {topic}", self.model)
    }
}

/// The model a `/model/<name>/…` topic belongs to.
pub(crate) fn topic_model(topic: &str) -> Option<&str> {
    topic.strip_prefix("/model/")?.split('/').next()
}

/// What the user set.
#[derive(Resource, Clone, Debug, PartialEq)]
pub(crate) struct TeleopState {
    pub mode: DriveMode,
    /// The target's drive topic.
    pub target: Option<String>,
    pub max_lin: f64,
    pub max_ang: f64,
    /// The sliders' command (and the gamepad's, last read).
    pub lin: f64,
    pub ang: f64,
    /// Publishing (§3.3: "armed"): the drive keys, X and the gamepad are
    /// Teleop's.
    pub armed: bool,
    /// Each joint's last commanded position, by topic.
    pub joints: HashMap<String, f64>,
}

impl Default for TeleopState {
    fn default() -> Self {
        Self {
            mode: DriveMode::Keyboard,
            target: None,
            max_lin: 1.0,
            max_ang: 1.5,
            lin: 0.0,
            ang: 0.0,
            armed: false,
            joints: HashMap::new(),
        }
    }
}

/// Which drive keys are held: forward, back, left, right.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Held {
    pub forward: bool,
    pub back: bool,
    pub left: bool,
    pub right: bool,
}

impl Held {
    pub(crate) fn any(self) -> bool {
        self.forward || self.back || self.left || self.right
    }

    /// The command they give: full speed each way (the pair cancels).
    pub(crate) fn command(self, max_lin: f64, max_ang: f64) -> (f64, f64) {
        let axis = |plus: bool, minus: bool| f64::from(i8::from(plus) - i8::from(minus));
        (
            axis(self.forward, self.back) * max_lin,
            axis(self.left, self.right) * max_ang,
        )
    }
}

/// A stick's reading past the dead zone, rescaled to start there.
pub(crate) fn stick(v: f32) -> f64 {
    if v.abs() < DEADZONE {
        0.0
    } else {
        f64::from(v.signum() * (v.abs() - DEADZONE) / (1.0 - DEADZONE))
    }
}

/// A speed as the sliders read it ("0.35 m/s").
pub(crate) fn speed_label(v: f64, unit: &str) -> String {
    format!("{v:.2} {unit}")
}
