//! `<submesh>` selection and centering for mesh files that load as scenes
//! (COLLADA, glTF, OBJ), by Gazebo's rules.
//!
//! gz-common turns every primitive (a COLLADA `<triangles>`, an OBJ
//! object's material group, a glTF primitive) into a submesh named after the
//! node it belongs to, baked into the mesh's frame. A node without a name
//! gives its geometry the name of its nearest named ancestor (gz-common's
//! COLLADA and Assimp loaders; Harmonic's COLLADA loader only looks at the
//! parent). gz-rendering then keeps **every** submesh with the requested
//! name and, with `center`, moves each one's own bounds centre to the
//! origin (`Ogre2MeshFactory`, `SubMesh::Center`).
//!
//! Here a submesh is an entity with a [`Mesh3d`]; its name is the `Name` of
//! the nearest ancestor that is a node (has no mesh of its own) and is not
//! an unnamed glTF node (Bevy calls those `GltfNode<index>`). A primitive's
//! own `Name` (glTF's `Mesh.Material`) never counts. Kept submeshes stay
//! under their ancestors' transforms; centering shifts each one's local
//! translation so its bounds, under those transforms, centre on the origin.
//!
//! gzweb instead keeps the first matching three.js mesh, depth first,
//! centres it on its untransformed vertices and zeroes its ancestors'
//! translations (keeping their rotations).

use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use bevy::scene::SceneInstanceReady;

/// On a `SceneRoot`: keep only this submesh once the scene is instanced.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct GzSubmesh {
    pub name: String,
    pub center: bool,
    /// Set once applied, whether or not it matched.
    pub applied: bool,
}

/// On a submesh [`select`] has centred, so it is never shifted twice.
#[derive(Component, Debug)]
pub struct GzSubmeshCentered;

/// Whether `name` is no name at all: empty, or Bevy's `GltfNode<index>`
/// for a glTF node without one.
fn is_default_name(name: &str) -> bool {
    name.is_empty()
        || name
            .strip_prefix("GltfNode")
            .is_some_and(|i| !i.is_empty() && i.bytes().all(|b| b.is_ascii_digit()))
}

/// Observer for [`SceneInstanceReady`] on an entity with [`GzSubmesh`].
/// Runs again on the fresh entities when Bevy respawns a modified scene.
#[allow(clippy::too_many_arguments)]
pub fn select(
    ready: On<SceneInstanceReady>,
    mut commands: Commands,
    mut roots: Query<&mut GzSubmesh>,
    children: Query<&Children>,
    names: Query<&Name>,
    mut transforms: Query<&mut Transform>,
    mesh3d: Query<&Mesh3d>,
    centered: Query<(), With<GzSubmeshCentered>>,
    meshes: Res<Assets<Mesh>>,
) {
    let root = ready.entity;
    let Ok(mut submesh) = roots.get_mut(root) else {
        return;
    };
    submesh.applied = true;
    let submesh = submesh.clone();
    let kids = |e: Entity| children.get(e).map(|c| c.to_vec()).unwrap_or_default();

    // Every submesh with the entities from below the root down to its
    // parent.
    let mut found: Vec<(Entity, Vec<Entity>)> = Vec::new();
    let mut stack: Vec<(Entity, Vec<Entity>)> =
        kids(root).into_iter().rev().map(|c| (c, vec![])).collect();
    while let Some((e, ancestors)) = stack.pop() {
        if mesh3d.contains(e) {
            found.push((e, ancestors.clone()));
        }
        let mut below = ancestors;
        below.push(e);
        for c in kids(e).into_iter().rev() {
            stack.push((c, below.clone()));
        }
    }
    let submesh_name = |ancestors: &[Entity]| {
        ancestors
            .iter()
            .rev()
            .filter(|&&a| !mesh3d.contains(a))
            .find_map(|&a| names.get(a).ok().filter(|n| !is_default_name(n.as_str())))
            .map(|n| n.as_str())
    };
    let keep: Vec<(Entity, Vec<Entity>)> = found
        .into_iter()
        .filter(|(_, ancestors)| submesh_name(ancestors) == Some(submesh.name.as_str()))
        .collect();
    if keep.is_empty() {
        warn!("submesh '{}' not found in its mesh", submesh.name);
        commands.entity(root).despawn_related::<Children>();
        return;
    }

    // Drop every subtree without a kept submesh, and the other submeshes.
    let kept = |e: Entity| keep.iter().any(|(k, _)| *k == e);
    let on_path = |e: Entity| keep.iter().any(|(_, a)| a.contains(&e));
    let mut stack = kids(root);
    while let Some(e) = stack.pop() {
        if on_path(e) {
            stack.extend(kids(e));
        } else if !kept(e) {
            commands.entity(e).despawn();
        }
    }

    if !submesh.center {
        return;
    }
    let local = |e: Entity| transforms.get(e).map(|t| t.to_matrix()).unwrap_or_default();
    let shifts: Vec<(Entity, Vec3)> = keep
        .iter()
        .filter(|(e, _)| !centered.contains(*e))
        .filter_map(|(e, ancestors)| {
            let parent = ancestors.iter().fold(Mat4::IDENTITY, |m, &a| m * local(a));
            let to_mesh = parent * local(*e);
            let mesh = meshes.get(&mesh3d.get(*e).ok()?.0)?;
            let Some(VertexAttributeValues::Float32x3(positions)) =
                mesh.attribute(Mesh::ATTRIBUTE_POSITION)
            else {
                return None;
            };
            let (lo, hi) =
                positions
                    .iter()
                    .fold((Vec3::INFINITY, Vec3::NEG_INFINITY), |(lo, hi), p| {
                        let w = to_mesh.transform_point3(Vec3::from_array(*p));
                        (lo.min(w), hi.max(w))
                    });
            if !lo.is_finite() || parent.determinant() == 0.0 {
                return None;
            }
            // Moving the bounds by -centre in the mesh frame is this shift
            // in the parent's frame.
            Some((*e, parent.inverse().transform_vector3(-(lo + hi) / 2.0)))
        })
        .collect();
    for (e, shift) in shifts {
        if let Ok(mut t) = transforms.get_mut(e) {
            t.translation += shift;
        }
        commands.entity(e).insert(GzSubmeshCentered);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::FRAC_PI_2;

    /// A triangle spanning (x..x+2, 0..4, 0).
    fn tri(x: f32) -> Mesh {
        Mesh::new(
            bevy::mesh::PrimitiveTopology::TriangleList,
            bevy::asset::RenderAssetUsages::default(),
        )
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![[x, 0.0, 0.0], [x + 2.0, 0.0, 0.0], [x, 4.0, 0.0]],
        )
    }

    /// The scene below an unnamed root:
    ///
    /// - `arm` (at x = 100) with a triangle (at y = 1), an unnamed `wrist`
    ///   node with another (x = 10), and a named `hand` node with a third;
    /// - a primitive named `Mesh` (as glTF names them) directly below the
    ///   root, and an unnamed glTF-style node `GltfNode7` with one;
    /// - a second `arm`, rotated 90° about Z and at y = 50, with a triangle;
    /// - `bare`, a node without geometry.
    fn scene(world: &mut World, meshes: &mut Assets<Mesh>) {
        let a = meshes.add(tri(0.0));
        let b = meshes.add(tri(10.0));
        let root = world.spawn(Transform::IDENTITY).id();
        let arm = world
            .spawn((
                Name::new("arm"),
                Transform::from_xyz(100.0, 0.0, 0.0),
                ChildOf(root),
            ))
            .id();
        world.spawn((
            Mesh3d(a.clone()),
            Transform::from_xyz(0.0, 1.0, 0.0),
            ChildOf(arm),
        ));
        let wrist = world.spawn((Transform::IDENTITY, ChildOf(arm))).id();
        world.spawn((Mesh3d(b.clone()), Transform::IDENTITY, ChildOf(wrist)));
        let hand = world
            .spawn((Name::new("hand"), Transform::IDENTITY, ChildOf(arm)))
            .id();
        world.spawn((Mesh3d(b), Transform::IDENTITY, ChildOf(hand)));
        world.spawn((
            Name::new("Mesh"),
            Mesh3d(a.clone()),
            Transform::IDENTITY,
            ChildOf(root),
        ));
        let gltf = world
            .spawn((Name::new("GltfNode7"), Transform::IDENTITY, ChildOf(root)))
            .id();
        world.spawn((Mesh3d(a.clone()), Transform::IDENTITY, ChildOf(gltf)));
        let arm2 = world
            .spawn((
                Name::new("arm"),
                Transform::from_xyz(0.0, 50.0, 0.0).with_rotation(Quat::from_rotation_z(FRAC_PI_2)),
                ChildOf(root),
            ))
            .id();
        world.spawn((Mesh3d(a), Transform::IDENTITY, ChildOf(arm2)));
        world.spawn((Name::new("bare"), Transform::IDENTITY, ChildOf(root)));
    }

    fn app_with(submesh: &str, center: bool) -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            TransformPlugin,
            bevy::scene::ScenePlugin,
        ))
        .init_asset::<Mesh>();
        let mut world = World::new();
        scene(
            &mut world,
            &mut app.world_mut().resource_mut::<Assets<Mesh>>(),
        );
        let scene = app
            .world_mut()
            .resource_mut::<Assets<Scene>>()
            .add(Scene::new(world));
        let e = app
            .world_mut()
            .spawn((
                SceneRoot(scene),
                Transform::IDENTITY,
                GzSubmesh {
                    name: submesh.into(),
                    center,
                    applied: false,
                },
            ))
            .observe(select)
            .id();
        for _ in 0..3 {
            app.update();
        }
        (app, e)
    }

    /// World bounds of each mesh below `e`, sorted by minimum x.
    fn bounds_below(app: &mut App, e: Entity) -> Vec<(Vec3, Vec3)> {
        let world = app.world_mut();
        let mut q = world.query::<(Entity, &Mesh3d, &GlobalTransform)>();
        let mut out: Vec<(Vec3, Vec3)> = q
            .iter(world)
            .filter(|(m, ..)| {
                std::iter::successors(Some(*m), |&x| world.get::<ChildOf>(x).map(|c| c.0))
                    .any(|x| x == e)
            })
            .map(|(_, m, g)| {
                let mesh = world.resource::<Assets<Mesh>>().get(&m.0).unwrap();
                let Some(VertexAttributeValues::Float32x3(p)) =
                    mesh.attribute(Mesh::ATTRIBUTE_POSITION)
                else {
                    panic!("no positions");
                };
                p.iter()
                    .map(|p| g.transform_point(Vec3::from_array(*p)))
                    .fold((Vec3::INFINITY, Vec3::NEG_INFINITY), |(lo, hi), w| {
                        (lo.min(w), hi.max(w))
                    })
            })
            .collect();
        out.sort_by(|a, b| a.0.x.total_cmp(&b.0.x));
        out
    }

    fn close(a: (Vec3, Vec3), b: (Vec3, Vec3)) -> bool {
        (a.0 - b.0).length() < 1e-4 && (a.1 - b.1).length() < 1e-4
    }

    #[test]
    fn keeps_every_match_and_the_unnamed_descendants() {
        let (mut app, e) = app_with("arm", false);
        assert!(app.world().get::<GzSubmesh>(e).unwrap().applied);
        // Both `arm`s, and `wrist`'s triangle (it takes `arm`'s name);
        // `hand`, the glTF-style meshes and `bare` are gone.
        let got = bounds_below(&mut app, e);
        let want = [
            // The rotated arm: (0..2, 0..4) turned 90° about Z, at y = 50.
            (Vec3::new(-4.0, 50.0, 0.0), Vec3::new(0.0, 52.0, 0.0)),
            (Vec3::new(100.0, 1.0, 0.0), Vec3::new(102.0, 5.0, 0.0)),
            (Vec3::new(110.0, 0.0, 0.0), Vec3::new(112.0, 4.0, 0.0)),
        ];
        assert_eq!(got.len(), 3, "{got:?}");
        for (g, w) in got.iter().zip(want) {
            assert!(close(*g, w), "{got:?}");
        }
        let world = app.world_mut();
        let names: Vec<String> = world
            .query::<&Name>()
            .iter(world)
            .map(|n| n.to_string())
            .collect();
        assert!(
            !names.iter().any(|n| n == "bare" || n == "hand"),
            "{names:?}"
        );
    }

    #[test]
    fn centers_each_submesh_on_its_own_bounds() {
        let (mut app, e) = app_with("arm", true);
        let got = bounds_below(&mut app, e);
        assert_eq!(got.len(), 3, "{got:?}");
        // Every triangle is 2 × 4, the rotated one 4 × 2; all centred.
        for (lo, hi) in &got {
            assert!(((*lo + *hi) / 2.0).length() < 1e-4, "{got:?}");
        }
        let rotated = got.iter().filter(|(lo, hi)| hi.x - lo.x > 3.0).count();
        assert_eq!(rotated, 1, "{got:?}");
        // The root itself doesn't move.
        assert_eq!(
            app.world().get::<Transform>(e).unwrap(),
            &Transform::IDENTITY
        );
    }

    #[test]
    fn a_respawned_scene_is_cut_and_centred_again_once() {
        let (mut app, e) = app_with("hand", true);
        let first = bounds_below(&mut app, e);
        assert_eq!(first.len(), 1);
        assert!(close(
            first[0],
            (Vec3::new(-1.0, -2.0, 0.0), Vec3::new(1.0, 2.0, 0.0))
        ));
        let meshes = |app: &mut App| {
            let world = app.world_mut();
            let mut q = world.query_filtered::<Entity, With<Mesh3d>>();
            q.iter(world).collect::<Vec<_>>()
        };
        // Past Bevy's debouncing of the scene's `Added` event.
        for _ in 0..5 {
            app.update();
        }
        let before = meshes(&mut app);
        // Modifying the scene makes Bevy respawn it: the new entities are
        // cut and centred from scratch.
        let scene = app.world().get::<SceneRoot>(e).unwrap().0.clone();
        app.world_mut()
            .resource_mut::<Assets<Scene>>()
            .get_mut(&scene)
            .unwrap();
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(bounds_below(&mut app, e), first);
        let after = meshes(&mut app);
        assert!(after.iter().all(|m| !before.contains(m)), "not respawned");
    }

    #[test]
    fn primitive_and_default_names_never_match() {
        for name in ["nope", "bare", "Mesh", "GltfNode7", ""] {
            let (mut app, e) = app_with(name, false);
            assert!(app.world().get::<GzSubmesh>(e).unwrap().applied);
            assert!(bounds_below(&mut app, e).is_empty(), "{name}");
        }
    }

    #[test]
    fn default_names() {
        assert!(is_default_name(""));
        assert!(is_default_name("GltfNode12"));
        assert!(!is_default_name("GltfNode"));
        assert!(!is_default_name("GltfNodeA"));
        assert!(!is_default_name("arm"));
    }
}
