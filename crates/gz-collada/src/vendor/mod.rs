//! Vendored from [mesh-loader](https://github.com/openrr/mesh-loader) 0.1.13
//! (Apache-2.0, see `LICENSE-mesh-loader`): the COLLADA XML, geometry,
//! effect, image and material parsers and their utilities.
//!
//! Changes from upstream are marked `gz-collada:` in the code:
//! - `instance.rs` (scene assembly) is replaced by `crate::assemble`;
//! - nodes keep their name, id, children and `instance_node`s, and
//!   `<library_nodes>` is parsed;
//! - `<bump>` inside `<extra><technique>` is read as a normal map;
//! - `<double_sided>` under a technique is read from the right element.
//!
//! This code is excluded from the coverage gate (`scripts/check.sh`); the
//! corpus tests in `tests/` exercise it.

#[macro_use]
mod error;
pub(crate) mod collada;
pub(crate) mod common;
pub(crate) mod utils;

pub(crate) use common::Color4;
