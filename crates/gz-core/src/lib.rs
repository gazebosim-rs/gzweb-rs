//! Renderer- and platform-independent core of gzweb-rs.
//!
//! - [`frame`]: the `op,topic,type,payload` websocket frame codec.
//! - [`control`]: simulation control service requests.
//! - [`transport`]: sans-IO client state machine for the gz websocket protocol.
//! - `net` (feature `native`): blocking `ws://`/`wss://` connect.
//! - [`scene`]: gzweb's rules for turning a `gz.msgs.Scene` into a node tree.
//! - [`uri`]: resource URI resolution (Fuel, `file://`, `model://`).
//! - [`snapshot`]: the parity-test snapshot format.

pub mod control;
pub mod frame;
#[cfg(feature = "native")]
pub mod net;
pub mod scene;
pub mod snapshot;
pub mod transport;
pub mod uri;

pub use gz_proto::msgs;
