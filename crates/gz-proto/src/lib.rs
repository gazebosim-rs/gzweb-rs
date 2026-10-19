//! Gazebo `gz.msgs` protobuf types, generated at build time from the vendored
//! gz-msgs (Jetty, gz-msgs 12) `.proto` files in `proto/`.

#[allow(clippy::all, missing_docs)]
pub mod gz {
    pub mod msgs {
        include!(concat!(env!("OUT_DIR"), "/gz.msgs.rs"));
    }
}

pub use gz::msgs;
