//! A topic's message as text, for the Topics panel's echo: decoded by its
//! type name against the `gz.msgs` descriptors and written in protobuf
//! text format, as `gz topic -e` prints it; and back, for the Console's
//! `gz topic -p` and `gz service --req`.

use std::sync::OnceLock;

use prost_reflect::text_format::FormatOptions;
use prost_reflect::{DescriptorPool, DynamicMessage, ReflectMessage, Value};

/// Why a message can't be shown as text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EchoError {
    /// No `gz.msgs` type has this name.
    UnknownType(String),
    /// The payload isn't a message of its type.
    Decode(String),
    /// The text isn't a message of its type in text format.
    Parse(String),
}

impl std::fmt::Display for EchoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EchoError::UnknownType(t) => write!(f, "unknown message type {t}"),
            EchoError::Decode(e) => write!(f, "can't decode the message: {e}"),
            EchoError::Parse(e) => write!(f, "can't read the message: {e}"),
        }
    }
}

impl std::error::Error for EchoError {}

/// The `gz.msgs` descriptors, decoded on first use.
fn pool() -> &'static DescriptorPool {
    static POOL: OnceLock<DescriptorPool> = OnceLock::new();
    POOL.get_or_init(|| {
        // Built from the vendored protos by gz-proto's build script.
        DescriptorPool::decode(gz_proto::DESCRIPTOR_SET).unwrap_or_default()
    })
}

/// The PNG signature: the websocket server's `image` op sends a camera's
/// frames as PNG data, not as `gz.msgs.Image`.
const PNG: &[u8] = &[0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];

/// `payload`, a serialized `msg_type`, in protobuf text format, one field
/// a line. Fortress's `ignition.msgs` names are read as `gz.msgs`. An
/// image topic's PNG frame is described, not printed.
pub fn message_text(msg_type: &str, payload: &[u8]) -> Result<String, EchoError> {
    if payload.starts_with(PNG) {
        return Ok(format!("PNG image, {} bytes", payload.len()));
    }
    let mut msg = decode(msg_type, payload)?;
    shorten_bytes(&mut msg);
    Ok(msg.to_text_format_with_options(&FormatOptions::new().pretty(true)))
}

/// `payload`, a serialized `msg_type`, decoded by its type name.
pub(crate) fn decode(msg_type: &str, payload: &[u8]) -> Result<DynamicMessage, EchoError> {
    DynamicMessage::decode(descriptor(msg_type)?, payload)
        .map_err(|e| EchoError::Decode(e.to_string()))
}

/// `text`, a `msg_type` in protobuf text format (`data: 1.5`,
/// `linear { x: 0.5 }`), serialized; empty text is the empty message.
pub fn encode_text(msg_type: &str, text: &str) -> Result<Vec<u8>, EchoError> {
    let msg = DynamicMessage::parse_text_format(descriptor(msg_type)?, text)
        .map_err(|e| EchoError::Parse(e.to_string()))?;
    Ok(prost::Message::encode_to_vec(&msg))
}

/// The descriptor of `msg_type`, Fortress's `ignition.msgs` names read as
/// `gz.msgs`.
fn descriptor(msg_type: &str) -> Result<prost_reflect::MessageDescriptor, EchoError> {
    let name = match msg_type.strip_prefix("ignition.msgs.") {
        Some(rest) => format!("gz.msgs.{rest}"),
        None => msg_type.to_string(),
    };
    pool()
        .get_message_by_name(&name)
        .ok_or_else(|| EchoError::UnknownType(msg_type.to_string()))
}

/// The longest `bytes` field printed as it is; a longer one (a point
/// cloud's data, an image's pixels) prints as its size.
pub const BYTES_SHOWN: usize = 64;

/// Replaces each `bytes` value longer than [`BYTES_SHOWN`], at any depth,
/// with `<N bytes>`.
fn shorten_bytes(msg: &mut DynamicMessage) {
    let fields: Vec<_> = msg.descriptor().fields().filter(|f| !f.is_map()).collect();
    for field in fields {
        if !msg.has_field(&field) {
            continue;
        }
        match msg.get_field_mut(&field) {
            Value::List(items) => items.iter_mut().for_each(shorten_value),
            value => shorten_value(value),
        }
    }
}

fn shorten_value(value: &mut Value) {
    match value {
        Value::Bytes(b) if b.len() > BYTES_SHOWN => {
            *b = format!("<{} bytes>", b.len()).into_bytes().into();
        }
        Value::Message(m) => shorten_bytes(m),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use prost::Message as _;

    use super::*;
    use crate::msgs;

    /// A message prints field by field, nested messages indented; the old
    /// `ignition.msgs` name reads the same; a PNG frame is described; an
    /// unknown type or a bad payload says why.
    #[test]
    fn a_message_prints_as_text() {
        let stats = msgs::WorldStatistics {
            iterations: 42,
            paused: true,
            sim_time: Some(msgs::Time { sec: 3, nsec: 5 }),
            ..Default::default()
        };
        let bytes = stats.encode_to_vec();
        let text = message_text("gz.msgs.WorldStatistics", &bytes).unwrap();
        assert!(
            text.contains("sim_time {\n  sec: 3\n  nsec: 5\n}"),
            "{text}"
        );
        assert!(text.contains("iterations: 42"), "{text}");
        assert!(text.contains("paused: true"), "{text}");
        assert_eq!(
            message_text("ignition.msgs.WorldStatistics", &bytes),
            Ok(text)
        );

        let png = [PNG, &[0; 4]].concat();
        assert_eq!(
            message_text("gz.msgs.Image", &png),
            Ok("PNG image, 12 bytes".into())
        );
        assert_eq!(
            message_text("gz.msgs.Nope", &bytes),
            Err(EchoError::UnknownType("gz.msgs.Nope".into()))
        );
        let cloud = msgs::PointCloudPacked {
            data: vec![7; 300_000],
            header: Some(msgs::Header {
                data: vec![msgs::header::Map {
                    key: "frame_id".into(),
                    value: vec!["lidar".into()],
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        let text = message_text("gz.msgs.PointCloudPacked", &cloud.encode_to_vec()).unwrap();
        assert!(text.contains("data: \"<300000 bytes>\""), "{text}");
        assert!(text.contains("lidar"), "{text}");
        assert!(text.len() < 1000, "{}", text.len());
        let short = msgs::PointCloudPacked {
            data: vec![b'a'; BYTES_SHOWN],
            ..Default::default()
        };
        let text = message_text("gz.msgs.PointCloudPacked", &short.encode_to_vec()).unwrap();
        assert!(text.contains(&"a".repeat(BYTES_SHOWN)), "{text}");

        let bad = message_text("gz.msgs.WorldStatistics", &[0xff]);
        assert!(matches!(bad, Err(EchoError::Decode(_))), "{bad:?}");
        assert_eq!(
            EchoError::UnknownType("x".into()).to_string(),
            "unknown message type x"
        );
    }

    /// Text format reads back into the message it describes; empty text is
    /// the empty message; a bad field or type says why.
    #[test]
    fn text_encodes_as_its_message() {
        let bytes = encode_text("gz.msgs.Twist", "linear { x: 0.5 } angular { z: -1 }").unwrap();
        let twist = msgs::Twist::decode(bytes.as_slice()).unwrap();
        assert_eq!(twist.linear.unwrap().x, 0.5);
        assert_eq!(twist.angular.unwrap().z, -1.0);
        assert_eq!(encode_text("gz.msgs.Empty", ""), Ok(vec![]));
        let bad = encode_text("gz.msgs.Twist", "nope: 1");
        assert!(matches!(bad, Err(EchoError::Parse(_))), "{bad:?}");
        assert!(bad
            .unwrap_err()
            .to_string()
            .starts_with("can't read the message"));
        assert_eq!(
            encode_text("gz.msgs.Nope", ""),
            Err(EchoError::UnknownType("gz.msgs.Nope".into()))
        );
    }
}
