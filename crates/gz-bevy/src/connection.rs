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
