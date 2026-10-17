//! Sans-IO client state machine for the gz websocket protocol.
//!
//! The platform drivers (tungstenite thread on native, `web_sys::WebSocket` on
//! wasm) feed raw messages into [`Transport::on_message`] and send whatever
//! [`Transport::poll_outgoing`] returns. Everything protocol-related lives here
//! so it behaves identically on both platforms and can be tested without a
//! socket.
//!
//! The handshake mirrors gzweb `Transport.ts`:
//!
//! ```text
//! open ─► auth,,,<key>  (if a key is set)  ─► "authorized" ─► protos,,,
//!     └─► protos,,,     (otherwise)
//! <proto definitions> ─► topics-types,,,  +  worlds,,,      status: Connected
//! pub,worlds          ─► scene,<world>,,
//! pub,scene           ─► Event::Scene                        status: Ready
//! ```

use std::collections::VecDeque;

use prost::Message;

use crate::frame::{self, Frame};
use gz_proto::msgs;

/// Connection status, matching gzweb's `status$` values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Disconnected,
    /// Socket open, waiting for the proto definitions.
    Connecting,
    /// Proto definitions received.
    Connected,
    /// Scene message received; the client can start working.
    Ready,
    Error(String),
}

/// Why an asset request failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetError {
    /// The server's `StringMsg` error code, e.g. `asset_not_found`.
    Server(String),
    /// The reply could not be decoded.
    Decode(String),
}

impl std::fmt::Display for AssetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AssetError::Server(code) => write!(f, "server error: {code}"),
            AssetError::Decode(e) => write!(f, "decode error: {e}"),
        }
    }
}

/// Something the application should react to.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Status(Status),
    /// The server rejected the auth key.
    AuthFailed,
    /// Reply to `topics-types`: (topic, msg_type) pairs.
    TopicsTypes(Vec<(String, String)>),
    /// Reply to `worlds`.
    Worlds(Vec<String>),
    /// Reply to `scene,<world>`.
    Scene(Box<msgs::Scene>),
    /// A message on a subscribed topic. The payload is still serialized.
    Publish {
        topic: String,
        msg_type: String,
        payload: Vec<u8>,
    },
    /// Reply to an `asset` request.
    Asset {
        uri: String,
        result: Result<Vec<u8>, AssetError>,
    },
    /// A frame that could not be parsed or decoded.
    Malformed(String),
}

pub struct Transport {
    key: Option<String>,
    status: Status,
    protos: Option<String>,
    world: Option<String>,
    outgoing: VecDeque<String>,
    events: VecDeque<Event>,
}

impl Transport {
    pub fn new(key: Option<String>) -> Self {
        Self {
            key: key.filter(|k| !k.is_empty()),
            status: Status::Disconnected,
            protos: None,
            world: None,
            outgoing: VecDeque::new(),
            events: VecDeque::new(),
        }
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    pub fn world(&self) -> Option<&str> {
        self.world.as_deref()
    }

    /// The proto definition text sent by the server, once received.
    pub fn protos(&self) -> Option<&str> {
        self.protos.as_deref()
    }

    /// Next frame to send to the server.
    pub fn poll_outgoing(&mut self) -> Option<String> {
        self.outgoing.pop_front()
    }

    /// Next event for the application.
    pub fn poll_event(&mut self) -> Option<Event> {
        self.events.pop_front()
    }

    /// The socket opened.
    pub fn on_open(&mut self) {
        self.protos = None;
        self.world = None;
        self.set_status(Status::Connecting);
        match self.key.clone() {
            Some(key) => self.send("auth", "", "", &key),
            None => self.send("protos", "", "", ""),
        }
    }

    /// The socket closed (cleanly or not).
    pub fn on_close(&mut self, error: Option<String>) {
        self.set_status(match error {
            Some(e) => Status::Error(e),
            None => Status::Disconnected,
        });
    }

    /// A message arrived. Text and binary frames are handled the same way,
    /// because the server sends the proto definitions as a binary frame.
    pub fn on_message(&mut self, data: &[u8]) {
        if self.protos.is_none() {
            self.on_handshake_message(data);
            return;
        }

        let Some(frame) = Frame::parse(data) else {
            self.events.push_back(Event::Malformed(format!(
                "frame without a 3-field header ({} bytes)",
                data.len()
            )));
            return;
        };

        match frame.op {
            "pub" => self.on_publish(frame),
            "asset" => self.on_asset(frame),
            // Service responses and anything else are not handled, as in gzweb.
            _ => {}
        }
    }

    pub fn subscribe(&mut self, topic: &str) {
        self.send("sub", topic, "", "");
    }

    pub fn unsubscribe(&mut self, topic: &str) {
        self.send("unsub", topic, "", "");
    }

    /// Limit the publication rate of a subscribed topic (messages per second).
    pub fn throttle(&mut self, topic: &str, rate: u32) {
        self.send("throttle", topic, "na", &rate.to_string());
    }

    pub fn request_asset(&mut self, uri: &str) {
        self.send("asset", "", "", uri);
    }

    fn send(&mut self, op: &str, topic: &str, msg_type: &str, payload: &str) {
        self.outgoing
            .push_back(frame::build(op, topic, msg_type, payload));
    }

    fn set_status(&mut self, status: Status) {
        if self.status != status {
            self.status = status.clone();
            self.events.push_back(Event::Status(status));
        }
    }

    /// Before the proto definitions arrive, every message is text: an auth
    /// reply or the definitions themselves.
    fn on_handshake_message(&mut self, data: &[u8]) {
        let text = String::from_utf8_lossy(data);
        match text.as_ref() {
            "authorized" => self.send("protos", "", "", ""),
            "invalid" => {
                self.events.push_back(Event::AuthFailed);
                self.set_status(Status::Error("invalid auth key".to_string()));
            }
            _ => {
                self.protos = Some(text.into_owned());
                self.send("topics-types", "", "", "");
                self.send("worlds", "", "", "");
                self.set_status(Status::Connected);
            }
        }
    }

    fn on_publish(&mut self, frame: Frame<'_>) {
        match frame.topic {
            "topics-types" => match msgs::Publishers::decode(frame.payload) {
                Ok(v) => {
                    let pairs = v
                        .publisher
                        .into_iter()
                        .map(|p| (p.topic, p.msg_type))
                        .collect();
                    self.events.push_back(Event::TopicsTypes(pairs));
                }
                Err(e) => self.malformed("topics-types", e),
            },
            "worlds" => match msgs::StringMsgV::decode(frame.payload) {
                Ok(v) => {
                    if let Some(world) = v.data.first() {
                        self.world = Some(world.clone());
                        self.send("scene", world, "", "");
                    }
                    self.events.push_back(Event::Worlds(v.data));
                }
                Err(e) => self.malformed("worlds", e),
            },
            "scene" => match msgs::Scene::decode(frame.payload) {
                Ok(scene) => {
                    self.events.push_back(Event::Scene(Box::new(scene)));
                    self.set_status(Status::Ready);
                }
                Err(e) => self.malformed("scene", e),
            },
            topic => self.events.push_back(Event::Publish {
                topic: topic.to_string(),
                msg_type: frame.msg_type.to_string(),
                payload: frame.payload.to_vec(),
            }),
        }
    }

    fn on_asset(&mut self, frame: Frame<'_>) {
        let result = if frame.msg_type.ends_with(".StringMsg") {
            match msgs::StringMsg::decode(frame.payload) {
                Ok(m) => Err(AssetError::Server(m.data)),
                Err(e) => Err(AssetError::Decode(e.to_string())),
            }
        } else {
            msgs::Bytes::decode(frame.payload)
                .map(|b| b.data)
                .map_err(|e| AssetError::Decode(e.to_string()))
        };
        self.events.push_back(Event::Asset {
            uri: frame.topic.to_string(),
            result,
        });
    }

    fn malformed(&mut self, what: &str, e: prost::DecodeError) {
        self.events
            .push_back(Event::Malformed(format!("failed to decode {what}: {e}")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::build_binary;

    fn drain_out(t: &mut Transport) -> Vec<String> {
        std::iter::from_fn(|| t.poll_outgoing()).collect()
    }

    fn drain_events(t: &mut Transport) -> Vec<Event> {
        std::iter::from_fn(|| t.poll_event()).collect()
    }

    fn worlds_frame(names: &[&str]) -> Vec<u8> {
        let msg = msgs::StringMsgV {
            data: names.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        };
        build_binary("pub", "worlds", "gz.msgs.StringMsg_V", &msg.encode_to_vec())
    }

    #[test]
    fn handshake_without_key() {
        let mut t = Transport::new(None);
        t.on_open();
        assert_eq!(drain_out(&mut t), ["protos,,,"]);

        t.on_message(b"syntax = \"proto3\";");
        assert_eq!(drain_out(&mut t), ["topics-types,,,", "worlds,,,"]);
        assert_eq!(t.status(), &Status::Connected);

        t.on_message(&worlds_frame(&["shapes"]));
        assert_eq!(drain_out(&mut t), ["scene,shapes,,"]);
        assert_eq!(t.world(), Some("shapes"));

        let scene = msgs::Scene {
            name: "shapes".into(),
            ..Default::default()
        };
        t.on_message(&build_binary(
            "pub",
            "scene",
            "gz.msgs.Scene",
            &scene.encode_to_vec(),
        ));
        assert_eq!(t.status(), &Status::Ready);
        let events = drain_events(&mut t);
        assert!(events.contains(&Event::Scene(Box::new(scene))));
        assert_eq!(events.last(), Some(&Event::Status(Status::Ready)));
    }

    #[test]
    fn handshake_with_key() {
        let mut t = Transport::new(Some("secret".into()));
        t.on_open();
        assert_eq!(drain_out(&mut t), ["auth,,,secret"]);
        t.on_message(b"authorized");
        assert_eq!(drain_out(&mut t), ["protos,,,"]);
        assert!(t.protos().is_none());
    }

    #[test]
    fn invalid_key() {
        let mut t = Transport::new(Some("bad".into()));
        t.on_open();
        t.on_message(b"invalid");
        assert!(drain_events(&mut t).contains(&Event::AuthFailed));
        assert!(matches!(t.status(), Status::Error(_)));
    }

    #[test]
    fn asset_reply_is_decoded_bytes() {
        let mut t = Transport::new(None);
        t.on_open();
        t.on_message(b"protos");
        drain_events(&mut t);

        let bytes = msgs::Bytes {
            data: b"solid x".to_vec(),
            ..Default::default()
        };
        t.on_message(&build_binary(
            "asset",
            "file:///m.stl",
            "gz.msgs.Bytes",
            &bytes.encode_to_vec(),
        ));
        let err = msgs::StringMsg {
            data: "asset_not_found".into(),
            ..Default::default()
        };
        t.on_message(&build_binary(
            "asset",
            "file:///x.dae",
            "gz.msgs.StringMsg",
            &err.encode_to_vec(),
        ));

        assert_eq!(
            drain_events(&mut t),
            [
                Event::Asset {
                    uri: "file:///m.stl".into(),
                    result: Ok(b"solid x".to_vec())
                },
                Event::Asset {
                    uri: "file:///x.dae".into(),
                    result: Err(AssetError::Server("asset_not_found".into()))
                },
            ]
        );
    }

    #[test]
    fn topic_messages_and_malformed_frames() {
        let mut t = Transport::new(None);
        t.on_open();
        t.on_message(b"protos");
        drain_events(&mut t);

        t.on_message(b"pub,/world/w/dynamic_pose/info,gz.msgs.Pose_V,\x0a\x00");
        t.on_message(b"garbage");
        let events = drain_events(&mut t);
        assert_eq!(
            events[0],
            Event::Publish {
                topic: "/world/w/dynamic_pose/info".into(),
                msg_type: "gz.msgs.Pose_V".into(),
                payload: b"\x0a\x00".to_vec(),
            }
        );
        assert!(matches!(events[1], Event::Malformed(_)));
    }

    #[test]
    fn requests() {
        let mut t = Transport::new(None);
        t.subscribe("/a");
        t.unsubscribe("/a");
        t.throttle("/a", 30);
        t.request_asset("model://m/meshes/a.dae");
        assert_eq!(
            drain_out(&mut t),
            [
                "sub,/a,,",
                "unsub,/a,,",
                "throttle,/a,na,30",
                "asset,,,model://m/meshes/a.dae"
            ]
        );
    }

    /// A transport that has finished the handshake up to `Connected`.
    fn connected() -> Transport {
        let mut t = Transport::new(None);
        t.on_open();
        t.on_message(b"protos");
        drain_out(&mut t);
        drain_events(&mut t);
        t
    }

    const GARBAGE: &[u8] = &[0xff, 0xff];

    #[test]
    fn undecodable_replies_are_malformed() {
        let mut t = connected();
        for topic in ["topics-types", "worlds", "scene"] {
            t.on_message(&build_binary("pub", topic, "gz.msgs.X", GARBAGE));
        }
        let events = drain_events(&mut t);
        assert_eq!(events.len(), 3, "{events:?}");
        for (e, what) in events.iter().zip(["topics-types", "worlds", "scene"]) {
            let prefix = format!("failed to decode {what}:");
            assert!(
                matches!(e, Event::Malformed(why) if why.starts_with(&prefix)),
                "{e:?}"
            );
        }
        // Nothing was requested and the status did not move.
        assert!(drain_out(&mut t).is_empty());
        assert_eq!(t.status(), &Status::Connected);
        assert_eq!(t.world(), None);
    }

    #[test]
    fn topics_types_are_decoded() {
        let mut t = connected();
        let msg = msgs::Publishers {
            publisher: vec![msgs::Publish {
                topic: "/clock".into(),
                msg_type: "gz.msgs.Clock".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        t.on_message(&build_binary(
            "pub",
            "topics-types",
            "gz.msgs.Publishers",
            &msg.encode_to_vec(),
        ));
        assert_eq!(
            drain_events(&mut t),
            [Event::TopicsTypes(vec![(
                "/clock".into(),
                "gz.msgs.Clock".into()
            )])]
        );
    }

    #[test]
    fn no_worlds_requests_no_scene() {
        let mut t = connected();
        t.on_message(&worlds_frame(&[]));
        assert!(drain_out(&mut t).is_empty());
        assert_eq!(drain_events(&mut t), [Event::Worlds(vec![])]);
        assert_eq!(t.world(), None);
    }

    #[test]
    fn undecodable_asset_replies() {
        let mut t = connected();
        t.on_message(&build_binary("asset", "/a.stl", "gz.msgs.Bytes", GARBAGE));
        t.on_message(&build_binary(
            "asset",
            "/b.stl",
            "gz.msgs.StringMsg",
            GARBAGE,
        ));
        let events = drain_events(&mut t);
        for (e, uri) in events.iter().zip(["/a.stl", "/b.stl"]) {
            assert!(
                matches!(e, Event::Asset { uri: u, result: Err(AssetError::Decode(_)) } if u == uri),
                "{e:?}"
            );
        }
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn asset_error_display() {
        assert_eq!(
            AssetError::Server("asset_not_found".into()).to_string(),
            "server error: asset_not_found"
        );
        assert_eq!(
            AssetError::Decode("bad".into()).to_string(),
            "decode error: bad"
        );
    }

    #[test]
    fn other_ops_are_ignored() {
        // Service replies and unknown ops are dropped, as in gzweb.
        let mut t = connected();
        t.on_message(b"srv,/world/w/control,gz.msgs.Boolean,\x08\x01");
        t.on_message(b"image,/camera,gz.msgs.Image,");
        assert!(drain_events(&mut t).is_empty());
        assert!(drain_out(&mut t).is_empty());
    }

    #[test]
    fn close_and_status_dedup() {
        let mut t = Transport::new(Some(String::new()));
        // An empty key means no auth.
        t.on_open();
        assert_eq!(drain_out(&mut t), ["protos,,,"]);
        t.on_close(None);
        assert_eq!(t.status(), &Status::Disconnected);
        t.on_close(Some("reset".into()));
        assert_eq!(t.status(), &Status::Error("reset".into()));
        // The same error again emits no second status event.
        t.on_close(Some("reset".into()));
        assert_eq!(
            drain_events(&mut t),
            [
                Event::Status(Status::Connecting),
                Event::Status(Status::Disconnected),
                Event::Status(Status::Error("reset".into())),
            ]
        );
    }

    #[test]
    fn reopening_restarts_the_handshake() {
        let mut t = connected();
        t.on_message(&worlds_frame(&["w"]));
        assert_eq!(t.protos(), Some("protos"));
        assert_eq!(t.world(), Some("w"));
        t.on_close(None);
        t.on_open();
        assert_eq!(t.protos(), None);
        assert_eq!(t.world(), None);
        assert_eq!(t.status(), &Status::Connecting);
        drain_out(&mut t);
        // The next message is treated as the proto definitions again.
        t.on_message(b"protos again");
        assert_eq!(drain_out(&mut t), ["topics-types,,,", "worlds,,,"]);
    }
}
