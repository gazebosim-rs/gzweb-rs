//! Instantiates a [`SceneDesc`] as Bevy entities and applies pose updates.
//!
//! Everything Gazebo lives under one [`GzWorldRoot`] entity that rotates
//! Gazebo's Z-up frame into Bevy's Y-up frame. Below it, entities mirror the
//! Gazebo tree (model → link → visual / light) with **raw Gazebo local
//! poses**, like gzweb's three.js `Object3D` tree. Mesh files load untouched
//! and pose updates are plain local-transform writes.

use std::collections::{HashMap, HashSet};
use std::f32::consts::FRAC_PI_2;

use bevy::prelude::*;
use bevy::render::render_resource::Face;
use gz_core::scene::{
    BuildOptions, GeometryDesc, LightDesc, LightType, MaterialDesc, Node, NodeKind, PathAllocator,
    Pose, SceneDesc, VisualDesc,
};
use gz_core::uri;

use crate::assets;
use crate::connection::GzInbox;
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

/// The child entity that carries a light's Bevy light component.
#[derive(Component)]
pub struct GzLightEmitter;

/// A stand-in mesh for an unsupported format; excluded from snapshots.
#[derive(Component)]
pub struct GzPlaceholder;

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
}

/// Asset handles whose loading the snapshot waits for.
#[derive(Resource, Default)]
pub struct GzLoads(pub Vec<UntypedHandle>);

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

/// Spawns the initial scene and `scene/info` additions.
#[allow(clippy::too_many_arguments)]
pub fn apply_scene(
    mut commands: Commands,
    mut inbox: ResMut<GzInbox>,
    mut index: ResMut<GzIndex>,
    mut loads: ResMut<GzLoads>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
    config: Res<GzConfig>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut cameras: Query<&mut Camera>,
) {
    let Some(root) = index.root else { return };

    let opts = BuildOptions {
        enable_lights: config.enable_lights,
    };
    let mut new_nodes = Vec::new();
    if let Some(msg) = inbox.scene.take() {
        let desc = SceneDesc::from_msg_with(&msg, opts, &mut index.paths);
        if let Some([r, g, b]) = desc.ambient {
            ambient.color = Color::srgb(r, g, b);
        }
        // gzweb keeps its fixed clear colour; we honour an explicit
        // background (alpha > 0 means it was set).
        if let Some([r, g, b, a]) = desc.background {
            if a > 0.0 {
                for mut cam in &mut cameras {
                    cam.clear_color = ClearColorConfig::Custom(Color::srgb(r, g, b));
                }
            }
        }
        new_nodes.extend(desc.roots.iter().cloned());
        index.scene = Some(desc);
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
    asset_server: &'a AssetServer,
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

fn spawn_node(ctx: &mut SpawnCtx, node: &Node, parent: Entity) {
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
    for child in &node.children {
        spawn_node(ctx, child, entity);
    }
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
        GeometryDesc::Sphere { radius } => (
            Sphere::new(*radius as f32).mesh().uv(32, 32),
            Transform::IDENTITY,
        ),
        GeometryDesc::Ellipsoid { radii } => (
            Sphere::new(1.0).mesh().uv(32, 32),
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
        GeometryDesc::Unsupported(kind) => {
            info!("visual '{name}': geometry '{kind}' not supported yet");
            return;
        }
    };

    let material = ctx
        .materials
        .add(standard_material(&visual.material, visual.transparency));
    let mesh = ctx.meshes.add(mesh);
    ctx.commands.spawn((
        Mesh3d(mesh),
        MeshMaterial3d(material),
        local,
        ChildOf(parent),
    ));
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
            let scene: Handle<Scene> = ctx
                .asset_server
                .load(GltfAssetLabel::Scene(0).from_asset(path));
            ctx.loads.0.push(scene.clone().untyped());
            ctx.commands
                .spawn((SceneRoot(scene), Transform::IDENTITY, ChildOf(parent)));
        }
        "obj" => {
            let scene: Handle<Scene> = ctx.asset_server.load(path);
            ctx.loads.0.push(scene.clone().untyped());
            ctx.commands
                .spawn((SceneRoot(scene), Transform::IDENTITY, ChildOf(parent)));
        }
        "stl" => {
            let mesh: Handle<Mesh> = ctx.asset_server.load(path);
            ctx.loads.0.push(mesh.clone().untyped());
            let material = ctx
                .materials
                .add(standard_material(&visual.material, visual.transparency));
            ctx.commands.spawn((
                Mesh3d(mesh),
                MeshMaterial3d(material),
                Transform::IDENTITY,
                ChildOf(parent),
            ));
        }
        "dae" => {
            warn!(
                "visual '{name}': Collada is not supported yet ({load_uri}); showing a placeholder"
            );
            let mesh = ctx.meshes.add(Cuboid::new(0.5, 0.5, 0.5));
            let material = ctx.materials.add(StandardMaterial {
                base_color: Color::srgb(1.0, 0.08, 0.58),
                unlit: true,
                ..default()
            });
            ctx.commands.spawn((
                Mesh3d(mesh),
                MeshMaterial3d(material),
                Transform::IDENTITY,
                GzPlaceholder,
                ChildOf(parent),
            ));
        }
        ext => warn!("visual '{name}': unsupported mesh extension '{ext}' ({load_uri})"),
    }
}

pub fn standard_material(mat: &MaterialDesc, transparency: f64) -> StandardMaterial {
    let [r, g, b] = mat.color;
    let alpha = if transparency > 0.0 {
        (1.0 - transparency as f32).clamp(0.0, 1.0)
    } else {
        1.0
    };
    let [er, eg, eb] = mat.emissive;
    let (metallic, perceptual_roughness) = mat
        .pbr
        .as_ref()
        .map(|p| (p.metalness as f32, p.roughness as f32))
        .unwrap_or((0.0, 0.5));
    StandardMaterial {
        base_color: Color::srgba(r, g, b, alpha),
        emissive: LinearRgba::rgb(er, eg, eb),
        metallic,
        perceptual_roughness,
        double_sided: mat.double_sided,
        cull_mode: if mat.double_sided {
            None
        } else {
            Some(Face::Back)
        },
        alpha_mode: if alpha < 1.0 {
            AlphaMode::Blend
        } else {
            AlphaMode::Opaque
        },
        ..default()
    }
}

/// The Bevy light sits on a child entity that looks along the Gazebo
/// direction (Bevy lights shine along local -Z).
fn spawn_light(ctx: &mut SpawnCtx, light: &LightDesc, parent: Entity) {
    let [r, g, b] = light.color;
    let color = Color::srgb(r, g, b);
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
    // Rendering units are provisional (Phase 2 calibrates them against gzweb).
    let intensity = light.intensity as f32;
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
                intensity: intensity * 800_000.0,
                range,
                shadows_enabled: light.cast_shadows,
                ..default()
            });
        }
        LightType::Spot => {
            let outer = light.angle.unwrap_or(0.5) as f32;
            let inner = outer * (1.0 - light.penumbra.unwrap_or(0.0) as f32);
            e.insert(SpotLight {
                color,
                intensity: intensity * 800_000.0,
                range,
                outer_angle: outer,
                inner_angle: inner,
                shadows_enabled: light.cast_shadows,
                ..default()
            });
        }
        LightType::Directional => {
            e.insert(DirectionalLight {
                color,
                illuminance: intensity * 10_000.0,
                shadows_enabled: light.cast_shadows,
                ..default()
            });
        }
    }
}

/// Applies `dynamic_pose/info` poses as local transforms. Entities are found
/// by id first (gzweb uses the first name match, which is ambiguous for
/// repeated names such as `link`), then by name.
pub fn apply_poses(
    mut inbox: ResMut<GzInbox>,
    index: Res<GzIndex>,
    mut transforms: Query<&mut Transform>,
) {
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
        if let Ok(mut t) = transforms.get_mut(entity) {
            let p = Pose::from_msg(Some(&pose));
            let next = pose_transform(&p, [1.0; 3]);
            t.translation = next.translation;
            t.rotation = next.rotation;
        }
    }
}
