//! Exports the live ECS scene as a `gz-parity-snapshot/1` (see
//! `.agent/PARITY.md` §3) and/or a screenshot once it has settled, then exits
//! the app.

use std::path::PathBuf;

use bevy::asset::RecursiveDependencyLoadState;
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use gz_core::scene::NodeKind;
use gz_core::snapshot::{
    Aabb, CameraInfo, GeometryInfo, LightInfo, Maps, MaterialInfo, SceneInfo, SkyInfo, SnapNode,
    Snapshot, Xform, FORMAT,
};
use gz_core::transport::Status;

use crate::camera::{GzCameraTarget, MainCamera};
use crate::connection::{GzInbox, GzStatus};
use crate::heightmap::GzHeightmap;
use crate::render::{GzEnvironment, GzGrid};
use crate::spawn::{
    GzEntity, GzGltf, GzIndex, GzLight, GzLightEmitter, GzLoads, GzVisual, GzWorldRoot,
};
use crate::submesh::GzSubmesh;

#[derive(Resource, Clone, Debug)]
pub struct SnapshotRequest {
    /// Where to write the snapshot JSON.
    pub out: Option<PathBuf>,
    /// Where to save a screenshot of the window (needs a window and GPU).
    pub screenshot: Option<PathBuf>,
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
    scene_roots: Query<(&WorldAssetRoot, Option<&Children>, Option<&GzSubmesh>)>,
    gltfs: Query<&GzGltf, Without<WorldAssetRoot>>,
    terrains: Query<&GzHeightmap>,
    world_q: SnapshotQueries,
    mut exit: MessageWriter<AppExit>,
    mut commands: Commands,
    mut stage: Local<Stage>,
) {
    let now = time.elapsed_secs_f64();
    let timed_out = now > req.timeout_secs;
    match *stage {
        Stage::Done => return,
        // The capture observer exits; this only catches a capture that
        // never arrives.
        Stage::Capturing => {
            if timed_out {
                error!(
                    "screenshot: no frame captured within {:.1}s",
                    req.timeout_secs
                );
                exit.write(AppExit::from_code(2));
                *stage = Stage::Done;
            }
            return;
        }
        Stage::Waiting => {}
    }

    let loads_done = loads.0.iter().all(|h| {
        matches!(
            asset_server.get_recursive_dependency_load_state(h.id()),
            Some(RecursiveDependencyLoadState::Loaded)
                | Some(RecursiveDependencyLoadState::Failed(_))
                | None
        )
    });
    // A loaded glTF/OBJ/COLLADA scene is instanced a frame later (a glTF
    // gets its `WorldAssetRoot` once loaded), and cut down to its submesh
    // (possibly to nothing) when it is.
    let failed = |id: bevy::asset::UntypedAssetId| {
        matches!(
            asset_server.get_recursive_dependency_load_state(id),
            Some(RecursiveDependencyLoadState::Failed(_))
        )
    };
    let scenes_instanced = scene_roots.iter().all(|(root, children, submesh)| {
        submesh.map_or_else(|| children.is_some_and(|c| !c.is_empty()), |s| s.applied)
            || failed(root.0.id().untyped())
    }) && gltfs.iter().all(|g| failed(g.0.id().untyped()));
    let settled = status.status == Status::Ready
        && index.scene.is_some()
        && loads_done
        && scenes_instanced
        && terrains.iter().all(|t| t.built)
        && now - inbox.last_activity_secs >= req.settle_secs;

    if !settled && !timed_out {
        return;
    }
    *stage = Stage::Done;
    if timed_out {
        error!(
            "snapshot: timed out after {:.1}s (status {:?}, loads done: {loads_done}, scenes instanced: {scenes_instanced})",
            req.timeout_secs, status.status
        );
    }

    if let Some(out) = &req.out {
        let snapshot = build(&status, &index, &world_q);
        let json = serde_json::to_string_pretty(&snapshot).expect("snapshot serializes");
        if let Some(dir) = out.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match std::fs::write(out, json) {
            Ok(()) => info!("snapshot written to {}", out.display()),
            Err(e) => error!("failed to write {}: {e}", out.display()),
        }
    }
    match (&req.screenshot, timed_out) {
        // `save_screenshot` exits once the frame is captured and saved.
        (Some(path), false) => {
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_screenshot(path.clone()));
            *stage = Stage::Capturing;
        }
        _ => {
            exit.write(if timed_out {
                AppExit::from_code(2)
            } else {
                AppExit::Success
            });
        }
    }
}

/// Where `snapshot_when_settled` is.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    #[default]
    Waiting,
    /// A screenshot has been requested.
    Capturing,
    Done,
}

/// Saves the captured frame as PNG to `path` (creating its directory), then
/// exits: 0 when saved, 1 when not.
pub fn save_screenshot(
    path: PathBuf,
) -> impl FnMut(On<ScreenshotCaptured>, MessageWriter<AppExit>) {
    move |captured, mut exit| {
        let saved = write_png(&captured.image, &path);
        match &saved {
            Ok(()) => info!("screenshot saved to {}", path.display()),
            Err(e) => error!("screenshot not saved to {}: {e}", path.display()),
        }
        exit.write(if saved.is_ok() {
            AppExit::Success
        } else {
            AppExit::from_code(1)
        });
    }
}

fn write_png(image: &Image, path: &std::path::Path) -> Result<(), String> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let pixels = image
        .clone()
        .try_into_dynamic()
        .map_err(|e| e.to_string())?;
    pixels.to_rgb8().save(path).map_err(|e| e.to_string())
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
    meshes_q: Query<'w, 's, MeshData>,
    terrains: Query<'w, 's, &'static GzHeightmap>,
    emitters: Query<'w, 's, &'static GlobalTransform, With<GzLightEmitter>>,
    camera: Query<
        'w,
        's,
        (
            &'static GlobalTransform,
            &'static Projection,
            &'static Camera,
        ),
        With<MainCamera>,
    >,
    target: Res<'w, GzCameraTarget>,
    ambient: Res<'w, GlobalAmbientLight>,
    clear: Res<'w, ClearColor>,
    meshes: Res<'w, Assets<Mesh>>,
    materials: Res<'w, Assets<StandardMaterial>>,
    grid: Res<'w, GzGrid>,
    environment: Res<'w, GzEnvironment>,
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
            grid_visible: Some(q.grid.visible),
            sky: q.environment.sky.as_ref().map(|cubemap| SkyInfo {
                cubemap: cubemap.clone(),
            }),
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

fn geometry_info(
    entity: Entity,
    proto_type: &str,
    to_gz: Mat4,
    q: &SnapshotQueries,
) -> GeometryInfo {
    let mut mesh_entities = Vec::new();
    descendant_meshes(entity, q, &mut mesh_entities);
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    let mut vertex_count = 0u64;
    let mut mesh_count = 0u32;
    let mut corners = Vec::new();
    for (e, (mesh3d, global, _)) in mesh_entities
        .iter()
        .filter_map(|e| q.meshes_q.get(*e).ok().map(|m| (*e, m)))
    {
        let Some(mesh) = q.meshes.get(&mesh3d.0) else {
            continue;
        };
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
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
        if let Ok(GzHeightmap {
            grid_size: (w, h), ..
        }) = q.terrains.get(e)
        {
            let (w, h) = (*w, *h);
            if w * h == positions.len() && w * h > 0 {
                for i in [0, w - 1, (h - 1) * w, h * w - 1] {
                    corners.push(v3(m.transform_point3(Vec3::from_array(positions[i]))));
                }
            }
        }
    }
    GeometryInfo {
        ty: proto_type.to_string(),
        aabb_world: (vertex_count > 0).then(|| Aabb {
            min: v3(min),
            max: v3(max),
        }),
        mesh_count,
        vertex_count,
        corners,
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
    let emissive = crate::render::units::emission_color(m.emissive);
    let path = |h: &Option<Handle<Image>>| h.as_ref().and_then(|h| h.path()).map(|p| p.to_string());
    Some(MaterialInfo {
        color: [base.red as f64, base.green as f64, base.blue as f64],
        opacity: base.alpha as f64,
        transparent: matches!(m.alpha_mode, AlphaMode::Blend),
        emissive: [
            emissive.red as f64,
            emissive.green as f64,
            emissive.blue as f64,
        ],
        metalness: Some(m.metallic as f64),
        roughness: Some(m.perceptual_roughness as f64),
        maps: Maps {
            albedo: path(&m.base_color_texture),
            normal: path(&m.normal_map_texture),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::scene_app;
    use crate::GzConfig;
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::PrimitiveTopology;
    use gz_core::scene::{GeometryDesc, LightDesc, LightType, MaterialDesc, VisualDesc};

    fn out_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "gz-bevy-snapshot-{}-{name}.json",
            std::process::id()
        ))
    }

    fn app_with(req: SnapshotRequest) -> App {
        let mut app = scene_app(GzConfig::default());
        app.insert_resource(req)
            .add_systems(Last, snapshot_when_settled);
        app
    }

    fn exit_code(app: &mut App) -> Option<AppExit> {
        app.world_mut()
            .resource_mut::<Messages<AppExit>>()
            .drain()
            .last()
    }

    fn gz(name: &str, kind: NodeKind) -> GzEntity {
        GzEntity {
            name: name.into(),
            id: 0,
            kind,
            path: name.into(),
        }
    }

    fn visual() -> GzVisual {
        GzVisual(VisualDesc {
            proto_type: "box".into(),
            geometry: GeometryDesc::Box { size: [1.0; 3] },
            material: MaterialDesc {
                color: [1.0; 3],
                diffuse_alpha: 1.0,
                emissive: [0.0; 3],
                pbr: None,
                double_sided: false,
                present: true,
            },
            cast_shadows: false,
            transparency: 0.0,
        })
    }

    #[test]
    fn writes_once_settled_and_skips_what_it_cannot_measure() {
        let out = out_path("settled");
        let mut app = app_with(SnapshotRequest {
            out: Some(out.clone()),
            screenshot: None,
            settle_secs: 0.0,
            timeout_secs: 1e9,
        });
        app.update();
        let world = app.world_mut();

        // A hidden visual whose meshes are: not loaded, without positions,
        // and real; plus a nested Gazebo node that is not part of it.
        let missing = Mesh3d(Handle::default());
        let empty = world.resource_mut::<Assets<Mesh>>().add(Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        ));
        let cube = world.resource_mut::<Assets<Mesh>>().add(Cuboid::default());
        let vis = world
            .spawn((
                gz("v", NodeKind::Visual),
                Transform::default(),
                Visibility::Hidden,
                visual(),
                children![
                    missing,
                    Mesh3d(empty),
                    Mesh3d(cube),
                    (
                        gz("nested", NodeKind::Link),
                        Transform::default(),
                        Visibility::default()
                    ),
                ],
            ))
            .id();
        // A light whose direction has no emitter to measure.
        let lamp = world
            .spawn((
                gz("lamp", NodeKind::Light),
                Transform::default(),
                Visibility::default(),
                GzLight(LightDesc {
                    ty: LightType::Directional,
                    color: [1.0; 3],
                    intensity: 1.0,
                    range: 0.0,
                    angle: None,
                    inner_angle: None,
                    penumbra: None,
                    decay: None,
                    cast_shadows: false,
                    direction: Some([0.0, 0.0, -1.0]),
                    msg_intensity: 1.0,
                }),
            ))
            .id();
        let gone = world.spawn_empty().id();
        world.despawn(gone);
        // Default clear colour and a non-perspective camera.
        let mut cams = world.query_filtered::<(&mut Camera, &mut Projection), With<MainCamera>>();
        let (mut cam, mut proj) = cams.single_mut(world).unwrap();
        cam.clear_color = ClearColorConfig::Default;
        *proj = Projection::Orthographic(OrthographicProjection::default_3d());

        let mut index = world.resource_mut::<GzIndex>();
        index.order = vec![vis, gone, lamp];
        index.scene = Some(gz_core::scene::SceneDesc::from_msg(
            &Default::default(),
            Default::default(),
        ));
        app.update();
        assert_eq!(exit_code(&mut app), Some(AppExit::Success));

        let snap: Snapshot = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
        assert_eq!(snap.nodes.len(), 2);
        let v = &snap.nodes[0];
        assert!(!v.visible);
        let g = v.geometry.as_ref().unwrap();
        assert_eq!((g.mesh_count, g.vertex_count), (1, 24));
        assert_eq!(snap.nodes[1].light.as_ref().unwrap().direction_world, None);
        assert_eq!(snap.nodes[1].light.as_ref().unwrap().range, None);
        let camera = snap.scene.camera.unwrap();
        assert_eq!((camera.fov_deg, camera.near, camera.far), (0.0, 0.0, 0.0));
        assert_eq!(
            snap.scene.clear_color,
            Some(rgb(crate::camera::CLEAR_COLOR))
        );

        // Done: later frames write nothing more.
        std::fs::remove_file(&out).unwrap();
        app.update();
        assert!(!out.exists());
        assert_eq!(exit_code(&mut app), None);
    }

    #[test]
    fn a_screenshot_waits_for_the_capture() {
        let dir = std::env::temp_dir().join(format!("gz-bevy-shot-{}", std::process::id()));
        let png = dir.join("nested/shot.png");
        let mut app = app_with(SnapshotRequest {
            out: None,
            screenshot: Some(png.clone()),
            settle_secs: 0.0,
            timeout_secs: 1e9,
        });
        app.update();
        app.world_mut().resource_mut::<GzIndex>().scene = Some(
            gz_core::scene::SceneDesc::from_msg(&Default::default(), Default::default()),
        );
        app.update();
        // No renderer here: the request is queued and the app keeps running.
        assert_eq!(exit_code(&mut app), None);
        let world = app.world_mut();
        let shot = world
            .query_filtered::<Entity, With<Screenshot>>()
            .single(world)
            .unwrap();
        let frame = Image::new_fill(
            bevy::render::render_resource::Extent3d {
                width: 2,
                height: 2,
                depth_or_array_layers: 1,
            },
            bevy::render::render_resource::TextureDimension::D2,
            &[255, 0, 0, 255],
            bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        world.trigger(ScreenshotCaptured {
            entity: shot,
            image: frame,
        });
        app.update();
        assert_eq!(exit_code(&mut app), Some(AppExit::Success));
        // The directory was created and a real PNG written.
        assert!(std::fs::read(&png).unwrap().starts_with(b"\x89PNG"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unsaved_screenshot_exits_with_an_error() {
        let mut world = World::new();
        world.init_resource::<Messages<AppExit>>();
        // An image that can't be converted, to a path that can't be created.
        let shot = world
            .spawn_empty()
            .observe(save_screenshot("/proc/no/such/shot.png".into()))
            .id();
        world.trigger(ScreenshotCaptured {
            entity: shot,
            image: Image::default(),
        });
        let exits: Vec<_> = world
            .resource::<Messages<AppExit>>()
            .iter_current_update_messages()
            .cloned()
            .collect();
        assert_eq!(exits, [AppExit::from_code(1)]);
    }

    #[test]
    fn a_capture_that_never_arrives_times_out() {
        let mut app = app_with(SnapshotRequest {
            out: None,
            screenshot: Some(out_path("never.png")),
            settle_secs: 0.0,
            timeout_secs: 0.05,
        });
        app.update();
        app.world_mut().resource_mut::<GzIndex>().scene = Some(
            gz_core::scene::SceneDesc::from_msg(&Default::default(), Default::default()),
        );
        app.update();
        assert_eq!(exit_code(&mut app), None);
        std::thread::sleep(std::time::Duration::from_millis(100));
        app.update();
        assert_eq!(exit_code(&mut app), Some(AppExit::from_code(2)));
        // Done: nothing more happens.
        app.update();
        assert_eq!(exit_code(&mut app), None);
    }

    #[test]
    fn a_timeout_still_writes_and_exits_with_an_error() {
        let out = out_path("timeout");
        let mut app = app_with(SnapshotRequest {
            out: Some(out.clone()),
            screenshot: Some(out_path("timeout.png")),
            settle_secs: 1e9,
            timeout_secs: 0.0,
        });
        app.update();
        app.update();
        assert_eq!(exit_code(&mut app), Some(AppExit::from_code(2)));
        assert!(out.exists());
        std::fs::remove_file(out).unwrap();
    }

    #[test]
    fn an_unwritable_path_is_reported_not_fatal() {
        // The output path is an existing directory.
        let out = std::env::temp_dir();
        let mut app = app_with(SnapshotRequest {
            out: Some(out),
            screenshot: None,
            settle_secs: 1e9,
            timeout_secs: 0.0,
        });
        app.update();
        app.update();
        assert_eq!(exit_code(&mut app), Some(AppExit::from_code(2)));
    }
}
