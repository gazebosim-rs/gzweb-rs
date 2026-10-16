//! Renderer- and platform-independent core of gzweb-rs.
//!
//! - [`frame`]: the `op,topic,type,payload` websocket frame codec.
//! - [`transport`]: sans-IO client state machine for the gz websocket protocol.
//! - [`scene`]: gzweb's rules for turning a `gz.msgs.Scene` into a node tree.
//! - [`uri`]: resource URI resolution (Fuel, `file://`, `model://`).
//! - [`snapshot`]: the parity-test snapshot format.

pub mod frame;
pub mod scene;
pub mod snapshot;
pub mod transport;
pub mod uri;

pub use gz_proto::msgs;
