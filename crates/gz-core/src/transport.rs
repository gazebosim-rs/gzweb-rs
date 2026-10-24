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
//!
//! Like gzweb's `sendMessage`, nothing but the handshake is sent before the
//! proto definitions arrive, and nothing at all after an error.
//!
//! ## What the server can carry
//!
//! Jetty's websocket system (`gz-sim-websocket-server-system`, gz-sim 10) and
//! the gz-launch `WebsocketServer` it was ported from (7 to `main`) read every
//! frame as a C string and split it with `strtok` on `,`, which merges empty
//! fields. So:
//!
//! - any field is cut at its first `0x00` byte, and a `,` (`0x2c`) in any
//!   field makes the frame invalid (dropped without a reply);
//! - an empty field the server indexes (the topic of `sub`/`image`/`unsub`/
//!   `throttle`, the service, type or payload of `req`) is an out-of-bounds
//!   read, which crashes it (on Jetty, the whole simulation);
//! - `throttle` with rate 0 divides by zero.
//!
//! [`Transport`] refuses all of these with [`SendError::Unsendable`], except
//! for a server that takes binary requests (patched, gazebosim/gz-sim#4093:
//! it keeps the frame's length and a `req`'s payload after its third comma
//! whole). [`Transport::probe_binary_requests`] finds out, and
//! [`Transport::set_binary_requests`] then lets `req` payloads hold any
//! bytes (DECISIONS.md D-76). No released server has an `adv` or `pub_in`
//! handler, so [`Transport::advertise`] and [`Transport::publish`]
//! (gzweb's API) do nothing there; the patched server (client-publish
//! patch) publishes a `pub_in` frame's payload, kept whole after its third
//! comma. [`Transport::probe_publishing`] finds out, and
//! [`Transport::set_publishing`] then lets `pub_in` payloads hold any
//! bytes (DECISIONS.md D-104).
//!
//! Jetty answers `auth` even without a key configured (`authorized`, for any
//! key); gz-launch didn't answer it at all.

use std::collections::{HashMap, HashSet, VecDeque};

use prost::Message;

use crate::frame::{self, Frame};
use gz_proto::msgs;

/// Connection status, matching gzweb's `status$` values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Disconnected,
    /// Socket open, waiting for the proto definitions. gzweb reports this as
    /// `disconnected`.
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

impl std::error::Error for AssetError {}

impl std::fmt::Display for AssetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AssetError::Server(code) => write!(f, "server error: {code}"),
            AssetError::Decode(e) => write!(f, "decode error: {e}"),
        }
    }
}

/// Why a frame was not queued.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendError {
    /// The handshake has not finished (or the socket is closed).
    NotConnected,
    /// The connection failed; nothing is sent until it reopens.
    Failed,
    /// The server would drop this frame or crash on it (see the module docs).
    Unsendable(&'static str),
}

impl std::error::Error for SendError {}

impl std::fmt::Display for SendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SendError::NotConnected => write!(f, "not connected"),
            SendError::Failed => write!(f, "connection failed"),
            SendError::Unsendable(why) => write!(f, "frame can't be sent: {why}"),
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
    /// Reply to `topics`: topic names.
    Topics(Vec<String>),
    /// Reply to `worlds`.
    Worlds(Vec<String>),
    /// Reply to `scene,<world>`.
    Scene(Box<msgs::Scene>),
    /// A message on a subscribed topic. The payload is still serialized (raw
    /// PNG data for `image` subscriptions).
    Publish {
        topic: String,
        msg_type: String,
        payload: Vec<u8>,
    },
    /// Reply to a `req`. `msg_type` is `gz.msgs.StringMsg` with
    /// `service_not_found` when nothing offers the service. gzweb ignores
    /// these.
    ServiceReply {
        service: String,
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
    /// topic → message type, from `topics-types`.
    topic_types: HashMap<String, String>,
    subscriptions: HashSet<String>,
    outgoing: VecDeque<Vec<u8>>,
    events: VecDeque<Event>,
    /// The server keeps binary `req` payloads whole.
    binary_requests: bool,
    /// The server publishes `pub_in` payloads whole (D-104).
    publishing: bool,
}

/// The service the binary-request probe calls: gz-sim's world list, which
/// takes a `gz.msgs.Empty` and has no effect.
pub const BINARY_PROBE_SERVICE: &str = "/gazebo/worlds";

/// The topic [`Transport::probe_publishing`] publishes on.
pub const PUBLISH_PROBE_TOPIC: &str = "/gzweb/publish_probe";

/// Whether a `topics-types` reply after [`Transport::probe_publishing`]
/// shows the server published the probe.
pub fn topics_show_publishing(pairs: &[(String, String)]) -> bool {
    pairs.iter().any(|(t, _)| t == PUBLISH_PROBE_TOPIC)
}

/// An Empty with an unknown field 15 of two bytes, a zero and a comma: a
/// server that cuts the frame at the zero byte passes on a truncated field,
/// which fails to parse, and replies with nothing.
const BINARY_PROBE: &[u8] = &[0x7a, 0x02, 0x00, b','];

/// Whether a probe's reply says the server keeps binary requests whole:
/// the world list, which a mangled request doesn't get. Any other type
/// (`gz.msgs.StringMsg` "service_not_found", which also decodes as a
/// one-entry list) is a no.
pub fn probe_reply_supports_binary(msg_type: &str, payload: &[u8]) -> bool {
    msg_type == "gz.msgs.StringMsg_V"
        && msgs::StringMsgV::decode(payload).is_ok_and(|v| !v.data.is_empty())
}

impl Transport {
    pub fn new(key: Option<String>) -> Self {
        Self {
            key: key.filter(|k| !k.is_empty()),
            status: Status::Disconnected,
            protos: None,
            world: None,
            topic_types: HashMap::new(),
            subscriptions: HashSet::new(),
            outgoing: VecDeque::new(),
            events: VecDeque::new(),
            binary_requests: false,
            publishing: false,
        }
    }

    /// Lets `pub_in` payloads hold any bytes (the server publishes them
    /// whole), or not. Off again with every new connection.
    pub fn set_publishing(&mut self, on: bool) {
        self.publishing = on;
    }

    pub fn publishing(&self) -> bool {
        self.publishing
    }

    /// Asks whether the server publishes what the client sends: an empty
    /// `gz.msgs.Empty` on [`PUBLISH_PROBE_TOPIC`] (harmless, and ignored by
    /// a server without `pub_in`), then the topic list, which holds the
    /// probe's topic once the server has advertised it; see
    /// [`topics_show_publishing`].
    pub fn probe_publishing(&mut self) -> Result<(), SendError> {
        self.queue("pub_in", PUBLISH_PROBE_TOPIC, "gz.msgs.Empty", b"", true)?;
        self.request_topics_types()
    }

    /// Lets `req` payloads hold any bytes (the server keeps them whole), or
    /// not. Off again with every new connection.
    pub fn set_binary_requests(&mut self, on: bool) {
        self.binary_requests = on;
    }

    pub fn binary_requests(&self) -> bool {
        self.binary_requests
    }

    /// Asks whether the server keeps binary requests whole: a request to
    /// [`BINARY_PROBE_SERVICE`] whose payload a stock server mangles; see
    /// [`probe_reply_supports_binary`].
    pub fn probe_binary_requests(&mut self) -> Result<(), SendError> {
        self.queue(
            "req",
            BINARY_PROBE_SERVICE,
            "gz.msgs.Empty",
            BINARY_PROBE,
            true,
        )
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

    /// Message type of `topic`, if `topics-types` listed it.
    pub fn topic_type(&self, topic: &str) -> Option<&str> {
        self.topic_types.get(topic).map(String::as_str)
    }

    pub fn is_subscribed(&self, topic: &str) -> bool {
        self.subscriptions.contains(topic)
    }

    /// Next frame to send to the server.
    pub fn poll_outgoing(&mut self) -> Option<Vec<u8>> {
        self.outgoing.pop_front()
    }

    /// Next event for the application.
    pub fn poll_event(&mut self) -> Option<Event> {
        self.events.pop_front()
    }

    /// The socket opened.
    pub fn on_open(&mut self) {
        self.reset();
        self.set_status(Status::Connecting);
        // Handshake ops are always allowed while connecting; only a key
        // the server can't carry fails.
        let sent = match self.key.clone() {
            Some(key) => self.send("auth", "", "", key.as_bytes()),
            None => self.send("protos", "", "", b""),
        };
        if let Err(e) = sent {
            self.set_status(Status::Error(format!("auth key: {e}")));
        }
    }

    /// The socket closed (cleanly or not). Like gzweb's `onClose`, this
    /// forgets the subscriptions and topic list.
    pub fn on_close(&mut self, error: Option<String>) {
        self.reset();
        self.set_status(match error {
            Some(e) => Status::Error(e),
            None => Status::Disconnected,
        });
    }

    fn reset(&mut self) {
        self.protos = None;
        self.world = None;
        self.topic_types.clear();
        self.subscriptions.clear();
        self.outgoing.clear();
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
            "req" => self.events.push_back(Event::ServiceReply {
                service: frame.topic.to_string(),
                msg_type: frame.msg_type.to_string(),
                payload: frame.payload.to_vec(),
            }),
            // Anything else is not handled, as in gzweb.
            _ => {}
        }
    }

    /// Subscribes to `topic`. Topics that `topics-types` lists as
    /// `gz.msgs.Image` use the `image` op, which streams PNG data. (gzweb
    /// checks for the `ignition.`/`gazebo.` names, which neither Harmonic nor
    /// Jetty reports, so it always sends `sub`.)
    pub fn subscribe(&mut self, topic: &str) -> Result<(), SendError> {
        let op = match self.topic_type(topic) {
            Some(t) if t.ends_with(".msgs.Image") => "image",
            _ => "sub",
        };
        self.send(op, topic, "", b"")?;
        self.subscriptions.insert(topic.to_string());
        Ok(())
    }

    /// Unsubscribes from `topic`; like gzweb, only if subscribed.
    pub fn unsubscribe(&mut self, topic: &str) -> Result<(), SendError> {
        if self.subscriptions.contains(topic) {
            self.send("unsub", topic, "", b"")?;
            self.subscriptions.remove(topic);
        }
        Ok(())
    }

    /// Limit the publication rate of a subscribed topic (messages per second,
    /// at least 1).
    pub fn throttle(&mut self, topic: &str, rate: u32) -> Result<(), SendError> {
        if rate == 0 {
            return Err(SendError::Unsendable(
                "rate 0 (divides by zero in the server)",
            ));
        }
        self.send("throttle", topic, "na", rate.to_string().as_bytes())
    }

    /// Asks for every topic and its message type again
    /// (`Event::TopicsTypes`); the handshake asks once.
    pub fn request_topics_types(&mut self) -> Result<(), SendError> {
        self.send("topics-types", "", "", b"")
    }

    /// Asks for the list of topic names (`Event::Topics`).
    pub fn request_topics(&mut self) -> Result<(), SendError> {
        self.send("topics", "", "", b"")
    }

    pub fn request_asset(&mut self, uri: &str) -> Result<(), SendError> {
        self.send("asset", "", "", uri.as_bytes())
    }

    /// gzweb `advertise`. No released server (gz-launch or Jetty) handles `adv`.
    pub fn advertise(&mut self, topic: &str, msg_type: &str) -> Result<(), SendError> {
        self.send("adv", topic, msg_type, b"")
    }

    /// gzweb `publish` (`pub_in`): publishes on a patched server
    /// ([`Transport::probe_publishing`]); no released server (gz-launch or
    /// Jetty) handles it.
    pub fn publish(
        &mut self,
        topic: &str,
        msg_type: &str,
        payload: &[u8],
    ) -> Result<(), SendError> {
        self.send("pub_in", topic, msg_type, payload)
    }

    /// Calls `service` with a serialized `msg_type` request. The reply comes
    /// back as [`Event::ServiceReply`]. Payloads the server would mangle are
    /// refused; see the module docs and [`crate::control`].
    pub fn request(
        &mut self,
        service: &str,
        msg_type: &str,
        payload: &[u8],
    ) -> Result<(), SendError> {
        self.send("req", service, msg_type, payload)
    }

    /// Queues a frame if the connection state allows it, as gzweb's
    /// `sendMessage` does.
    fn send(
        &mut self,
        op: &str,
        topic: &str,
        msg_type: &str,
        payload: &[u8],
    ) -> Result<(), SendError> {
        let binary = (op == "req" && self.binary_requests) || (op == "pub_in" && self.publishing);
        self.queue(op, topic, msg_type, payload, binary)
    }

    /// Queues a frame; `binary`: a `req` whose payload may hold any bytes.
    fn queue(
        &mut self,
        op: &str,
        topic: &str,
        msg_type: &str,
        payload: &[u8],
        binary: bool,
    ) -> Result<(), SendError> {
        let handshake = matches!(op, "auth" | "protos" | "topics-types" | "worlds");
        match self.status {
            Status::Error(_) => return Err(SendError::Failed),
            Status::Disconnected | Status::Connecting if !handshake => {
                return Err(SendError::NotConnected)
            }
            _ => {}
        }
        check_frame(op, topic, msg_type, payload, binary)?;
        self.outgoing
            .push_back(frame::build(op, topic, msg_type, payload));
        Ok(())
    }

    fn set_status(&mut self, status: Status) {
        if !matches!(status, Status::Ready) {
            self.binary_requests = false;
            self.publishing = false;
        }
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
            "authorized" => {
                let _ = self.send("protos", "", "", b"");
            }
            "invalid" => {
                self.events.push_back(Event::AuthFailed);
                self.set_status(Status::Error("invalid auth key".to_string()));
            }
            _ => {
                self.protos = Some(text.into_owned());
                let _ = self.send("topics-types", "", "", b"");
                let _ = self.send("worlds", "", "", b"");
                self.set_status(Status::Connected);
            }
        }
    }

    fn on_publish(&mut self, frame: Frame<'_>) {
        match frame.topic {
            "topics-types" => match msgs::Publishers::decode(frame.payload) {
                Ok(v) => {
                    let pairs: Vec<_> = v
                        .publisher
                        .into_iter()
                        .map(|p| (p.topic, p.msg_type))
                        .collect();
                    // The whole list: topics no longer in it have ended.
                    self.topic_types = pairs.iter().cloned().collect();
                    self.events.push_back(Event::TopicsTypes(pairs));
                }
                Err(e) => self.malformed("topics-types", e),
            },
            "topics" => match msgs::StringMsgV::decode(frame.payload) {
                Ok(v) => self.events.push_back(Event::Topics(v.data)),
                Err(e) => self.malformed("topics", e),
            },
            "worlds" => match msgs::StringMsgV::decode(frame.payload) {
                Ok(v) => {
                    if let Some(world) = v.data.first() {
                        self.world = Some(world.clone());
                        let _ = self.send("scene", world, "", b"");
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
            // Like gzweb, only topics we are subscribed to.
            topic if self.subscriptions.contains(topic) => self.events.push_back(Event::Publish {
                topic: topic.to_string(),
                msg_type: frame.msg_type.to_string(),
                payload: frame.payload.to_vec(),
            }),
            _ => {}
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

/// Refuses frames the server would drop or crash on (see the module docs);
/// `binary`: a `req` to a server that keeps its payload whole, any bytes
/// and empty included.
fn check_frame(
    op: &str,
    topic: &str,
    msg_type: &str,
    payload: &[u8],
    binary: bool,
) -> Result<(), SendError> {
    let bad = |b: &[u8]| b.iter().any(|&c| c == 0 || c == b',');
    if bad(topic.as_bytes()) || bad(msg_type.as_bytes()) || (!binary && bad(payload)) {
        return Err(SendError::Unsendable("a field contains ',' or 0x00"));
    }
    let needs_topic = matches!(
        op,
        "sub" | "image" | "unsub" | "throttle" | "req" | "scene" | "pub_in"
    );
    if needs_topic && topic.is_empty() {
        return Err(SendError::Unsendable("empty topic or service"));
    }
    if op == "req" && (msg_type.is_empty() || (!binary && payload.is_empty())) {
        return Err(SendError::Unsendable("empty request type or payload"));
    }
    if op == "pub_in" && msg_type.is_empty() {
        return Err(SendError::Unsendable("empty message type"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::build;

    fn drain_out(t: &mut Transport) -> Vec<String> {
        std::iter::from_fn(|| t.poll_outgoing())
            .map(|f| String::from_utf8_lossy(&f).into_owned())
            .collect()
    }

    fn drain_events(t: &mut Transport) -> Vec<Event> {
        std::iter::from_fn(|| t.poll_event()).collect()
    }

    fn worlds_frame(names: &[&str]) -> Vec<u8> {
        let msg = msgs::StringMsgV {
            data: names.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        };
        build("pub", "worlds", "gz.msgs.StringMsg_V", &msg.encode_to_vec())
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
        t.on_message(&build(
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
        drain_out(&mut t);
        t.on_message(b"invalid");
        assert!(drain_events(&mut t).contains(&Event::AuthFailed));
        assert!(matches!(t.status(), Status::Error(_)));
        // Nothing more is sent after an error.
        assert_eq!(t.request_topics(), Err(SendError::Failed));
        t.on_message(b"authorized");
        assert!(drain_out(&mut t).is_empty());
    }

    #[test]
    fn asset_reply_is_decoded_bytes() {
        let mut t = connected();

        let bytes = msgs::Bytes {
            data: b"solid x".to_vec(),
            ..Default::default()
        };
        t.on_message(&build(
            "asset",
            "file:///m.stl",
            "gz.msgs.Bytes",
            &bytes.encode_to_vec(),
        ));
        let err = msgs::StringMsg {
            data: "asset_not_found".into(),
            ..Default::default()
        };
        t.on_message(&build(
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
        let mut t = connected();
        t.subscribe("/world/w/dynamic_pose/info").unwrap();
        t.on_message(b"pub,/world/w/dynamic_pose/info,gz.msgs.Pose_V,\x0a\x00");
        // Not subscribed: dropped, as in gzweb.
        t.on_message(b"pub,/other,gz.msgs.Clock,");
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
        let mut t = connected();
        t.subscribe("/a").unwrap();
        assert!(t.is_subscribed("/a"));
        t.unsubscribe("/a").unwrap();
        assert!(!t.is_subscribed("/a"));
        // Not subscribed any more: nothing to send.
        t.unsubscribe("/a").unwrap();
        t.throttle("/a", 30).unwrap();
        t.request_asset("model://m/meshes/a.dae").unwrap();
        t.request_topics().unwrap();
        t.advertise("/cmd", "gz.msgs.Twist").unwrap();
        t.publish("/cmd", "gz.msgs.Twist", b"\x12\x02").unwrap();
        t.request("/world/w/control", "gz.msgs.WorldControl", b"\x10\x01")
            .unwrap();
        assert_eq!(
            drain_out(&mut t),
            [
                "sub,/a,,",
                "unsub,/a,,",
                "throttle,/a,na,30",
                "asset,,,model://m/meshes/a.dae",
                "topics,,,",
                "adv,/cmd,gz.msgs.Twist,",
                "pub_in,/cmd,gz.msgs.Twist,\x12\x02",
                "req,/world/w/control,gz.msgs.WorldControl,\x10\x01",
            ]
        );
    }

    #[test]
    fn frames_the_server_would_mangle_are_refused() {
        let mut t = connected();
        let svc = "/world/w/control";
        let ty = "gz.msgs.WorldControl";
        let field = Err(SendError::Unsendable("a field contains ',' or 0x00"));
        let empty_topic = Err(SendError::Unsendable("empty topic or service"));
        let empty_req = Err(SendError::Unsendable("empty request type or payload"));
        assert_eq!(t.request(svc, ty, b"\x10\x00"), field);
        assert_eq!(t.request(svc, ty, b"\x20\x2c"), field);
        assert_eq!(t.request(svc, ty, b""), empty_req);
        assert_eq!(t.request(svc, "", b"\x10\x01"), empty_req);
        assert_eq!(t.request("", ty, b"\x10\x01"), empty_topic);
        assert_eq!(t.request("/a,b", ty, b"\x10\x01"), field);
        assert_eq!(t.subscribe(""), empty_topic);
        assert_eq!(t.subscribe("/a\0"), field);
        assert!(!t.is_subscribed(""));
        assert_eq!(t.throttle("", 5), empty_topic);
        assert_eq!(
            t.throttle("/a", 0),
            Err(SendError::Unsendable(
                "rate 0 (divides by zero in the server)"
            ))
        );
        assert_eq!(t.request_asset("/m,1.stl"), field);
        assert_eq!(t.advertise("/cmd", "gz.msgs.A,B"), field);
        assert!(drain_out(&mut t).is_empty());
        // An empty world name would make `scene,,,`: no scene request.
        t.on_message(&worlds_frame(&[""]));
        assert!(drain_out(&mut t).is_empty());
    }

    /// The publish probe is an empty publication and a topic list; a
    /// publication's zero bytes go whole only once the server publishes,
    /// and it needs a topic and a type.
    #[test]
    fn publications_go_whole_once_the_server_publishes() {
        let mut t = connected();
        t.probe_publishing().unwrap();
        assert_eq!(
            std::iter::from_fn(|| t.poll_outgoing()).collect::<Vec<_>>(),
            [
                build("pub_in", PUBLISH_PROBE_TOPIC, "gz.msgs.Empty", b""),
                build("topics-types", "", "", b""),
            ]
        );
        let pairs = |t: &str| vec![(t.to_string(), "gz.msgs.Empty".to_string())];
        assert!(topics_show_publishing(&pairs(PUBLISH_PROBE_TOPIC)));
        assert!(!topics_show_publishing(&pairs("/other")));
        let twist = b"\x0a\x09\x09\x00\x00\x00\x00\x00\x00\xf0\x3f";
        let field = Err(SendError::Unsendable("a field contains ',' or 0x00"));
        assert_eq!(t.publish("/cmd_vel", "gz.msgs.Twist", twist), field);
        t.set_publishing(true);
        assert!(t.publishing());
        t.publish("/cmd_vel", "gz.msgs.Twist", twist).unwrap();
        assert_eq!(
            t.poll_outgoing(),
            Some(build("pub_in", "/cmd_vel", "gz.msgs.Twist", twist))
        );
        assert_eq!(
            t.publish("", "gz.msgs.Twist", b"x"),
            Err(SendError::Unsendable("empty topic or service"))
        );
        assert_eq!(
            t.publish("/cmd_vel", "", b"x"),
            Err(SendError::Unsendable("empty message type"))
        );
        t.on_open();
        assert!(!t.publishing());
    }

    #[test]
    fn binary_requests_go_whole_once_the_server_takes_them() {
        let mut t = connected();
        // The probe goes out whatever the switch says, its zero and comma
        // intact.
        t.probe_binary_requests().unwrap();
        assert_eq!(
            std::iter::from_fn(|| t.poll_outgoing()).collect::<Vec<_>>(),
            [build(
                "req",
                BINARY_PROBE_SERVICE,
                "gz.msgs.Empty",
                BINARY_PROBE
            )]
        );
        let svc = "/world/w/set_pose";
        let pose = b"\x12\x00,\x01";
        let field = Err(SendError::Unsendable("a field contains ',' or 0x00"));
        assert_eq!(t.request(svc, "gz.msgs.Pose", pose), field);
        t.set_binary_requests(true);
        assert!(t.binary_requests());
        t.request(svc, "gz.msgs.Pose", pose).unwrap();
        t.request(svc, "gz.msgs.Empty", b"").unwrap();
        assert_eq!(
            std::iter::from_fn(|| t.poll_outgoing()).collect::<Vec<_>>(),
            [
                build("req", svc, "gz.msgs.Pose", pose),
                build("req", svc, "gz.msgs.Empty", b""),
            ]
        );
        // Still refused: a comma in the service, and other ops' fields.
        assert_eq!(t.request("/a,b", "gz.msgs.Pose", pose), field);
        assert_eq!(t.subscribe("/a\0"), field);
        // A new connection starts without.
        t.on_open();
        assert!(!t.binary_requests());
        // The reply: the world list from a server that kept the probe whole,
        // nothing from one that cut it.
        let worlds = msgs::StringMsgV {
            data: vec!["w".into()],
            ..Default::default()
        };
        let v = "gz.msgs.StringMsg_V";
        assert!(probe_reply_supports_binary(v, &worlds.encode_to_vec()));
        assert!(!probe_reply_supports_binary(v, b""));
        assert!(!probe_reply_supports_binary(v, &[0xff, 0xff]));
        // A server that can't find the service yet says so in a StringMsg,
        // which also parses as a list of one.
        let not_found = msgs::StringMsg {
            data: "service_not_found".into(),
            ..Default::default()
        };
        assert!(!probe_reply_supports_binary(
            "gz.msgs.StringMsg",
            &not_found.encode_to_vec()
        ));
    }

    #[test]
    fn an_unsendable_key_is_an_error() {
        let mut t = Transport::new(Some("a,b".into()));
        t.on_open();
        assert!(drain_out(&mut t).is_empty());
        assert!(matches!(t.status(), Status::Error(e) if e.starts_with("auth key:")));
    }

    #[test]
    fn nothing_but_the_handshake_before_connected() {
        let mut t = Transport::new(None);
        assert_eq!(t.subscribe("/a"), Err(SendError::NotConnected));
        assert!(!t.is_subscribed("/a"));
        t.on_open();
        drain_out(&mut t);
        assert_eq!(t.request_asset("/m.stl"), Err(SendError::NotConnected));
        assert!(drain_out(&mut t).is_empty());
    }

    #[test]
    fn image_topics_use_the_image_op() {
        let mut t = connected();
        let msg = msgs::Publishers {
            publisher: vec![
                msgs::Publish {
                    topic: "/camera".into(),
                    msg_type: "gz.msgs.Image".into(),
                    ..Default::default()
                },
                msgs::Publish {
                    topic: "/clock".into(),
                    msg_type: "gz.msgs.Clock".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        t.on_message(&build(
            "pub",
            "topics-types",
            "gz.msgs.Publishers",
            &msg.encode_to_vec(),
        ));
        assert_eq!(t.topic_type("/camera"), Some("gz.msgs.Image"));
        t.subscribe("/camera").unwrap();
        t.subscribe("/clock").unwrap();
        t.subscribe("/unknown").unwrap();
        assert_eq!(
            drain_out(&mut t),
            ["image,/camera,,", "sub,/clock,,", "sub,/unknown,,"]
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
        let topics = ["topics-types", "topics", "worlds", "scene"];
        for topic in topics {
            t.on_message(&build("pub", topic, "gz.msgs.X", GARBAGE));
        }
        let events = drain_events(&mut t);
        assert_eq!(events.len(), topics.len(), "{events:?}");
        for (e, what) in events.iter().zip(topics) {
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
    fn topics_and_topic_types_are_decoded() {
        let mut t = connected();
        let msg = msgs::Publishers {
            publisher: vec![msgs::Publish {
                topic: "/clock".into(),
                msg_type: "gz.msgs.Clock".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        t.on_message(&build(
            "pub",
            "topics-types",
            "gz.msgs.Publishers",
            &msg.encode_to_vec(),
        ));
        let names = msgs::StringMsgV {
            data: vec!["/clock".into(), "/stats".into()],
            ..Default::default()
        };
        t.on_message(&build(
            "pub",
            "topics",
            "gz.msgs.StringMsg_V",
            &names.encode_to_vec(),
        ));
        assert_eq!(
            drain_events(&mut t),
            [
                Event::TopicsTypes(vec![("/clock".into(), "gz.msgs.Clock".into())]),
                Event::Topics(vec!["/clock".into(), "/stats".into()]),
            ]
        );
    }

    /// Asking again sends `topics-types`; the reply replaces the list, so
    /// a topic that has ended loses its type.
    #[test]
    fn a_refreshed_topic_list_replaces_the_last() {
        let mut t = connected();
        let list = |topics: &[&str]| msgs::Publishers {
            publisher: topics
                .iter()
                .map(|topic| msgs::Publish {
                    topic: topic.to_string(),
                    msg_type: "gz.msgs.Clock".into(),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        let reply = |p: msgs::Publishers| build("pub", "topics-types", "", &p.encode_to_vec());
        t.on_message(&reply(list(&["/a", "/b"])));
        t.request_topics_types().unwrap();
        assert_eq!(drain_out(&mut t), ["topics-types,,,"]);
        t.on_message(&reply(list(&["/b"])));
        assert_eq!(t.topic_type("/a"), None);
        assert_eq!(t.topic_type("/b"), Some("gz.msgs.Clock"));
    }

    #[test]
    fn service_replies_are_events() {
        let mut t = connected();
        t.on_message(b"req,/world/w/control,gz.msgs.Boolean,\x10\x01");
        assert_eq!(
            drain_events(&mut t),
            [Event::ServiceReply {
                service: "/world/w/control".into(),
                msg_type: "gz.msgs.Boolean".into(),
                payload: b"\x10\x01".to_vec(),
            }]
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
        t.on_message(&build("asset", "/a.stl", "gz.msgs.Bytes", GARBAGE));
        t.on_message(&build("asset", "/b.stl", "gz.msgs.StringMsg", GARBAGE));
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
    fn error_display() {
        assert_eq!(
            AssetError::Server("asset_not_found".into()).to_string(),
            "server error: asset_not_found"
        );
        assert_eq!(
            AssetError::Decode("bad".into()).to_string(),
            "decode error: bad"
        );
        assert_eq!(SendError::NotConnected.to_string(), "not connected");
        assert_eq!(SendError::Failed.to_string(), "connection failed");
        assert_eq!(
            SendError::Unsendable("empty").to_string(),
            "frame can't be sent: empty"
        );
    }

    #[test]
    fn other_ops_are_ignored() {
        let mut t = connected();
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
        t.subscribe("/a").unwrap();
        assert_eq!(t.protos(), Some("protos"));
        assert_eq!(t.world(), Some("w"));
        t.on_close(None);
        // Closing forgets subscriptions and drops unsent frames.
        assert!(!t.is_subscribed("/a"));
        assert!(drain_out(&mut t).is_empty());
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
