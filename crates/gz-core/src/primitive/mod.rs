// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
//! Bounded local primitive / glTF viewer contracts; no transport runtime.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub mod lifecycle;
pub mod replay;
mod scan;
pub use scan::{Scan, ScanFrame, MAX_SCAN_BEAMS};

pub const MAX_SCENE_BYTES: usize = 64 * 1024;
pub const MAX_VISUALS: usize = 128;

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Visual {
    pub id: u64,
    pub name: String,
    pub translation: [f64; 3],
    pub rotation: [f64; 4],
    pub shape: Shape,
    pub color: [f64; 4],
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Shape {
    Box { size: [f64; 3] },
    Sphere { radius: f64 },
    Mesh { uri: String, scale: [f64; 3] },
}

/// Separate HTTP-only source: legacy Gazebo asset fallback is never involved.
pub const LOCAL_MESH_SOURCE: &str = "m2mesh";
pub const LOCAL_MESH_PREFIX: &str = "m2mesh://";

fn asset_segments(path: &str) -> bool {
    !path.is_empty()
        && path.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        })
}

fn asset_authority(authority: &str) -> bool {
    authority.strip_prefix("127.0.0.1:").is_some_and(|port| {
        !port.is_empty()
            && port.bytes().all(|b| b.is_ascii_digit())
            && port.parse::<u16>().is_ok_and(|value| value > 0)
    })
}

/// Retain a scope guard *outside* the directory resolved by the glTF loader.
/// Document validation rejects walks above the original scope; the repeated
/// scope check additionally detects changed path components after resolution.
pub fn local_mesh_asset_uri(uri: &str) -> Result<String, String> {
    let invalid = || "Mesh URI must be a canonical loopback HTTP glTF asset URL".to_string();
    if uri.len() > 2048 {
        return Err(invalid());
    }
    let (authority, path) = uri
        .strip_prefix("http://")
        .and_then(|s| s.split_once('/'))
        .ok_or_else(invalid)?;
    let (scope, relative) = path
        .strip_prefix("assets/")
        .and_then(|s| s.split_once('/'))
        .ok_or_else(invalid)?;
    if !asset_authority(authority)
        || scope.is_empty()
        || !scope.bytes().all(|b| b.is_ascii_digit())
        || !asset_segments(relative)
        || !(relative.ends_with(".gltf") || relative.ends_with(".glb"))
    {
        return Err(invalid());
    }
    Ok(format!(
        "{LOCAL_MESH_SOURCE}://{authority}/{scope}/assets/{scope}/{relative}"
    ))
}

/// Decode only paths belonging to the M2 source, including relative dependencies.
pub fn local_mesh_http_url(path: &str) -> Result<String, String> {
    let invalid = || "Local mesh dependency escaped its declared asset scope".to_string();
    let (authority, rest) = path.split_once('/').ok_or_else(invalid)?;
    let (guard, rest) = rest.split_once('/').ok_or_else(invalid)?;
    let (scope, relative) = rest
        .strip_prefix("assets/")
        .and_then(|s| s.split_once('/'))
        .ok_or_else(invalid)?;
    if !asset_authority(authority)
        || guard != scope
        || scope.is_empty()
        || !scope.bytes().all(|b| b.is_ascii_digit())
        || !asset_segments(relative)
    {
        return Err(invalid());
    }
    Ok(format!("http://{authority}/assets/{scope}/{relative}"))
}

/// glTF dependencies may walk up within this asset scope, never above it.
/// Inline data is decoded by the maintained glTF loader, not fetched.
pub fn local_mesh_dependency_allowed(base: &str, dependency: &str) -> bool {
    if local_mesh_asset_uri(base).is_err() {
        return false;
    }
    if dependency.starts_with("data:") {
        return true;
    }
    let relative = base
        .split_once("/assets/")
        .and_then(|(_, p)| p.split_once('/'))
        .map(|(_, p)| p);
    let Some(relative) = relative else {
        return false;
    };
    let mut depth = relative.split('/').count() - 1;
    for part in dependency.split('/') {
        match part {
            "." => {}
            ".." => {
                let Some(parent) = depth.checked_sub(1) else {
                    return false;
                };
                depth = parent;
            }
            part if asset_segments(part) => depth += 1,
            _ => return false,
        }
    }
    !dependency.is_empty() && !matches!(dependency.rsplit('/').next(), Some("." | ".."))
}

impl Visual {
    /// glTF Y-up -> Gazebo Z-up once, before the unchanged world-root basis.
    /// Bevy applies R*S: (worldQ*Rx90)*Scale(sx,sz,sy) equals
    /// worldQ*Scale(sx,sy,sz)*Rx90. Pose updates must use this same rotation.
    pub fn render_rotation(&self) -> [f64; 4] {
        if !matches!(self.shape, Shape::Mesh { .. }) {
            return self.rotation;
        }
        let [x, y, z, w] = self.rotation;
        let s = std::f64::consts::FRAC_1_SQRT_2;
        [s * (w + x), s * (y + z), s * (z - y), s * (w - x)]
    }

    fn render_scale(&self) -> [f64; 3] {
        match self.shape {
            Shape::Mesh {
                scale: [x, y, z], ..
            } => [x, z, y],
            _ => [1.; 3],
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Scene {
    pub version: u32,
    pub world: String,
    pub world_id: u64,
    pub epoch: u64,
    pub seconds: u64,
    pub nanoseconds: u32,
    pub sequence: u64,
    pub paused: bool,
    pub visuals: Vec<Visual>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum NullableError {
    Text(String),
    None,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ack {
    pub version: u32,
    pub request_id: u64,
    pub ok: bool,
    pub epoch: u64,
    pub seconds: u64,
    pub nanoseconds: u32,
    pub paused: bool,
    pub error: NullableError,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ServerMessage {
    Scene(Scene),
    Ack(Ack),
    Scan(ScanFrame),
    LifecycleCatalog(lifecycle::Catalog),
    LifecycleAck(lifecycle::Reply),
    PublicationCredit(PublicationCredit),
    ReplayStatus(replay::ReplayStatus),
}

/// Opt-in, connection-scoped transport credit; not a simulation request ID.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicationCredit {
    version: u32,
    token: u64,
}
impl PublicationCredit {
    fn validate(&self) -> Result<(), String> {
        if self.version != 1 || self.token == 0 {
            return Err("Invalid publication credit".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Pause,
    Resume,
    Reset,
}

#[derive(Debug, Serialize)]
pub struct Control {
    version: u32,
    kind: &'static str,
    request_id: u64,
    action: Action,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Stamp {
    pub epoch: u64,
    pub seconds: u64,
    pub nanoseconds: u32,
}

impl Scene {
    pub fn stamp(&self) -> Stamp {
        Stamp {
            epoch: self.epoch,
            seconds: self.seconds,
            nanoseconds: self.nanoseconds,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        let version = if self
            .visuals
            .iter()
            .any(|v| matches!(v.shape, Shape::Mesh { .. }))
        {
            2
        } else {
            1
        };
        if self.version != version
            || self.world != "default"
            || self.world_id == 0
            || self.world_id > u32::MAX as u64
            || self.nanoseconds >= 1_000_000_000
        {
            return Err("Unsupported scene version/world or invalid integer timestamp".into());
        }
        if self.visuals.len() > MAX_VISUALS {
            return Err("Scene exceeds 128 visuals".into());
        }
        let mut ids = BTreeSet::new();
        for visual in &self.visuals {
            let norm = visual.rotation.iter().map(|v| v * v).sum::<f64>();
            if visual.id == 0
                || visual.id > u32::MAX as u64
                || visual.id == self.world_id
                || !ids.insert(visual.id)
                || visual.name.len() > 256
                || visual
                    .translation
                    .iter()
                    .any(|v| !v.is_finite() || !(*v as f32).is_finite())
                || visual.rotation.iter().any(|v| !v.is_finite())
                || !norm.is_finite()
                || (norm - 1.0).abs() > 1e-6
                || visual.color[3] != 1.0
                || visual
                    .color
                    .iter()
                    .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
            {
                return Err("Invalid visual ID/name, pose, unit quaternion or RGBA".into());
            }
            let dimensions: &[f64] = match &visual.shape {
                Shape::Box { size } => size,
                Shape::Sphere { radius } => std::slice::from_ref(radius),
                Shape::Mesh { uri, scale } => {
                    local_mesh_asset_uri(uri)?;
                    scale
                }
            };
            if dimensions.iter().any(|v| {
                !v.is_finite() || *v <= 0.0 || !(*v as f32).is_finite() || *v as f32 <= 0.0
            }) {
                return Err(
                    "Primitive dimensions are outside the finite positive rendering domain".into(),
                );
            }
        }
        Ok(())
    }
}

impl Ack {
    fn validate(&self) -> Result<(), String> {
        if self.version != 1
            || self.request_id == 0
            || self.nanoseconds >= 1_000_000_000
            || self.ok != matches!(self.error, NullableError::None)
        {
            return Err("Invalid acknowledgement metadata".into());
        }
        Ok(())
    }
}

pub fn parse(text: &str) -> Result<ServerMessage, String> {
    if text.len() > MAX_SCENE_BYTES {
        return Err("Server message exceeds 64 KiB".into());
    }
    let message: ServerMessage =
        serde_json::from_str(text).map_err(|e| format!("Invalid server JSON: {e}"))?;
    match &message {
        ServerMessage::Scene(scene) => scene.validate()?,
        ServerMessage::Ack(ack) => ack.validate()?,
        ServerMessage::Scan(scan) => scan.validate()?,
        ServerMessage::LifecycleCatalog(catalog) => catalog.validate()?,
        ServerMessage::LifecycleAck(reply) => reply.validate()?,
        ServerMessage::PublicationCredit(credit) => credit.validate()?,
        ServerMessage::ReplayStatus(status) => status.validate()?,
    }
    Ok(message)
}

#[derive(Clone, Copy)]
struct Current {
    world_id: u64,
    stamp: Stamp,
    sequence: u64,
    paused: bool,
}

#[derive(Clone, Copy)]
struct Pending {
    id: u64,
    action: Action,
    stamp: Stamp,
}

/// One coalesced complete scene and at most one control. Callbacks never mutate
/// Bevy entities. Disconnection invalidates the scene before reconnect begins.
pub struct Session {
    pub connected: bool,
    pub status: String,
    pub notice: Option<String>,
    pub clear_scene: bool,
    pub latest: Option<Scene>,
    /// Coalesced scan change; Some(None) explicitly retires the preview.
    pub latest_scan: Option<Option<ScanFrame>>,
    scan_time: Option<u64>,
    acquired_time: Option<u64>,
    cleared_acquired_time: Option<u64>,
    current: Option<Current>,
    fence: Option<(Stamp, bool)>,
    pending: Option<Pending>,
    lifecycle_enabled: bool,
    replay_enabled: bool,
    pub replay: Option<replay::ReplayStatus>,
    replay_frames: Option<u32>,
    pub catalog: Option<lifecycle::Catalog>,
    pub lifecycle_reply: Option<lifecycle::Reply>,
    pending_edit: Option<(u64, lifecycle::Command)>,
    lifecycle_scene: Option<Scene>,
    next_request: Option<u64>,
    publication_enabled: bool,
    publication_last: Option<u64>,
    publication_pending: Option<u64>,
    publication_scene: Option<(u64, u64, u64)>,
    publication_catalog: bool,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            connected: false,
            status: "Disconnected".into(),
            notice: None,
            clear_scene: true,
            latest: None,
            latest_scan: None,
            scan_time: None,
            acquired_time: None,
            cleared_acquired_time: None,
            current: None,
            fence: None,
            pending: None,
            lifecycle_enabled: false,
            replay_enabled: false,
            replay: None,
            replay_frames: None,
            catalog: None,
            lifecycle_reply: None,
            pending_edit: None,
            lifecycle_scene: None,
            next_request: Some(1),
            publication_enabled: false,
            publication_last: None,
            publication_pending: None,
            publication_scene: None,
            publication_catalog: false,
        }
    }
}

impl Session {
    pub fn disconnect(&mut self, reason: impl Into<String>) {
        self.connected = false;
        self.status = reason.into();
        if self.pending_control() {
            self.status.push_str("; pending control outcome unknown");
        }
        self.notice = None;
        self.clear_scene = true;
        self.latest = None;
        self.clear_scan();
        self.current = None;
        self.replay_frames = None;
        self.fence = None;
        self.pending = None;
        self.clear_lifecycle();
        self.publication_last = None;
        self.publication_pending = None;
    }

    /// Configure only at the connection boundary, after lifecycle opt-in.
    pub fn enable_publication_credit(&mut self, enabled: bool) -> Result<(), String> {
        if enabled && !self.lifecycle_enabled {
            return Err("Publication credit requires lifecycle opt-in".into());
        }
        self.publication_enabled = enabled;
        self.publication_last = None;
        self.publication_pending = None;
        self.publication_scene = None;
        self.publication_catalog = false;
        Ok(())
    }

    /// Configure at a disconnected boundary after lifecycle and credit opt-in.
    pub fn enable_replay(&mut self, enabled: bool) -> Result<(), String> {
        if self.connected || (enabled && (!self.lifecycle_enabled || !self.publication_enabled)) {
            return Err(
                "Replay requires lifecycle and publication credit at the connection boundary"
                    .into(),
            );
        }
        self.replay_enabled = enabled;
        self.replay = None;
        self.replay_frames = None;
        Ok(())
    }

    /// Echo only after validated inbox flush. This does not consume a command ID.
    pub fn publication_ready(&self) -> Option<String> {
        self.publication_pending
            .filter(|_| self.connected)
            .map(|token| format!(r#"{{"version":1,"kind":"publication_ready","token":{token}}}"#))
    }

    /// Fixed-size diagnostic state: last accepted credit and Ready awaiting
    /// local driver submission. It does not establish remote Ready receipt.
    pub fn publication_tokens(&self) -> (Option<u64>, Option<u64>) {
        (self.publication_last, self.publication_pending)
    }

    /// Call only after the bounded driver accepted that one Ready; failed sends
    /// instead disconnect, with no transport or mutation retry.
    pub fn publication_ready_sent(&mut self) {
        self.publication_pending = None;
    }

    fn clear_scan(&mut self) {
        self.latest_scan = Some(None);
        self.scan_time = None;
        self.acquired_time = None;
        self.cleared_acquired_time = None;
    }

    pub fn enable_lifecycle(&mut self, enabled: bool) {
        self.lifecycle_enabled = enabled;
        self.clear_lifecycle();
    }
    fn clear_lifecycle(&mut self) {
        self.catalog = None;
        self.replay = None;
        self.lifecycle_reply = None;
        self.pending_edit = None;
        self.lifecycle_scene = None;
        // A reset may retire world/catalog state while an older transport
        // credit is already validated. Preserve pending/last until Ready or
        // actual disconnect, independently of epoch and command request IDs.
        self.publication_scene = None;
        self.publication_catalog = false;
    }
    pub fn lifecycle(&mut self, command: lifecycle::Command) -> Result<String, String> {
        if self.replay_enabled {
            return Err("Replay is read-only; lifecycle commands are unavailable".into());
        }
        if !self.lifecycle_enabled || !self.can_control() {
            return Err("Lifecycle requires an admitted scene and no pending command".into());
        }
        let catalog = self
            .catalog
            .as_ref()
            .ok_or("No lifecycle capability/catalog")?;
        let scene = self
            .lifecycle_scene
            .as_ref()
            .ok_or("No committed lifecycle scene")?;
        if !catalog.matches_scene(scene) {
            return Err("Waiting for current lifecycle catalog".into());
        }
        command.validate(catalog)?;
        #[derive(Serialize)]
        struct Request<'a> {
            version: u32,
            kind: &'static str,
            request_id: u64,
            world_id: u64,
            epoch: u64,
            #[serde(flatten)]
            command: &'a lifecycle::Command,
        }
        let id = self.next_request.ok_or("Request IDs exhausted")?;
        let text = serde_json::to_string(&Request {
            version: 1,
            kind: "lifecycle",
            request_id: id,
            world_id: catalog.world_id,
            epoch: catalog.epoch,
            command: &command,
        })
        .map_err(|e| e.to_string())?;
        if text.len() > 4096 {
            return Err("Lifecycle request exceeds 4 KiB".into());
        }
        self.next_request = id.checked_add(1);
        self.pending_edit = Some((id, command));
        self.lifecycle_reply = None;
        self.notice = None;
        self.status = format!("Waiting for lifecycle acknowledgement #{id}");
        Ok(text)
    }

    pub fn opened(&mut self) {
        self.connected = true;
        self.status = "Connected; waiting for a complete scene".into();
    }

    pub fn pending_control(&self) -> bool {
        self.pending.is_some() || self.pending_edit.is_some()
    }

    pub fn can_control(&self) -> bool {
        self.connected
            && self.current.is_some()
            && !self.pending_control()
            && self.next_request.is_some()
            && (!self.replay_enabled || self.replay.is_some())
    }

    pub fn clock(&self) -> Option<(Stamp, bool)> {
        self.fence
            .or_else(|| self.current.map(|c| (c.stamp, c.paused)))
    }

    pub fn paused(&self) -> Option<bool> {
        self.fence
            .map(|(_, paused)| paused)
            .or_else(|| self.current.map(|c| c.paused))
    }

    pub fn control(&mut self, action: Action) -> Result<String, String> {
        if !self.can_control() {
            return Err("A connected scene and no pending control are required".into());
        }
        if self.replay_enabled
            && action == Action::Resume
            && self.replay.as_ref().is_some_and(|r| r.finished)
        {
            return Err("Playback finished; Reset before Play".into());
        }
        let id = self
            .next_request
            .ok_or("Request IDs exhausted; reload the viewer")?;
        let text = serde_json::to_string(&Control {
            version: 1,
            kind: "control",
            request_id: id,
            action,
        })
        .map_err(|e| e.to_string())?;
        self.notice = None;
        self.pending = Some(Pending {
            id,
            action,
            stamp: self.clock().ok_or("No committed scene")?.0,
        });
        self.next_request = id.checked_add(1);
        self.status = format!("Waiting for server acknowledgement: {action:?} #{id}");
        Ok(text)
    }

    pub fn receive(&mut self, message: ServerMessage) -> Result<(), String> {
        if !self.connected {
            return Err("Message arrived without an open connection".into());
        }
        match message {
            ServerMessage::Scene(scene) => {
                scene.validate()?;
                if let Some(current) = self.current {
                    if current.world_id == scene.world_id {
                        if scene.epoch < current.stamp.epoch
                            || (scene.epoch == current.stamp.epoch
                                && scene.sequence <= current.sequence)
                        {
                            return Ok(());
                        }
                        if scene.stamp() < current.stamp {
                            return Err("Scene clock moved backwards within its epoch".into());
                        }
                    } else {
                        if self.pending_control() {
                            return Err("World changed while a control was pending".into());
                        }
                        self.clear_scene = true;
                        self.fence = None;
                    }
                }
                if let Some((stamp, paused)) = self.fence {
                    if scene.stamp() < stamp || (scene.stamp() == stamp && scene.paused != paused) {
                        return Ok(());
                    }
                    self.fence = None;
                }
                if self.current.is_some_and(|current| {
                    current.world_id != scene.world_id || current.stamp.epoch != scene.epoch
                }) {
                    self.clear_scan();
                }
                self.replay = None;
                self.current = Some(Current {
                    world_id: scene.world_id,
                    stamp: scene.stamp(),
                    sequence: scene.sequence,
                    paused: scene.paused,
                });
                if self.pending.is_none() {
                    self.status = if scene.paused {
                        "Connected; server paused"
                    } else {
                        "Connected; server running"
                    }
                    .into();
                }
                if self.current.is_some_and(|c| {
                    self.catalog
                        .as_ref()
                        .is_some_and(|v| v.world_id != c.world_id || v.epoch != c.stamp.epoch)
                }) {
                    self.clear_lifecycle();
                }
                if self.lifecycle_enabled {
                    self.lifecycle_scene = Some(scene.clone());
                }
                if self.publication_enabled {
                    self.publication_scene = Some((scene.world_id, scene.epoch, scene.sequence));
                    self.publication_catalog = false;
                }
                self.latest = Some(scene);
            }
            ServerMessage::PublicationCredit(credit) => {
                credit.validate()?;
                if !self.publication_enabled
                    || !self.lifecycle_enabled
                    || self.publication_pending.is_some()
                    || self
                        .publication_last
                        .is_some_and(|last| credit.token <= last)
                    || self.publication_scene.is_none()
                    || !self.publication_catalog
                    || (self.replay_enabled && self.replay.is_none())
                {
                    return Err("Unexpected, stale or incomplete publication credit".into());
                }
                self.publication_pending = Some(credit.token);
                self.publication_last = Some(credit.token);
                self.publication_scene = None;
                self.publication_catalog = false;
            }
            ServerMessage::LifecycleCatalog(catalog) => {
                if !self.lifecycle_enabled {
                    return Err("Lifecycle payload without opt-in".into());
                }
                catalog.validate()?;
                if self.replay_enabled
                    && (!catalog.templates.is_empty()
                        || catalog
                            .models
                            .iter()
                            .any(|m| m.can_remove || m.can_set_state))
                {
                    return Err("Replay catalog must be read-only".into());
                }
                let scene = self
                    .lifecycle_scene
                    .as_ref()
                    .ok_or("Catalog before committed scene")?;
                if catalog.world_id != scene.world_id
                    || catalog.epoch < scene.epoch
                    || catalog.scene_sequence < scene.sequence
                {
                    return Ok(());
                }
                if !catalog.matches_scene(scene) {
                    return Err("Catalog does not match committed Scene".into());
                }
                if self.publication_enabled
                    && self.publication_scene
                        == Some((catalog.world_id, catalog.epoch, catalog.scene_sequence))
                {
                    self.publication_catalog = true;
                }
                self.catalog = Some(catalog);
            }
            ServerMessage::ReplayStatus(status) => {
                status.validate()?;
                if !self.replay_enabled {
                    return Err("Replay status without explicit opt-in".into());
                }
                let scene = self
                    .lifecycle_scene
                    .as_ref()
                    .ok_or("Replay status before Scene")?;
                if !status.matches_scene(scene)
                    || self
                        .catalog
                        .as_ref()
                        .is_none_or(|catalog| !catalog.matches_scene(scene))
                    || self.replay.is_some()
                    || self
                        .replay_frames
                        .is_some_and(|frames| frames != status.frames)
                {
                    return Err("Replay status does not match the committed Scene/catalog".into());
                }
                self.replay_frames = Some(status.frames);
                self.replay = Some(status);
            }
            ServerMessage::LifecycleAck(reply) => {
                if !self.lifecycle_enabled {
                    return Err("Lifecycle reply without opt-in".into());
                }
                reply.validate()?;
                let (id, command) = self
                    .pending_edit
                    .as_ref()
                    .ok_or("Unsolicited lifecycle reply")?;
                let scene = self
                    .lifecycle_scene
                    .as_ref()
                    .ok_or("Lifecycle reply before Scene")?;
                if reply.request_id != *id
                    || reply.action != command.action()
                    || reply.world_id != scene.world_id
                    || reply.epoch != scene.epoch
                    || reply.scene_sequence != scene.sequence
                    || reply.seconds != scene.seconds
                    || reply.nanoseconds != scene.nanoseconds
                    || reply.paused != scene.paused
                {
                    return Err("Lifecycle reply does not match request and committed Scene".into());
                }
                if reply.ok {
                    if reply.model_id.is_none()
                        || reply
                            .model_id
                            .is_some_and(|id| scene.visuals.iter().any(|visual| visual.id == id))
                        || command
                            .model()
                            .is_some_and(|model| Some(model) != reply.model_id)
                        || matches!(
                            reply.action,
                            lifecycle::Action::GetState | lifecycle::Action::SetState
                        ) != reply.state.is_some()
                    {
                        return Err("Lifecycle result has contradictory identity/state".into());
                    }
                } else if reply.model_id.is_some() || reply.state.is_some() {
                    return Err("Rejected lifecycle result contains state".into());
                }
                self.pending_edit = None;
                self.notice = Some(if reply.ok {
                    format!(
                        "Server acknowledged {:?} #{}: epoch {}, {}.{:09}s; sequence {}",
                        reply.action,
                        reply.request_id,
                        reply.epoch,
                        reply.seconds,
                        reply.nanoseconds,
                        reply.scene_sequence
                    )
                } else {
                    format!(
                        "Server refused lifecycle: {}",
                        reply.error.as_deref().unwrap_or("refused")
                    )
                });
                self.lifecycle_reply = Some(reply);
            }
            ServerMessage::Scan(frame) => {
                if self.replay_enabled {
                    return Err("Sensor scans are outside replay admission".into());
                }
                frame.validate()?;
                let Some(current) = self.current else {
                    return Ok(());
                };
                if u64::from(frame.world_id) != current.world_id
                    || frame.epoch != self.clock().expect("admitted scene has a clock").0.epoch
                    || self.scan_time.is_some_and(|time| {
                        frame.sim_time_ns < time
                            || (frame.sim_time_ns == time && frame.scan.is_some())
                    })
                    || frame.scan.as_ref().is_some_and(|scan| {
                        self.acquired_time
                            .is_some_and(|time| scan.acquired_ns < time)
                            || self
                                .cleared_acquired_time
                                .is_some_and(|time| scan.acquired_ns <= time)
                    })
                {
                    return Ok(());
                }
                self.scan_time = Some(frame.sim_time_ns);
                if let Some(scan) = &frame.scan {
                    self.acquired_time = Some(scan.acquired_ns);
                } else {
                    self.cleared_acquired_time = Some(frame.sim_time_ns);
                }
                self.latest_scan = Some(if frame.scan.is_some() {
                    Some(frame)
                } else {
                    None
                });
            }
            ServerMessage::Ack(ack) => {
                ack.validate()?;
                if self.replay_enabled && self.replay.is_none() {
                    return Err("Replay acknowledgement before committed replay status".into());
                }
                let pending = self.pending.ok_or("Unsolicited acknowledgement")?;
                if ack.request_id != pending.id {
                    return Err("Acknowledgement request ID does not match".into());
                }
                let stamp = Stamp {
                    epoch: ack.epoch,
                    seconds: ack.seconds,
                    nanoseconds: ack.nanoseconds,
                };
                if stamp < pending.stamp
                    || (ack.ok
                        && match pending.action {
                            Action::Pause => !ack.paused,
                            Action::Resume => ack.paused,
                            Action::Reset => {
                                pending.stamp.epoch.checked_add(1) != Some(ack.epoch)
                                    || !ack.paused
                                    || ack.seconds != 0
                                    || ack.nanoseconds != 0
                            }
                        })
                {
                    return Err("Acknowledgement contradicts the committed command".into());
                }
                self.pending = None;
                if self.current.is_none_or(|c| c.stamp <= stamp) {
                    self.fence = Some((stamp, ack.paused));
                }
                if self.current.is_some_and(|c| stamp.epoch > c.stamp.epoch) {
                    self.clear_scene = true;
                    self.clear_scan();
                    self.clear_lifecycle();
                    self.latest = None;
                    self.current = None;
                }
                self.notice = Some(match ack.error {
                    NullableError::Text(error) => format!("Server rejected control: {error}"),
                    NullableError::None => format!(
                        "Server acknowledged {:?}: epoch {}, {}.{:09}s, paused={}",
                        pending.action, ack.epoch, ack.seconds, ack.nanoseconds, ack.paused
                    ),
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;

impl Scene {
    /// Flat authoritative world poses remain raw Z-up below GzWorldRoot.
    pub fn description(&self) -> Result<crate::scene::SceneDesc, String> {
        self.validate()?;
        use crate::scene::*;
        Ok(SceneDesc {
            name: self.world.clone(),
            ambient: Some([0.7; 3]),
            background: Some([0.12, 0.14, 0.18, 1.0]),
            sky: None,
            fog: None,
            grid: true,
            shadows: false,
            roots: self
                .visuals
                .iter()
                .map(|v| {
                    let mut material = MaterialDesc::absent();
                    material.diffuse = Some(v.color.map(|x| x as f32));
                    material.present = !matches!(v.shape, Shape::Mesh { .. });
                    Ok(Node {
                        name: v.name.clone(),
                        id: v.id as u32,
                        kind: NodeKind::Visual,
                        path: format!("primitive/{}", v.id),
                        pose: Pose {
                            pos: v.translation,
                            quat: v.render_rotation(),
                        },
                        scale: v.render_scale(),
                        visible: true,
                        light: None,
                        sensors: Vec::new(),
                        children: Vec::new(),
                        visual: Some(VisualDesc {
                            proto_type: "primitive".into(),
                            geometry: match &v.shape {
                                Shape::Box { size } => GeometryDesc::Box { size: *size },
                                Shape::Sphere { radius } => {
                                    GeometryDesc::Sphere { radius: *radius }
                                }
                                Shape::Mesh { uri, .. } => GeometryDesc::Mesh {
                                    uri: uri.clone(),
                                    load_uri: local_mesh_asset_uri(uri)?,
                                    submesh: None,
                                    center_submesh: false,
                                },
                            },
                            material,
                            cast_shadows: false,
                            transparency: 0.0,
                        }),
                    })
                })
                .collect::<Result<Vec<_>, String>>()?,
        })
    }
    pub fn same_layout(&self, other: &Self) -> bool {
        self.world_id == other.world_id
            && self.epoch == other.epoch
            && self.visuals.len() == other.visuals.len()
            && self.visuals.iter().zip(&other.visuals).all(|(a, b)| {
                a.id == b.id && a.name == b.name && a.shape == b.shape && a.color == b.color
            })
    }
}

#[cfg(test)]
mod lifecycle_tests;

#[cfg(test)]
mod publication_tests;

#[cfg(test)]
mod replay_tests;
