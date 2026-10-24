//! Renderer- and platform-independent core of gzweb-rs.
//!
//! - [`frame`]: the `op,topic,type,payload` websocket frame codec.
//! - [`control`]: simulation control service requests.
//! - [`echo`]: a topic's message as text, by its type name.
//! - [`fields`]: a message's numbers, by field path (the Plot panel).
//! - [`fuel`]: Gazebo Fuel's model search (the Assets panel).
//! - [`insert`]: shapes, lights and models to insert, as SDFormat.
//! - [`sensor`]: lidar scans and camera frames, for the Sensors panel.
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
pub mod fields;
pub mod frame;
pub mod fuel;
pub mod insert;
#[cfg(feature = "native")]
pub mod net;
pub mod scene;
pub mod sdf;
pub mod sensor;
pub mod snapshot;
pub mod topic;
pub mod transport;
pub mod uri;

pub use gz_proto::msgs;
