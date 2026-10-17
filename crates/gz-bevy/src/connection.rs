//! Drives the websocket: driver bytes → [`Transport`] → Bevy resources.
//!
//! Also owns reconnecting (with backoff, unless the auth key was rejected)
//! and the [`GzCommand`] API for simulation control and custom topics.

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
    driver: Driver,
    connector: Connector,
    /// The server rejected the key; retrying would fail the same way.
    auth_failed: bool,
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

/// Requests for the server, sent as soon as the connection allows. Failures
/// are logged.
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

pub fn connect(world: &mut World) {
    let config = world.resource::<GzConfig>().clone();
    info!("Connecting to {}", config.url);
    let mut status = GzStatus::new(config.url.clone());
    match connector(&config) {
        Ok(connector) => world.insert_non_send_resource(GzConnection {
            transport: Transport::new(config.key.clone()),
            driver: connector(),
            connector,
            auth_failed: false,
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
    let tls = match &config.ca_cert {
        Some(path) => Some(gz_core::net::tls_config(Some(path))?),
        None => None,
    };
    let url = config.url.clone();
    Ok(Box::new(move || driver::connect(&url, tls.clone())))
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
        conn.driver = (conn.connector)();
    }

    while let Ok(ev) = conn.driver.events.try_recv() {
        match ev {
            DriverEvent::Open => conn.transport.on_open(),
            DriverEvent::Message(data) => conn.transport.on_message(&data),
            DriverEvent::Closed(err) => {
                if let Some(e) = &err {
                    error!("websocket closed: {e}");
                }
                conn.transport.on_close(err);
                store.fail_all("connection closed");
                status.sim = None;
                if config.reconnect && !conn.auth_failed {
                    let wait = backoff(status.attempts);
                    info!("Reconnecting in {wait:.1} s");
                    status.reconnect_at = Some(now + wait);
                }
            }
        }
    }

    for uri in store.take_requests() {
        if let Err(e) = conn.transport.request_asset(&uri) {
            store.deliver(&uri, Err(e.to_string()));
        }
    }

    for cmd in commands.read() {
        if let Err(e) = send_command(&mut conn.transport, cmd) {
            warn!("{cmd:?} not sent: {e}");
        }
    }

    while let Some(event) = conn.transport.poll_event() {
        match event {
            Event::Status(s) => {
                info!("gz status: {s:?}");
                if s == Status::Ready {
                    status.attempts = 0;
                    if let Some(world) = conn.transport.world().map(str::to_string) {
                        subscribe(&mut conn.transport, &world, config.headless);
                    }
                }
                status.status = s;
            }
            Event::AuthFailed => {
                error!("websocket auth key rejected");
                conn.auth_failed = true;
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
            Event::Publish { topic, payload, .. } => {
                if topic.ends_with("/dynamic_pose/info") {
                    match msgs::PoseV::decode(payload.as_slice()) {
                        Ok(v) => inbox.poses.extend(v.pose),
                        Err(e) => warn!("bad Pose_V on {topic}: {e}"),
                    }
                } else if topic.ends_with("/scene/info") {
                    match msgs::Scene::decode(payload.as_slice()) {
                        Ok(s) => inbox.scene_updates.push(s),
                        Err(e) => warn!("bad Scene on {topic}: {e}"),
                    }
                    // Pose streams never stop on a live simulation, so only
                    // structural changes count as activity for settling.
                    inbox.last_activity_secs = now;
                } else if topic.ends_with("/stats") {
                    match msgs::WorldStatistics::decode(payload.as_slice()) {
                        Ok(s) => status.sim = Some(sim_stats(&s)),
                        Err(e) => warn!("bad WorldStatistics on {topic}: {e}"),
                    }
                }
            }
            Event::ServiceReply {
                service,
                msg_type,
                payload,
            } => match control::reply_ok(&msg_type, &payload) {
                Some(true) => debug!("{service}: ok"),
                _ => warn!("{service} failed ({msg_type}, {} bytes)", payload.len()),
            },
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

    while let Some(frame) = conn.transport.poll_outgoing() {
        conn.driver.send(frame);
    }
}

/// Same subscriptions, in the same order, as gzweb
/// `SceneManager.subscribeToTopics()`; interactive mode adds the stats.
fn subscribe(t: &mut Transport, world: &str, headless: bool) {
    let mut topics = vec![
        format!("/world/{world}/dynamic_pose/info"),
        format!("/world/{world}/scene/info"),
    ];
    if !headless {
        topics.push(format!("/world/{world}/stats"));
    }
    for topic in &topics {
        if let Err(e) = t.subscribe(topic) {
            warn!("subscribe {topic}: {e}");
        }
    }
    if !headless {
        let _ = t.throttle(&topics[2], STATS_RATE);
    }
}

fn send_command(t: &mut Transport, cmd: &GzCommand) -> Result<(), SendError> {
    let world = || t.world().map(str::to_string).ok_or(SendError::NotConnected);
    let req = match cmd {
        GzCommand::Play => control::play(&world()?),
        GzCommand::Pause => control::pause(&world()?),
        GzCommand::Step(n) => control::step(&world()?, *n),
        GzCommand::Reset => control::reset(&world()?),
        GzCommand::Stop => control::stop(),
        GzCommand::Subscribe(topic) => return t.subscribe(topic),
        GzCommand::Unsubscribe(topic) => return t.unsubscribe(topic),
        GzCommand::Throttle(topic, rate) => return t.throttle(topic, *rate),
        GzCommand::Request {
            service,
            msg_type,
            payload,
        } => return t.request(service, msg_type, payload),
    };
    t.request(&req.service, req.msg_type, &req.payload)
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
    use gz_core::frame::build_binary;
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
                headless: true,
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
            world.insert_non_send_resource(GzConnection {
                transport: Transport::new(config.key.clone()),
                driver: connector(),
                connector,
                auth_failed: false,
            });
            world.insert_resource(config);
            world.insert_resource(WsAssetStore::default());
            world.insert_resource(Time::<Real>::default());
            world.insert_resource(GzInbox::default());
            world.init_resource::<Messages<GzCommand>>();
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

        fn advance(&mut self, secs: f64) {
            let mut time = self.world.resource_mut::<Time<Real>>();
            // The first update only records the start instant.
            let now = time.last_update().unwrap_or_else(|| {
                let now = std::time::Instant::now();
                time.update_with_instant(now);
                now
            });
            time.update_with_instant(now + std::time::Duration::from_secs_f64(secs));
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

    fn publish(topic: &str, msg_type: &str, payload: &[u8]) -> Vec<u8> {
        build_binary("pub", topic, msg_type, payload)
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
            headless: false,
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
        // A close forgets them.
        h.feed(DriverEvent::Closed(None));
        assert_eq!(h.world.resource::<GzStatus>().sim, None);
        assert_eq!(sim_stats(&msgs::WorldStatistics::default()).sim_time, 0.0);
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
            GzCommand::Step(3),
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
                "req,/server_control",
                "sub,/clock",
                "throttle,/clock",
                "unsub,/clock",
                "req,/s",
            ]
        );

        // Replies are logged, good or bad.
        let ok = msgs::Boolean {
            data: true,
            ..Default::default()
        };
        h.msg(build_binary(
            "req",
            "/world/w/control",
            "gz.msgs.Boolean",
            &ok.encode_to_vec(),
        ));
        h.msg(build_binary("req", "/nope", "gz.msgs.StringMsg", b"x"));
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
        h.msg(build_binary(
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
        while store.waiting() == 0 {
            std::thread::yield_now();
        }
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
        while store.waiting() == 0 {
            std::thread::yield_now();
        }
        h.feed(DriverEvent::Closed(Some("reset".into())));
        assert_eq!(h.status(), Status::Error("reset".into()));
        assert_eq!(fetch.join().unwrap(), Err("connection closed".into()));
        // Reconnecting is off: nothing scheduled.
        assert_eq!(h.world.resource::<GzStatus>().reconnect_at, None);
    }

    #[test]
    fn reconnects_with_backoff_and_replaces_the_scene() {
        let mut h = Harness::with(GzConfig {
            headless: true,
            reconnect: true,
            ..Default::default()
        });
        h.ready();
        h.feed(DriverEvent::Closed(Some("reset".into())));
        let due = h.world.resource::<GzStatus>().reconnect_at.unwrap();
        // Not yet: still one driver.
        h.run();
        assert_eq!(h.drivers.borrow().len(), 1);
        h.advance(due + 0.01);
        h.run();
        assert_eq!(h.drivers.borrow().len(), 2);
        assert_eq!(h.world.resource::<GzStatus>().attempts, 1);

        // The second attempt fails too: a longer wait.
        h.feed(DriverEvent::Closed(Some("refused".into())));
        let next = h.world.resource::<GzStatus>().reconnect_at.unwrap();
        assert!(next - due > backoff(0), "{next} vs {due}");
        h.advance(next + 0.01);
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
            headless: true,
            reconnect: true,
            ..Default::default()
        });
        assert_eq!(h.feed(DriverEvent::Open), ["auth,,,k"]);
        h.msg(b"invalid".to_vec());
        assert_eq!(h.status(), Status::Error("invalid auth key".into()));
        h.feed(DriverEvent::Closed(None));
        assert_eq!(h.world.resource::<GzStatus>().reconnect_at, None);
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
        assert!(world.get_non_send_resource::<GzConnection>().is_none());
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
        world.insert_resource(GzStatus::new(""));
        world.run_system_once(pump).unwrap();
    }
}
