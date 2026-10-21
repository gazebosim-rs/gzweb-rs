//! The Collisions overlay: each link's collision shapes as meshes in the
//! Gazebo GUI's collision material (gz-sim `VisualizationCapabilities`:
//! diffuse (1.0, 0.5088, 0.0468) at 0.7 alpha), children of the link so
//! pose updates move them.
//!
//! They are spawned (once per [`SdfExtras`] reply) the first time the
//! overlay is on, as visuals with the collision's geometry, so mesh files
//! load like any visual's, then shown and hidden with the overlay.
//! Heightmap collisions aren't drawn (the terrain's visual is the same
//! surface).

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use gz_core::scene::{GeometryDesc, MaterialDesc, NodeKind, VisualDesc};
use std::collections::HashMap;

use super::extras::SdfExtras;
use super::Overlays;
use crate::spawn::{pose_transform, GeometryAssets, GzEntity};

/// Gazebo's collision colour (linear, as Gazebo's materials are).
pub const COLLISION_RGBA: [f32; 4] = [1.0, 0.5088, 0.0468, 0.7];

/// The root of one collision shape, under its link.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct CollisionOverlay;

/// The visual a collision is drawn as.
pub fn collision_visual(geometry: &GeometryDesc) -> VisualDesc {
    VisualDesc {
        proto_type: "collision".into(),
        geometry: geometry.clone(),
        material: MaterialDesc {
            diffuse: Some(COLLISION_RGBA),
            present: true,
            ..MaterialDesc::absent()
        },
        cast_shadows: false,
        transparency: 0.0,
    }
}

/// The generation the collision meshes were spawned for.
#[derive(Default)]
pub struct Spawned(Option<u32>);

/// Spawns every link's collision shapes when the overlay is on and the
/// world's extras are new (replacing the previous ones).
pub fn spawn_collisions(
    overlays: Res<Overlays>,
    extras: Res<SdfExtras>,
    links: Query<(Entity, &GzEntity)>,
    existing: Query<Entity, With<CollisionOverlay>>,
    mut spawned: Local<Spawned>,
    mut assets: GeometryAssets,
    mut commands: Commands,
) {
    let Some(world) = extras.extras.as_ref() else {
        return;
    };
    if !overlays.collisions || spawned.0 == Some(extras.generation) {
        return;
    }
    spawned.0 = Some(extras.generation);
    for e in &existing {
        commands.entity(e).despawn();
    }
    let by_path: HashMap<&str, Entity> = links
        .iter()
        .filter(|(_, g)| g.kind == NodeKind::Link)
        .map(|(e, g)| (g.path.as_str(), e))
        .collect();
    for (path, link) in &world.links {
        let Some(&parent) = by_path.get(path.as_str()) else {
            continue;
        };
        for c in &link.collisions {
            if matches!(
                c.geometry,
                GeometryDesc::Heightmap(_) | GeometryDesc::Unsupported(_)
            ) {
                continue;
            }
            let root = commands
                .spawn((
                    Name::new(format!("collision {}", c.name)),
                    CollisionOverlay,
                    pose_transform(&c.pose, c.scale),
                    Visibility::Inherited,
                    NotShadowCaster,
                    ChildOf(parent),
                ))
                .id();
            assets.spawn(&mut commands, &collision_visual(&c.geometry), root, &c.name);
        }
    }
}

/// Shows the collision meshes while the overlay is on.
pub fn show_collisions(
    overlays: Res<Overlays>,
    mut roots: Query<&mut Visibility, With<CollisionOverlay>>,
) {
    let want = if overlays.collisions {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    for mut v in &mut roots {
        v.set_if_neq(want);
    }
}
