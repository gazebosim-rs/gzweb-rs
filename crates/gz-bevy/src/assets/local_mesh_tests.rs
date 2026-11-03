// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
use super::*;
use bevy::tasks::block_on;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};

const GLTF: &str = r#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"mesh":0,"translation":[0.25,0.5,0.75]}],"buffers":[{"uri":"nested/triangle.bin","byteLength":36}],"bufferViews":[{"buffer":0,"byteLength":36,"target":34962}],"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}],"meshes":[{"primitives":[{"attributes":{"POSITION":0},"material":0}]}],"materials":[{"name":"embedded_blue","pbrMetallicRoughness":{"baseColorFactor":[0.1,0.4,0.8,1],"metallicFactor":0,"roughnessFactor":1}}]}"#;

struct LocalServer {
    authority: String,
    stop: Arc<AtomicBool>,
    requests: Arc<Mutex<Vec<String>>>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl LocalServer {
    fn new() -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let authority = listener.local_addr().unwrap().to_string();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stopped = stop.clone();
        let redirect = format!("http://{authority}/assets/8/forbidden.gltf");
        let seen = requests.clone();
        let thread = std::thread::spawn(move || {
            let triangle: Vec<u8> = [0_f32, 0., 0., 1., 0., 0., 0., 1., 0.]
                .into_iter()
                .flat_map(f32::to_le_bytes)
                .collect();
            while !stopped.load(Ordering::Relaxed) {
                let Ok((mut stream, _)) = listener.accept() else {
                    std::thread::sleep(Duration::from_millis(2));
                    continue;
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                let mut request = [0; 4096];
                let n = stream.read(&mut request).unwrap_or(0);
                let text = String::from_utf8_lossy(&request[..n]);
                let path = text.split_whitespace().nth(1).unwrap_or("").to_string();
                seen.lock().unwrap().push(path.clone());
                if path == "/assets/7/redirect.gltf" {
                    let _ = write!(stream,"HTTP/1.1 302 Found\r\nLocation: {redirect}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    continue;
                }
                let bytes: Option<&[u8]> = match path.as_str() {
                    "/assets/7/models/arrow.gltf" => Some(GLTF.as_bytes()),
                    "/assets/7/models/nested/triangle.bin" => Some(&triangle),
                    "/assets/7/invalid.gltf" => Some(b"invalid glTF bytes"),
                    "/assets/7/no-scene.gltf" => Some(br#"{"asset":{"version":"2.0"}}"#),
                    _ => None,
                };
                let status = if bytes.is_some() {
                    "200 OK"
                } else {
                    "404 Not Found"
                };
                let body = bytes.unwrap_or_default();
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(body);
            }
        });
        Self {
            authority,
            stop,
            requests,
            thread: Some(thread),
        }
    }
    fn url(&self, relative: &str) -> String {
        format!("http://{}/assets/7/{relative}", self.authority)
    }
    fn reader_path(&self, relative: &str) -> String {
        format!("{}/7/assets/7/{relative}", self.authority)
    }
}
impl Drop for LocalServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

#[test]
fn m2_reader_fetches_declared_relative_resources_and_fails_without_rpc_fallback() {
    let server = LocalServer::new();
    let reader = LocalMeshReader;
    block_on(async {
        let document_path = server.reader_path("models/arrow.gltf");
        let mut document = reader.read(Path::new(&document_path)).await.unwrap();
        let mut bytes = Vec::new();
        document.read_to_end(&mut bytes).await.unwrap();
        assert_eq!(bytes, GLTF.as_bytes());
        let binary_path = server.reader_path("models/nested/triangle.bin");
        let mut binary = reader.read(Path::new(&binary_path)).await.unwrap();
        let mut bytes = Vec::new();
        binary.read_to_end(&mut bytes).await.unwrap();
        assert_eq!(bytes.len(), 36);
        let Err(missing) = reader
            .read(Path::new(&server.reader_path("missing.gltf")))
            .await
        else {
            panic!("missing resource accepted");
        };
        assert!(missing.to_string().contains("404"));
        assert!(missing.to_string().contains("missing.gltf"));
        let Err(invalid) = reader
            .read(Path::new(&server.reader_path("invalid.gltf")))
            .await
        else {
            panic!("invalid glTF accepted");
        };
        assert!(invalid.to_string().contains("Invalid glTF"));
        assert!(reader
            .read(Path::new("remote/7/assets/7/a.gltf"))
            .await
            .is_err());
        assert!(reader.read_meta(Path::new("x")).await.is_err());
        assert!(reader.read_directory(Path::new("x")).await.is_err());
        assert!(!reader.is_directory(Path::new("x")).await.unwrap());
    });
    assert_eq!(server.requests.lock().unwrap().len(), 4);
    assert!(validate_local_gltf(&server.url("models/arrow.gltf"), GLTF.as_bytes()).is_ok());
    for uri in [
        "../../outside.bin",
        "https://remote/a.bin",
        "m2mesh://other/a.bin",
        "/assets/8/a.bin",
    ] {
        let document = GLTF.replace("nested/triangle.bin", uri);
        assert!(
            validate_local_gltf(&server.url("models/arrow.gltf"), document.as_bytes()).is_err()
        );
    }
    let image = GLTF.replace(
        "\"scene\":0",
        "\"images\":[{\"uri\":\"https://remote/a.png\"}],\"scene\":0",
    );
    assert!(validate_local_gltf(&server.url("models/arrow.gltf"), image.as_bytes()).is_err());
    assert!(validate_local_gltf(&server.url("a.bin"), b"binary bytes").is_ok());
}

fn imported_mesh(world: &mut World, visual: Entity) -> Option<(Entity, AssetId<Mesh>)> {
    let mut meshes = world.query::<(Entity, &Mesh3d)>();
    for (entity, mesh) in meshes.iter(world) {
        let mut ancestor = entity;
        while let Some(parent) = world.get::<ChildOf>(ancestor) {
            ancestor = parent.parent();
            if ancestor == visual {
                return Some((entity, mesh.0.id()));
            }
        }
    }
    None
}

#[test]
fn m2_real_gltf_loader_preserves_material_and_visual_pose_resource_ownership() {
    use crate::{
        connection::GzInbox,
        material::GzMeshMaterial,
        spawn::{GzGltf, GzIndex, GzLoads, GzWorldRoot},
    };
    use gz_core::primitive::{parse, ServerMessage};
    let server = LocalServer::new();
    let mut app = crate::test_util::scene_app(crate::GzConfig {
        backend: crate::BackendKind::Primitive,
        ..Default::default()
    });
    app.add_plugins((
        bevy::gltf::GltfPlugin::default(),
        bevy::world_serialization::WorldSerializationPlugin,
    ));
    app.finish();
    app.cleanup();
    let message = serde_json::json!({"version":2,"kind":"scene","world":"default","world_id":1,"epoch":0,"seconds":0,"nanoseconds":0,"sequence":1,"paused":true,
        "visuals":[{"id":2,"name":"visual","translation":[1,2,3],"rotation":[0,0,std::f64::consts::FRAC_1_SQRT_2,std::f64::consts::FRAC_1_SQRT_2],"shape":{"kind":"mesh","uri":server.url("models/arrow.gltf"),"scale":[2,3,5]},"color":[1,0,0,1]}]});
    let ServerMessage::Scene(scene) = parse(&message.to_string()).unwrap() else {
        unreachable!()
    };
    app.world_mut().resource_mut::<GzInbox>().prepared_scene = Some(scene.description().unwrap());
    let asset_server = app.world().resource::<AssetServer>().clone();
    crate::test_util::wait_until(|| {
        app.update();
        let loads = &app.world().resource::<GzLoads>().0;
        let loaded = !loads.is_empty()
            && loads
                .iter()
                .all(|h| asset_server.is_loaded_with_dependencies(h.id()));
        let visual = app.world().resource::<GzIndex>().by_id.get(&2).copied();
        loaded && visual.is_some_and(|v| imported_mesh(app.world_mut(), v).is_some())
    });
    app.update();
    let visual = app.world().resource::<GzIndex>().by_id[&2];
    let transform = app.world().get::<Transform>(visual).unwrap();
    assert_eq!(transform.scale, Vec3::new(2., 5., 3.));
    let expected = Vec3::new(3.25, 5.5, -2.5); // world root * world Rz90 * ScaleZ * Rx90 * local glTF translation
    let (imported, mesh_id) = imported_mesh(app.world_mut(), visual).unwrap();
    let actual = app
        .world()
        .get::<GlobalTransform>(imported)
        .unwrap()
        .transform_point(Vec3::ZERO);
    assert!((actual - expected).length() < 1e-5, "{actual:?}");
    let vertex = app
        .world()
        .get::<GlobalTransform>(imported)
        .unwrap()
        .transform_point(Vec3::X);
    assert!(
        (vertex - Vec3::new(3.25, 5.5, -4.5)).length() < 1e-5,
        "{vertex:?}"
    );
    assert_eq!(
        app.world()
            .get::<bevy::gltf::GltfMaterialName>(imported)
            .unwrap()
            .0,
        "embedded_blue"
    );
    let mut pending = app
        .world_mut()
        .query::<(Entity, &GzGltf, &GzMeshMaterial)>();
    let (holder, handle, rule) = pending.iter(app.world()).next().unwrap();
    let handle = handle.0.clone();
    assert!(
        rule.replace.is_none(),
        "wire red must not replace embedded blue material"
    );
    let gltfs = app.world().resource::<Assets<Gltf>>();
    let gltf = gltfs.get(&handle).unwrap();
    let material = app
        .world()
        .resource::<Assets<bevy::gltf::GltfMaterial>>()
        .get(&gltf.materials[0])
        .unwrap();
    assert_eq!(
        material.base_color.to_linear(),
        LinearRgba::new(0.1, 0.4, 0.8, 1.)
    );
    let mesh_asset = app
        .world()
        .resource::<Assets<bevy::gltf::GltfMesh>>()
        .get(&gltf.meshes[0])
        .unwrap();
    assert_eq!(mesh_asset.primitives[0].mesh.id(), mesh_id);
    assert_eq!(
        mesh_asset.primitives[0].material.as_ref().unwrap().id(),
        gltf.materials[0].id()
    );
    assert_eq!(
        app.world()
            .resource::<Assets<Mesh>>()
            .get(&mesh_asset.primitives[0].mesh)
            .unwrap()
            .count_vertices(),
        3
    );
    let counts = (
        app.world().resource::<Assets<Mesh>>().len(),
        app.world().resource::<GzLoads>().0.len(),
    );
    let mut moved = scene.clone();
    moved.visuals[0].translation = [4., 5., 6.];
    moved.visuals[0].rotation = [0., 0., 0., 1.];
    let q = moved.visuals[0].render_rotation();
    app.world_mut()
        .resource_mut::<GzInbox>()
        .poses
        .push(gz_core::msgs::Pose {
            id: 2,
            position: Some(gz_core::msgs::Vector3d {
                x: 4.,
                y: 5.,
                z: 6.,
                ..Default::default()
            }),
            orientation: Some(gz_core::msgs::Quaternion {
                x: q[0],
                y: q[1],
                z: q[2],
                w: q[3],
                ..Default::default()
            }),
            ..Default::default()
        });
    app.update();
    assert_eq!(app.world().resource::<GzIndex>().by_id[&2], visual);
    assert_eq!(
        app.world().get::<GzGltf>(holder).unwrap().0.id(),
        handle.id()
    );
    assert_eq!(
        counts,
        (
            app.world().resource::<Assets<Mesh>>().len(),
            app.world().resource::<GzLoads>().0.len()
        )
    );
    assert_eq!(
        imported_mesh(app.world_mut(), visual),
        Some((imported, mesh_id))
    );
    let actual = app
        .world()
        .get::<GlobalTransform>(imported)
        .unwrap()
        .transform_point(Vec3::ZERO);
    assert!(
        (actual - Vec3::new(4.5, 8.5, -2.75)).length() < 1e-5,
        "{actual:?}"
    );
    let vertex = app
        .world()
        .get::<GlobalTransform>(imported)
        .unwrap()
        .transform_point(Vec3::X);
    assert!(
        (vertex - Vec3::new(6.5, 8.5, -2.75)).length() < 1e-5,
        "{vertex:?}"
    );
    let mut roots = app
        .world_mut()
        .query_filtered::<Entity, With<GzWorldRoot>>();
    assert_eq!(roots.iter(app.world()).count(), 1);
    assert_eq!(
        server
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|p| p.ends_with("triangle.bin"))
            .count(),
        1
    );
    let mut empty = scene.description().unwrap();
    empty.roots.clear();
    app.world_mut().resource_mut::<GzInbox>().prepared_scene = Some(empty);
    app.update();
    assert!(app.world().get_entity(visual).is_err());
    assert!(app.world().get_entity(holder).is_err());
    assert!(app.world().get_entity(imported).is_err());
    assert!(app.world().resource::<GzLoads>().0.is_empty());
}

#[test]
fn missing_invalid_and_no_scene_meshes_surface_once_in_the_existing_message_ui() {
    use crate::{
        connection::GzInbox,
        messages::{self, MessageLevel, SystemMessages},
        spawn::{GzIndex, LocalMesh},
    };
    use bevy::log::tracing_subscriber::prelude::*;
    let server = LocalServer::new();
    let mut app = crate::test_util::scene_app(crate::GzConfig {
        backend: crate::BackendKind::Primitive,
        ..Default::default()
    });
    app.add_plugins(bevy::gltf::GltfPlugin::default())
        .init_resource::<SystemMessages>()
        .add_systems(Update, messages::drain_log);
    // with_default is thread-local. Keep only this test's Update/reporting
    // systems in its dispatcher scope; production uses Bevy's global logger.
    app.edit_schedule(Update, |schedule| {
        schedule.set_executor(bevy::ecs::schedule::SingleThreadedExecutor::new());
    });
    let layer = messages::log_layer(&mut app).unwrap();
    let subscriber = bevy::log::tracing_subscriber::registry().with(layer);
    app.finish();
    app.cleanup();
    let visuals:Vec<_>=["missing.gltf","invalid.gltf","no-scene.gltf"].into_iter().enumerate().map(|(i,file)|serde_json::json!({"id":i+2,"name":file,"translation":[0,0,0],"rotation":[0,0,0,1],"shape":{"kind":"mesh","uri":server.url(file),"scale":[1,1,1]},"color":[1,1,1,1]})).collect();
    let input = serde_json::json!({"version":2,"kind":"scene","world":"default","world_id":1,"epoch":0,"seconds":0,"nanoseconds":0,"sequence":1,"paused":true,"visuals":visuals});
    let gz_core::primitive::ServerMessage::Scene(scene) =
        gz_core::primitive::parse(&input.to_string()).unwrap()
    else {
        unreachable!()
    };
    app.world_mut().resource_mut::<GzInbox>().prepared_scene = Some(scene.description().unwrap());
    bevy::log::tracing::subscriber::with_default(subscriber, || {
        crate::test_util::wait_until(|| {
            app.update();
            app.world().resource::<SystemMessages>().errors == 3
        });
        for _ in 0..3 {
            app.update();
        }
    });
    let messages = app.world().resource::<SystemMessages>();
    assert_eq!(messages.errors, 3);
    for file in ["missing.gltf", "invalid.gltf", "no-scene.gltf"] {
        assert_eq!(
            messages
                .list
                .iter()
                .filter(|m| m.level == MessageLevel::Error && m.text.contains(file))
                .count(),
            1,
            "{file}"
        );
    }
    assert!(messages
        .list
        .iter()
        .any(|m| m.text.contains("has no scene")));
    assert_eq!(app.world().resource::<GzIndex>().by_id.len(), 3);
    let mut markers = app.world_mut().query::<&LocalMesh>();
    assert_eq!(markers.iter(app.world()).count(), 0);
    assert_eq!(
        server.requests.lock().unwrap().len(),
        3,
        "no repeated fetch or fallback request"
    );
}

#[test]
fn m2_redirects_never_fetch_the_target_and_versions_must_be_compatible_2_0() {
    let server = LocalServer::new();
    let error = block_on(async {
        match LocalMeshReader
            .read(Path::new(&server.reader_path("redirect.gltf")))
            .await
        {
            Err(e) => e,
            Ok(_) => panic!("redirect accepted"),
        }
    });
    assert!(error.to_string().contains("302"));
    assert_eq!(
        *server.requests.lock().unwrap(),
        ["/assets/7/redirect.gltf"]
    );
    for (version, minimum) in [
        ("1.0", None),
        ("2.1", None),
        ("3.0", None),
        ("garbage", None),
        ("2.0", Some("2.1")),
        ("2.0", Some("1.0")),
        ("2.0", Some("garbage")),
    ] {
        let mut document: serde_json::Value = serde_json::from_str(GLTF).unwrap();
        document["asset"]["version"] = serde_json::json!(version);
        if let Some(value) = minimum {
            document["asset"]["minVersion"] = serde_json::json!(value);
        }
        let error = validate_local_gltf(
            &server.url("models/arrow.gltf"),
            document.to_string().as_bytes(),
        )
        .unwrap_err();
        assert!(error.contains("version/minVersion"), "{error}");
    }
    let mut supported: serde_json::Value = serde_json::from_str(GLTF).unwrap();
    supported["asset"]["minVersion"] = serde_json::json!("2.0");
    assert!(validate_local_gltf(
        &server.url("models/arrow.gltf"),
        supported.to_string().as_bytes()
    )
    .is_ok());
}
