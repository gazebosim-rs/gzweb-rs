//! Drives the websocket: driver bytes → [`Transport`] → Bevy resources.

use bevy::prelude::*;
use gz_core::msgs;
use gz_core::transport::{Event, Status, Transport};
use prost::Message;

use crate::assets::WsAssetStore;
use crate::driver::{self, Driver, DriverEvent};
use crate::GzConfig;

/// The live connection. Non-`Send` because the wasm socket is a JS object.
pub struct GzConnection {
    transport: Transport,
    driver: Driver,
}

/// Connection state visible to the rest of the app.
#[derive(Resource, Debug)]
pub struct GzStatus {
    pub status: Status,
    pub url: String,
    pub world: Option<String>,
    /// Frames that could not be parsed or decoded.
    pub malformed: u32,
}

/// Decoded messages waiting to be applied to the ECS world.
#[derive(Resource, Default)]
pub struct GzInbox {
    /// The initial scene (reply to `scene,<world>`).
    pub scene: Option<Box<msgs::Scene>>,
    /// `scene/info` updates, in arrival order.
    pub scene_updates: Vec<msgs::Scene>,
    /// `dynamic_pose/info` poses, in arrival order.
    pub poses: Vec<msgs::Pose>,
    /// Real time of the last message that changes the scene; used to decide
    /// when the scene has settled.
    pub last_activity_secs: f64,
}

pub fn connect(world: &mut World) {
    let config = world.resource::<GzConfig>().clone();
    info!("Connecting to {}", config.url);
    let conn = GzConnection {
        transport: Transport::new(config.key.clone()),
        driver: driver::connect(&config.url),
    };
    world.insert_non_send_resource(conn);
    world.insert_resource(GzStatus {
        status: Status::Disconnected,
        url: config.url,
        world: None,
        malformed: 0,
    });
}

pub fn pump(
    conn: Option<NonSendMut<GzConnection>>,
    store: Res<WsAssetStore>,
    time: Res<Time<Real>>,
    mut status: ResMut<GzStatus>,
    mut inbox: ResMut<GzInbox>,
) {
    let Some(mut conn) = conn else { return };
    let conn = &mut *conn;

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
            }
        }
    }

    for uri in store.take_requests() {
        conn.transport.request_asset(&uri);
    }

    while let Some(event) = conn.transport.poll_event() {
        match event {
            Event::Status(s) => {
                info!("gz status: {s:?}");
                if s == Status::Ready {
                    // Same subscriptions, in the same order, as gzweb
                    // SceneManager.subscribeToTopics().
                    if let Some(world) = conn.transport.world().map(str::to_string) {
                        conn.transport
                            .subscribe(&format!("/world/{world}/dynamic_pose/info"));
                        conn.transport
                            .subscribe(&format!("/world/{world}/scene/info"));
                    }
                }
                status.status = s;
            }
            Event::AuthFailed => error!("websocket auth key rejected"),
            Event::Worlds(worlds) => {
                info!("worlds: {worlds:?}");
                status.world = worlds.first().cloned();
            }
            Event::TopicsTypes(_) => {}
            Event::Scene(scene) => {
                info!(
                    "scene '{}': {} models, {} lights",
                    scene.name,
                    scene.model.len(),
                    scene.light.len()
                );
                inbox.scene = Some(scene);
                inbox.last_activity_secs = time.elapsed_secs_f64();
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
                    inbox.last_activity_secs = time.elapsed_secs_f64();
                }
            }
            Event::Asset { uri, result } => {
                store.deliver(&uri, result.map_err(|e| e.to_string()));
                inbox.last_activity_secs = time.elapsed_secs_f64();
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

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;
    use gz_core::frame::build_binary;

    struct Harness {
        world: World,
        events: crossbeam_channel::Sender<DriverEvent>,
        sent: crossbeam_channel::Receiver<String>,
    }

    impl Harness {
        fn new() -> Harness {
            // Logging at trace level makes the `info!` arguments run too.
            let _ = bevy::log::tracing_subscriber::fmt()
                .with_max_level(bevy::log::Level::TRACE)
                .with_test_writer()
                .try_init();
            let (driver, events, sent) = driver::fake();
            let mut world = World::new();
            world.insert_non_send_resource(GzConnection {
                transport: Transport::new(None),
                driver,
            });
            world.insert_resource(WsAssetStore::default());
            world.insert_resource(Time::<Real>::default());
            world.insert_resource(GzInbox::default());
            world.insert_resource(GzStatus {
                status: Status::Disconnected,
                url: "ws://test".into(),
                world: None,
                malformed: 0,
            });
            Harness {
                world,
                events,
                sent,
            }
        }

        fn feed(&mut self, ev: DriverEvent) -> Vec<String> {
            self.events.send(ev).unwrap();
            self.world.run_system_once(pump).unwrap();
            self.sent.try_iter().collect()
        }

        fn msg(&mut self, frame: Vec<u8>) -> Vec<String> {
            self.feed(DriverEvent::Message(frame))
        }

        fn status(&self) -> Status {
            self.world.resource::<GzStatus>().status.clone()
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
        h.msg(publish("/world/w/stats", "gz.msgs.WorldStatistics", &[]));
        let inbox = h.world.resource::<GzInbox>();
        assert_eq!(inbox.poses.len(), 2);
        assert_eq!(inbox.scene_updates.len(), 1);

        // Malformed frames are counted.
        h.msg(b"nonsense".to_vec());
        assert_eq!(h.world.resource::<GzStatus>().malformed, 1);
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
    }

    #[test]
    fn auth_rejection_is_an_error() {
        let mut h = Harness::new();
        h.world.non_send_resource_mut::<GzConnection>().transport =
            Transport::new(Some("k".into()));
        assert_eq!(h.feed(DriverEvent::Open), ["auth,,,k"]);
        h.msg(b"invalid".to_vec());
        assert_eq!(h.status(), Status::Error("invalid auth key".into()));
    }

    #[test]
    fn pump_without_a_connection_does_nothing() {
        let mut world = World::new();
        world.insert_resource(WsAssetStore::default());
        world.insert_resource(Time::<Real>::default());
        world.insert_resource(GzInbox::default());
        world.insert_resource(GzStatus {
            status: Status::Disconnected,
            url: String::new(),
            world: None,
            malformed: 0,
        });
        world.run_system_once(pump).unwrap();
    }
}
