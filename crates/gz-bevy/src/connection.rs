//! Drives the websocket: driver bytes → [`Transport`] → Bevy resources.
//!
//! Also owns reconnecting (with backoff, unless the key or URL was rejected)
//! and the message API: [`GzCommand`] in, [`GzPublish`] and
//! [`GzServiceReply`] out.

use bevy::prelude::*;
use gz_core::control;
use gz_core::msgs;
use gz_core::transport::{Event, SendError, Status, Transport};
use prost::Message as _;

use crate::assets::WsAssetStore;
use crate::driver::{self, Driver, DriverEvent};
use crate::GzConfig;

/// Opens a new driver to the configured server.
type Connector = Box<dyn Fn() -> Driver>;

/// The live connection. Non-`Send` because the wasm socket is a JS object.
pub struct GzConnection {
    transport: Transport,
    /// `None` after a handshake timeout dropped it.
    driver: Option<Driver>,
    connector: Connector,
    /// Why retrying would fail the same way (rejected key or URL).
    fatal: Option<String>,
    /// When the socket opened, for the handshake timeout.
    opened_at: Option<f64>,
}

/// Simulation state from `/world/<world>/stats` (interactive mode only).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SimStats {
    pub paused: bool,
    pub sim_time: f64,
    pub real_time_factor: f64,
    pub iterations: u64,
}

/// Connection state visible to the rest of the app.
#[derive(Resource, Debug)]
pub struct GzStatus {
    pub status: Status,
    pub url: String,
    pub world: Option<String>,
    /// Frames that could not be parsed or decoded.
    pub malformed: u32,
    /// When the next reconnect attempt is due (`Time<Real>` seconds).
    pub reconnect_at: Option<f64>,
    /// Reconnect attempts since the last time the scene was ready.
    pub attempts: u32,
    pub sim: Option<SimStats>,
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
        }
    }
}

/// Requests for the server. A command the connection can't send right now
/// (not connected yet, or a frame gz-launch can't carry) is dropped and
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
    /// Call a service with a serialized request.
    Request {
        service: String,
        msg_type: String,
        payload: Vec<u8>,
    },
}

/// A message on a topic subscribed with [`GzCommand::Subscribe`]; the payload
/// is still serialized (PNG data for image topics).
#[derive(Message, Debug, Clone, PartialEq)]
pub struct GzPublish {
    pub topic: String,
    pub msg_type: String,
    pub payload: Vec<u8>,
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
/// server without a key never answers `auth`, so this is the only way out.
pub const HANDSHAKE_TIMEOUT: f64 = 10.0;

pub fn connect(world: &mut World) {
    let config = world.resource::<GzConfig>().clone();
    info!("Connecting to {}", config.url);
    let mut status = GzStatus::new(config.url.clone());
    match connector(&config) {
        Ok(connector) => world.insert_non_send(GzConnection {
            transport: Transport::new(config.key.clone()),
            driver: Some(connector()),
            connector,
            fatal: None,
            opened_at: None,
        }),
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
    let url = config.url.clone();
    Ok(Box::new(move || driver::connect(&url, Some(tls.clone()))))
}

#[cfg(target_arch = "wasm32")]
fn connector(config: &GzConfig) -> Result<Connector, String> {
    let url = config.url.clone();
    Ok(Box::new(move || driver::connect(&url)))
}

#[allow(clippy::too_many_arguments)]
pub fn pump(
    conn: Option<NonSendMut<GzConnection>>,
    config: Res<GzConfig>,
    store: Res<WsAssetStore>,
    time: Res<Time<Real>>,
    mut commands: MessageReader<GzCommand>,
    mut publishes: MessageWriter<GzPublish>,
    mut replies: MessageWriter<GzServiceReply>,
    mut status: ResMut<GzStatus>,
    mut inbox: ResMut<GzInbox>,
) {
    let Some(mut conn) = conn else { return };
    let conn = &mut *conn;
    let now = time.elapsed_secs_f64();

    if status.reconnect_at.is_some_and(|at| now >= at) {
        status.reconnect_at = None;
        status.attempts += 1;
        info!(
            "Reconnecting to {} (attempt {})",
            status.url, status.attempts
        );
        conn.driver = Some((conn.connector)());
    }

    let mut closed = None;
    let handshake_expired = conn.transport.status() == &Status::Connecting
        && conn.opened_at.is_some_and(|t| now - t > HANDSHAKE_TIMEOUT);
    if handshake_expired {
        // Dropping the driver closes the socket.
        conn.driver = None;
        closed = Some(Some(format!(
            "no reply to the handshake in {HANDSHAKE_TIMEOUT} s"
        )));
    }
    while let Some(ev) = conn.driver.as_ref().and_then(|d| d.events.try_recv().ok()) {
        match ev {
            DriverEvent::Open => {
                conn.opened_at = Some(now);
                conn.transport.on_open();
            }
            DriverEvent::Message(data) => conn.transport.on_message(&data),
            DriverEvent::Closed(err) => closed = Some(err),
            DriverEvent::Rejected(why) => {
                conn.fatal = Some(why.clone());
                closed = Some(Some(why));
            }
        }
    }
    if let Some(err) = closed {
        conn.opened_at = None;
        // Keep the reason that makes retrying pointless.
        let err = conn.fatal.clone().or(err);
        if let Some(e) = &err {
            error!("websocket closed: {e}");
        }
        conn.transport.on_close(err);
        store.fail_all("connection closed");
        status.sim = None;
        if config.reconnect && conn.fatal.is_none() {
            let wait = backoff(status.attempts);
            info!("Reconnecting in {wait:.1} s");
            status.reconnect_at = Some(now + wait);
        }
    }

    for uri in store.take_requests(now) {
        if let Err(e) = conn.transport.request_asset(&uri) {
            store.deliver(&uri, Err(e.to_string()));
        }
    }
    store.expire(now, crate::assets::WS_ASSET_TIMEOUT);

    for cmd in commands.read() {
        if let Err(e) = send_command(&mut conn.transport, cmd) {
            warn!("{cmd:?} not sent: {e}");
        }
    }

    let world = conn.transport.world().unwrap_or_default().to_string();
    while let Some(event) = conn.transport.poll_event() {
        match event {
            Event::Status(s) => {
                info!("gz status: {s:?}");
                if s == Status::Ready {
                    status.attempts = 0;
                    subscribe(&mut conn.transport, &world, config.interactive());
                }
                status.status = s;
            }
            Event::AuthFailed => {
                error!("websocket auth key rejected");
                conn.fatal = Some("invalid auth key".into());
            }
            Event::Worlds(worlds) => {
                info!("worlds: {worlds:?}");
                status.world = worlds.first().cloned();
            }
            Event::TopicsTypes(_) => {}
            Event::Topics(topics) => info!("topics: {topics:?}"),
            Event::Scene(scene) => {
                info!(
                    "scene '{}': {} models, {} lights",
                    scene.name,
                    scene.model.len(),
                    scene.light.len()
                );
                inbox.scene = Some(scene);
                inbox.scene_updates.clear();
                inbox.poses.clear();
                inbox.last_activity_secs = now;
            }
            Event::Publish {
                topic,
                msg_type,
                payload,
            } => match WorldTopic::of(&topic, &world) {
                Some(WorldTopic::Poses) => match msgs::PoseV::decode(payload.as_slice()) {
                    Ok(v) => inbox.poses.extend(v.pose),
                    Err(e) => warn!("bad Pose_V on {topic}: {e}"),
                },
                Some(WorldTopic::SceneInfo) => {
                    match msgs::Scene::decode(payload.as_slice()) {
                        Ok(s) => inbox.scene_updates.push(s),
                        Err(e) => warn!("bad Scene on {topic}: {e}"),
                    }
                    // Pose streams never stop on a live simulation, so only
                    // structural changes count as activity for settling.
                    inbox.last_activity_secs = now;
                }
                Some(WorldTopic::Stats) => {
                    match msgs::WorldStatistics::decode(payload.as_slice()) {
                        Ok(s) => status.sim = Some(sim_stats(&s)),
                        Err(e) => warn!("bad WorldStatistics on {topic}: {e}"),
                    }
                }
                None => {
                    publishes.write(GzPublish {
                        topic,
                        msg_type,
                        payload,
                    });
                }
            },
            Event::ServiceReply {
                service,
                msg_type,
                payload,
            } => {
                log_reply(&service, &msg_type, &payload);
                replies.write(GzServiceReply {
                    service,
                    msg_type,
                    payload,
                });
            }
            Event::Asset { uri, result } => {
                store.deliver(&uri, result.map_err(|e| e.to_string()));
                inbox.last_activity_secs = now;
            }
            Event::Malformed(why) => {
                warn!("malformed frame: {why}");
                status.malformed += 1;
            }
        }
    }

    if let Some(driver) = &conn.driver {
        while let Some(frame) = conn.transport.poll_outgoing() {
            driver.send(frame);
        }
    }
}

/// The world topics the client consumes itself.
#[derive(Debug, PartialEq, Eq)]
enum WorldTopic {
    Poses,
    SceneInfo,
    Stats,
}

impl WorldTopic {
    fn of(topic: &str, world: &str) -> Option<Self> {
        match topic.strip_prefix("/world/")?.strip_prefix(world)? {
            "/dynamic_pose/info" => Some(Self::Poses),
            "/scene/info" => Some(Self::SceneInfo),
            "/stats" => Some(Self::Stats),
            _ => None,
        }
    }
}

/// Warns about failed calls: `Boolean { data: false }` or gz-launch's
/// `StringMsg` errors such as `service_not_found`.
fn log_reply(service: &str, msg_type: &str, payload: &[u8]) {
    match control::reply_ok(msg_type, payload) {
        Some(false) => warn!("{service} returned false"),
        _ if msg_type.ends_with(".StringMsg") => {
            let why = msgs::StringMsg::decode(payload)
                .map(|m| m.data)
                .unwrap_or_default();
            warn!("{service} failed: {why}");
        }
        _ => debug!("{service}: {msg_type} reply"),
    }
}

/// Same subscriptions, in the same order, as gzweb
/// `SceneManager.subscribeToTopics()`; interactive mode adds the stats.
fn subscribe(t: &mut Transport, world: &str, interactive: bool) {
    let mut topics = vec![
        format!("/world/{world}/dynamic_pose/info"),
        format!("/world/{world}/scene/info"),
    ];
    let stats = format!("/world/{world}/stats");
    if interactive {
        topics.push(stats.clone());
    }
    for topic in &topics {
        if let Err(e) = t.subscribe(topic) {
            warn!("subscribe {topic}: {e}");
        }
    }
    if interactive {
        let _ = t.throttle(&stats, STATS_RATE);
    }
}

fn send_command(t: &mut Transport, cmd: &GzCommand) -> Result<(), SendError> {
    let world = || t.world().map(str::to_string).ok_or(SendError::NotConnected);
    let reqs = match cmd {
        GzCommand::Play => vec![control::play(&world()?)],
        GzCommand::Pause => vec![control::pause(&world()?)],
        GzCommand::Step(n) => control::step(&world()?, *n),
        GzCommand::Reset => vec![control::reset(&world()?)],
        GzCommand::Stop => vec![control::stop()],
        GzCommand::Subscribe(topic) => return t.subscribe(topic),
        GzCommand::Unsubscribe(topic) => return t.unsubscribe(topic),
        GzCommand::Throttle(topic, rate) => return t.throttle(topic, *rate),
        GzCommand::Request {
            service,
            msg_type,
            payload,
        } => return t.request(service, msg_type, payload),
    };
    reqs.iter()
        .try_for_each(|r| t.request(&r.service, r.msg_type, &r.payload))
}

fn sim_stats(s: &msgs::WorldStatistics) -> SimStats {
    let secs = |t: &Option<msgs::Time>| {
        t.as_ref()
            .map_or(0.0, |t| t.sec as f64 + t.nsec as f64 * 1e-9)
    };
    SimStats {
        paused: s.paused,
        sim_time: secs(&s.sim_time),
        real_time_factor: s.real_time_factor,
        iterations: s.iterations,
    }
}

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;
    use gz_core::frame::build;
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
            let connector: Connector = Box::new(move || {
                let (driver, events, sent) = driver::fake();
                made.borrow_mut().push((events, sent));
                driver
            });
            let mut world = World::new();
            world.insert_non_send(GzConnection {
                transport: Transport::new(config.key.clone()),
                driver: Some(connector()),
                connector,
                fatal: None,
                opened_at: None,
            });
            world.insert_resource(config);
            world.insert_resource(WsAssetStore::default());
            world.insert_resource(Time::<Real>::default());
            world.insert_resource(GzInbox::default());
            world.init_resource::<Messages<GzCommand>>();
            world.init_resource::<Messages<GzPublish>>();
            world.init_resource::<Messages<GzServiceReply>>();
            world.insert_resource(GzStatus::new("ws://test"));
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
            self.feed(DriverEvent::Open);
            self.msg(b"protos".to_vec());
            self.msg(publish(
                "worlds",
                "gz.msgs.StringMsg_V",
                &msgs::StringMsgV {
                    data: vec!["w".into()],
                    ..Default::default()
                }
                .encode_to_vec(),
            ));
            let scene = msgs::Scene {
                name: "w".into(),
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
                "throttle,/world/w/stats,na,5",
            ]
        );
        let stats = msgs::WorldStatistics {
            paused: true,
            iterations: 42,
            real_time_factor: 0.5,
            sim_time: Some(msgs::Time {
                sec: 3,
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
                real_time_factor: 0.5,
                iterations: 42,
            })
        );
        h.msg(publish(
            "/world/w/stats",
            "gz.msgs.WorldStatistics",
            &[0xff],
        ));
        // Stats without a sim time read as 0.
        h.msg(publish(
            "/world/w/stats",
            "gz.msgs.WorldStatistics",
            &msgs::WorldStatistics::default().encode_to_vec(),
        ));
        assert_eq!(h.world.resource::<GzStatus>().sim.unwrap().sim_time, 0.0);
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
    }

    #[test]
    fn commands_become_requests() {
        let mut h = Harness::new();
        // Before the handshake nothing can be sent.
        assert!(h.command(GzCommand::Play).is_empty());
        assert!(h.command(GzCommand::Stop).is_empty());
        h.ready();
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
            // Refused: an empty payload would crash gz-launch.
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
        for _ in 0..1000 {
            sent = h.feed(DriverEvent::Message(Vec::new()));
            if !sent.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
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
        assert!(world.get_non_send::<GzConnection>().is_none());
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
        world.init_resource::<Messages<GzPublish>>();
        world.init_resource::<Messages<GzServiceReply>>();
        world.insert_resource(GzStatus::new(""));
        world.run_system_once(pump).unwrap();
    }
}
