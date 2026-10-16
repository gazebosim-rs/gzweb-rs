//! Exports the live ECS scene as a `gz-parity-snapshot/1` (see
//! `.agent/PARITY.md` §3) once it has settled, then exits the app.

use std::path::PathBuf;

use bevy::asset::RecursiveDependencyLoadState;
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use gz_core::scene::NodeKind;
use gz_core::snapshot::{
    Aabb, CameraInfo, GeometryInfo, LightInfo, Maps, MaterialInfo, SceneInfo, SkyInfo, SnapNode, Snapshot,
    Xform, FORMAT,
};
use gz_core::transport::Status;

use crate::camera::{GzCameraTarget, MainCamera};
use crate::connection::{GzInbox, GzStatus};
use crate::spawn::{GzEntity, GzIndex, GzLight, GzLightEmitter, GzLoads, GzPlaceholder, GzVisual, GzWorldRoot};

#[derive(Resource, Clone)]
pub struct SnapshotRequest {
    pub out: PathBuf,
    /// Quiet time required after the last scene-changing message.
    pub settle_secs: f64,
    /// Give up (and write what there is, exiting non-zero) after this long.
    pub timeout_secs: f64,
}

#[allow(clippy::too_many_arguments)]
pub fn snapshot_when_settled(
    req: Res<SnapshotRequest>,
    time: Res<Time<Real>>,
    status: Res<GzStatus>,
    inbox: Res<GzInbox>,
    index: Res<GzIndex>,
    loads: Res<GzLoads>,
    asset_server: Res<AssetServer>,
    scene_roots: Query<(&SceneRoot, Option<&Children>)>,
    world_q: SnapshotQueries,
    mut exit: MessageWriter<AppExit>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    let now = time.elapsed_secs_f64();
    let timed_out = now > req.timeout_secs;

    let loads_done = loads.0.iter().all(|h| {
        matches!(
            asset_server.get_recursive_dependency_load_state(h.id()),
            Some(RecursiveDependencyLoadState::Loaded) | Some(RecursiveDependencyLoadState::Failed(_)) | None
        )
    });
    // A loaded glTF/OBJ scene is instanced a frame later.
    let scenes_instanced = scene_roots.iter().all(|(root, children)| {
        children.is_some_and(|c| !c.is_empty())
            || matches!(
                asset_server.get_recursive_dependency_load_state(root.0.id()),
                Some(RecursiveDependencyLoadState::Failed(_))
            )
    });
    let settled = status.status == Status::Ready
        && index.scene.is_some()
        && loads_done
        && scenes_instanced
        && now - inbox.last_activity_secs >= req.settle_secs;

    if !settled && !timed_out {
        return;
    }
    *done = true;
    if timed_out {
        error!(
            "snapshot: timed out after {:.1}s (status {:?}, loads done: {loads_done}, scenes instanced: {scenes_instanced})",
            req.timeout_secs, status.status
        );
    }

    let snapshot = build(&status, &index, &world_q);
    let json = serde_json::to_string_pretty(&snapshot).expect("snapshot serializes");
    if let Some(dir) = req.out.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match std::fs::write(&req.out, json) {
        Ok(()) => info!("snapshot written to {}", req.out.display()),
        Err(e) => error!("failed to write {}: {e}", req.out.display()),
    }
    exit.write(if timed_out { AppExit::from_code(2) } else { AppExit::Success });
}

type NodeData = (
    &'static GzEntity,
    &'static Transform,
    &'static GlobalTransform,
    &'static Visibility,
    Option<&'static GzVisual>,
    Option<&'static GzLight>,
);
type MeshData = (
    &'static Mesh3d,
    &'static GlobalTransform,
    Option<&'static MeshMaterial3d<StandardMaterial>>,
);

#[derive(bevy::ecs::system::SystemParam)]
pub struct SnapshotQueries<'w, 's> {
    root: Query<'w, 's, &'static GlobalTransform, With<GzWorldRoot>>,
    nodes: Query<'w, 's, NodeData>,
    children: Query<'w, 's, &'static Children>,
    meshes_q: Query<'w, 's, MeshData, Without<GzPlaceholder>>,
    emitters: Query<'w, 's, &'static GlobalTransform, With<GzLightEmitter>>,
    camera: Query<'w, 's, (&'static GlobalTransform, &'static Projection, &'static Camera), With<MainCamera>>,
    target: Res<'w, GzCameraTarget>,
    ambient: Res<'w, GlobalAmbientLight>,
    clear: Res<'w, ClearColor>,
    meshes: Res<'w, Assets<Mesh>>,
    materials: Res<'w, Assets<StandardMaterial>>,
}

fn v3(v: Vec3) -> [f64; 3] {
    [v.x as f64, v.y as f64, v.z as f64]
}

fn rgb(c: Color) -> [f64; 3] {
    let c = c.to_srgba();
    [c.red as f64, c.green as f64, c.blue as f64]
}

fn xform(affine: Mat4) -> Xform {
    let (scale, rot, pos) = affine.to_scale_rotation_translation();
    Xform {
        pos: v3(pos),
        quat: [rot.x as f64, rot.y as f64, rot.z as f64, rot.w as f64],
        scale: v3(scale),
    }
}

fn build(status: &GzStatus, index: &GzIndex, q: &SnapshotQueries) -> Snapshot {
    // Everything is reported in Gazebo axes: undo the root's Z-up → Y-up turn.
    let to_gz = q
        .root
        .single()
        .map(|g| g.to_matrix().inverse())
        .unwrap_or(Mat4::IDENTITY);

    let mut nodes = Vec::new();
    for &entity in &index.order {
        let Ok((gz, local, global, vis, visual, light)) = q.nodes.get(entity) else {
            continue;
        };
        let geometry = visual.map(|v| geometry_info(entity, &v.0.proto_type, to_gz, q));
        let material = visual.and_then(|_| material_info(entity, q));
        let light = light.map(|l| {
            let d = &l.0;
            let direction_world = d.direction.and_then(|_| {
                q.children
                    .get(entity)
                    .ok()?
                    .iter()
                    .find_map(|c| q.emitters.get(c).ok())
                    .map(|g| v3(to_gz.transform_vector3(g.forward().as_vec3()).normalize()))
            });
            LightInfo {
                ty: d.ty.as_str().to_string(),
                color: d.color.map(|c| c as f64),
                intensity: d.intensity,
                range: (d.ty != gz_core::scene::LightType::Directional).then_some(d.range),
                angle: d.angle,
                penumbra: d.penumbra,
                decay: d.decay,
                cast_shadow: d.cast_shadows,
                direction_world,
            }
        });
        nodes.push(SnapNode {
            path: gz.path.clone(),
            kind: gz.kind.as_str().to_string(),
            local: Some(Xform {
                pos: v3(local.translation),
                quat: {
                    let r = local.rotation;
                    [r.x as f64, r.y as f64, r.z as f64, r.w as f64]
                },
                scale: v3(local.scale),
            }),
            world: Some(xform(to_gz * global.to_matrix())),
            visible: *vis != Visibility::Hidden,
            geometry,
            material,
            light,
        });
        debug_assert!(gz.kind != NodeKind::Visual || visual.is_some());
    }

    let camera = q.camera.single().ok().map(|(g, proj, _)| {
        let (fov, near, far) = match proj {
            Projection::Perspective(p) => (p.fov.to_degrees() as f64, p.near as f64, p.far as f64),
            _ => (0.0, 0.0, 0.0),
        };
        CameraInfo {
            position: v3(to_gz.transform_point3(g.translation())),
            target: q.target.0.map(|c| c as f64),
            fov_deg: fov,
            near,
            far,
        }
    });
    let clear_color = match q.camera.single().ok().map(|(_, _, c)| c.clear_color) {
        Some(ClearColorConfig::Custom(c)) => rgb(c),
        _ => rgb(q.clear.0),
    };

    Snapshot {
        format: FORMAT.to_string(),
        source: "gzweb-rs".to_string(),
        world: status.world.clone().unwrap_or_default(),
        scene: SceneInfo {
            ambient: Some(rgb(q.ambient.color)),
            clear_color: Some(clear_color),
            camera,
            grid_visible: Some(false),
            // The sky is not rendered yet.
            sky: None::<SkyInfo>,
        },
        nodes,
    }
}

/// Every renderable mesh under `entity`, depth-first.
fn descendant_meshes(entity: Entity, q: &SnapshotQueries, out: &mut Vec<Entity>) {
    if let Ok(children) = q.children.get(entity) {
        for c in children.iter() {
            if q.meshes_q.contains(c) {
                out.push(c);
            }
            // Nested Gazebo nodes are not part of this visual.
            if !q.nodes.contains(c) {
                descendant_meshes(c, q, out);
            }
        }
    }
}

fn geometry_info(entity: Entity, proto_type: &str, to_gz: Mat4, q: &SnapshotQueries) -> GeometryInfo {
    let mut mesh_entities = Vec::new();
    descendant_meshes(entity, q, &mut mesh_entities);
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    let mut vertex_count = 0u64;
    let mut mesh_count = 0u32;
    for e in mesh_entities {
        let Ok((mesh3d, global, _)) = q.meshes_q.get(e) else { continue };
        let Some(mesh) = q.meshes.get(&mesh3d.0) else { continue };
        let Some(VertexAttributeValues::Float32x3(positions)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else {
            continue;
        };
        mesh_count += 1;
        vertex_count += positions.len() as u64;
        let m = to_gz * global.to_matrix();
        for p in positions {
            let w = m.transform_point3(Vec3::from_array(*p));
            min = min.min(w);
            max = max.max(w);
        }
    }
    GeometryInfo {
        ty: proto_type.to_string(),
        aabb_world: (vertex_count > 0).then(|| Aabb { min: v3(min), max: v3(max) }),
        mesh_count,
        vertex_count,
    }
}

fn material_info(entity: Entity, q: &SnapshotQueries) -> Option<MaterialInfo> {
    let mut mesh_entities = Vec::new();
    descendant_meshes(entity, q, &mut mesh_entities);
    let handle = mesh_entities
        .iter()
        .find_map(|e| q.meshes_q.get(*e).ok().and_then(|(_, _, m)| m))?;
    let m = q.materials.get(&handle.0)?;
    let base = m.base_color.to_srgba();
    let path = |h: &Option<Handle<Image>>| h.as_ref().and_then(|h| h.path()).map(|p| p.to_string());
    Some(MaterialInfo {
        color: [base.red as f64, base.green as f64, base.blue as f64],
        opacity: base.alpha as f64,
        transparent: matches!(m.alpha_mode, AlphaMode::Blend),
        emissive: [m.emissive.red as f64, m.emissive.green as f64, m.emissive.blue as f64],
        metalness: Some(m.metallic as f64),
        roughness: Some(m.perceptual_roughness as f64),
        maps: Maps {
            albedo: path(&m.base_color_texture),
            normal: path(&m.normal_map_texture),
        },
    })
}
