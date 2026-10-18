//! `<submesh>` selection and centering for mesh files that load as scenes
//! (COLLADA, glTF, OBJ).
//!
//! Gazebo's rules: the submesh is the first node, depth first, with that
//! name and geometry of its own. Only that geometry is kept, still under its
//! ancestors' transforms. With `center`, the centre of its bounds moves to
//! the visual's origin. gzweb instead zeroes the ancestors' translations and
//! keeps their rotations; the two agree unless an ancestor rotates.

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

/// Observer for [`SceneInstanceReady`] on an entity with [`GzSubmesh`].
#[allow(clippy::too_many_arguments)]
pub fn select(
    ready: On<SceneInstanceReady>,
    mut commands: Commands,
    mut roots: Query<&mut GzSubmesh>,
    children: Query<&Children>,
    names: Query<&Name>,
    mut transforms: Query<&mut Transform>,
    mesh3d: Query<&Mesh3d>,
    meshes: Res<Assets<Mesh>>,
) {
    let root = ready.entity;
    let Ok(mut submesh) = roots.get_mut(root) else {
        return;
    };
    submesh.applied = true;
    let submesh = submesh.clone();
    let kids = |e: Entity| children.get(e).map(|c| c.to_vec()).unwrap_or_default();
    // The submesh's own geometry: the node itself, or its mesh children.
    let geometry = |e: Entity| -> Vec<Entity> {
        let mut out: Vec<Entity> = mesh3d.contains(e).then_some(e).into_iter().collect();
        out.extend(kids(e).into_iter().filter(|&c| mesh3d.contains(c)));
        out
    };

    let mut path = Vec::new();
    let found = kids(root).into_iter().any(|c| {
        find(
            c,
            &mut path,
            &|e| names.get(e).is_ok_and(|n| n.as_str() == submesh.name) && !geometry(e).is_empty(),
            &kids,
        )
    });
    if !found {
        warn!("submesh '{}' not found in its mesh", submesh.name);
        commands.entity(root).despawn_related::<Children>();
        return;
    }
    let node = *path.last().expect("found implies a path");
    let keep = geometry(node);

    // Everything off the path to the submesh goes, as do the submesh's
    // child nodes.
    let mut parents = vec![root];
    parents.extend(&path);
    for &p in &parents {
        for c in kids(p) {
            if !(parents.contains(&c) || p == node && keep.contains(&c)) {
                commands.entity(c).despawn();
            }
        }
    }

    if submesh.center {
        let local = |e: Entity| transforms.get(e).map(|t| t.to_matrix()).unwrap_or_default();
        let to_node = path.iter().fold(Mat4::IDENTITY, |m, &e| m * local(e));
        let mut bounds: Option<(Vec3, Vec3)> = None;
        for &e in &keep {
            let m = if e == node {
                to_node
            } else {
                to_node * local(e)
            };
            let Some(mesh) = mesh3d.get(e).ok().and_then(|h| meshes.get(&h.0)) else {
                continue;
            };
            let Some(VertexAttributeValues::Float32x3(positions)) =
                mesh.attribute(Mesh::ATTRIBUTE_POSITION)
            else {
                continue;
            };
            for p in positions {
                let w = m.transform_point3(Vec3::from_array(*p));
                bounds = Some(bounds.map_or((w, w), |(lo, hi)| (lo.min(w), hi.max(w))));
            }
        }
        if let (Some((lo, hi)), Ok(mut t)) = (bounds, transforms.get_mut(root)) {
            let shifted = t.to_matrix() * Mat4::from_translation(-(lo + hi) / 2.0);
            *t = Transform::from_matrix(shifted);
        }
    }
}

/// Depth-first search below `e` for the first entity `is_match` accepts,
/// leaving the entities from `e` down to it in `path`.
fn find(
    e: Entity,
    path: &mut Vec<Entity>,
    is_match: &dyn Fn(Entity) -> bool,
    kids: &dyn Fn(Entity) -> Vec<Entity>,
) -> bool {
    path.push(e);
    if is_match(e) || kids(e).into_iter().any(|c| find(c, path, is_match, kids)) {
        return true;
    }
    path.pop();
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `root` holds `bare` (no geometry), `other` (itself a mesh) and `arm`
    /// (translated) with a triangle and a child node `hand` with another.
    fn app_with(submesh: &str, center: bool) -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            TransformPlugin,
            bevy::scene::ScenePlugin,
        ))
        .init_asset::<Mesh>();
        let tri = |x: f32| {
            Mesh::new(
                bevy::mesh::PrimitiveTopology::TriangleList,
                bevy::asset::RenderAssetUsages::default(),
            )
            .with_inserted_attribute(
                Mesh::ATTRIBUTE_POSITION,
                vec![[x, 0.0, 0.0], [x + 2.0, 0.0, 0.0], [x, 4.0, 0.0]],
            )
        };
        let a = app.world_mut().resource_mut::<Assets<Mesh>>().add(tri(0.0));
        let b = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(tri(10.0));
        let mut world = World::new();
        let root = world.spawn((Name::new("root"), Transform::IDENTITY)).id();
        world.spawn((Name::new("bare"), Transform::IDENTITY, ChildOf(root)));
        world.spawn((
            Name::new("other"),
            Transform::IDENTITY,
            Mesh3d(b.clone()),
            ChildOf(root),
        ));
        let arm = world
            .spawn((
                Name::new("arm"),
                Transform::from_xyz(100.0, 0.0, 0.0),
                ChildOf(root),
            ))
            .id();
        world.spawn((Mesh3d(a), Transform::from_xyz(0.0, 1.0, 0.0), ChildOf(arm)));
        let hand = world
            .spawn((Name::new("hand"), Transform::IDENTITY, ChildOf(arm)))
            .id();
        world.spawn((Mesh3d(b), Transform::IDENTITY, ChildOf(hand)));
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

    fn meshes_below(app: &mut App, e: Entity) -> Vec<Vec3> {
        let world = app.world_mut();
        let mut q = world.query::<(Entity, &Mesh3d, &GlobalTransform)>();
        let below: Vec<_> = q
            .iter(world)
            .filter(|(m, ..)| {
                std::iter::successors(Some(*m), |&x| world.get::<ChildOf>(x).map(|c| c.0))
                    .any(|x| x == e)
            })
            .map(|(_, _, g)| g.translation())
            .collect();
        below
    }

    #[test]
    fn keeps_only_the_named_node_geometry() {
        let (mut app, e) = app_with("arm", false);
        assert!(app.world().get::<GzSubmesh>(e).unwrap().applied);
        // `other` and `hand` are gone; `arm`'s triangle keeps its pose.
        assert_eq!(meshes_below(&mut app, e), vec![Vec3::new(100.0, 1.0, 0.0)]);
    }

    #[test]
    fn centers_the_bounds_on_the_origin() {
        let (mut app, e) = app_with("arm", true);
        // The triangle spans (100..102, 1..5): its centre (101, 3) moves to 0.
        assert_eq!(meshes_below(&mut app, e), vec![Vec3::new(-1.0, -2.0, 0.0)]);
        assert_eq!(
            app.world().get::<Transform>(e).unwrap().translation,
            Vec3::new(-101.0, -3.0, 0.0)
        );
    }

    #[test]
    fn a_node_that_is_itself_a_mesh_matches() {
        let (mut app, e) = app_with("other", true);
        assert_eq!(meshes_below(&mut app, e), vec![Vec3::new(-11.0, -2.0, 0.0)]);
    }

    #[test]
    fn an_unknown_name_or_one_without_geometry_leaves_nothing() {
        for name in ["nope", "bare"] {
            let (mut app, e) = app_with(name, false);
            assert!(app.world().get::<GzSubmesh>(e).unwrap().applied);
            assert!(meshes_below(&mut app, e).is_empty(), "{name}");
        }
    }
}
