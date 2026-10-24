//! The message API to the simulation and its websocket backend: [`pump`]
//! runs the app's [`Backend`] with
//! [`GzCommand`] and [`GzReconnect`] in, [`GzStatus`], [`GzInbox`],
//! [`GzPublish`] and [`GzServiceReply`] out (spec §9.4, DECISIONS.md
//! D-65). [`WebsocketBackend`] is the Gazebo websocket server's: driver
//! bytes → [`Transport`] → those resources, with reconnecting (with
//! backoff, unless the key or URL was rejected).

use std::collections::HashSet;

use bevy::prelude::*;
use gz_core::control;
use gz_core::msgs;
use gz_core::topic::world_topic;
use gz_core::transport::{
    probe_reply_supports_binary, topics_show_publishing, Event, SendError, Status, Transport,
    BINARY_PROBE_SERVICE,
};
use prost::Message as _;

use crate::assets::WsAssetStore;
use crate::backend::{Backend, BackendIo, SimBackend, WorldControl};
use crate::driver::{self, Driver, DriverEvent};
use crate::topics::{GzTopics, GzTraffic};
use crate::GzConfig;

/// Opens a new driver to a server URL.
type Connector = Box<dyn Fn(&str) -> Driver>;

/// The Gazebo websocket server as the app's [`SimBackend`]: the socket
/// ([`Driver`]), the protocol ([`Transport`]), reconnecting, and the
/// `asset` requests of the `ws://` asset source.
pub struct WebsocketBackend {
    transport: Transport,
    /// `None` after a handshake timeout dropped it.
    driver: Option<Driver>,
    connector: Connector,
    /// The server it connects to (changed by [`GzReconnect`]).
    url: String,
    /// Why retrying would fail the same way (rejected key or URL).
    fatal: Option<String>,
    /// When the socket opened, for the handshake timeout.
    opened_at: Option<f64>,
    /// The last non-zero step size this server reported (Gazebo reports 0
    /// while paused).
    last_step: Option<f64>,
    /// The `ws://` asset source's requests and replies.
    store: WsAssetStore,
    /// The world topics the client subscribed for itself on this
    /// connection ([`subscribe`]).
    own: HashSet<String>,
    /// Those of them the app subscribed too: their messages also go out
    /// as [`GzPublish`] (DECISIONS.md D-97).
    taps: HashSet<String>,
    /// Topic lists still to read for the publish probe (D-104).
    publish_probes: u32,
}

/// How many topic lists the publish probe reads before it gives up.
const PUBLISH_PROBES: u32 = 3;

/// Simulation state from `/world/<world>/stats` (interactive mode only).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SimStats {
    pub paused: bool,
    pub sim_time: f64,
    /// Seconds of real time the simulation has run.
    pub real_time: f64,
    pub real_time_factor: f64,
    pub iterations: u64,
    /// Seconds per iteration. Gazebo reports 0 while paused; this keeps
    /// the last non-zero step the server reported, or for a world paused
    /// since before the connection, sim time over iterations (0 before the
    /// first iteration), so a paused world still shows its step.
    pub step_size: f64,
}

/// Connection state visible to the rest of the app.
#[derive(Resource, Debug)]
pub struct GzStatus {
    pub status: Status,
    pub url: String,
    pub world: Option<String>,
    /// Frames that could not be parsed or decoded.
    pub malformed: u32,
    /// Whether the server publishes what the client sends (`pub_in`,
    /// DECISIONS.md D-104): `None` until its probe is answered.
    pub publishes: Option<bool>,
    /// When the next reconnect attempt is due (`Time<Real>` seconds).
    pub reconnect_at: Option<f64>,
    /// Reconnect attempts since the last time the scene was ready.
    pub attempts: u32,
    pub sim: Option<SimStats>,
    /// A socket is open or opening (false once it closes, until the next
    /// attempt).
    pub attempting: bool,
    /// Whether the server takes edits (keeps binary requests whole,
    /// DECISIONS.md D-76): `None` until its probe is answered.
    pub edits: Option<bool>,
}

impl GzStatus {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            status: Status::Disconnected,
            url: url.into(),
            world: None,
            malformed: 0,
            reconnect_at: None,
            attempts: 0,
            sim: None,
            attempting: false,
            edits: None,
            publishes: None,
        }
    }
}

/// Requests for the server. A command the connection can't send right now
/// (not connected yet, or a frame the Gazebo websocket server can't carry)
/// is dropped and
/// logged, not queued.
#[derive(Message, Debug, Clone, PartialEq)]
pub enum GzCommand {
    Play,
    Pause,
    /// Step a paused simulation by this many iterations.
    Step(u32),
    /// Reset the world and leave it paused.
    Reset,
    /// Stop the simulation server.
    Stop,
    Subscribe(String),
    Unsubscribe(String),
    /// Limit a subscribed topic to this many messages per second.
    Throttle(String, u32),
    /// Publish a serialized message (refused by the websocket server,
    /// which takes none).
    Publish {
        topic: String,
        msg_type: String,
        payload: Vec<u8>,
    },
    /// Call a service with a serialized request.
    Request {
        service: String,
        msg_type: String,
        payload: Vec<u8>,
    },
    /// Ask for the server's topics again ([`GzTopics`]).
    RefreshTopics,
}

/// Iterations one step request takes: the top bar's Step button and the
/// `sim.step` key (⌘→) both use it, so the button's tooltip tells the
/// truth about its shortcut (spec §3.1 "Steps per click"). The full UI's
/// steps menu sets it; minimal mode steps 1.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepsPerClick(pub u32);

impl Default for StepsPerClick {
    fn default() -> Self {
        StepsPerClick(1)
    }
}

/// A message on a topic subscribed with [`GzCommand::Subscribe`]; the payload
/// is still serialized (PNG data for image topics).
#[derive(Message, Debug, Clone, PartialEq)]
pub struct GzPublish {
    pub topic: String,
    pub msg_type: String,
    pub payload: Vec<u8>,
}

/// Connect again now: to `url`, or to the current server when `None` (the
/// UI's "Retry now" and "Connect"). Cancels a pending backoff and clears a
/// rejection, so a corrected key or URL can be tried.
#[derive(Message, Debug, Clone, PartialEq, Eq, Default)]
pub struct GzReconnect {
    pub url: Option<String>,
}

/// The reply to a service call (including sim control).
#[derive(Message, Debug, Clone, PartialEq)]
pub struct GzServiceReply {
    pub service: String,
    pub msg_type: String,
    pub payload: Vec<u8>,
}

/// Decoded messages waiting to be applied to the ECS world.
#[derive(Resource, Default)]
pub struct GzInbox {
    /// The initial scene (reply to `scene,<world>`). A second one, after a
    /// reconnect, replaces everything.
    pub scene: Option<Box<msgs::Scene>>,
    /// `scene/info` updates, in arrival order.
    pub scene_updates: Vec<msgs::Scene>,
    /// Entity ids from `scene/deletion`, in arrival order.
    pub deletions: Vec<u32>,
    /// `dynamic_pose/info` poses, in arrival order.
    pub poses: Vec<msgs::Pose>,
    /// Real time of the last message that changes the scene; used to decide
    /// when the scene has settled.
    pub last_activity_secs: f64,
}

/// Seconds to wait before reconnect attempt `attempt` (0-based).
pub fn backoff(attempt: u32) -> f64 {
    (0.5 * 2f64.powi(attempt.min(5) as i32)).min(10.0)
}

/// Stats are throttled to this many updates per second.
const STATS_RATE: u32 = 5;

/// Seconds from the socket opening to the proto definitions arriving. A
/// gz-launch server without a key never answers `auth`, so this is the only
/// way out there (Jetty's answers `authorized`).
pub const HANDSHAKE_TIMEOUT: f64 = 10.0;

/// Starts the app's [`Backend`]: the websocket server at `GzConfig::url`.
pub fn connect(world: &mut World) {
    let config = world.resource::<GzConfig>().clone();
    info!("Connecting to {}", config.url);
    let mut status = GzStatus::new(config.url.clone());
    match connector(&config) {
        Ok(connector) => {
            status.attempting = true;
            let store = world
                .get_resource::<WsAssetStore>()
                .cloned()
                .unwrap_or_default();
            let backend = WebsocketBackend::new(&config, connector, store);
            world.insert_non_send(Backend(Box::new(backend)));
        }
        Err(e) => {
            error!("{e}");
            status.status = Status::Error(e);
        }
    }
    world.insert_resource(status);
}

#[cfg(not(target_arch = "wasm32"))]
fn connector(config: &GzConfig) -> Result<Connector, String> {
    // Built once, not on every reconnect.
    let tls = gz_core::net::tls_config(config.ca_cert.as_deref())?;
    Ok(Box::new(move |url| driver::connect(url, Some(tls.clone()))))
}

#[cfg(target_arch = "wasm32")]
fn connector(_config: &GzConfig) -> Result<Connector, String> {
    Ok(Box::new(driver::connect))
}

/// Runs the app's [`Backend`] for a frame: the reconnects asked for, what
/// arrived, the commands, then what to deliver and send.
#[allow(clippy::too_many_arguments)]
pub fn pump(
    backend: Option<NonSendMut<Backend>>,
    config: Res<GzConfig>,
    time: Res<Time<Real>>,
    mut commands: MessageReader<GzCommand>,
    mut reconnects: MessageReader<GzReconnect>,
    mut publishes: MessageWriter<GzPublish>,
    mut replies: MessageWriter<GzServiceReply>,
    mut status: ResMut<GzStatus>,
    mut inbox: ResMut<GzInbox>,
    topics: Option<ResMut<GzTopics>>,
    traffic: Option<ResMut<GzTraffic>>,
) {
    let Some(mut backend) = backend else { return };
    let backend = &mut *backend.bypass_change_detection().0;
    let mut io = BackendIo {
        now: time.elapsed_secs_f64(),
        config: &config,
        status: status.reborrow(),
        inbox: inbox.reborrow(),
        publishes: Vec::new(),
        replies: Vec::new(),
        topics: None,
        arrivals: Vec::new(),
    };
    for r in reconnects.read() {
        backend.connect(r.url.as_deref(), &mut io);
    }
    backend.poll(&mut io);
    for cmd in commands.read() {
        if let Err(e) = send_command(backend, cmd) {
            warn!("{cmd:?} not sent: {e}");
        }
    }
    backend.flush(&mut io);
    publishes.write_batch(io.publishes);
    replies.write_batch(io.replies);
    if let (Some(list), Some(mut topics)) = (io.topics, topics) {
        topics.set(list);
    }
    if let Some(mut traffic) = traffic.filter(|_| !io.arrivals.is_empty()) {
        for topic in &io.arrivals {
            traffic.arrived(topic, io.now);
        }
    }
}

/// Sends `cmd` through `backend`.
fn send_command(backend: &mut dyn SimBackend, cmd: &GzCommand) -> Result<(), SendError> {
    match cmd {
        GzCommand::Play => backend.world_control(WorldControl::Play),
        GzCommand::Pause => backend.world_control(WorldControl::Pause),
        GzCommand::Step(n) => backend.world_control(WorldControl::Step(*n)),
        GzCommand::Reset => backend.world_control(WorldControl::Reset),
        GzCommand::Stop => backend.world_control(WorldControl::Stop),
        GzCommand::Subscribe(topic) => backend.subscribe(topic),
        GzCommand::Unsubscribe(topic) => backend.unsubscribe(topic),
        GzCommand::Throttle(topic, rate) => backend.throttle(topic, *rate),
        GzCommand::Publish {
            topic,
            msg_type,
            payload,
        } => backend.publish(topic, msg_type, payload),
        GzCommand::Request {
            service,
            msg_type,
            payload,
        } => backend.request(service, msg_type, payload),
        GzCommand::RefreshTopics => backend.list_topics(),
    }
}

impl WebsocketBackend {
    fn new(config: &GzConfig, connector: Connector, store: WsAssetStore) -> Self {
        Self {
            transport: Transport::new(config.key.clone()),
            driver: Some(connector(&config.url)),
            connector,
            url: config.url.clone(),
            fatal: None,
            opened_at: None,
            last_step: None,
            store,
            own: HashSet::new(),
            taps: HashSet::new(),
            publish_probes: 0,
        }
    }

    /// The connection closed (or never opened): fail the waiting asset
    /// readers and schedule a retry unless retrying can't help.
    fn closed(&mut self, err: Option<String>, io: &mut BackendIo) {
        self.opened_at = None;
        // Keep the reason that makes retrying pointless.
        let err = self.fatal.clone().or(err);
        if let Some(e) = &err {
            error!("websocket closed: {e}");
        }
        self.transport.on_close(err);
        self.own.clear();
        self.taps.clear();
        self.store.fail_all("connection closed");
        io.status.sim = None;
        io.status.attempting = false;
        if io.config.reconnect && self.fatal.is_none() {
            let wait = backoff(io.status.attempts);
            info!("Reconnecting in {wait:.1} s");
            io.status.reconnect_at = Some(io.now + wait);
        }
    }

    /// Hands one transport event to the app.
    fn deliver(&mut self, event: Event, world: &str, io: &mut BackendIo) {
        let now = io.now;
        match event {
            Event::Status(s) => {
                info!("gz status: {s:?}");
                if s == Status::Ready {
                    io.status.attempts = 0;
                    self.own = subscribe(&mut self.transport, world, io.config.interactive());
                    if io.config.interactive() {
                        if let Err(e) = self.transport.probe_binary_requests() {
                            warn!("edit probe: {e}");
                        }
                        match self.transport.probe_publishing() {
                            Ok(()) => self.publish_probes = PUBLISH_PROBES,
                            Err(e) => warn!("publish probe: {e}"),
                        }
                    }
                } else {
                    if io.status.edits.is_some() {
                        io.status.edits = None;
                    }
                    if io.status.publishes.is_some() {
                        io.status.publishes = None;
                    }
                    self.publish_probes = 0;
                }
                io.status.status = s;
            }
            Event::AuthFailed => {
                error!("websocket auth key rejected");
                self.fatal = Some("invalid auth key".into());
            }
            Event::Worlds(worlds) => {
                info!("worlds: {worlds:?}");
                io.status.world = worlds.first().cloned();
            }
            Event::TopicsTypes(pairs) => {
                // The probe's topic shows once the server has advertised it;
                // a list without it is asked for again, a few times.
                if self.publish_probes > 0 && io.status.publishes.is_none() {
                    let publishes = topics_show_publishing(&pairs);
                    self.publish_probes -= 1;
                    if publishes || self.publish_probes == 0 {
                        self.transport.set_publishing(publishes);
                        io.status.publishes = Some(publishes);
                        if publishes {
                            info!("the server publishes for the client");
                        } else {
                            info!("the server doesn't publish for the client");
                        }
                    } else if let Err(e) = self.transport.request_topics_types() {
                        warn!("publish probe: {e}");
                    }
                }
                io.topics = Some(pairs);
            }
            Event::Topics(topics) => info!("topics: {topics:?}"),
            Event::Scene(scene) => {
                info!(
                    "scene '{}': {} models, {} lights",
                    scene.name,
                    scene.model.len(),
                    scene.light.len()
                );
                let inbox = &mut *io.inbox;
                inbox.scene = Some(scene);
                inbox.scene_updates.clear();
                inbox.deletions.clear();
                inbox.poses.clear();
                inbox.last_activity_secs = now;
            }
            Event::Publish {
                topic,
                msg_type,
                payload,
            } => self.deliver_topic(topic, msg_type, payload, world, io),
            Event::ServiceReply {
                service,
                msg_type,
                payload,
            } => {
                if service == BINARY_PROBE_SERVICE && io.status.edits.is_none() {
                    let edits = probe_reply_supports_binary(&msg_type, &payload);
                    self.transport.set_binary_requests(edits);
                    io.status.edits = Some(edits);
                    if edits {
                        info!("the server takes edits (binary requests)");
                    } else {
                        info!("the server mangles binary requests: editing is off");
                    }
                }
                log_reply(&service, &msg_type, &payload);
                io.replies.push(GzServiceReply {
                    service,
                    msg_type,
                    payload,
                });
            }
            Event::Asset { uri, result } => {
                self.store.deliver(&uri, result.map_err(|e| e.to_string()));
                io.inbox.last_activity_secs = now;
            }
            Event::Malformed(why) => {
                warn!("malformed frame: {why}");
                io.status.malformed += 1;
            }
        }
    }

    /// A topic message: the world topics the client consumes itself go to
    /// the inbox and the status, the rest out as [`GzPublish`].
    fn deliver_topic(
        &mut self,
        topic: String,
        msg_type: String,
        payload: Vec<u8>,
        world: &str,
        io: &mut BackendIo,
    ) {
        let now = io.now;
        io.arrivals.push(topic.clone());
        if self.taps.contains(&topic) {
            io.publishes.push(GzPublish {
                topic: topic.clone(),
                msg_type: msg_type.clone(),
                payload: payload.clone(),
            });
        }
        match WorldTopic::of(&topic, world) {
            Some(WorldTopic::Poses) => match msgs::PoseV::decode(payload.as_slice()) {
                Ok(v) => io.inbox.poses.extend(v.pose),
                Err(e) => warn!("bad Pose_V on {topic}: {e}"),
            },
            Some(WorldTopic::SceneInfo) => {
                match msgs::Scene::decode(payload.as_slice()) {
                    Ok(s) => io.inbox.scene_updates.push(s),
                    Err(e) => warn!("bad Scene on {topic}: {e}"),
                }
                // Pose streams never stop on a live simulation, so only
                // structural changes count as activity for settling.
                io.inbox.last_activity_secs = now;
            }
            Some(WorldTopic::Deletion) => {
                match msgs::UInt32V::decode(payload.as_slice()) {
                    Ok(v) => io.inbox.deletions.extend(v.data),
                    Err(e) => warn!("bad UInt32_V on {topic}: {e}"),
                }
                io.inbox.last_activity_secs = now;
            }
            Some(WorldTopic::Stats) => match msgs::WorldStatistics::decode(payload.as_slice()) {
                Ok(s) => {
                    let mut stats = sim_stats(&s);
                    if stats.step_size > 0.0 {
                        self.last_step = Some(stats.step_size);
                    } else {
                        stats.step_size = self.last_step.unwrap_or_else(|| mean_step(&stats));
                    }
                    io.status.sim = Some(stats);
                }
                Err(e) => warn!("bad WorldStatistics on {topic}: {e}"),
            },
            None => {
                io.publishes.push(GzPublish {
                    topic,
                    msg_type,
                    payload,
                });
            }
        }
    }
}

impl SimBackend for WebsocketBackend {
    fn connect(&mut self, url: Option<&str>, io: &mut BackendIo) {
        match url {
            Some(url) if url != self.url => {
                self.url = url.to_string();
                io.status.url = url.to_string();
                io.status.attempts = 0;
                // Another server: its world and step are not this one's.
                io.status.world = None;
                self.last_step = None;
            }
            _ => io.status.attempts += 1,
        }
        // Dropping the old driver closes its socket; its close isn't
        // reported, so the transport is closed here.
        self.driver = None;
        self.fatal = None;
        self.opened_at = None;
        self.transport.on_close(None);
        self.store.fail_all("reconnecting");
        io.status.sim = None;
        io.status.reconnect_at = None;
        info!("Connecting to {}", self.url);
        self.driver = Some((self.connector)(&self.url));
        io.status.attempting = true;
    }

    fn poll(&mut self, io: &mut BackendIo) {
        let now = io.now;
        if io.status.reconnect_at.is_some_and(|at| now >= at) {
            io.status.reconnect_at = None;
            io.status.attempts += 1;
            info!(
                "Reconnecting to {} (attempt {})",
                io.status.url, io.status.attempts
            );
            self.driver = Some((self.connector)(&self.url));
            io.status.attempting = true;
        }

        let mut closed = None;
        let handshake_expired = self.transport.status() == &Status::Connecting
            && self.opened_at.is_some_and(|t| now - t > HANDSHAKE_TIMEOUT);
        if handshake_expired {
            // Dropping the driver closes the socket.
            self.driver = None;
            closed = Some(Some(format!(
                "no reply to the handshake in {HANDSHAKE_TIMEOUT} s"
            )));
        }
        while let Some(ev) = self.driver.as_ref().and_then(|d| d.events.try_recv().ok()) {
            match ev {
                DriverEvent::Open => {
                    self.opened_at = Some(now);
                    self.transport.on_open();
                }
                DriverEvent::Message(data) => self.transport.on_message(&data),
                DriverEvent::Closed(err) => closed = Some(err),
                DriverEvent::Rejected(why) => {
                    self.fatal = Some(why.clone());
                    closed = Some(Some(why));
                }
            }
        }
        if let Some(err) = closed {
            self.closed(err, io);
        }

        for uri in self.store.take_requests(now) {
            if let Err(e) = self.transport.request_asset(&uri) {
                self.store.deliver(&uri, Err(e.to_string()));
            }
        }
        self.store.expire(now, crate::assets::WS_ASSET_TIMEOUT);
    }

    /// A world topic the client already takes is tapped, not subscribed
    /// again (DECISIONS.md D-97).
    fn subscribe(&mut self, topic: &str) -> Result<(), SendError> {
        if self.own.contains(topic) {
            self.taps.insert(topic.to_string());
            return Ok(());
        }
        self.transport.subscribe(topic)
    }

    /// Releasing a tapped world topic leaves the client's subscription.
    fn unsubscribe(&mut self, topic: &str) -> Result<(), SendError> {
        if self.own.contains(topic) {
            self.taps.remove(topic);
            return Ok(());
        }
        self.transport.unsubscribe(topic)
    }

    /// The client's own world topics keep the rate it set.
    fn throttle(&mut self, topic: &str, rate: u32) -> Result<(), SendError> {
        if self.own.contains(topic) {
            return Ok(());
        }
        self.transport.throttle(topic, rate)
    }

    fn list_topics(&mut self) -> Result<(), SendError> {
        self.transport.request_topics_types()
    }

    /// Publishes through a server that takes publications (D-104); a
    /// stock server's would be dropped, so it isn't sent.
    fn publish(&mut self, topic: &str, msg_type: &str, payload: &[u8]) -> Result<(), SendError> {
        if !self.transport.publishing() {
            return Err(SendError::Unsendable(
                "the Gazebo websocket server doesn't take publications",
            ));
        }
        self.transport.publish(topic, msg_type, payload)
    }

    fn request(&mut self, service: &str, msg_type: &str, payload: &[u8]) -> Result<(), SendError> {
        self.transport.request(service, msg_type, payload)
    }

    fn world_control(&mut self, cmd: WorldControl) -> Result<(), SendError> {
        let world = self
            .transport
            .world()
            .map(str::to_string)
            .ok_or(SendError::NotConnected);
        let reqs = match cmd {
            WorldControl::Play => vec![control::play(&world?)],
            WorldControl::Pause => vec![control::pause(&world?)],
            WorldControl::Step(n) => control::step(&world?, n),
            WorldControl::Reset => vec![control::reset(&world?)],
            WorldControl::Stop => vec![control::stop()],
        };
        reqs.iter()
            .try_for_each(|r| self.transport.request(&r.service, r.msg_type, &r.payload))
    }

    fn flush(&mut self, io: &mut BackendIo) {
        let world = self.transport.world().unwrap_or_default().to_string();
        while let Some(event) = self.transport.poll_event() {
            self.deliver(event, &world, io);
        }
        if let Some(driver) = &self.driver {
            while let Some(frame) = self.transport.poll_outgoing() {
                driver.send(frame);
            }
        }
    }
}

/// The world topics the client consumes itself.
#[derive(Debug, PartialEq, Eq)]
enum WorldTopic {
    Poses,
    SceneInfo,
    Stats,
    Deletion,
}

impl WorldTopic {
    /// Matches the topics [`subscribe`] builds from the sanitized name.
    fn of(topic: &str, world: &str) -> Option<Self> {
        [Self::Poses, Self::SceneInfo, Self::Stats, Self::Deletion]
            .into_iter()
            .find(|t| topic == world_topic(world, t.leaf()))
    }

    fn leaf(&self) -> &'static str {
        match self {
            Self::Poses => "dynamic_pose/info",
            Self::SceneInfo => "scene/info",
            Self::Stats => "stats",
            Self::Deletion => "scene/deletion",
        }
    }
}

/// Warns about failed calls: `Boolean { data: false }` or the Gazebo
/// websocket server's `StringMsg` errors such as `service_not_found`.
fn log_reply(service: &str, msg_type: &str, payload: &[u8]) {
    match control::reply_ok(msg_type, payload) {
        Some(false) => warn!("{service} returned false"),
        // The websocket server's reply to a service it can't find; other
        // services answer with a StringMsg too (generate_world_sdf).
        _ if msg_type.ends_with(".StringMsg")
            && msgs::StringMsg::decode(payload).is_ok_and(|m| m.data == "service_not_found") =>
        {
            warn!("{service} failed: service_not_found");
        }
        _ => debug!("{service}: {msg_type} reply"),
    }
}

/// Same subscriptions, in the same order, as gzweb
/// `SceneManager.subscribeToTopics()`; interactive mode adds the stats and
/// `scene/deletion` (gzweb has neither, so the capture modes keep its
/// protocol for the parity check). Unlike gzweb, the world name is
/// sanitized as gz-sim does ([`world_topic`]).
fn subscribe(t: &mut Transport, world: &str, interactive: bool) -> HashSet<String> {
    let mut topics = vec![
        world_topic(world, WorldTopic::Poses.leaf()),
        world_topic(world, WorldTopic::SceneInfo.leaf()),
    ];
    let stats = world_topic(world, WorldTopic::Stats.leaf());
    if interactive {
        topics.push(stats.clone());
        topics.push(world_topic(world, WorldTopic::Deletion.leaf()));
    }
    for topic in &topics {
        if let Err(e) = t.subscribe(topic) {
            warn!("subscribe {topic}: {e}");
        }
    }
    if interactive {
        let _ = t.throttle(&stats, STATS_RATE);
    }
    topics.into_iter().collect()
}

/// A paused world's step when it hasn't reported one on this connection:
/// sim time over iterations (exact while the step has been constant),
/// to the nanosecond; 0 before the first iteration.
fn mean_step(s: &SimStats) -> f64 {
    if s.iterations == 0 || s.sim_time <= 0.0 {
        return 0.0;
    }
    (s.sim_time / s.iterations as f64 * 1e9).round() / 1e9
}

fn sim_stats(s: &msgs::WorldStatistics) -> SimStats {
    let secs = |t: &Option<msgs::Time>| {
        t.as_ref()
            .map_or(0.0, |t| t.sec as f64 + t.nsec as f64 * 1e-9)
    };
    SimStats {
        paused: s.paused,
        sim_time: secs(&s.sim_time),
        real_time: secs(&s.real_time),
        real_time_factor: s.real_time_factor,
        iterations: s.iterations,
        step_size: secs(&s.step_size),
    }
}

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;
    use gz_core::frame::build;
    use gz_core::transport::PUBLISH_PROBE_TOPIC;
    use std::cell::RefCell;
    use std::rc::Rc;

    type Ends = (
        crossbeam_channel::Sender<DriverEvent>,
        crossbeam_channel::Receiver<Vec<u8>>,
    );

    struct Harness {
        world: World,
        /// One entry per driver the connector made, newest last.
        drivers: Rc<RefCell<Vec<Ends>>>,
    }

    impl Harness {
        fn new() -> Harness {
            Self::with(GzConfig {
                ui: crate::UiMode::None,
                reconnect: false,
                ..Default::default()
            })
        }

        fn with(config: GzConfig) -> Harness {
            // Logging at trace level makes the `info!` arguments run too.
            let _ = bevy::log::tracing_subscriber::fmt()
                .with_max_level(bevy::log::Level::TRACE)
                .with_test_writer()
                .try_init();
            let drivers = Rc::new(RefCell::new(Vec::new()));
            let made = drivers.clone();
            let connector: Connector = Box::new(move |_url| {
                let (driver, events, sent) = driver::fake();
                made.borrow_mut().push((events, sent));
                driver
            });
            let mut world = World::new();
            let store = WsAssetStore::default();
            let config = GzConfig {
                url: "ws://test".into(),
                ..config
            };
            world.insert_non_send(Backend(Box::new(WebsocketBackend::new(
                &config,
                connector,
                store.clone(),
            ))));
            world.insert_resource(config);
            world.insert_resource(store);
            world.insert_resource(Time::<Real>::default());
            world.insert_resource(GzInbox::default());
            world.init_resource::<Messages<GzCommand>>();
            world.init_resource::<Messages<GzReconnect>>();
            world.init_resource::<Messages<GzPublish>>();
            world.init_resource::<Messages<GzServiceReply>>();
            world.insert_resource(GzStatus::new("ws://test"));
            world.init_resource::<GzTopics>();
            world.init_resource::<GzTraffic>();
            Harness { world, drivers }
        }

        fn run(&mut self) -> Vec<String> {
            self.world.run_system_once(pump).unwrap();
            let drivers = self.drivers.borrow();
            let (_, sent) = drivers.last().unwrap();
            sent.try_iter()
                .map(|f| String::from_utf8_lossy(&f).into_owned())
                .collect()
        }

        fn feed(&mut self, ev: DriverEvent) -> Vec<String> {
            self.drivers.borrow().last().unwrap().0.send(ev).unwrap();
            self.run()
        }

        fn msg(&mut self, frame: Vec<u8>) -> Vec<String> {
            self.feed(DriverEvent::Message(frame))
        }

        fn command(&mut self, cmd: GzCommand) -> Vec<String> {
            self.world.write_message(cmd);
            let sent = self.run();
            // Each run_system_once has a fresh reader; drop what it read.
            self.world.resource_mut::<Messages<GzCommand>>().clear();
            sent
        }

        fn status(&self) -> Status {
            self.world.resource::<GzStatus>().status.clone()
        }

        fn now(&self) -> f64 {
            self.world.resource::<Time<Real>>().elapsed_secs_f64()
        }

        /// Moves real time forward by `secs`.
        fn advance(&mut self, secs: f64) {
            let mut time = self.world.resource_mut::<Time<Real>>();
            // The first update only records the start instant.
            let last = time.last_update().unwrap_or_else(|| {
                let start = std::time::Instant::now();
                time.update_with_instant(start);
                start
            });
            time.update_with_instant(last + std::time::Duration::from_secs_f64(secs));
        }

        /// Messages of type `M` written so far (then cleared).
        fn take<M: Message + Clone>(&mut self) -> Vec<M> {
            let mut msgs = self.world.resource_mut::<Messages<M>>();
            let out = msgs.iter_current_update_messages().cloned().collect();
            msgs.clear();
            out
        }

        /// Runs the handshake up to `Ready` on world `w`; returns the
        /// subscriptions sent.
        fn ready(&mut self) -> Vec<String> {
            self.ready_on("w")
        }

        /// [`Self::ready`] on world `world`.
        fn ready_on(&mut self, world: &str) -> Vec<String> {
            self.feed(DriverEvent::Open);
            self.msg(b"protos".to_vec());
            self.msg(publish(
                "worlds",
                "gz.msgs.StringMsg_V",
                &msgs::StringMsgV {
                    data: vec![world.into()],
                    ..Default::default()
                }
                .encode_to_vec(),
            ));
            let scene = msgs::Scene {
                name: world.into(),
                ..Default::default()
            };
            self.msg(publish("scene", "gz.msgs.Scene", &scene.encode_to_vec()))
        }
    }

    /// Blocks until a reader is waiting on the store (or fails after 10 s).
    fn wait_for_waiter(store: &WsAssetStore) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while store.waiting() == 0 {
            assert!(std::time::Instant::now() < deadline, "no reader started");
            std::thread::yield_now();
        }
    }

    fn publish(topic: &str, msg_type: &str, payload: &[u8]) -> Vec<u8> {
        build("pub", topic, msg_type, payload)
    }

    /// gz-sim publishes a world's topics under its sanitized name; the
    /// `scene` request keeps the raw name (the server sanitizes it).
    #[test]
    fn world_topics_use_the_sanitized_name() {
        let mut h = Harness::new();
        assert_eq!(
            h.ready_on("my world@2"),
            [
                "sub,/world/my_world2/dynamic_pose/info,,",
                "sub,/world/my_world2/scene/info,,"
            ]
        );
        h.msg(publish(
            "/world/my_world2/dynamic_pose/info",
            "gz.msgs.Pose_V",
            &msgs::PoseV {
                pose: vec![msgs::Pose::default()],
                ..Default::default()
            }
            .encode_to_vec(),
        ));
        assert_eq!(h.world.resource::<GzInbox>().poses.len(), 1);
        assert!(h.take::<GzPublish>().is_empty());
        let sent = h.command(GzCommand::Pause);
        assert!(
            sent[0].starts_with("req,/world/my_world2/control,"),
            "{sent:?}"
        );
    }

    #[test]
    fn handshake_subscribes_and_routes_messages() {
        let mut h = Harness::new();
        assert_eq!(h.feed(DriverEvent::Open), ["protos,,,"]);
        assert_eq!(h.status(), Status::Connecting);
        assert_eq!(
            h.msg(b"syntax = \"proto3\";".to_vec()),
            ["topics-types,,,", "worlds,,,"]
        );
        assert_eq!(h.status(), Status::Connected);

        let topics = msgs::Publishers::default().encode_to_vec();
        assert!(h
            .msg(publish("topics-types", "gz.msgs.Publishers", &topics))
            .is_empty());
        let names = msgs::StringMsgV::default().encode_to_vec();
        h.msg(publish("topics", "gz.msgs.StringMsg_V", &names));
        let worlds = msgs::StringMsgV {
            data: vec!["w".into()],
            ..Default::default()
        };
        assert_eq!(
            h.msg(publish(
                "worlds",
                "gz.msgs.StringMsg_V",
                &worlds.encode_to_vec()
            )),
            ["scene,w,,"]
        );
        assert_eq!(h.world.resource::<GzStatus>().world.as_deref(), Some("w"));

        let scene = msgs::Scene {
            name: "w".into(),
            ..Default::default()
        };
        assert_eq!(
            h.msg(publish("scene", "gz.msgs.Scene", &scene.encode_to_vec())),
            [
                "sub,/world/w/dynamic_pose/info,,",
                "sub,/world/w/scene/info,,"
            ]
        );
        assert_eq!(h.status(), Status::Ready);
        assert!(h.world.resource::<GzInbox>().scene.is_some());

        // Streams: good and bad payloads, and an unrelated topic.
        let poses = msgs::PoseV {
            pose: vec![msgs::Pose::default(); 2],
            ..Default::default()
        };
        h.msg(publish(
            "/world/w/dynamic_pose/info",
            "gz.msgs.Pose_V",
            &poses.encode_to_vec(),
        ));
        h.msg(publish(
            "/world/w/dynamic_pose/info",
            "gz.msgs.Pose_V",
            &[0xff],
        ));
        h.msg(publish(
            "/world/w/scene/info",
            "gz.msgs.Scene",
            &scene.encode_to_vec(),
        ));
        h.msg(publish("/world/w/scene/info", "gz.msgs.Scene", &[0xff]));
        h.msg(publish("/world/w/other", "gz.msgs.Clock", &[]));
        let inbox = h.world.resource::<GzInbox>();
        assert_eq!(inbox.poses.len(), 2);
        assert_eq!(inbox.scene_updates.len(), 1);

        // Malformed frames are counted.
        h.msg(b"nonsense".to_vec());
        assert_eq!(h.world.resource::<GzStatus>().malformed, 1);
    }

    #[test]
    fn the_probes_answer_turns_editing_on_or_off_for_the_connection() {
        let config = || GzConfig {
            ui: crate::UiMode::Minimal,
            reconnect: false,
            ..Default::default()
        };
        let worlds = msgs::StringMsgV {
            data: vec!["w".into()],
            ..Default::default()
        }
        .encode_to_vec();
        let not_found = msgs::StringMsg {
            data: "service_not_found".into(),
            ..Default::default()
        }
        .encode_to_vec();
        for (msg_type, payload, edits) in [
            ("gz.msgs.StringMsg_V", worlds.as_slice(), true),
            ("gz.msgs.StringMsg_V", &[][..], false),
            // Not found yet (discovery): not a yes.
            ("gz.msgs.StringMsg", not_found.as_slice(), false),
        ] {
            let mut h = Harness::with(config());
            h.ready();
            assert_eq!(h.world.resource::<GzStatus>().edits, None);
            h.msg(build("req", BINARY_PROBE_SERVICE, msg_type, payload));
            assert_eq!(
                h.world.resource::<GzStatus>().edits,
                Some(edits),
                "{msg_type}"
            );
            // Only the first answer counts.
            h.msg(build(
                "req",
                BINARY_PROBE_SERVICE,
                "gz.msgs.StringMsg_V",
                &worlds,
            ));
            assert_eq!(h.world.resource::<GzStatus>().edits, Some(edits));
            // A yes lets a request's zero bytes through whole.
            let sent = h.command(GzCommand::Request {
                service: "/world/w/set_pose".into(),
                msg_type: "gz.msgs.Pose".into(),
                payload: vec![0x08, 0x00, b','],
            });
            assert_eq!(sent.len(), usize::from(edits), "{msg_type}: {sent:?}");
        }
    }

    /// The publish probe's topic in a topic list says the server
    /// publishes; lists without it are asked for again, a few times, then
    /// it doesn't. Either way, the probe's topic isn't one of the world's.
    #[test]
    fn the_publish_probe_reads_the_topic_list() {
        let config = || GzConfig {
            ui: crate::UiMode::Minimal,
            reconnect: false,
            ..Default::default()
        };
        let list = |topics: &[&str]| {
            let msg = msgs::Publishers {
                publisher: topics
                    .iter()
                    .map(|t| msgs::Publish {
                        topic: t.to_string(),
                        msg_type: "gz.msgs.Empty".into(),
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            };
            build(
                "pub",
                "topics-types",
                "gz.msgs.Publishers",
                &msg.encode_to_vec(),
            )
        };
        let mut h = Harness::with(config());
        h.ready();
        assert_eq!(h.world.resource::<GzStatus>().publishes, None);
        let again = h.msg(list(&["/a"]));
        assert!(again.iter().any(|f| f == "topics-types,,,"), "{again:?}");
        assert_eq!(h.world.resource::<GzStatus>().publishes, None);
        h.msg(list(&["/a", PUBLISH_PROBE_TOPIC]));
        assert_eq!(h.world.resource::<GzStatus>().publishes, Some(true));
        assert!(h
            .world
            .resource::<GzTopics>()
            .get(PUBLISH_PROBE_TOPIC)
            .is_none());
        let sent = h.command(GzCommand::Publish {
            topic: "/cmd_vel".into(),
            msg_type: "gz.msgs.Twist".into(),
            payload: vec![0x08, 0x00, b','],
        });
        assert_eq!(sent.len(), 1, "{sent:?}");

        let mut h = Harness::with(config());
        h.ready();
        for _ in 0..3 {
            h.msg(list(&["/a"]));
        }
        assert_eq!(h.world.resource::<GzStatus>().publishes, Some(false));
    }

    #[test]
    fn interactive_mode_follows_the_sim_stats() {
        let mut h = Harness::with(GzConfig {
            ui: crate::UiMode::Minimal,
            reconnect: false,
            ..Default::default()
        });
        assert_eq!(
            h.ready(),
            [
                "sub,/world/w/dynamic_pose/info,,",
                "sub,/world/w/scene/info,,",
                "sub,/world/w/stats,,",
                "sub,/world/w/scene/deletion,,",
                "throttle,/world/w/stats,na,5",
                "req,/gazebo/worlds,gz.msgs.Empty,z\u{2}\0,",
                "pub_in,/gzweb/publish_probe,gz.msgs.Empty,",
                "topics-types,,,",
            ]
        );
        // Deletions queue for the scene, in order; a bad one is dropped.
        let ids = |data: Vec<u32>| msgs::UInt32V {
            data,
            ..Default::default()
        };
        h.msg(publish(
            "/world/w/scene/deletion",
            "gz.msgs.UInt32_V",
            &ids(vec![7, 9]).encode_to_vec(),
        ));
        h.msg(publish(
            "/world/w/scene/deletion",
            "gz.msgs.UInt32_V",
            &[0xff],
        ));
        h.msg(publish(
            "/world/w/scene/deletion",
            "gz.msgs.UInt32_V",
            &ids(vec![3]).encode_to_vec(),
        ));
        assert_eq!(h.world.resource::<GzInbox>().deletions, [7, 9, 3]);
        let stats = msgs::WorldStatistics {
            paused: true,
            iterations: 42,
            real_time_factor: 0.5,
            sim_time: Some(msgs::Time {
                sec: 3,
                nsec: 500_000_000,
            }),
            real_time: Some(msgs::Time { sec: 7, nsec: 0 }),
            step_size: Some(msgs::Time {
                sec: 0,
                nsec: 500_000_000,
            }),
            ..Default::default()
        };
        h.msg(publish(
            "/world/w/stats",
            "gz.msgs.WorldStatistics",
            &stats.encode_to_vec(),
        ));
        assert_eq!(
            h.world.resource::<GzStatus>().sim,
            Some(SimStats {
                paused: true,
                sim_time: 3.5,
                real_time: 7.0,
                real_time_factor: 0.5,
                iterations: 42,
                step_size: 0.5,
            })
        );
        h.msg(publish(
            "/world/w/stats",
            "gz.msgs.WorldStatistics",
            &[0xff],
        ));
        // Stats without a sim time read as 0; a paused world reports a
        // zero step, and the last real one is kept (DECISIONS.md D-37).
        h.msg(publish(
            "/world/w/stats",
            "gz.msgs.WorldStatistics",
            &msgs::WorldStatistics::default().encode_to_vec(),
        ));
        let sim = h.world.resource::<GzStatus>().sim.unwrap();
        assert_eq!((sim.sim_time, sim.step_size), (0.0, 0.5));
        // A world paused before this connection never reports its step:
        // sim time over iterations stands in.
        let mut fresh = Harness::with(GzConfig {
            ui: crate::UiMode::Minimal,
            reconnect: false,
            ..Default::default()
        });
        fresh.ready();
        let paused = |iterations, sec| msgs::WorldStatistics {
            paused: true,
            iterations,
            sim_time: Some(msgs::Time {
                sec,
                nsec: 52_000_000,
            }),
            ..Default::default()
        };
        for (iterations, sec, step) in [(126_081_052, 126_081, 0.001), (0, 0, 0.0)] {
            fresh.msg(publish(
                "/world/w/stats",
                "gz.msgs.WorldStatistics",
                &paused(iterations, sec).encode_to_vec(),
            ));
            let sim = fresh.world.resource::<GzStatus>().sim.unwrap();
            assert_eq!(sim.step_size, step, "{iterations}");
        }
        // A close forgets them.
        h.feed(DriverEvent::Closed(None));
        assert_eq!(h.world.resource::<GzStatus>().sim, None);
    }

    #[test]
    fn only_this_worlds_topics_are_consumed_the_rest_are_forwarded() {
        let mut h = Harness::new();
        h.ready();
        h.command(GzCommand::Subscribe("/foo/stats".into()));
        h.command(GzCommand::Subscribe("/world/other/stats".into()));
        h.msg(publish(
            "/foo/stats",
            "gz.msgs.WorldStatistics",
            b"\x28\x01",
        ));
        h.msg(publish(
            "/world/other/stats",
            "gz.msgs.WorldStatistics",
            b"",
        ));
        assert_eq!(h.world.resource::<GzStatus>().sim, None);
        let topics: Vec<String> = h.take::<GzPublish>().into_iter().map(|p| p.topic).collect();
        assert_eq!(topics, ["/foo/stats", "/world/other/stats"]);
        assert_eq!(
            WorldTopic::of("/world/w/stats", "w"),
            Some(WorldTopic::Stats)
        );
        assert_eq!(WorldTopic::of("/world/w/other", "w"), None);
        assert_eq!(WorldTopic::of("/stats", "w"), None);
        assert_eq!(
            WorldTopic::of("/world/a_b/scene/info", "a b"),
            Some(WorldTopic::SceneInfo)
        );
        assert_eq!(WorldTopic::of("/world/a b/scene/info", "a b"), None);
    }

    /// The app subscribing a world topic the client takes itself taps it:
    /// nothing is sent, its messages come out as GzPublish too, and
    /// letting go leaves the client's own subscription and rate. Every
    /// message counts toward its topic's traffic; a refresh asks for the
    /// topic list again, and the reply replaces it.
    #[test]
    fn a_world_topic_the_client_takes_is_tapped() {
        let mut h = Harness::new();
        h.ready();
        let poses = "/world/w/dynamic_pose/info".to_string();
        assert!(h.command(GzCommand::Subscribe(poses.clone())).is_empty());
        assert!(h.command(GzCommand::Throttle(poses.clone(), 2)).is_empty());
        let pose_v = msgs::PoseV {
            pose: vec![msgs::Pose::default()],
            ..Default::default()
        }
        .encode_to_vec();
        h.msg(publish(&poses, "gz.msgs.Pose_V", &pose_v));
        let tapped: Vec<String> = h.take::<GzPublish>().into_iter().map(|p| p.topic).collect();
        assert_eq!(tapped, std::slice::from_ref(&poses));
        assert_eq!(h.world.resource::<GzInbox>().poses.len(), 1);
        assert!(h.command(GzCommand::Unsubscribe(poses.clone())).is_empty());
        h.msg(publish(&poses, "gz.msgs.Pose_V", &pose_v));
        assert!(h.take::<GzPublish>().is_empty());
        assert_eq!(h.world.resource::<GzInbox>().poses.len(), 2);
        assert_eq!(h.world.resource::<GzTraffic>().topics[&poses].total, 2);

        assert_eq!(h.command(GzCommand::RefreshTopics), ["topics-types,,,"]);
        let list = msgs::Publishers {
            publisher: vec![msgs::Publish {
                topic: "/clock".into(),
                msg_type: "gz.msgs.Clock".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        h.msg(publish(
            "topics-types",
            "gz.msgs.Publishers",
            &list.encode_to_vec(),
        ));
        let topics = h.world.resource::<GzTopics>();
        assert_eq!(topics.get("/clock").unwrap().msg_type, "gz.msgs.Clock");
    }

    #[test]
    fn commands_become_requests() {
        let mut h = Harness::new();
        // Before the handshake nothing can be sent.
        assert!(h.command(GzCommand::Play).is_empty());
        assert!(h.command(GzCommand::Stop).is_empty());
        h.ready();
        // A server that hasn't said it publishes: refused, not sent.
        assert!(h
            .command(GzCommand::Publish {
                topic: "/cmd_vel".into(),
                msg_type: "gz.msgs.Twist".into(),
                payload: vec![1],
            })
            .is_empty());
        let sent: Vec<String> = [
            GzCommand::Play,
            GzCommand::Pause,
            // 44 can't be carried in one request: two.
            GzCommand::Step(44),
            GzCommand::Reset,
            GzCommand::Stop,
            GzCommand::Subscribe("/clock".into()),
            GzCommand::Throttle("/clock".into(), 2),
            GzCommand::Unsubscribe("/clock".into()),
            GzCommand::Request {
                service: "/s".into(),
                msg_type: "gz.msgs.Empty".into(),
                payload: vec![0x0a, 0x02, 0x08, 0x01],
            },
            // Refused: an empty payload would crash the Gazebo websocket
            // server.
            GzCommand::Request {
                service: "/s".into(),
                msg_type: "gz.msgs.Empty".into(),
                payload: vec![],
            },
        ]
        .into_iter()
        .flat_map(|c| h.command(c))
        .collect();
        let ops: Vec<_> = sent
            .iter()
            .map(|f| f.splitn(3, ',').take(2).collect::<Vec<_>>().join(","))
            .collect();
        assert_eq!(
            ops,
            [
                "req,/world/w/control",
                "req,/world/w/control",
                "req,/world/w/control",
                "req,/world/w/control",
                "req,/world/w/control",
                "req,/server_control",
                "sub,/clock",
                "throttle,/clock",
                "unsub,/clock",
                "req,/s",
            ]
        );

        // Every reply is forwarded, good or bad.
        let boolean = |data| {
            msgs::Boolean {
                data,
                ..Default::default()
            }
            .encode_to_vec()
        };
        let not_found = msgs::StringMsg {
            data: "service_not_found".into(),
            ..Default::default()
        }
        .encode_to_vec();
        h.msg(build(
            "req",
            "/world/w/control",
            "gz.msgs.Boolean",
            &boolean(true),
        ));
        h.msg(build(
            "req",
            "/world/w/control",
            "gz.msgs.Boolean",
            &boolean(false),
        ));
        h.msg(build("req", "/nope", "gz.msgs.StringMsg", &not_found));
        h.msg(build("req", "/odd", "gz.msgs.StringMsg", &[0xff]));
        h.msg(build("req", "/pose", "gz.msgs.Pose", b""));
        let services: Vec<String> = h
            .take::<GzServiceReply>()
            .into_iter()
            .map(|r| r.service)
            .collect();
        assert_eq!(
            services,
            [
                "/world/w/control",
                "/world/w/control",
                "/nope",
                "/odd",
                "/pose"
            ]
        );
    }

    #[test]
    fn asset_requests_and_replies_go_through_the_store() {
        let mut h = Harness::new();
        h.feed(DriverEvent::Open);
        h.msg(b"protos".to_vec());
        let store = h.world.resource::<WsAssetStore>().clone();
        // A reader waiting for /m.stl makes pump send the request.
        let fetch = std::thread::spawn(move || bevy::tasks::block_on(store.fetch("/m.stl")));
        let mut sent = Vec::new();
        crate::test_util::wait_until(|| {
            sent = h.feed(DriverEvent::Message(Vec::new()));
            !sent.is_empty()
        });
        assert_eq!(sent, ["asset,,,/m.stl"]);
        let bytes = msgs::Bytes {
            data: vec![7],
            ..Default::default()
        };
        h.msg(build(
            "asset",
            "/m.stl",
            "gz.msgs.Bytes",
            &bytes.encode_to_vec(),
        ));
        assert_eq!(fetch.join().unwrap(), Ok(vec![7]));
    }

    #[test]
    fn asset_requests_before_the_handshake_fail() {
        let mut h = Harness::new();
        let store = h.world.resource::<WsAssetStore>().clone();
        let fetch = std::thread::spawn({
            let store = store.clone();
            move || bevy::tasks::block_on(store.fetch("/early.stl"))
        });
        wait_for_waiter(&store);
        h.run();
        assert_eq!(fetch.join().unwrap(), Err("not connected".into()));
    }

    #[test]
    fn closing_fails_waiting_readers_and_sets_the_status() {
        let mut h = Harness::new();
        h.feed(DriverEvent::Open);
        h.feed(DriverEvent::Closed(None));
        assert_eq!(h.status(), Status::Disconnected);

        h.feed(DriverEvent::Open);
        let store = h.world.resource::<WsAssetStore>().clone();
        let fetch = std::thread::spawn({
            let store = store.clone();
            move || bevy::tasks::block_on(store.fetch("/never"))
        });
        wait_for_waiter(&store);
        h.feed(DriverEvent::Closed(Some("reset".into())));
        assert_eq!(h.status(), Status::Error("reset".into()));
        assert_eq!(fetch.join().unwrap(), Err("connection closed".into()));
        // Reconnecting is off: nothing scheduled.
        assert_eq!(h.world.resource::<GzStatus>().reconnect_at, None);
    }

    #[test]
    fn reconnects_with_backoff_and_replaces_the_scene() {
        let mut h = Harness::with(GzConfig {
            ui: crate::UiMode::None,
            reconnect: true,
            ..Default::default()
        });
        h.ready();
        h.feed(DriverEvent::Closed(Some("reset".into())));
        let due = h.world.resource::<GzStatus>().reconnect_at.unwrap();
        assert_eq!(due - h.now(), backoff(0));
        // Not yet: still one driver.
        h.run();
        assert_eq!(h.drivers.borrow().len(), 1);
        h.advance(backoff(0));
        h.run();
        assert_eq!(h.drivers.borrow().len(), 2);
        assert_eq!(h.world.resource::<GzStatus>().attempts, 1);

        // The second attempt fails too: the next, longer wait.
        h.feed(DriverEvent::Closed(Some("refused".into())));
        let next = h.world.resource::<GzStatus>().reconnect_at.unwrap();
        assert_eq!(next - h.now(), backoff(1));
        h.advance(backoff(1));
        h.run();
        assert_eq!(h.drivers.borrow().len(), 3);

        // Success: a fresh scene replaces the old one and the count resets.
        h.world.resource_mut::<GzInbox>().scene = None;
        h.world
            .resource_mut::<GzInbox>()
            .poses
            .push(msgs::Pose::default());
        h.ready();
        assert_eq!(h.world.resource::<GzStatus>().attempts, 0);
        let inbox = h.world.resource::<GzInbox>();
        assert!(inbox.scene.is_some() && inbox.poses.is_empty());
    }

    #[test]
    fn reconnect_now_to_the_same_or_another_server() {
        let mut h = Harness::with(GzConfig {
            ui: crate::UiMode::None,
            reconnect: true,
            ..Default::default()
        });
        assert_eq!(h.drivers.borrow().len(), 1);
        h.feed(DriverEvent::Rejected("invalid URL: x".into()));
        let status = h.world.resource::<GzStatus>();
        assert!(!status.attempting && status.reconnect_at.is_none());
        // Retry now: same server, a new driver, the rejection forgotten.
        h.world.write_message(GzReconnect::default());
        h.run();
        assert_eq!(h.drivers.borrow().len(), 2);
        let status = h.world.resource::<GzStatus>();
        assert!(status.attempting);
        assert_eq!((status.attempts, status.url.as_str()), (1, "ws://test"));
        h.world.resource_mut::<Messages<GzReconnect>>().clear();
        // A failure now schedules a retry again.
        h.feed(DriverEvent::Closed(Some("refused".into())));
        assert!(h.world.resource::<GzStatus>().reconnect_at.is_some());
        // Another server: the count restarts and the backoff is cancelled.
        h.world.resource_mut::<GzStatus>().world = Some("w".into());
        h.world.write_message(GzReconnect {
            url: Some("ws://other:9002".into()),
        });
        h.run();
        assert_eq!(h.drivers.borrow().len(), 3);
        let status = h.world.resource::<GzStatus>();
        assert_eq!(
            (status.attempts, status.url.as_str()),
            (0, "ws://other:9002")
        );
        assert_eq!(status.reconnect_at, None);
        // The old server's world name doesn't show for the new one.
        assert_eq!(status.world, None);
        assert_eq!(h.status(), Status::Disconnected);
        h.world.resource_mut::<Messages<GzReconnect>>().clear();
        h.ready();
        assert_eq!(h.status(), Status::Ready);
    }

    #[test]
    fn a_rejected_key_is_not_retried() {
        let mut h = Harness::with(GzConfig {
            key: Some("k".into()),
            ui: crate::UiMode::None,
            reconnect: true,
            ..Default::default()
        });
        assert_eq!(h.feed(DriverEvent::Open), ["auth,,,k"]);
        h.msg(b"invalid".to_vec());
        assert_eq!(h.status(), Status::Error("invalid auth key".into()));
        h.feed(DriverEvent::Closed(None));
        assert_eq!(h.world.resource::<GzStatus>().reconnect_at, None);
        // The reason survives the close.
        assert_eq!(h.status(), Status::Error("invalid auth key".into()));
    }

    #[test]
    fn a_rejected_url_is_not_retried() {
        let mut h = Harness::with(GzConfig {
            ui: crate::UiMode::None,
            reconnect: true,
            ..Default::default()
        });
        h.feed(DriverEvent::Rejected("invalid URL: x".into()));
        assert_eq!(h.status(), Status::Error("invalid URL: x".into()));
        assert_eq!(h.world.resource::<GzStatus>().reconnect_at, None);
    }

    #[test]
    fn a_silent_handshake_times_out_and_retries() {
        let mut h = Harness::with(GzConfig {
            key: Some("k".into()),
            ui: crate::UiMode::None,
            reconnect: true,
            ..Default::default()
        });
        h.feed(DriverEvent::Open);
        h.advance(HANDSHAKE_TIMEOUT / 2.0);
        h.run();
        assert_eq!(h.status(), Status::Connecting);
        h.advance(HANDSHAKE_TIMEOUT);
        h.run();
        assert!(matches!(h.status(), Status::Error(e) if e.starts_with("no reply")));
        assert!(h.world.resource::<GzStatus>().reconnect_at.is_some());
        // The driver was dropped; frames have nowhere to go until the retry.
        assert!(h.command(GzCommand::Play).is_empty());
    }

    #[test]
    fn backoff_grows_to_ten_seconds() {
        let waits: Vec<f64> = (0..8).map(backoff).collect();
        assert_eq!(waits, [0.5, 1.0, 2.0, 4.0, 8.0, 10.0, 10.0, 10.0]);
    }

    #[test]
    fn a_bad_ca_file_is_a_connection_error() {
        let mut world = World::new();
        world.insert_resource(GzConfig {
            url: "wss://localhost:1".into(),
            ca_cert: Some("/nonexistent/ca.pem".into()),
            ..Default::default()
        });
        world.run_system_once(connect).unwrap();
        assert!(world.get_non_send::<Backend>().is_none());
        assert!(matches!(
            &world.resource::<GzStatus>().status,
            Status::Error(e) if e.contains("ca.pem")
        ));
    }

    #[test]
    fn pump_without_a_connection_does_nothing() {
        let mut world = World::new();
        world.insert_resource(GzConfig::default());
        world.insert_resource(WsAssetStore::default());
        world.insert_resource(Time::<Real>::default());
        world.insert_resource(GzInbox::default());
        world.init_resource::<Messages<GzCommand>>();
        world.init_resource::<Messages<GzReconnect>>();
        world.init_resource::<Messages<GzPublish>>();
        world.init_resource::<Messages<GzServiceReply>>();
        world.insert_resource(GzStatus::new(""));
        world.write_message(GzReconnect::default());
        world.run_system_once(pump).unwrap();
    }
}
