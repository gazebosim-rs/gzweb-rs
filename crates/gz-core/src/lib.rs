//! Renderer- and platform-independent core of gzweb-rs.
//!
//! - [`frame`]: the `op,topic,type,payload` websocket frame codec.
//! - [`control`]: simulation control service requests.
//! - [`echo`]: a topic's message as text, by its type name.
//! - [`transport`]: sans-IO client state machine for the gz websocket protocol.
//! - `net` (feature `native`): blocking `ws://`/`wss://` connect.
//! - [`scene`]: gzweb's rules for turning a `gz.msgs.Scene` into a node tree.
//! - [`sdf`]: collisions, inertials and joints from a world's SDFormat (the
//!   viewport's overlays; the scene message has none).
//! - [`uri`]: resource URI resolution (Fuel, `file://`, `model://`).
//! - [`snapshot`]: the parity-test snapshot format.
//! - [`topic`]: gz-transport topic-name sanitization and world topics.

pub mod control;
pub mod echo;
pub mod frame;
#[cfg(feature = "native")]
pub mod net;
pub mod scene;
pub mod sdf;
pub mod snapshot;
pub mod topic;
pub mod transport;
pub mod uri;

pub use gz_proto::msgs;
