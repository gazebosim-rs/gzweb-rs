// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
//! Explicit bounded primitive JSON backend. No Gazebo protobuf negotiation.
use crate::backend::{BackendIo, SimBackend, WorldControl};
use crate::connection::{backoff, Connector};
use crate::driver::{self, Driver, DriverEvent};
use crate::GzConfig;
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
        })
    }
    fn closed(&mut self, reason: String, io: &mut BackendIo) {
        self.driver = None;
        self.session.disconnect(reason);
        let reason = self.session.status.clone();
        self.previous = None;
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
    }
    fn unsupported() -> Result<(), SendError> {
        Err(SendError::Unsendable(
            "primitive backend supports only pause, resume and reset",
        ))
    }
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
        self.driver = Some((self.connector)(&self.url));
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
        if self.driver.is_some()
            && (io.now - self.last_message > 10.0
                || self.pending_since.is_some_and(|at| io.now - at > 7.0))
        {
            self.closed(
                "Connection or committed acknowledgement timed out; control outcome unknown".into(),
                io,
            );
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
                            x: v.rotation[0],
                            y: v.rotation[1],
                            z: v.rotation[2],
                            w: v.rotation[3],
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
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
