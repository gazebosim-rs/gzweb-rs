//! `gz-parity`: differential testing of gzweb-rs against gzweb.
//!
//! ```text
//! gz-parity record  --url ws://localhost:9002 --out tests/corpus/shapes [--duration-ms 3000]
//!                   [--max-stream-frames 200] [--world-file shapes.sdf] [--timeout-ms 20000]
//!                   [--gazebo jetty] [--ca-cert ca.pem]
//! gz-parity replay  --corpus tests/corpus/shapes --port 9100 [--client-log FILE]
//!                   [--stream-limit N] [--connections N] [--key KEY]
//! gz-parity compare --expected gzweb.json --actual rs.json [--allow allowlist.toml]
//!                   [--gaps gaps.toml] [--report out.json]
//! gz-parity compare-protocol EXPECTED.log ACTUAL.log [--report out.json]
//! gz-parity mirror  --cache target/http-cache --port 9101 [--lock http.lock.json]
//!                   [--upstream URL] [--offline true]
//! ```
//! See `.agent/PARITY.md`.

pub mod cli;
pub mod compare;
pub mod corpus;
pub mod mirror;
pub mod record;
pub mod replay;

/// Error → message, for `map_err`. A named function rather than a closure per
/// call site, so unreachable error paths don't each count as uncovered code.
pub(crate) fn message(e: impl std::fmt::Display) -> String {
    e.to_string()
}
