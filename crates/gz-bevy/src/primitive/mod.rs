// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
//! Explicit bounded primitive JSON backend. No Gazebo protobuf negotiation.
use crate::backend::{BackendIo, SimBackend, WorldControl};
use crate::connection::{backoff, Connector};
use crate::driver::{self, Driver, DriverEvent};
use crate::GzConfig;

pub(crate) mod scan_view;
use gz_core::primitive::{self, Action, Scene, Session};
use gz_core::transport::{SendError, Status};

pub(crate) struct PrimitiveBackend {
    connector: Connector,
    driver: Option<Driver>,
    session: Session,
    previous: Option<Scene>,
    url: String,
    last_message: f64,
    pending_since: Option<f64>,
    /// Only an actual driver send error, never informational connection text.
    pending_send_failure: Option<String>,
    start: bool,
    lifecycle: bool,
}
impl PrimitiveBackend {
    pub fn new(config: &GzConfig) -> Result<Self, String> {
        if config.key.is_some() {
            return Err("primitive backend does not support authentication keys".into());
        }
        #[cfg(not(target_arch = "wasm32"))]
        let connector: Connector = {
            let tls = gz_core::net::tls_config(config.ca_cert.as_deref())?;
            Box::new(move |url| driver::connect_bounded(url, Some(tls.clone())))
        };
        #[cfg(target_arch = "wasm32")]
        let connector: Connector = Box::new(driver::connect_bounded);
        Ok(Self {
            connector,
            driver: None,
            session: Session::default(),
            previous: None,
            url: config.url.clone(),
            last_message: 0.0,
            pending_since: None,
            pending_send_failure: None,
            start: true,
            lifecycle: config.lifecycle,
        })
    }
    fn closed(&mut self, reason: String, io: &mut BackendIo) {
        // Retiring the driver makes this once per close, never once per frame.
        if self.driver.is_some() && reason != "Connecting" {
            bevy::log::warn!("Primitive connection closed: {reason}");
        }
        self.driver = None;
        self.session.disconnect(reason);
        let reason = self.session.status.clone();
        self.previous = None;
        io.status.lifecycle = None;
        io.status.lifecycle_reply = None;
        self.pending_since = None;
        io.status.control_error = Some(reason.clone());
        io.status.status = Status::Error(reason);
        io.status.attempting = false;
        io.status.partial_clock = None;
        io.status.control_pending = false;
        io.status.reconnect_at = io
            .config
            .reconnect
            .then(|| io.now + backoff(io.status.attempts));
        io.status.attempts = io.status.attempts.saturating_add(1);
        io.inbox.prepared_scene = Some(empty_scene());
        io.inbox.poses.clear();
        io.inbox.primitive_scan = Some(None);
    }
    fn unsupported() -> Result<(), SendError> {
        Err(SendError::Unsendable(
            "primitive backend supports only pause, resume and reset",
        ))
    }
}
// Only the primitive backend opts in; default Gazebo URLs are untouched.
fn scan_query(query: &str) -> Result<String, String> {
    let mut flags = query
        .split('&')
        .filter(|part| part.split('=').next() == Some("scan"));
    if let Some(first) = flags.next() {
        return if first == "scan=1" && flags.next().is_none() {
            Ok(query.to_string())
        } else {
            Err("Primitive scan query must contain exactly one scan=1".into())
        };
    }
    Ok(if query.is_empty() {
        "scan=1".into()
    } else {
        format!("{query}&scan=1")
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn scan_url(url: &str) -> Result<String, String> {
    use tungstenite::http::{uri::PathAndQuery, Uri};
    let uri: Uri = url
        .parse()
        .map_err(|e| format!("Invalid primitive URL: {e}"))?;
    let query = scan_query(uri.query().unwrap_or(""))?;
    let path = if uri.path().is_empty() {
        "/"
    } else {
        uri.path()
    };
    let value: PathAndQuery = format!("{path}?{query}")
        .parse()
        .map_err(|e| format!("Invalid primitive query: {e}"))?;
    let mut parts = uri.into_parts();
    parts.path_and_query = Some(value);
    Uri::from_parts(parts)
        .map(|uri| uri.to_string())
        .map_err(|e| format!("Invalid primitive URL: {e}"))
}

#[cfg(target_arch = "wasm32")]
fn scan_url(url: &str) -> Result<String, String> {
    let parsed = web_sys::Url::new(url).map_err(|_| "Invalid primitive URL".to_string())?;
    if !parsed.hash().is_empty() {
        return Err("Primitive URL must not have a fragment".into());
    }
    let search = parsed.search();
    let query = scan_query(search.strip_prefix('?').unwrap_or(&search))?;
    parsed.set_search(&query);
    Ok(parsed.href())
}

// Only explicit lifecycle clients negotiate transport credit. Default primitive
// and Gazebo sockets keep their existing publication protocol.
fn lifecycle_url(mut url: String, enabled: bool) -> Result<String, String> {
    for key in ["lifecycle", "lifecycle_credit"] {
        let fields: Vec<_> = url
            .split_once('?')
            .map_or("", |(_, q)| q)
            .split('&')
            .filter(|part| part.split('=').next() == Some(key))
            .collect();
        if enabled {
            if fields.is_empty() {
                url.push_str(&format!("&{key}=1"));
            } else if fields != [format!("{key}=1")] {
                return Err(format!("Invalid {key} query"));
            }
        } else if !fields.is_empty() {
            return Err("Lifecycle query requires explicit viewer opt-in".into());
        }
    }
    Ok(url)
}

fn empty_scene() -> gz_core::scene::SceneDesc {
    gz_core::scene::SceneDesc {
        name: "default".into(),
        ambient: None,
        background: None,
        sky: None,
        fog: None,
        grid: true,
        shadows: false,
        roots: Vec::new(),
    }
}
impl SimBackend for PrimitiveBackend {
    fn connect(&mut self, url: Option<&str>, io: &mut BackendIo) {
        if let Some(url) = url {
            self.url = url.into();
        }
        self.closed("Connecting".into(), io);
        self.pending_send_failure = None;
        self.session.enable_lifecycle(self.lifecycle);
        if let Err(error) = self.session.enable_publication_credit(self.lifecycle) {
            self.closed(error, io);
            return;
        }
        let scan_url = match scan_url(&self.url) {
            Ok(url) => url,
            Err(error) => {
                self.closed(error, io);
                io.status.reconnect_at = None;
                self.start = false;
                return;
            }
        };
        let scan_url = match lifecycle_url(scan_url, self.lifecycle) {
            Ok(url) => url,
            Err(error) => {
                self.closed(error, io);
                io.status.reconnect_at = None;
                self.start = false;
                return;
            }
        };
        self.driver = Some((self.connector)(&scan_url));
        self.last_message = io.now;
        self.start = false;
        io.status.url = self.url.clone();
        io.status.status = Status::Connecting;
        io.status.attempting = true;
        io.status.reconnect_at = None;
        io.status.control_error = None;
        io.status.sim = None;
        io.status.partial_metrics = true;
        io.status.world_sdf = Some(false);
        io.status.edits = Some(false);
        io.status.publishes = Some(false);
    }
    fn poll(&mut self, io: &mut BackendIo) {
        if self.start || io.status.reconnect_at.is_some_and(|at| io.now >= at) {
            self.connect(None, io);
        }
        let events: Vec<_> = self
            .driver
            .as_ref()
            .map(|d| d.bounded_events())
            .unwrap_or_default();
        for event in events {
            match event {
                DriverEvent::Open => {
                    self.session.opened();
                    io.status.status = Status::Connected;
                }
                DriverEvent::Message(bytes) => {
                    self.last_message = io.now;
                    let result = std::str::from_utf8(&bytes)
                        .map_err(|e| e.to_string())
                        .and_then(primitive::parse)
                        .and_then(|m| self.session.receive(m));
                    if let Err(error) = result {
                        io.status.malformed = io.status.malformed.saturating_add(1);
                        self.closed(error, io);
                        break;
                    }
                }
                DriverEvent::Closed(reason) => {
                    self.closed(reason.unwrap_or_else(|| "Connection closed".into()), io);
                    break;
                }
                DriverEvent::Rejected(reason) => {
                    self.closed(reason, io);
                    io.status.reconnect_at = None;
                    break;
                }
            }
        }
        // An ACK/reply in this poll may have completed the command even after
        // a slow frame. Clear its old deadline before testing it, and before
        // the pump can queue a new command ahead of this frame's flush.
        if !self.session.pending_control() {
            self.pending_since = None;
        }
        if self.driver.is_some() {
            let silence = io.now - self.last_message;
            let (last_credit, pending_ready) = self.session.publication_tokens();
            let reason = if let Some(elapsed) = self
                .pending_since
                .filter(|_| self.session.pending_control())
                .map(|at| io.now - at)
                .filter(|elapsed| *elapsed > 7.0)
            {
                Some(format!(
                    "Committed acknowledgement timed out after {elapsed:.3}s (limit 7s); control outcome unknown; last_credit={last_credit:?}, pending_ready={pending_ready:?}"
                ))
            } else if silence > 10.0 {
                let phase = if !self.session.connected {
                    "Connection opening"
                } else if self.session.clock().is_none() {
                    "Initial scene"
                } else {
                    "Connection idle"
                };
                let command = if self.session.pending_control() {
                    "pending control outcome unknown"
                } else {
                    "no control request pending"
                };
                Some(format!(
                    "{phase} timed out after {silence:.3}s without a server message (limit 10s); {command}; last_credit={last_credit:?}, pending_ready={pending_ready:?}"
                ))
            } else {
                None
            };
            if let Some(reason) = reason {
                self.closed(reason, io);
            }
        }
    }
    fn subscribe(&mut self, _: &str) -> Result<(), SendError> {
        Self::unsupported()
    }
    fn unsubscribe(&mut self, _: &str) -> Result<(), SendError> {
        Self::unsupported()
    }
    fn throttle(&mut self, _: &str, _: u32) -> Result<(), SendError> {
        Self::unsupported()
    }
    fn list_topics(&mut self) -> Result<(), SendError> {
        Self::unsupported()
    }
    fn publish(&mut self, _: &str, _: &str, _: &[u8]) -> Result<(), SendError> {
        Self::unsupported()
    }
    fn request(&mut self, _: &str, _: &str, _: &[u8]) -> Result<(), SendError> {
        Self::unsupported()
    }
    fn lifecycle(&mut self, command: primitive::lifecycle::Command) -> Result<(), SendError> {
        let text = self.session.lifecycle(command).map_err(|error| {
            self.session.notice = Some(error);
            SendError::Unsendable("lifecycle request refused")
        })?;
        if self
            .driver
            .as_ref()
            .ok_or(SendError::NotConnected)?
            .try_send(text.into_bytes())
            .is_err()
        {
            self.pending_send_failure = Some("Lifecycle send failed; outcome unknown".into());
            return Err(SendError::Failed);
        }
        self.pending_since = None;
        Ok(())
    }
    fn world_control(&mut self, cmd: WorldControl) -> Result<(), SendError> {
        let action = match cmd {
            WorldControl::Play => Action::Resume,
            WorldControl::Pause => Action::Pause,
            WorldControl::Reset => Action::Reset,
            _ => return Self::unsupported(),
        };
        let text = self.session.control(action).map_err(|_| {
            SendError::Unsendable("waiting for a scene or committed acknowledgement")
        })?;
        if self
            .driver
            .as_ref()
            .ok_or(SendError::NotConnected)?
            .try_send(text.into_bytes())
            .is_err()
        {
            self.pending_send_failure = Some("Control send failed; outcome unknown".into());
            return Err(SendError::Failed);
        }
        Ok(())
    }
    fn flush(&mut self, io: &mut BackendIo) {
        if let Some(reason) = self.pending_send_failure.take() {
            self.closed(reason, io);
        }
        if self.session.clear_scene {
            io.inbox.prepared_scene = Some(empty_scene());
            io.inbox.poses.clear();
            self.previous = None;
            self.session.clear_scene = false;
        }
        if let Some(scene) = self.session.latest.take() {
            if self
                .previous
                .as_ref()
                .is_some_and(|old| old.same_layout(&scene))
            {
                io.inbox.poses = scene
                    .visuals
                    .iter()
                    .map(|v| gz_core::msgs::Pose {
                        id: v.id as u32,
                        name: v.name.clone(),
                        position: Some(gz_core::msgs::Vector3d {
                            x: v.translation[0],
                            y: v.translation[1],
                            z: v.translation[2],
                            ..Default::default()
                        }),
                        orientation: Some(gz_core::msgs::Quaternion {
                            x: v.render_rotation()[0],
                            y: v.render_rotation()[1],
                            z: v.render_rotation()[2],
                            w: v.render_rotation()[3],
                            ..Default::default()
                        }),
                        ..Default::default()
                    })
                    .collect();
            } else {
                match scene.description() {
                    Ok(desc) => io.inbox.prepared_scene = Some(desc),
                    Err(error) => {
                        self.closed(error, io);
                        return;
                    }
                }
                io.inbox.poses.clear();
            }
            io.inbox.last_activity_secs = io.now;
            io.status.world = Some(scene.world.clone());
            io.status.status = Status::Ready;
            io.status.attempts = 0;
            self.previous = Some(scene);
        }
        if io.status.lifecycle != self.session.catalog {
            io.status.lifecycle = self.session.catalog.clone();
        }
        if io.status.lifecycle_reply != self.session.lifecycle_reply {
            io.status.lifecycle_reply = self.session.lifecycle_reply.clone();
        }
        if let Some(scan) = self.session.latest_scan.take() {
            io.inbox.primitive_scan = Some(scan);
        }
        io.status.partial_clock = self.session.clock();
        let pending =
            self.session.connected && !self.session.can_control() && self.session.clock().is_some();
        if self.session.pending_control() && self.pending_since.is_none() {
            self.pending_since = Some(io.now);
        }
        if !self.session.pending_control() {
            self.pending_since = None;
        }
        io.status.control_pending = pending;
        if let Some(notice) = self.session.notice.take() {
            io.status.control_error = Some(notice);
        }
        // Credit is released only after all validated scene/catalog/scan data
        // reached the coalesced inbox, including any priority reset/commit above.
        if let Some(ready) = self.session.publication_ready() {
            if self
                .driver
                .as_ref()
                .is_none_or(|driver| driver.try_send(ready.into_bytes()).is_err())
            {
                self.closed(
                    "Publication Ready send failed; connection closed without retry".into(),
                    io,
                );
            } else {
                self.session.publication_ready_sent();
            }
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod scan_backend_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod lifecycle_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod publication_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod timeout_tests;
