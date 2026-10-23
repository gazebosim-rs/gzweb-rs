//! A topic's message as text, for the Topics panel's echo: decoded by its
//! type name against the `gz.msgs` descriptors and written in protobuf
//! text format, as `gz topic -e` prints it.

use std::sync::OnceLock;

use prost_reflect::text_format::FormatOptions;
use prost_reflect::{DescriptorPool, DynamicMessage};

/// Why a message can't be shown as text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EchoError {
    /// No `gz.msgs` type has this name.
    UnknownType(String),
    /// The payload isn't a message of its type.
    Decode(String),
}

impl std::fmt::Display for EchoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EchoError::UnknownType(t) => write!(f, "unknown message type {t}"),
            EchoError::Decode(e) => write!(f, "can't decode the message: {e}"),
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
    let name = match msg_type.strip_prefix("ignition.msgs.") {
        Some(rest) => format!("gz.msgs.{rest}"),
        None => msg_type.to_string(),
    };
    let desc = pool()
        .get_message_by_name(&name)
        .ok_or_else(|| EchoError::UnknownType(msg_type.to_string()))?;
    let msg =
        DynamicMessage::decode(desc, payload).map_err(|e| EchoError::Decode(e.to_string()))?;
    Ok(msg.to_text_format_with_options(&FormatOptions::new().pretty(true)))
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
        let bad = message_text("gz.msgs.WorldStatistics", &[0xff]);
        assert!(matches!(bad, Err(EchoError::Decode(_))), "{bad:?}");
        assert_eq!(
            EchoError::UnknownType("x".into()).to_string(),
            "unknown message type x"
        );
    }
}
