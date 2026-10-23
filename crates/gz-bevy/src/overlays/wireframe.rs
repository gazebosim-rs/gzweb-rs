//! The Wireframe overlay: every visual's triangle edges, drawn over it.
//!
//! Each visual mesh gets a child line mesh built from its triangles
//! ([`edge_lines`]), cached per mesh, on the wireframe's render layer, so
//! only the views showing the overlay draw it (DECISIONS.md D-62, D-93).
//! Not Bevy's `WireframePlugin`: it draws a mesh's wireframe in every view
//! that sees the mesh, and needs line polygons, which the browser's GPUs
//! don't have. Overlay geometry (collision shapes) is left alone.

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;

use super::collisions::CollisionOverlay;
use super::Overlays;
use crate::spawn::GzWorldRoot;

/// The lines' colour: dark, as the edges of a lit surface read.
pub const WIRE_COLOR: Color = Color::srgba(0.05, 0.05, 0.08, 0.85);

/// A line mesh child drawing its parent's edges.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct WireLines;

/// A visual mesh with its wireframe on.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Wired;

/// The line meshes by source mesh, and their material.
#[derive(Resource, Default)]
pub struct WireframeLines {
    lines: HashMap<AssetId<Mesh>, Handle<Mesh>>,
    material: Option<Handle<StandardMaterial>>,
}

/// What isn't a visual mesh: overlay geometry, the wireframe's lines and
/// the selection outline's mask clones.
type NotVisual = Or<(
    With<CollisionOverlay>,
    With<WireLines>,
    With<crate::view::outline::OutlineClone>,
)>;

/// The scene's visual meshes: under the world root, not part of an
/// overlay or a wireframe line mesh.
fn is_visual(
    e: Entity,
    parents: &Query<&ChildOf>,
    roots: &Query<(), With<GzWorldRoot>>,
    skip: &Query<(), NotVisual>,
) -> bool {
    if skip.contains(e) {
        return false;
    }
    for a in parents.iter_ancestors(e) {
        if skip.contains(a) {
            return false;
        }
        if roots.contains(a) {
            return true;
        }
    }
    false
}

/// Turns wireframes on and off with the overlay (in any view), and on for
/// meshes that arrive while it is on (mesh files instance late).
#[allow(clippy::too_many_arguments)]
pub fn apply_wireframe(
    overlays: Res<Overlays>,
    meshes: Query<(Entity, Has<Wired>), With<Mesh3d>>,
    added: Query<Entity, Added<Mesh3d>>,
    parents: Query<&ChildOf>,
    roots: Query<(), With<GzWorldRoot>>,
    skip: Query<(), NotVisual>,
    lines: Query<Entity, With<WireLines>>,
    mut commands: Commands,
) {
    if overlays.is_changed() {
        if overlays.wireframe {
            for (e, wired) in &meshes {
                if !wired && is_visual(e, &parents, &roots, &skip) {
                    commands.entity(e).insert(Wired);
                }
            }
        } else {
            for (e, wired) in &meshes {
                if wired {
                    commands.entity(e).remove::<Wired>();
                }
            }
            for e in &lines {
                commands.entity(e).despawn();
            }
        }
    } else if overlays.wireframe {
        for e in &added {
            if is_visual(e, &parents, &roots, &skip) {
                commands.entity(e).insert(Wired);
            }
        }
    }
}

/// A line mesh under each wired visual, once its mesh has loaded.
pub fn build_lines(
    wired: Query<(Entity, &Mesh3d, Option<&Children>), With<Wired>>,
    has_lines: Query<(), With<WireLines>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cache: ResMut<WireframeLines>,
    mut commands: Commands,
) {
    for (e, mesh, children) in &wired {
        if children.is_some_and(|c| c.iter().any(|c| has_lines.contains(c))) {
            continue;
        }
        let id = mesh.0.id();
        let handle = match cache.lines.get(&id) {
            Some(h) => h.clone(),
            None => {
                let Some(lines) = meshes.get(id).map(edge_lines) else {
                    continue;
                };
                let h = meshes.add(lines);
                cache.lines.insert(id, h.clone());
                h
            }
        };
        let material = cache
            .material
            .get_or_insert_with(|| {
                materials.add(StandardMaterial {
                    base_color: WIRE_COLOR,
                    unlit: true,
                    // Blended, so drawn after the surfaces with depth writes
                    // off; reversed-Z's greater-or-equal test keeps the
                    // edges lying on them. (No depth bias: WebGPU refuses
                    // one on line lists.)
                    alpha_mode: AlphaMode::Blend,
                    ..default()
                })
            })
            .clone();
        commands.spawn((
            WireLines,
            Mesh3d(handle),
            MeshMaterial3d(material),
            Transform::IDENTITY,
            bevy::light::NotShadowCaster,
            RenderLayers::layer(super::Overlay::Wireframe.layer()),
            ChildOf(e),
        ));
    }
}

/// A line list of `mesh`'s triangle edges, each once. Meshes that aren't
/// triangle lists, or have no positions, give an empty line list.
pub fn edge_lines(mesh: &Mesh) -> Mesh {
    let mut out = Mesh::new(PrimitiveTopology::LineList, RenderAssetUsages::default());
    let positions: Vec<[f32; 3]> = match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
        Some(VertexAttributeValues::Float32x3(p)) => p.clone(),
        _ => Vec::new(),
    };
    let triangles: Vec<u32> = if mesh.primitive_topology() != PrimitiveTopology::TriangleList {
        Vec::new()
    } else {
        match mesh.indices() {
            Some(Indices::U16(i)) => i.iter().map(|&v| u32::from(v)).collect(),
            Some(Indices::U32(i)) => i.clone(),
            None => (0..positions.len() as u32).collect(),
        }
    };
    let mut seen = HashSet::new();
    let mut indices = Vec::new();
    for t in triangles.as_chunks::<3>().0 {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let key = (a.min(b), a.max(b));
            if (key.1 as usize) < positions.len() && seen.insert(key) {
                indices.extend([key.0, key.1]);
            }
        }
    }
    out.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    out.insert_indices(Indices::U32(indices));
    out
}
