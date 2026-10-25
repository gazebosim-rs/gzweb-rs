//! Instantiates a [`SceneDesc`] as Bevy entities and applies pose updates.
//!
//! Everything Gazebo lives under one [`GzWorldRoot`] entity that rotates
//! Gazebo's Z-up frame into Bevy's Y-up frame. Below it, entities mirror the
//! Gazebo tree (model → link → visual / light) with **raw Gazebo local
//! poses**, like gzweb's three.js `Object3D` tree. Mesh files load untouched
//! and pose updates are plain local-transform writes.

use std::collections::{HashMap, HashSet};
use std::f32::consts::FRAC_PI_2;

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use gz_core::scene::{
    BuildOptions, GeometryDesc, LightDesc, LightType, Node, NodeKind, PathAllocator, Pose,
    SceneDesc, SensorDesc, VisualDesc,
};
use gz_core::uri;

use crate::assets;
use crate::connection::GzInbox;
use crate::heightmap;
use crate::material::{self, Fallback, GzMaterials, GzMeshMaterial};
use crate::render::{self, GzEnvironment, GzGrid};
use crate::submesh;
use crate::GzConfig;

/// Rotates Gazebo Z-up into Bevy Y-up: gz `(x, y, z)` → bevy `(x, z, -y)`.
#[derive(Component)]
pub struct GzWorldRoot;

pub fn gz_to_bevy() -> Quat {
    Quat::from_rotation_x(-FRAC_PI_2)
}

/// A Gazebo model, link, visual or light.
#[derive(Component, Debug, Clone)]
pub struct GzEntity {
    pub name: String,
    pub id: u32,
    pub kind: NodeKind,
    pub path: String,
}

#[derive(Component, Debug, Clone)]
pub struct GzVisual(pub VisualDesc);

#[derive(Component, Debug, Clone)]
pub struct GzLight(pub LightDesc);

/// A link's sensors, as the scene message lists them (the UI's scene tree
/// shows them; nothing renders them).
#[derive(Component, Debug, Clone, PartialEq)]
pub struct GzSensors(pub Vec<SensorDesc>);

/// The child entity that carries a light's Bevy light component.
#[derive(Component)]
pub struct GzLightEmitter;

/// Lookup tables and spawn bookkeeping.
#[derive(Resource, Default)]
pub struct GzIndex {
    pub root: Option<Entity>,
    pub by_id: HashMap<u32, Entity>,
    pub by_name: HashMap<String, Vec<Entity>>,
    pub model_names: HashSet<String>,
    /// Entities in node-walk order (for deterministic snapshots).
    pub order: Vec<Entity>,
    pub paths: PathAllocator,
    pub scene: Option<SceneDesc>,
    /// Scenes received: one after the first replaced it (a reconnect).
    pub scenes: u32,
}

/// Asset handles whose loading the snapshot waits for.
#[derive(Resource, Default)]
pub struct GzLoads(pub Vec<UntypedHandle>);

/// A glTF mesh waiting for its file; [`instance_gltf`] then adds the
/// `WorldAssetRoot` for its scene. It is loaded by asset type rather than as
/// `file#Scene0`, which Bevy would match to a loader by extension, case
/// sensitively (a `.GLB` would find none).
#[derive(Component, Debug, Clone)]
pub struct GzGltf(pub Handle<Gltf>);

/// Instances each loaded glTF's default scene (else its first), as three.js's
/// `GLTFLoader` does.
pub fn instance_gltf(
    mut commands: Commands,
    pending: Query<(Entity, &GzGltf), Without<WorldAssetRoot>>,
    gltfs: Option<Res<Assets<Gltf>>>,
) {
    let Some(gltfs) = gltfs else { return };
    for (entity, gltf) in &pending {
        let Some(gltf) = gltfs.get(&gltf.0) else {
            continue;
        };
        match gltf
            .default_scene
            .clone()
            .or_else(|| gltf.scenes.first().cloned())
        {
            Some(scene) => {
                commands.entity(entity).insert(WorldAssetRoot(scene));
            }
            None => {
                warn!("glTF has no scene");
                commands.entity(entity).remove::<GzGltf>();
            }
        }
    }
}

pub fn spawn_root(mut commands: Commands, mut index: ResMut<GzIndex>) {
    let root = commands
        .spawn((
            Name::new("gz_world"),
            GzWorldRoot,
            Transform::from_rotation(gz_to_bevy()),
            Visibility::default(),
        ))
        .id();
    index.root = Some(root);
}

/// Spawns the initial scene and `scene/info` additions. [`GzIndex`] is
/// marked changed only when entities come or go, so the UI's scene tree
/// rebuilds on structure changes, not every frame.
#[allow(clippy::too_many_arguments)]
pub fn apply_scene(
    mut commands: Commands,
    mut inbox: ResMut<GzInbox>,
    mut index: ResMut<GzIndex>,
    mut loads: ResMut<GzLoads>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut gz_materials: ResMut<GzMaterials>,
    asset_server: Res<AssetServer>,
    config: Res<GzConfig>,
    mut environment: ResMut<GzEnvironment>,
    mut grid: ResMut<GzGrid>,
) {
    let Some(root) = index.root else { return };
    if inbox.scene.is_none() && inbox.scene_updates.is_empty() {
        return;
    }

    let opts = BuildOptions {
        enable_lights: config.enable_lights,
    };
    let mut new_nodes = Vec::new();
    if let Some(msg) = inbox.scene.take() {
        // A new scene after a reconnect replaces everything.
        if index.scene.is_some() {
            commands.entity(root).despawn_related::<Children>();
            *index = GzIndex {
                root: Some(root),
                scenes: index.scenes,
                ..Default::default()
            };
            loads.0.clear();
        }
        let desc = SceneDesc::from_msg_with(&msg, opts, &mut index.paths);
        // An identical scene (a reconnect) leaves the environment, camera and
        // the user's grid toggle alone.
        if environment.set_if_neq(GzEnvironment::from_scene(&desc)) {
            grid.visible = desc.grid;
        }
        new_nodes.extend(desc.roots.iter().cloned());
        index.scene = Some(desc);
        index.scenes += 1;
    }

    for update in std::mem::take(&mut inbox.scene_updates) {
        let known = index.model_names.clone();
        let mut paths = std::mem::take(&mut index.paths);
        let nodes = SceneDesc::new_models(&update, &|name| known.contains(name), &mut paths, opts);
        index.paths = paths;
        new_nodes.extend(nodes);
    }

    let mut ctx = SpawnCtx {
        commands: &mut commands,
        index: &mut index,
        loads: &mut loads,
        meshes: &mut meshes,
        materials: &mut materials,
        gz_materials: &mut gz_materials,
        asset_server: &asset_server,
    };
    for node in &new_nodes {
        if node.kind == NodeKind::Model {
            ctx.index.model_names.insert(node.name.clone());
        }
        spawn_node(&mut ctx, node, root);
    }
}

struct SpawnCtx<'a, 'w, 's> {
    commands: &'a mut Commands<'w, 's>,
    index: &'a mut GzIndex,
    loads: &'a mut GzLoads,
    meshes: &'a mut Assets<Mesh>,
    materials: &'a mut Assets<StandardMaterial>,
    gz_materials: &'a mut GzMaterials,
    asset_server: &'a AssetServer,
}

impl SpawnCtx<'_, '_, '_> {
    /// The visual's material ([`material::build`]); snapshots wait for its
    /// textures.
    fn material(&mut self, visual: &VisualDesc, fallback: Fallback) -> material::VisualMaterial {
        let m = material::build(
            self.asset_server,
            self.materials,
            self.gz_materials,
            &visual.material,
            visual.transparency,
            fallback,
        );
        self.loads.0.extend(m.loads.iter().cloned());
        m
    }
}

/// What spawning a visual's geometry needs, for geometry that isn't part
/// of the scene message: the overlays' collision shapes, drawn the way a
/// visual with the same geometry and material would be.
#[derive(bevy::ecs::system::SystemParam)]
pub struct GeometryAssets<'w> {
    loads: ResMut<'w, GzLoads>,
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<StandardMaterial>>,
    gz_materials: ResMut<'w, GzMaterials>,
    asset_server: Res<'w, AssetServer>,
}

impl GeometryAssets<'_> {
    /// Spawns `visual`'s geometry (mesh files included) under `parent`.
    pub fn spawn(
        &mut self,
        commands: &mut Commands,
        visual: &VisualDesc,
        parent: Entity,
        name: &str,
    ) {
        let mut index = GzIndex::default();
        let mut ctx = SpawnCtx {
            commands,
            index: &mut index,
            loads: &mut self.loads,
            meshes: &mut self.meshes,
            materials: &mut self.materials,
            gz_materials: &mut self.gz_materials,
            asset_server: &self.asset_server,
        };
        spawn_geometry(&mut ctx, visual, parent, name);
    }

    /// Spawns `node` and what it holds under `parent`, as the scene
    /// message's nodes are, but known to no [`GzIndex`]: a part added to a
    /// model being edited (`view::editing::parts`). Returns it.
    pub fn spawn_node(&mut self, commands: &mut Commands, node: &Node, parent: Entity) -> Entity {
        let mut index = GzIndex::default();
        let mut ctx = SpawnCtx {
            commands,
            index: &mut index,
            loads: &mut self.loads,
            meshes: &mut self.meshes,
            materials: &mut self.materials,
            gz_materials: &mut self.gz_materials,
            asset_server: &self.asset_server,
        };
        spawn_node(&mut ctx, node, parent)
    }
}

pub fn pose_transform(pose: &Pose, scale: [f64; 3]) -> Transform {
    let [x, y, z] = pose.pos;
    let [qx, qy, qz, qw] = pose.quat;
    Transform {
        translation: Vec3::new(x as f32, y as f32, z as f32),
        rotation: Quat::from_xyzw(qx as f32, qy as f32, qz as f32, qw as f32),
        scale: Vec3::new(scale[0] as f32, scale[1] as f32, scale[2] as f32),
    }
}

fn spawn_node(ctx: &mut SpawnCtx, node: &Node, parent: Entity) -> Entity {
    let entity = ctx
        .commands
        .spawn((
            Name::new(node.name.clone()),
            GzEntity {
                name: node.name.clone(),
                id: node.id,
                kind: node.kind,
                path: node.path.clone(),
            },
            pose_transform(&node.pose, node.scale),
            if node.visible {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            },
            ChildOf(parent),
        ))
        .id();

    ctx.index.order.push(entity);
    if node.id != 0 {
        ctx.index.by_id.insert(node.id, entity);
    }
    ctx.index
        .by_name
        .entry(node.name.clone())
        .or_default()
        .push(entity);

    if let Some(visual) = &node.visual {
        ctx.commands.entity(entity).insert(GzVisual(visual.clone()));
        spawn_geometry(ctx, visual, entity, &node.name);
    }
    if let Some(light) = &node.light {
        ctx.commands.entity(entity).insert(GzLight(light.clone()));
        spawn_light(ctx, light, entity);
    }
    if !node.sensors.is_empty() {
        ctx.commands
            .entity(entity)
            .insert(GzSensors(node.sensors.clone()));
    }
    for child in &node.children {
        spawn_node(ctx, child, entity);
    }
    entity
}

/// Child entity holding the renderable geometry, in the visual's Gazebo frame.
/// Bevy's round primitives have their axis along +Y; gzweb rotates three's
/// (also +Y) by +90° about X so the axis is +Z, and so do we.
fn spawn_geometry(ctx: &mut SpawnCtx, visual: &VisualDesc, parent: Entity, name: &str) {
    let axis_to_z = Transform::from_rotation(Quat::from_rotation_x(FRAC_PI_2));
    let (mesh, local): (Mesh, Transform) = match &visual.geometry {
        GeometryDesc::Box { size } => (
            Cuboid::new(size[0] as f32, size[1] as f32, size[2] as f32).into(),
            Transform::IDENTITY,
        ),
        GeometryDesc::Cylinder { radius, length } => (
            Cylinder::new(*radius as f32, *length as f32)
                .mesh()
                .resolution(32)
                .into(),
            axis_to_z,
        ),
        GeometryDesc::Capsule { radius, length } => (
            Capsule3d::new(*radius as f32, *length as f32)
                .mesh()
                .longitudes(32)
                .latitudes(16)
                .into(),
            axis_to_z,
        ),
        GeometryDesc::Cone { radius, length } => (
            Cone::new(*radius as f32, *length as f32)
                .mesh()
                .resolution(32)
                .into(),
            axis_to_z,
        ),
        GeometryDesc::Sphere { radius } => {
            (three_sphere(*radius as f32, 32, 32), Transform::IDENTITY)
        }
        GeometryDesc::Ellipsoid { radii } => (
            three_sphere(1.0, 32, 32),
            Transform::from_scale(Vec3::new(radii[0] as f32, radii[1] as f32, radii[2] as f32)),
        ),
        GeometryDesc::Plane { normal, size } => {
            let n = Vec3::new(normal[0] as f32, normal[1] as f32, normal[2] as f32)
                .normalize_or(Vec3::Z);
            (
                Plane3d::new(
                    Vec3::Z,
                    Vec2::new(size[0] as f32 / 2.0, size[1] as f32 / 2.0),
                )
                .mesh()
                .into(),
                Transform::from_rotation(Quat::from_rotation_arc(Vec3::Z, n)),
            )
        }
        GeometryDesc::Mesh { load_uri, .. } => {
            spawn_mesh_file(ctx, visual, parent, name, load_uri);
            return;
        }
        GeometryDesc::Heightmap(desc) => {
            let loads = heightmap::spawn(ctx.commands, ctx.asset_server, desc, parent);
            ctx.loads.0.extend(loads);
            return;
        }
        GeometryDesc::Unsupported(kind) => {
            info!("visual '{name}': geometry '{kind}' not supported yet");
            return;
        }
    };

    let mut mesh = mesh;
    let m = ctx.material(visual, Fallback::GzGrey);
    material::prepare_mesh(&mut mesh, m.tangents, m.lightmap.as_ref().map(|l| l.1));
    let mesh = ctx.meshes.add(mesh);
    let mut e = ctx.commands.spawn((
        Mesh3d(mesh),
        MeshMaterial3d(m.material),
        local,
        ChildOf(parent),
    ));
    if let Some((image, _)) = &m.lightmap {
        e.insert(material::lightmap(image));
    }
    if !visual.cast_shadows {
        e.insert(NotShadowCaster);
    }
}

/// three.js `SphereGeometry(radius, width, height)`, vertex for vertex, so
/// bounds of rotated spheres match gzweb's. Poles are on local +/-Y.
pub fn three_sphere(radius: f32, width: u32, height: u32) -> Mesh {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};
    use std::f32::consts::{PI, TAU};

    let (w, h) = (width.max(3), height.max(2));
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    for iy in 0..=h {
        let v = iy as f32 / h as f32;
        let u_offset = match iy {
            0 => 0.5 / w as f32,
            _ if iy == h => -0.5 / w as f32,
            _ => 0.0,
        };
        for ix in 0..=w {
            let u = ix as f32 / w as f32;
            let (sin_t, cos_t) = (v * PI).sin_cos();
            let (sin_p, cos_p) = (u * TAU).sin_cos();
            let n = Vec3::new(-cos_p * sin_t, cos_t, sin_p * sin_t);
            positions.push((n * radius).to_array());
            normals.push(n.normalize_or_zero().to_array());
            // three's v runs bottom-up; Bevy's runs top-down.
            uvs.push([u + u_offset, v]);
        }
    }
    let at = |iy: u32, ix: u32| iy * (w + 1) + ix;
    let mut indices = Vec::new();
    for iy in 0..h {
        for ix in 0..w {
            let (a, b, c, d) = (
                at(iy, ix + 1),
                at(iy, ix),
                at(iy + 1, ix),
                at(iy + 1, ix + 1),
            );
            if iy != 0 {
                indices.extend([a, b, d]);
            }
            if iy != h - 1 {
                indices.extend([b, c, d]);
            }
        }
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(indices))
}

fn spawn_mesh_file(
    ctx: &mut SpawnCtx,
    visual: &VisualDesc,
    parent: Entity,
    name: &str,
    load_uri: &str,
) {
    let path = assets::asset_path(load_uri);
    match uri::extension(load_uri).as_str() {
        "glb" | "gltf" => {
            let gltf: Handle<Gltf> = ctx.asset_server.load(path);
            ctx.loads.0.push(gltf.clone().untyped());
            let entity = ctx
                .commands
                .spawn((GzGltf(gltf), Transform::IDENTITY, ChildOf(parent)))
                .id();
            add_submesh(ctx, visual, entity);
            add_mesh_material(ctx, visual, entity);
        }
        "stl" => {
            let mesh: Handle<Mesh> = ctx.asset_server.load(path);
            ctx.loads.0.push(mesh.clone().untyped());
            // STL has no UVs, so no tangents or light map either.
            let m = ctx.material(visual, Fallback::Mesh);
            let mut e = ctx.commands.spawn((
                Mesh3d(mesh),
                MeshMaterial3d(m.material),
                Transform::IDENTITY,
                ChildOf(parent),
            ));
            if !visual.cast_shadows {
                e.insert(NotShadowCaster);
            }
        }
        // `SceneFileLoader` reads both, in any case.
        "obj" | "dae" => {
            let scene: Handle<WorldAsset> = ctx.asset_server.load(path);
            ctx.loads.0.push(scene.clone().untyped());
            let entity = ctx
                .commands
                .spawn((WorldAssetRoot(scene), Transform::IDENTITY, ChildOf(parent)))
                .id();
            add_submesh(ctx, visual, entity);
            add_mesh_material(ctx, visual, entity);
        }
        ext => warn!("visual '{name}': unsupported mesh extension '{ext}' ({load_uri})"),
    }
}

/// Cuts a mesh file's scene on `entity` down to its `<submesh>`, if the
/// visual names one, once it is instanced.
fn add_submesh(ctx: &mut SpawnCtx, visual: &VisualDesc, entity: Entity) {
    if let GeometryDesc::Mesh {
        submesh: Some(name),
        center_submesh,
        ..
    } = &visual.geometry
    {
        ctx.commands
            .entity(entity)
            .insert(submesh::GzSubmesh {
                name: name.clone(),
                center: *center_submesh,
                applied: false,
            })
            .observe(submesh::select);
    }
}

/// Applies the visual's material rules to a mesh file's scene on `entity`
/// once it is instanced ([`material::apply_to_scene`]): its SDF material
/// replaces every submesh's, as in Gazebo.
fn add_mesh_material(ctx: &mut SpawnCtx, visual: &VisualDesc, entity: Entity) {
    let replace = visual
        .material
        .present
        .then(|| ctx.material(visual, Fallback::Mesh));
    ctx.commands
        .entity(entity)
        .insert(GzMeshMaterial {
            replace,
            transparency: visual.transparency as f32,
            cast_shadows: visual.cast_shadows,
        })
        .observe(material::apply_to_scene);
}

/// The Bevy light sits on a child entity that looks along the Gazebo
/// direction (Bevy lights shine along local -Z).
fn spawn_light(ctx: &mut SpawnCtx, light: &LightDesc, parent: Entity) {
    let [r, g, b] = light.color;
    // Linear, as ogre2 uses it (like material colours).
    let color = Color::linear_rgb(r, g, b);
    let mut local = Transform::IDENTITY;
    if let Some([x, y, z]) = light.direction {
        let dir = Vec3::new(x as f32, y as f32, z as f32);
        if dir.length_squared() > 0.0 {
            let up = if dir.cross(Vec3::Z).length_squared() < 1e-8 {
                Vec3::Y
            } else {
                Vec3::Z
            };
            local = local.looking_to(dir, up);
        }
    }
    // Gazebo's own intensity, not gzweb's attenuation-derived one. Servers
    // that leave it unset (0) mean SDF's default of 1.
    let intensity = match light.msg_intensity as f32 {
        i if i > 0.0 => i,
        _ => 1.0,
    };
    let range = if light.range > 0.0 {
        light.range as f32
    } else {
        1000.0
    };
    let mut e = ctx.commands.spawn((GzLightEmitter, local, ChildOf(parent)));
    match light.ty {
        LightType::Point => {
            e.insert(PointLight {
                color,
                intensity: render::units::point_lumens(intensity),
                range,
                shadow_maps_enabled: light.cast_shadows,
                ..default()
            });
        }
        LightType::Spot => {
            let outer = light.angle.unwrap_or(0.5) as f32;
            let inner = light.inner_angle.map_or(0.0, |a| a as f32);
            e.insert(SpotLight {
                color,
                intensity: render::units::point_lumens(intensity),
                range,
                outer_angle: outer,
                inner_angle: inner,
                shadow_maps_enabled: light.cast_shadows,
                ..default()
            });
        }
        LightType::Directional => {
            e.insert((
                DirectionalLight {
                    color,
                    illuminance: render::units::directional_lux(intensity),
                    shadow_maps_enabled: light.cast_shadows,
                    ..default()
                },
                render::sun_cascades(),
            ));
        }
    }
}

/// Applies `dynamic_pose/info` poses as local transforms. Entities are found
/// by id first (gzweb uses the first name match, which is ambiguous for
/// repeated names such as `link`), then by name.
/// Despawns the entities `scene/deletion` names (gz-sim's
/// `SceneBroadcaster` publishes the ids of removed models, links, visuals,
/// lights and sensors) with everything under them, and forgets them in
/// [`GzIndex`]: a model spawned later under a deleted one's name is new.
/// It runs before [`apply_scene`], so a model deleted and spawned again
/// under the same name in one frame is spawned anew; an id it doesn't know
/// yet (a model spawned in the same frame, which `apply_scene` indexes
/// next) is tried again once on the next frame, then dropped (sensors,
/// children of a parent already removed). gzweb doesn't handle deletions.
pub fn apply_deletions(
    mut commands: Commands,
    mut inbox: ResMut<GzInbox>,
    mut index: ResMut<GzIndex>,
    entities: Query<&GzEntity>,
    children: Query<&Children>,
    mut retry: Local<Vec<u32>>,
) {
    if inbox.deletions.is_empty() && retry.is_empty() {
        return;
    }
    let ids: Vec<(u32, bool)> = std::mem::take(&mut *retry)
        .into_iter()
        .map(|id| (id, false))
        .chain(
            std::mem::take(&mut inbox.deletions)
                .into_iter()
                .map(|id| (id, true)),
        )
        .collect();
    let mut gone = HashSet::new();
    for (id, first) in ids {
        let Some(&top) = index.by_id.get(&id).filter(|_| id != 0) else {
            if first && id != 0 {
                retry.push(id);
            }
            continue;
        };
        if gone.contains(&top) {
            continue;
        }
        let parent_is_root = index
            .root
            .is_some_and(|root| children.get(root).is_ok_and(|c| c.iter().any(|e| e == top)));
        for e in std::iter::once(top).chain(children.iter_descendants(top)) {
            let Ok(gz) = entities.get(e) else { continue };
            gone.insert(e);
            // A model spawned again under the name gets the same paths.
            index.paths.free(&gz.path);
            if index.by_id.get(&gz.id) == Some(&e) {
                index.by_id.remove(&gz.id);
            }
            if let Some(v) = index.by_name.get_mut(&gz.name) {
                v.retain(|&x| x != e);
                if v.is_empty() {
                    index.by_name.remove(&gz.name);
                }
            }
            if e == top && parent_is_root && gz.kind == NodeKind::Model {
                index.model_names.remove(&gz.name);
            }
        }
        commands.entity(top).despawn();
    }
    if !gone.is_empty() {
        index.order.retain(|e| !gone.contains(e));
    }
}

pub fn apply_poses(
    mut inbox: ResMut<GzInbox>,
    index: Res<GzIndex>,
    held: Option<Res<crate::edit::Held>>,
    time: Option<Res<Time<Real>>>,
    mut transforms: Query<&mut Transform>,
) {
    let now = time.map_or(0.0, |t| t.elapsed_secs_f64());
    if inbox.poses.is_empty() {
        return;
    }
    for pose in std::mem::take(&mut inbox.poses) {
        let entity = index
            .by_id
            .get(&pose.id)
            .filter(|_| pose.id != 0)
            .or_else(|| index.by_name.get(&pose.name).and_then(|v| v.first()));
        let Some(&entity) = entity else { continue };
        // Being edited: the edit's pose stands until the sim has it.
        if held.as_ref().is_some_and(|h| h.holds(entity, now)) {
            continue;
        }
        if let Ok(mut t) = transforms.get_mut(entity) {
            let p = Pose::from_msg(Some(&pose));
            let next = pose_transform(&p, [1.0; 3]);
            t.translation = next.translation;
            t.rotation = next.rotation;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::scene_app;
    use gz_core::msgs;

    fn v3(x: f64, y: f64, z: f64) -> Option<msgs::Vector3d> {
        Some(msgs::Vector3d {
            x,
            y,
            z,
            ..Default::default()
        })
    }

    fn light(name: &str, ty: msgs::light::LightType, dir: [f64; 3]) -> msgs::Light {
        msgs::Light {
            name: name.into(),
            r#type: ty as i32,
            direction: v3(dir[0], dir[1], dir[2]),
            spot_outer_angle: 0.8,
            spot_falloff: 0.5,
            ..Default::default()
        }
    }

    fn box_visual(name: &str, material: msgs::Material) -> msgs::Visual {
        msgs::Visual {
            name: name.into(),
            geometry: Some(msgs::Geometry {
                r#box: Some(msgs::BoxGeom::default()),
                ..Default::default()
            }),
            material: Some(material),
            ..Default::default()
        }
    }

    fn entity(app: &mut App, name: &str) -> Entity {
        app.world().resource::<GzIndex>().by_name[name][0]
    }

    fn emitter(app: &mut App, light: Entity) -> Transform {
        let world = app.world_mut();
        let children = world.get::<Children>(light).unwrap().to_vec();
        children
            .into_iter()
            .find(|c| world.get::<GzLightEmitter>(*c).is_some())
            .map(|c| *world.get::<Transform>(c).unwrap())
            .unwrap()
    }

    fn scene() -> msgs::Scene {
        use msgs::light::LightType;
        let link = msgs::Link {
            name: "l".into(),
            visual: vec![
                box_visual(
                    "two_sided",
                    msgs::Material {
                        double_sided: true,
                        ..Default::default()
                    },
                ),
                msgs::Visual {
                    name: "polyline".into(),
                    geometry: Some(msgs::Geometry {
                        r#type: msgs::geometry::Type::Polyline as i32,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            ],
            light: vec![light("lamp", LightType::Point, [0.0, 0.0, -1.0])],
            ..Default::default()
        };
        msgs::Scene {
            name: "w".into(),
            ambient: Some(msgs::Color {
                r: 0.5,
                g: 0.5,
                b: 0.5,
                a: 1.0,
                ..Default::default()
            }),
            // Alpha 0: no background was set, so the clear colour stays.
            background: Some(msgs::Color::default()),
            model: vec![msgs::Model {
                name: "m".into(),
                id: 7,
                link: vec![link],
                ..Default::default()
            }],
            light: vec![
                light("down", LightType::Directional, [0.0, 0.0, -1.0]),
                light("aside", LightType::Spot, [1.0, 0.0, 0.0]),
                light("nowhere", LightType::Spot, [0.0, 0.0, 0.0]),
            ],
            ..Default::default()
        }
    }

    #[test]
    fn lights_use_gazebo_intensity_cones_and_cascades() {
        let mut app = scene_app(GzConfig::default());
        let mut spot = light("cone", msgs::light::LightType::Spot, [0.0, 0.0, -1.0]);
        spot.intensity = 2.0;
        spot.spot_inner_angle = 0.3;
        // Unset (0): SDF's default of 1.
        let sun = light("sun", msgs::light::LightType::Directional, [0.0, 0.0, -1.0]);
        app.world_mut().resource_mut::<GzInbox>().scene = Some(Box::new(msgs::Scene {
            light: vec![spot, sun],
            ..Default::default()
        }));
        app.update();
        app.update();
        let cone = entity(&mut app, "cone");
        let world = app.world_mut();
        let child = world.get::<Children>(cone).unwrap()[0];
        let s = world.get::<SpotLight>(child).unwrap();
        assert_eq!(s.intensity, render::units::point_lumens(2.0));
        assert!((s.inner_angle - 0.3).abs() < 1e-6 && (s.outer_angle - 0.8).abs() < 1e-6);
        let sun = entity(&mut app, "sun");
        let world = app.world_mut();
        let child = world.get::<Children>(sun).unwrap()[0];
        let d = world.get::<DirectionalLight>(child).unwrap();
        assert_eq!(d.illuminance, render::units::directional_lux(1.0));
        let cascades = world
            .get::<bevy::light::CascadeShadowConfig>(child)
            .unwrap();
        assert_eq!(cascades.bounds.len(), 4);
        assert!((cascades.bounds[3] - 150.0).abs() < 1e-3);
    }

    #[test]
    fn spawns_lights_materials_and_hidden_link_lights() {
        let mut app = scene_app(GzConfig {
            enable_lights: false,
            ..default()
        });
        app.world_mut().resource_mut::<GzInbox>().scene = Some(Box::new(scene()));
        app.update();

        assert_eq!(
            app.world().resource::<GlobalAmbientLight>().color,
            Color::srgb(0.5, 0.5, 0.5)
        );
        let world = app.world_mut();
        let cam = world.query::<&Camera>().single(world).unwrap().clear_color;
        assert!(matches!(cam, ClearColorConfig::Custom(c) if c == crate::camera::CLEAR_COLOR));

        // enableLights = false hides lights attached to links.
        let lamp = entity(&mut app, "lamp");
        assert_eq!(
            app.world().get::<Visibility>(lamp),
            Some(&Visibility::Hidden)
        );

        // Emitters look along the Gazebo direction (Bevy lights shine along -Z).
        let down = entity(&mut app, "down");
        let fwd = emitter(&mut app, down).forward();
        assert!((fwd.as_vec3() - Vec3::NEG_Z).length() < 1e-5, "{fwd:?}");
        let aside = entity(&mut app, "aside");
        assert!((emitter(&mut app, aside).forward().as_vec3() - Vec3::X).length() < 1e-5);
        let nowhere = entity(&mut app, "nowhere");
        assert_eq!(emitter(&mut app, nowhere).rotation, Quat::IDENTITY);

        // Double-sided materials disable culling.
        let two_sided = entity(&mut app, "two_sided");
        let world = app.world_mut();
        let child = world.get::<Children>(two_sided).unwrap()[0];
        let handle = world
            .get::<MeshMaterial3d<StandardMaterial>>(child)
            .unwrap()
            .0
            .clone();
        let mat = world
            .resource::<Assets<StandardMaterial>>()
            .get(&handle)
            .unwrap();
        assert!(mat.double_sided);
        assert_eq!(mat.cull_mode, None);

        // Unsupported geometry spawns the node but no mesh.
        let polyline = entity(&mut app, "polyline");
        assert!(app.world().get::<Children>(polyline).is_none());
    }

    #[test]
    fn primitives_get_gazebo_materials_tangents_and_light_maps() {
        let mut app = scene_app(GzConfig::default());
        let pbr = msgs::Material {
            diffuse: Some(msgs::Color {
                r: 0.5,
                g: 0.5,
                b: 0.5,
                a: 1.0,
                ..Default::default()
            }),
            pbr: Some(msgs::material::Pbr {
                normal_map: "model://m/n.png".into(),
                light_map: "model://m/l.png".into(),
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut lit = box_visual("lit", pbr);
        lit.cast_shadows = true;
        let bare = msgs::Visual {
            material: None,
            ..box_visual("bare", Default::default())
        };
        let mut mesh = box_visual("dae", Default::default());
        mesh.geometry = Some(msgs::Geometry {
            mesh: Some(msgs::MeshGeom {
                filename: "/m/a.dae".into(),
                ..Default::default()
            }),
            ..Default::default()
        });
        let link = msgs::Link {
            name: "l".into(),
            visual: vec![lit, bare, mesh],
            ..Default::default()
        };
        app.world_mut().resource_mut::<GzInbox>().scene = Some(Box::new(msgs::Scene {
            model: vec![msgs::Model {
                name: "m".into(),
                link: vec![link],
                ..Default::default()
            }],
            ..Default::default()
        }));
        app.update();
        let child = |app: &mut App, name: &str| {
            let e = entity(app, name);
            app.world().get::<Children>(e).unwrap()[0]
        };
        let material = |app: &mut App, e: Entity| {
            let h = app
                .world()
                .get::<MeshMaterial3d<StandardMaterial>>(e)
                .unwrap()
                .0
                .clone();
            app.world()
                .resource::<Assets<StandardMaterial>>()
                .get(&h)
                .unwrap()
                .clone()
        };

        let lit = child(&mut app, "lit");
        let m = material(&mut app, lit);
        assert_eq!(m.base_color, Color::linear_rgba(0.5, 0.5, 0.5, 1.0));
        assert!(m.normal_map_texture.is_some());
        assert!(app.world().get::<bevy::pbr::Lightmap>(lit).is_some());
        assert!(app.world().get::<NotShadowCaster>(lit).is_none());
        let h = app.world().get::<Mesh3d>(lit).unwrap().0.clone();
        let meshes = app.world().resource::<Assets<Mesh>>();
        let mesh = meshes.get(&h).unwrap();
        assert!(mesh.contains_attribute(Mesh::ATTRIBUTE_TANGENT));
        assert!(mesh.contains_attribute(Mesh::ATTRIBUTE_UV_1));
        // Snapshots wait for the two maps.
        assert_eq!(app.world().resource::<GzLoads>().0.len(), 3);

        // No material: gz-grey; `cast_shadows` unset: no shadow.
        let bare = child(&mut app, "bare");
        let m = material(&mut app, bare);
        assert_eq!(m.base_color, Color::linear_rgba(0.7, 0.7, 0.7, 1.0));
        assert_eq!(m.metallic, 1.0);
        assert!(app.world().get::<NotShadowCaster>(bare).is_some());

        // A mesh visual's material waits for the instanced scene.
        let dae = child(&mut app, "dae");
        let rule = app.world().get::<GzMeshMaterial>(dae).unwrap();
        assert!(rule.replace.is_some() && !rule.cast_shadows);
    }

    #[test]
    fn scene_updates_add_only_new_models_and_poses_skip_unknown_entities() {
        let mut app = scene_app(GzConfig::default());
        app.world_mut().resource_mut::<GzInbox>().scene = Some(Box::new(scene()));
        app.update();
        let before = app.world().resource::<GzIndex>().order.len();

        let mut inbox = app.world_mut().resource_mut::<GzInbox>();
        inbox.scene_updates.push(msgs::Scene {
            model: vec![
                msgs::Model {
                    name: "m".into(),
                    ..Default::default()
                },
                msgs::Model {
                    name: "late".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        });
        inbox.poses = vec![
            msgs::Pose {
                name: "nobody".into(),
                ..Default::default()
            },
            msgs::Pose {
                id: 7,
                position: v3(1.0, 2.0, 3.0),
                ..Default::default()
            },
        ];
        app.update();
        let index = app.world().resource::<GzIndex>();
        assert_eq!(index.order.len(), before + 1);
        assert_eq!(index.by_name["m"].len(), 1);
        let m = entity(&mut app, "m");
        assert_eq!(
            app.world().get::<Transform>(m).unwrap().translation,
            Vec3::new(1.0, 2.0, 3.0)
        );
    }

    #[test]
    fn a_second_scene_replaces_the_first() {
        let mut app = scene_app(GzConfig::default());
        app.world_mut().resource_mut::<GzInbox>().scene = Some(Box::new(scene()));
        app.update();
        let first = app.world().resource::<GzIndex>().order.len();
        let old_m = entity(&mut app, "m");

        let mut fresh = scene();
        fresh.light.clear();
        app.world_mut().resource_mut::<GzInbox>().scene = Some(Box::new(fresh));
        app.update();
        let index = app.world().resource::<GzIndex>();
        assert_eq!(index.order.len(), first - 3);
        assert_eq!(index.by_name["m"].len(), 1);
        assert!(app.world().get_entity(old_m).is_err());
        // The paths restart too: no `m#1`.
        let new_m = entity(&mut app, "m");
        let path = &app.world().get::<GzEntity>(new_m).unwrap().path;
        assert_eq!(path, "m");
    }

    /// A model with one link (id `id + 1`) carrying a camera.
    fn robot(name: &str, id: u32) -> msgs::Model {
        msgs::Model {
            name: name.into(),
            id,
            link: vec![msgs::Link {
                name: "base".into(),
                id: id + 1,
                sensor: vec![msgs::Sensor {
                    name: "cam".into(),
                    id: id + 2,
                    r#type: "camera".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    fn update_with(app: &mut App, models: Vec<msgs::Model>, deletions: Vec<u32>) {
        let mut inbox = app.world_mut().resource_mut::<GzInbox>();
        if !models.is_empty() {
            inbox.scene_updates.push(msgs::Scene {
                model: models,
                ..Default::default()
            });
        }
        inbox.deletions = deletions;
        app.update();
    }

    #[test]
    fn deletions_despawn_and_forget_entities_and_sensors_ride_on_links() {
        let mut app = scene_app(GzConfig::default());
        app.world_mut().resource_mut::<GzInbox>().scene = Some(Box::new(scene()));
        app.update();
        let before = app.world().resource::<GzIndex>().order.len();
        update_with(&mut app, vec![robot("r", 20)], vec![]);
        let r = entity(&mut app, "r");
        let base = app.world().resource::<GzIndex>().by_id[&21];
        assert_eq!(
            app.world().get::<GzSensors>(base).unwrap().0[0].ty,
            "camera"
        );
        assert!(app.world().get::<GzSensors>(r).is_none());
        // Nothing new: the index isn't marked changed.
        app.update();
        let tick = app.world().resource_ref::<GzIndex>().last_changed();
        app.update();
        assert_eq!(app.world().resource_ref::<GzIndex>().last_changed(), tick);

        // The model goes with its link; unknown and zero ids are skipped.
        update_with(&mut app, vec![], vec![20, 21, 0, 999]);
        app.update();
        let index = app.world().resource::<GzIndex>();
        assert_eq!(index.order.len(), before);
        assert!(!index.by_id.contains_key(&20) && !index.by_id.contains_key(&21));
        assert!(!index.by_name.contains_key("r") && !index.by_name.contains_key("base"));
        assert!(!index.model_names.contains("r"));
        assert!(app.world().get_entity(r).is_err() && app.world().get_entity(base).is_err());

        // Deleted and spawned again in one frame: the new one stays, with
        // the paths the first had (the SDFormat's links and joints, and the
        // tree's state, are keyed on them).
        let paths = |app: &mut App, id: u32| {
            let index = app.world().resource::<GzIndex>();
            let (m, l) = (index.by_id[&id], index.by_id[&(id + 1)]);
            let path = |e| app.world().get::<GzEntity>(e).unwrap().path.clone();
            (path(m), path(l))
        };
        update_with(&mut app, vec![robot("r", 30)], vec![]);
        assert_eq!(paths(&mut app, 30), ("r".into(), "r/base".into()));
        update_with(&mut app, vec![robot("r", 40)], vec![30]);
        let index = app.world().resource::<GzIndex>();
        assert_eq!(index.by_name["r"].len(), 1);
        assert!(index.by_id.contains_key(&40) && !index.by_id.contains_key(&30));
        assert_eq!(paths(&mut app, 40), ("r".into(), "r/base".into()));
        // Spawned and deleted in one frame: gone on the next.
        update_with(&mut app, vec![robot("brief", 50)], vec![50]);
        app.update();
        let index = app.world().resource::<GzIndex>();
        assert!(!index.by_name.contains_key("brief"));
        assert!(!index.model_names.contains("brief"));
        // A nested model's deletion leaves the top-level names alone.
        let inner = msgs::Model {
            model: vec![robot("r", 70)],
            ..robot("outer", 60)
        };
        update_with(&mut app, vec![inner], vec![]);
        update_with(&mut app, vec![], vec![70]);
        let index = app.world().resource::<GzIndex>();
        assert!(index.model_names.contains("outer") && index.model_names.contains("r"));
        assert_eq!(index.by_name["r"].len(), 1);
    }

    #[test]
    fn nothing_spawns_before_the_root_exists() {
        use bevy::ecs::system::RunSystemOnce;
        // No update yet, so Startup has not created the root.
        let mut app = scene_app(GzConfig::default());
        app.world_mut().resource_mut::<GzInbox>().scene = Some(Box::new(scene()));
        app.world_mut().run_system_once(apply_scene).unwrap();
        assert!(app.world().resource::<GzInbox>().scene.is_some());
    }
}
