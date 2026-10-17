//! Collada spike: evaluate `mesh-loader` as the basis for a Bevy `.dae` loader.
//!
//! * [`summary`] — per-file JSON summary (same shape as `js/ref.mjs`).
//! * [`bevy_conv`] — prototype conversion to `bevy_mesh::Mesh` +
//!   `bevy_pbr::StandardMaterial`, plus an `AssetLoader` sketch.

pub mod bevy_conv;
pub mod summary;
