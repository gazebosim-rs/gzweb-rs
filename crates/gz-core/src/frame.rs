//! The gz websocket frame format: `operation,topic,msgType,payload`.
//!
//! The first three fields are ASCII and the payload is everything after the
//! third comma (usually a serialized protobuf message). See gz-launch / gz-sim
//! `WebsocketServer.cc` (`BUILD_MSG`) and gzweb `Transport.ts`.

/// A borrowed view of one incoming frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame<'a> {
    pub op: &'a str,
    pub topic: &'a str,
    pub msg_type: &'a str,
    pub payload: &'a [u8],
}

impl<'a> Frame<'a> {
    /// Splits `data` at its first three commas. Returns `None` if there are
    /// fewer than three commas or the header is not UTF-8.
    pub fn parse(data: &'a [u8]) -> Option<Self> {
        let mut commas = data
            .iter()
            .enumerate()
            .filter(|(_, &b)| b == b',')
            .map(|(i, _)| i);
        let (c0, c1, c2) = (commas.next()?, commas.next()?, commas.next()?);
        Some(Frame {
            op: std::str::from_utf8(&data[..c0]).ok()?,
            topic: std::str::from_utf8(&data[c0 + 1..c1]).ok()?,
            msg_type: std::str::from_utf8(&data[c1 + 1..c2]).ok()?,
            payload: &data[c2 + 1..],
        })
    }
}

/// Builds an outgoing frame, matching gzweb's `parts.join(",")`.
pub fn build(op: &str, topic: &str, msg_type: &str, payload: &str) -> String {
    format!("{op},{topic},{msg_type},{payload}")
}

/// Builds a frame with a binary payload (used by the replay server and tests).
pub fn build_binary(op: &str, topic: &str, msg_type: &str, payload: &[u8]) -> Vec<u8> {
    let mut out = format!("{op},{topic},{msg_type},").into_bytes();
    out.extend_from_slice(payload);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_header_and_payload() {
        let f = Frame::parse(b"pub,/world/a/scene/info,gz.msgs.Scene,\x0a\x02,,").unwrap();
        assert_eq!(f.op, "pub");
        assert_eq!(f.topic, "/world/a/scene/info");
        assert_eq!(f.msg_type, "gz.msgs.Scene");
        // Commas inside the payload belong to the payload.
        assert_eq!(f.payload, b"\x0a\x02,,");
    }

    #[test]
    fn empty_fields() {
        let f = Frame::parse(b"protos,,,").unwrap();
        assert_eq!((f.op, f.topic, f.msg_type, f.payload), ("protos", "", "", &b""[..]));
    }

    #[test]
    fn too_few_commas() {
        assert_eq!(Frame::parse(b"short,data"), None);
        assert_eq!(Frame::parse(b""), None);
    }

    #[test]
    fn build_matches_gzweb_join() {
        assert_eq!(build("asset", "", "", "file:///a.dae"), "asset,,,file:///a.dae");
        assert_eq!(build("sub", "/t", "", ""), "sub,/t,,");
        assert_eq!(build_binary("pub", "worlds", "gz.msgs.StringMsg_V", b"\x01"), b"pub,worlds,gz.msgs.StringMsg_V,\x01");
    }
}
