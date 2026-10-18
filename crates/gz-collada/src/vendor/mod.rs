//! Vendored from [mesh-loader](https://github.com/openrr/mesh-loader) 0.1.13
//! (Apache-2.0, see `LICENSE-mesh-loader`): the COLLADA XML, geometry,
//! effect, image and material parsers and their utilities.
//!
//! Changes from upstream are marked `gz-collada:` in the code, and every
//! changed file says so at the top:
//! - `instance.rs` (scene assembly) is replaced by `crate::assemble`, and
//!   the document types are `pub(crate)` for it;
//! - nodes keep their name, id, children and `instance_node`s, and
//!   `<library_nodes>` is parsed; `<node>` nesting is capped;
//! - no panics on bad input: references are looked up with `get`, the
//!   iterators yield nothing for a source they can't read, untrusted
//!   counts don't size allocations, and a `<Name_array>` no longer hangs;
//! - `<bump>` and `<double_sided>` are read, best effort, from any `<extra>`
//!   of the effect, `<double_sided>` directly under a technique from the
//!   right element, and textures' `wrapU`/`wrapV`;
//! - polylists skip polygons of fewer than three vertices correctly, and
//!   tristrips are triangulated as strips rather than fans;
//! - `<rotate>` axes are normalised and bad `<unit>`s fall back to 1;
//! - the OBJ and STL code is removed.
//!
//! This code is excluded from the coverage gate (`scripts/check.sh`).
//! `tests/collada.rs` exercises it on hand-written probes and malformed
//! documents. The comparison with three.js on real Fuel meshes
//! (`fuel_corpus_matches_three_js`) is `--ignored`: it needs the spike's
//! corpus and Node (`spikes/collada/run.sh`), so CI doesn't run it.

#[macro_use]
mod error;
pub(crate) mod collada;
pub(crate) mod common;
pub(crate) mod utils;

pub(crate) use common::Color4;
