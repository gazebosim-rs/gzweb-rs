//! The Sensors panel's data and rules: a model's sensors, each sensor's
//! topic, and what its preview shows (DECISIONS.md D-103).

use bevy::prelude::*;
use gz_bevy::topics::GzTopics;
use gz_core::sensor::Scan;
use gz_core::topic::world_topic;

use crate::inspectors::sensor::message_type;

/// How long a preview may go without data before it says so (§3.3.1).
pub(crate) const STALE_SECS: f64 = 2.0;

/// The rate a preview asks its topic for.
pub(crate) const PREVIEW_RATE: u32 = 10;

/// One of the model's sensors.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SensorEntry {
    /// The link it's on, and that link's scene path (`model/link`).
    pub link: Entity,
    pub link_path: String,
    pub name: String,
    /// Its type as Gazebo names it (`camera`, `gpu_lidar`).
    pub ty: String,
    /// Its topic, once the server's list and the world's SDFormat say it.
    pub topic: Option<String>,
    /// Its update rate (SDFormat), if set.
    pub rate: Option<f64>,
}

impl SensorEntry {
    pub(crate) fn key(&self) -> (Entity, String) {
        (self.link, self.name.clone())
    }
}

/// A sensor type's badge (§3.3: `camera`, `lidar`, `imu`, `depth`).
pub(crate) fn badge(ty: &str) -> &str {
    match ty {
        "gpu_lidar" | "gpu_ray" | "lidar" | "ray" => "lidar",
        "depth_camera" | "depth" => "depth",
        "rgbd_camera" | "rgbd" => "rgbd",
        "thermal_camera" | "thermal" => "thermal",
        "segmentation_camera" => "segment",
        "boundingbox_camera" => "bbox",
        "wideanglecamera" => "camera",
        "logical_camera" => "logical",
        "air_pressure" => "pressure",
        "force_torque" => "wrench",
        other => other,
    }
}

/// A sensor's topic among the server's: its `<topic>` (a sensor that
/// publishes several, an RGBD camera's image and depth, puts them under
/// it), else the one Gazebo derives under
/// `/world/W/model/M/link/L/sensor/S/`; of those, the one that carries
/// its type's message (a camera's image, not its info), the shortest
/// first. With no list yet, the `<topic>` as written.
pub(crate) fn sensor_topic(
    world: &str,
    link_path: &str,
    name: &str,
    ty: &str,
    explicit: Option<&str>,
    topics: &GzTopics,
) -> Option<String> {
    let explicit = explicit.map(|t| {
        if t.starts_with('/') {
            t.to_string()
        } else {
            format!("/{t}")
        }
    });
    let base = explicit.clone().unwrap_or_else(|| {
        let parts: Vec<&str> = link_path.split('/').collect();
        let (link, models) = parts.split_last().unwrap_or((&"", &[]));
        let mut leaf = String::new();
        for m in models {
            leaf.push_str(&format!("model/{m}/"));
        }
        leaf.push_str(&format!("link/{link}/sensor/{name}"));
        world_topic(world, &leaf)
    });
    let under = format!("{}/", base.trim_end_matches('/'));
    let want = message_type(ty);
    let mut candidates: Vec<(&str, &str)> = topics
        .list
        .iter()
        .filter(|t| t.name == base || t.name.starts_with(&under))
        .map(|t| (t.name.as_str(), t.msg_type.as_str()))
        .collect();
    candidates.sort_by_key(|(n, t)| (Some(*t) != want, n.len()));
    candidates
        .first()
        .map(|(n, _)| (*n).to_string())
        .or(explicit)
}

/// What a preview shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum PreviewKind {
    /// A camera's frames (`gz.msgs.Image`, sent as PNG).
    Image,
    /// A lidar's scan, from above.
    Scan,
    /// Any other message, as text.
    #[default]
    Text,
}

impl PreviewKind {
    pub(crate) fn of(msg_type: &str) -> PreviewKind {
        match msg_type.rsplit('.').next().unwrap_or_default() {
            "Image" => PreviewKind::Image,
            "LaserScan" => PreviewKind::Scan,
            _ => PreviewKind::Text,
        }
    }
}

/// The chosen sensor, by its link and name.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct SensorsState {
    pub chosen: Option<(Entity, String)>,
}

/// The preview's data: what the chosen sensor last sent.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct Preview {
    /// The topic it shows, and its message type.
    pub topic: Option<String>,
    pub msg_type: String,
    pub kind: PreviewKind,
    /// The camera frame's image (kept across a switch, dimmed until the
    /// next sensor's first frame), and its size.
    pub image: Option<Handle<Image>>,
    pub size: Option<(u32, u32)>,
    /// A frame came since the topic was chosen.
    pub fresh: bool,
    pub scan: Option<Scan>,
    pub text: Option<String>,
    /// When the last message came (real seconds).
    pub last_at: Option<f64>,
}

impl Preview {
    /// Shows `topic` (of `msg_type`) from its next message; a camera's
    /// last frame stays, dimmed, until then.
    pub(crate) fn start(&mut self, topic: Option<String>, msg_type: &str) {
        *self = Preview {
            topic,
            msg_type: msg_type.to_string(),
            kind: PreviewKind::of(msg_type),
            image: self.image.take(),
            size: self.size,
            ..default()
        };
    }

    /// How long it has gone without data, once that is over
    /// [`STALE_SECS`].
    pub(crate) fn stale(&self, now: f64) -> Option<f64> {
        let quiet = now - self.last_at?;
        (quiet > STALE_SECS).then_some(quiet)
    }
}

/// "No data for 3.2 s".
pub(crate) fn stale_label(secs: f64) -> String {
    format!("No data for {secs:.1} s")
}
