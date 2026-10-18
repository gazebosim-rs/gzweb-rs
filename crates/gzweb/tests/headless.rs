//! End-to-end tests: the real `gzweb` binary in headless `--snapshot` mode
//! against an in-process `gz-parity` replay server.
//!
//! The recorded corpora are also checked against the committed gzweb
//! reference output (`tests/corpus/<name>/gzweb.*`), so these tests are a
//! parity regression suite that needs no browser.

// `allow-unwrap-in-tests` doesn't reach helper functions in an integration test.
#![allow(clippy::unwrap_used)]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use gz_core::msgs;
use gz_core::snapshot::{SnapNode, Snapshot};
use gz_parity::compare;
use gz_parity::corpus::{Builder, Corpus};
use gz_parity::mirror::{self, MirrorOptions};
use gz_parity::replay::{self, ReplayOptions};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gzweb-headless-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Serves `corpus` for one connection; returns the websocket URL.
fn start_replay(corpus: Corpus, client_log: Option<PathBuf>, key: Option<&str>) -> String {
    start_replay_with(
        corpus,
        ReplayOptions {
            client_log,
            connections: 1,
            key: key.map(str::to_string),
            ..Default::default()
        },
    )
}

fn start_replay_with(corpus: Corpus, opts: ReplayOptions) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || replay::serve_on(listener, &corpus, &opts));
    url
}

fn gzweb(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gzweb"))
        .args(args)
        .env("RUST_LOG", "warn")
        .output()
        .unwrap()
}

/// Runs a headless snapshot; returns the process output and the snapshot.
fn snapshot(url: &str, out: &Path, extra: &[&str]) -> (Output, Option<Snapshot>) {
    let out_s = out.to_string_lossy();
    let mut args = vec![
        "--url",
        url,
        "--snapshot",
        &out_s,
        "--settle-ms",
        "300",
        "--timeout-ms",
        "30000",
    ];
    args.extend_from_slice(extra);
    let output = gzweb(&args);
    let snap = std::fs::read_to_string(out)
        .ok()
        .map(|t| serde_json::from_str(&t).unwrap());
    (output, snap)
}

fn node<'a>(snap: &'a Snapshot, path: &str) -> &'a SnapNode {
    snap.nodes
        .iter()
        .find(|n| n.path == path)
        .unwrap_or_else(|| {
            let paths: Vec<_> = snap.nodes.iter().map(|n| &n.path).collect();
            panic!("no node '{path}' in {paths:#?}")
        })
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// Starts `gz-parity mirror` on `lock`; the cache is `GZ_HTTP_CACHE` or
/// `target/http-cache`, and `GZ_HTTP_OFFLINE=true` forbids fetching.
fn start_mirror(lock: &Path) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let opts = MirrorOptions {
        cache: std::env::var_os("GZ_HTTP_CACHE")
            .map_or_else(|| repo().join("target/http-cache"), PathBuf::from),
        lock: Some(lock.to_path_buf()),
        upstream: None,
        offline: std::env::var("GZ_HTTP_OFFLINE").is_ok_and(|v| v == "true"),
    };
    std::thread::spawn(move || mirror::serve_on(listener, opts));
    base
}

/// Replays a recorded corpus and diffs gzweb-rs against gzweb's committed
/// snapshot and client frame log.
fn recorded_corpus_matches_gzweb(name: &str) {
    let dir = repo().join("tests/corpus").join(name);
    let tmp = scratch(name);
    let log = tmp.join("rs.client.log");
    let url = start_replay(Corpus::load(&dir).unwrap(), Some(log.clone()), None);
    // Worlds with web assets get them through a caching mirror, pinned by
    // the corpus's lock file.
    let lock = dir.join("http.lock.json");
    let mirror = lock.exists().then(|| start_mirror(&lock));
    let extra: Vec<&str> = match &mirror {
        Some(m) => vec!["--http-mirror", m],
        None => vec![],
    };
    let (output, snap) = snapshot(&url, &tmp.join("rs.json"), &extra);
    assert!(output.status.success(), "{}", stderr(&output));
    let actual = snap.unwrap();

    let expected: Snapshot =
        serde_json::from_str(&std::fs::read_to_string(dir.join("gzweb.snapshot.json")).unwrap())
            .unwrap();
    let mut allow = compare::load_allowlist(&repo().join("tests/parity/allowlist.toml")).unwrap();
    // Features the world needs that aren't ported yet.
    let gaps = dir.join("gaps.toml");
    if gaps.exists() {
        allow.extend(compare::load_allowlist(&gaps).unwrap());
    }
    let report = compare::compare_snapshots(&expected, &actual, &allow, Default::default());
    assert!(report.passed(), "{name}: {:#?}", report.diffs);

    let expected_log = std::fs::read_to_string(dir.join("gzweb.client.log")).unwrap();
    let actual_log = std::fs::read_to_string(&log).unwrap();
    let protocol = compare::compare_protocol(&expected_log, &actual_log);
    assert!(protocol.passed(), "{name}: {protocol:#?}");
}

#[test]
fn shapes_matches_gzweb() {
    recorded_corpus_matches_gzweb("shapes");
}

#[test]
fn empty_matches_gzweb() {
    recorded_corpus_matches_gzweb("empty");
}

// Worlds with Fuel meshes or textures. Ignored by default because a cold
// cache fetches from fuel.gazebosim.org; CI runs them in the `fuel` job with
// `cargo test -p gzweb --test headless -- --ignored`.

#[test]
#[ignore = "fetches from fuel.gazebosim.org on a cold cache"]
fn heightmap_matches_gzweb() {
    recorded_corpus_matches_gzweb("heightmap");
}

#[test]
#[ignore = "fetches from fuel.gazebosim.org on a cold cache"]
fn import_mesh_matches_gzweb() {
    recorded_corpus_matches_gzweb("import_mesh");
}

#[test]
fn lights_matches_gzweb() {
    recorded_corpus_matches_gzweb("lights");
}

#[test]
fn nested_model_matches_gzweb() {
    recorded_corpus_matches_gzweb("nested_model");
}

#[test]
fn diff_drive_matches_gzweb() {
    recorded_corpus_matches_gzweb("diff_drive");
}

#[test]
fn polylines_matches_gzweb() {
    recorded_corpus_matches_gzweb("polylines");
}

// ─── Synthetic world ────────────────────────────────────────────────────────

/// gz-collada's hand-written probe: hierarchy, instancing, two materials.
fn probe_dae() -> Vec<u8> {
    std::fs::read(repo().join("crates/gz-collada/tests/data/probe.dae")).unwrap()
}

fn v3(x: f64, y: f64, z: f64) -> Option<msgs::Vector3d> {
    Some(msgs::Vector3d {
        x,
        y,
        z,
        ..Default::default()
    })
}

fn color(r: f32, g: f32, b: f32, a: f32) -> Option<msgs::Color> {
    Some(msgs::Color {
        r,
        g,
        b,
        a,
        ..Default::default()
    })
}

fn pose(x: f64, y: f64, z: f64) -> Option<msgs::Pose> {
    Some(msgs::Pose {
        position: v3(x, y, z),
        orientation: Some(msgs::Quaternion {
            w: 1.0,
            ..Default::default()
        }),
        ..Default::default()
    })
}

/// Sets `Geometry.type` from whichever shape is present, as Gazebo does.
fn typed(mut g: msgs::Geometry) -> msgs::Geometry {
    use msgs::geometry::Type;
    let ty = if g.r#box.is_some() {
        Type::Box
    } else if g.cylinder.is_some() {
        Type::Cylinder
    } else if g.capsule.is_some() {
        Type::Capsule
    } else if g.cone.is_some() {
        Type::Cone
    } else if g.sphere.is_some() {
        Type::Sphere
    } else if g.ellipsoid.is_some() {
        Type::Ellipsoid
    } else if g.plane.is_some() {
        Type::Plane
    } else if g.mesh.is_some() {
        Type::Mesh
    } else if g.heightmap.is_some() {
        Type::Heightmap
    } else {
        Type::Empty
    };
    g.r#type = ty as i32;
    g
}

fn visual(name: &str, geometry: msgs::Geometry) -> msgs::Visual {
    msgs::Visual {
        name: name.into(),
        pose: pose(0.0, 0.0, 0.0),
        geometry: Some(typed(geometry)),
        material: Some(msgs::Material {
            ambient: color(0.2, 0.2, 0.2, 1.0),
            diffuse: color(0.8, 0.1, 0.1, 1.0),
            emissive: color(0.0, 0.1, 0.0, 1.0),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn mesh(name: &str, filename: &str) -> msgs::Visual {
    visual(
        name,
        msgs::Geometry {
            mesh: Some(msgs::MeshGeom {
                filename: filename.into(),
                scale: v3(2.0, 2.0, 2.0),
                ..Default::default()
            }),
            ..Default::default()
        },
    )
}

/// The probe `.dae` cut down to one node, centred.
fn submesh(name: &str, node: &str) -> msgs::Visual {
    submesh_of(name, "/data/robot.dae", node)
}

fn submesh_of(name: &str, file: &str, node: &str) -> msgs::Visual {
    let mut v = mesh(name, file);
    let m = v.geometry.as_mut().unwrap().mesh.as_mut().unwrap();
    m.submesh = node.into();
    m.center_submesh = true;
    v
}

fn model(name: &str, id: u32, links: Vec<msgs::Link>) -> msgs::Model {
    msgs::Model {
        name: name.into(),
        id,
        pose: pose(0.0, 0.0, 0.0),
        link: links,
        ..Default::default()
    }
}

fn link(name: &str, visuals: Vec<msgs::Visual>) -> msgs::Link {
    msgs::Link {
        name: name.into(),
        pose: pose(0.0, 0.0, 0.0),
        visual: visuals,
        ..Default::default()
    }
}

fn light(name: &str, ty: msgs::light::LightType) -> msgs::Light {
    msgs::Light {
        name: name.into(),
        r#type: ty as i32,
        pose: pose(0.0, 0.0, 5.0),
        diffuse: color(1.0, 0.9, 0.8, 1.0),
        direction: v3(0.5, 0.1, -0.9),
        range: 20.0,
        attenuation_linear: 0.01,
        attenuation_quadratic: 0.001,
        spot_inner_angle: 0.6,
        spot_outer_angle: 1.0,
        spot_falloff: 1.0,
        cast_shadows: true,
        intensity: 1.0,
        ..Default::default()
    }
}

const OBJ: &[u8] = b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";
/// Two objects with their own materials; the second's texture is a bare
/// file name, which gzweb looks for in `materials/textures`.
const OBJ_MTL: &[u8] = b"mtllib two.mtl\nv 0 0 0\nv 1 0 0\nv 0 1 0\nv 0 0 1\nvt 0 0\n\
o first\nusemtl red\nf 1 2 3\no second\nusemtl tex\nf 1/1 2/1 4/1\nf 1/1 3/1 4/1\n";
const MTL: &[u8] = b"newmtl red\nKd 1 0 0\nnewmtl tex\nKd 0 0 1\nmap_Kd t.png\n";
const STL: &[u8] = b"solid t\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\n\
vertex 0 1 0\nendloop\nendfacet\nendsolid t\n";
const GLTF: &str = r#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],
"nodes":[{"mesh":0,"name":"tri"}],"meshes":[{"primitives":[{"attributes":{"POSITION":0}}]}],
"buffers":[{"byteLength":36,"uri":"tri.bin"}],"bufferViews":[{"buffer":0,"byteLength":36}],
"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3",
"min":[0,0,0],"max":[1,1,0]}]}"#;

fn tri_bin() -> Vec<u8> {
    [0f32, 0., 0., 1., 0., 0., 0., 1., 0.]
        .iter()
        .flat_map(|f| f.to_le_bytes())
        .collect()
}

/// A one-shot-per-request HTTP server for `gzhttps` assets; returns its base URL.
fn start_http(files: Vec<(&'static str, &'static [u8])>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let mut buf = [0u8; 4096];
            let n = s.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]);
            let path = req.split_whitespace().nth(1).unwrap_or("").to_string();
            let reply = match files.iter().find(|(p, _)| *p == path) {
                Some((_, body)) => {
                    let mut r = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .into_bytes();
                    r.extend_from_slice(body);
                    r
                }
                None => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    .to_vec(),
            };
            let _ = s.write_all(&reply);
        }
    });
    base
}

fn synthetic_world(http: &str) -> Corpus {
    use msgs::light::LightType;
    let shapes = vec![
        visual(
            "box",
            msgs::Geometry {
                r#box: Some(msgs::BoxGeom {
                    size: v3(1.0, 2.0, 3.0),
                    ..Default::default()
                }),
                ..Default::default()
            },
        ),
        visual(
            "cylinder",
            msgs::Geometry {
                cylinder: Some(msgs::CylinderGeom {
                    radius: 0.5,
                    length: 2.0,
                    ..Default::default()
                }),
                ..Default::default()
            },
        ),
        visual(
            "capsule",
            msgs::Geometry {
                capsule: Some(msgs::CapsuleGeom {
                    radius: 0.5,
                    length: 1.0,
                    ..Default::default()
                }),
                ..Default::default()
            },
        ),
        visual(
            "cone",
            msgs::Geometry {
                cone: Some(msgs::ConeGeom {
                    radius: 0.5,
                    length: 1.0,
                    ..Default::default()
                }),
                ..Default::default()
            },
        ),
        visual(
            "sphere",
            msgs::Geometry {
                sphere: Some(msgs::SphereGeom {
                    radius: 0.5,
                    ..Default::default()
                }),
                ..Default::default()
            },
        ),
        visual(
            "ellipsoid",
            msgs::Geometry {
                ellipsoid: Some(msgs::EllipsoidGeom {
                    radii: v3(1.0, 0.5, 0.25),
                    ..Default::default()
                }),
                ..Default::default()
            },
        ),
        visual(
            "wall",
            msgs::Geometry {
                plane: Some(msgs::PlaneGeom {
                    normal: v3(0.0, 1.0, 0.0),
                    size: Some(msgs::Vector2d {
                        x: 4.0,
                        y: 2.0,
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
        ),
        visual(
            "terrain",
            msgs::Geometry {
                heightmap: Some(msgs::HeightmapGeom::default()),
                ..Default::default()
            },
        ),
        msgs::Visual {
            transparency: 0.5,
            ..visual(
                "glass",
                msgs::Geometry {
                    r#box: Some(msgs::BoxGeom::default()),
                    ..Default::default()
                },
            )
        },
    ];
    let meshes = vec![
        mesh("obj", "model://pkg/meshes/tri.obj"),
        mesh("obj_mtl", "model://pkg/meshes/two.obj"),
        submesh_of("obj_sub", "model://pkg/meshes/two.obj", "second"),
        mesh("stl", "file:///data/tri.stl"),
        mesh("gltf", "/data/tri.gltf"),
        mesh("dae", "/data/robot.dae"),
        submesh("dae_child", "ChildNode"),
        submesh("dae_nosuch", "NoSuchNode"),
        mesh("missing", "/data/missing.stl"),
        mesh("fbx", "/data/thing.fbx"),
        mesh("http", &format!("{http}/web/tri.obj")),
        mesh("http404", &format!("{http}/web/nope.obj")),
        mesh("http_server", &format!("{http}/web/server_only.obj")),
    ];
    // A 3×3 image: black, a white centre and one mid-grey corner.
    let terrain = visual(
        "ground",
        msgs::Geometry {
            heightmap: Some(msgs::HeightmapGeom {
                filename: "/data/terrain.png".into(),
                size: v3(4.0, 4.0, 2.0),
                origin: v3(0.0, 0.0, 0.0),
                ..Default::default()
            }),
            ..Default::default()
        },
    );
    let mut lit = link("lit", vec![]);
    lit.light.push(light("lamp", LightType::Point));

    let nested = msgs::Model {
        model: vec![model(
            "inner",
            31,
            vec![link("l", vec![visual("dup", Default::default())])],
        )],
        ..model(
            "outer",
            30,
            vec![link(
                "l",
                vec![
                    visual("dup", Default::default()),
                    visual("dup", Default::default()),
                ],
            )],
        )
    };

    let scene = msgs::Scene {
        name: "synthetic".into(),
        ambient: color(0.3, 0.3, 0.3, 1.0),
        background: color(0.1, 0.2, 0.3, 1.0),
        model: vec![
            model("shapes", 10, vec![link("l", shapes)]),
            model("meshes", 20, vec![link("l", meshes)]),
            model("terrain", 50, vec![link("l", vec![terrain])]),
            model("lamp_post", 40, vec![lit]),
            nested,
        ],
        light: vec![
            light("sun", LightType::Directional),
            light("bulb", LightType::Point),
            light("spot", LightType::Spot),
        ],
        ..Default::default()
    };

    // Poses: one by id, one by name (id 0), one unknown.
    let poses = msgs::PoseV {
        pose: vec![
            msgs::Pose {
                id: 10,
                ..pose(1.0, 2.0, 3.0).unwrap()
            },
            msgs::Pose {
                name: "outer".into(),
                ..pose(-1.0, 0.0, 0.0).unwrap()
            },
            msgs::Pose {
                name: "ghost".into(),
                id: 999,
                ..pose(0.0, 0.0, 0.0).unwrap()
            },
        ],
        ..Default::default()
    };
    // scene/info: one known model (ignored) and one new one.
    let update = msgs::Scene {
        model: vec![
            model("shapes", 10, vec![]),
            model(
                "late",
                50,
                vec![link(
                    "l",
                    vec![visual(
                        "late_box",
                        msgs::Geometry {
                            r#box: Some(msgs::BoxGeom::default()),
                            ..Default::default()
                        },
                    )],
                )],
            ),
        ],
        ..Default::default()
    };

    let pose_topic = "/world/synthetic/dynamic_pose/info";
    let scene_topic = "/world/synthetic/scene/info";
    Builder::new("synthetic")
        .scene(&scene)
        .asset("pkg/meshes/tri.obj", OBJ)
        .asset(
            "/data/terrain.png",
            &std::fs::read(repo().join("crates/gzweb/tests/data/terrain.png")).unwrap(),
        )
        // Not on the HTTP server: the websocket fallback finds it.
        .asset(&format!("{http}/web/server_only.obj"), OBJ)
        .asset("pkg/meshes/two.obj", OBJ_MTL)
        .asset("pkg/meshes/two.mtl", MTL)
        .asset("/data/tri.stl", STL)
        .asset("/data/tri.gltf", GLTF.as_bytes())
        .asset("/data/tri.bin", &tri_bin())
        .asset("/data/robot.dae", &probe_dae())
        .publish(pose_topic, "gz.msgs.Pose_V", &poses)
        // Not a Pose_V / Scene, and a frame without a header: all tolerated.
        .publish_raw(
            pose_topic,
            &gz_core::frame::build("pub", pose_topic, "gz.msgs.Pose_V", &[0xff, 0xff]),
        )
        .publish_raw(
            scene_topic,
            &gz_core::frame::build("pub", scene_topic, "gz.msgs.Scene", &[0xff, 0xff]),
        )
        .publish_raw(pose_topic, b"no header here")
        .publish(scene_topic, "gz.msgs.Scene", &update)
        .build()
}

#[test]
fn synthetic_world_spawns_every_feature() {
    let http = start_http(vec![("/web/tri.obj", OBJ)]);
    let tmp = scratch("synthetic");
    let url = start_replay(synthetic_world(&http), None, None);
    let (output, snap) = snapshot(&url, &tmp.join("rs.json"), &[]);
    assert!(output.status.success(), "{}", stderr(&output));
    let snap = snap.unwrap();

    assert_eq!(snap.world, "synthetic");
    let clear = snap.scene.clear_color.unwrap();
    for (got, want) in clear.iter().zip([0.1, 0.2, 0.3]) {
        assert!((got - want).abs() < 1e-3, "{clear:?}");
    }

    // Primitives.
    for (path, ty) in [
        ("shapes/l/box", "box"),
        ("shapes/l/cylinder", "cylinder"),
        ("shapes/l/capsule", "capsule"),
        ("shapes/l/cone", "cone"),
        ("shapes/l/sphere", "sphere"),
        ("shapes/l/ellipsoid", "ellipsoid"),
        ("shapes/l/wall", "plane"),
    ] {
        let g = node(&snap, path).geometry.as_ref().unwrap();
        assert_eq!(g.ty, ty, "{path}");
        assert!(g.vertex_count > 0, "{path}");
        assert!(g.aabb_world.is_some(), "{path}");
    }
    let glass = node(&snap, "shapes/l/glass").material.as_ref().unwrap();
    assert!(glass.transparent);
    assert!((glass.opacity - 0.5).abs() < 1e-6);

    // Mesh files over the websocket (relative, absolute, glTF with an
    // external buffer), over HTTP and from the server after HTTP fails all
    // load a 3-vertex triangle.
    for path in [
        "meshes/l/obj",
        "meshes/l/stl",
        "meshes/l/gltf",
        "meshes/l/http",
        "meshes/l/http_server",
    ] {
        let g = node(&snap, path).geometry.as_ref().unwrap();
        assert_eq!(g.ty, "mesh", "{path}");
        assert_eq!(g.vertex_count, 3, "{path}: {g:?}");
        // <mesh><scale> 2 goes on the visual.
        let aabb = g.aabb_world.as_ref().unwrap();
        let extent = (0..3)
            .map(|i| aabb.max[i] - aabb.min[i])
            .fold(0.0, f64::max);
        assert!((extent - 2.0).abs() < 1e-3, "{path}: {aabb:?}");
    }
    // Collada: the probe's node tree (5 primitive instances, 6 triangles,
    // one through <instance_node>), its textures missing.
    let dae = node(&snap, "meshes/l/dae").geometry.as_ref().unwrap();
    assert_eq!((dae.mesh_count, dae.vertex_count), (5, 18), "{dae:?}");
    // <submesh> keeps one node's triangle (1 cm, scaled by 2) and centres it.
    let sub = node(&snap, "meshes/l/dae_child").geometry.as_ref().unwrap();
    assert_eq!((sub.mesh_count, sub.vertex_count), (1, 3), "{sub:?}");
    let aabb = sub.aabb_world.as_ref().unwrap();
    for i in 0..3 {
        assert!((aabb.max[i] + aabb.min[i]).abs() < 1e-6, "{aabb:?}");
        assert!(aabb.max[i] - aabb.min[i] < 0.02 + 1e-6, "{aabb:?}");
    }
    // OBJ with an MTL: an object per `o`, each with its material; the
    // bare texture name is looked for in materials/textures (not served).
    let two = node(&snap, "meshes/l/obj_mtl");
    let g = two.geometry.as_ref().unwrap();
    assert_eq!((g.mesh_count, g.vertex_count), (2, 3 + 6), "{g:?}");
    assert_eq!(two.material.as_ref().unwrap().color, [1.0, 0.0, 0.0]);
    let sub = node(&snap, "meshes/l/obj_sub");
    let g = sub.geometry.as_ref().unwrap();
    assert_eq!((g.mesh_count, g.vertex_count), (1, 6), "{g:?}");
    let m = sub.material.as_ref().unwrap();
    assert_eq!(m.color, [0.0, 0.0, 1.0]);
    assert_eq!(
        m.maps.albedo.as_deref(),
        Some("wsasset://rel/pkg/materials/textures/t.png")
    );
    // Heightmap from an image: centred, white at size.z, top row at +Y.
    let g = node(&snap, "terrain/l/ground").geometry.as_ref().unwrap();
    assert_eq!(
        (g.ty.as_str(), g.mesh_count, g.vertex_count),
        ("heightmap", 1, 9),
        "{g:?}"
    );
    let aabb = g.aabb_world.as_ref().unwrap();
    let want = ([-2.0, -2.0, 0.0], [2.0, 2.0, 2.0]);
    for i in 0..3 {
        assert!((aabb.min[i] - want.0[i]).abs() < 1e-5, "{aabb:?}");
        assert!((aabb.max[i] - want.1[i]).abs() < 1e-5, "{aabb:?}");
    }
    // Failed or unsupported loads have no geometry.
    for path in [
        "meshes/l/missing",
        "meshes/l/fbx",
        "meshes/l/http404",
        "meshes/l/dae_nosuch",
    ] {
        let g = node(&snap, path).geometry.as_ref();
        assert!(g.is_none_or(|g| g.vertex_count == 0), "{path}: {g:?}");
    }

    // Lights at scene level and on a link.
    for (path, ty) in [
        ("sun", "directional"),
        ("bulb", "point"),
        ("spot", "spot"),
        ("lamp_post/lit/lamp", "point"),
    ] {
        let l = node(&snap, path).light.as_ref().unwrap();
        assert_eq!(l.ty, ty, "{path}");
        assert!(l.intensity > 0.0, "{path}");
    }
    assert!(node(&snap, "sun")
        .light
        .as_ref()
        .unwrap()
        .direction_world
        .is_some());

    // Repeated sibling names get #n suffixes; nested models nest.
    node(&snap, "outer/l/dup");
    node(&snap, "outer/l/dup#1");
    node(&snap, "outer/inner/l/dup");

    // Poses by id and by name.
    let shapes = node(&snap, "shapes").local.as_ref().unwrap();
    assert_eq!(shapes.pos, [1.0, 2.0, 3.0]);
    let outer = node(&snap, "outer").local.as_ref().unwrap();
    assert_eq!(outer.pos, [-1.0, 0.0, 0.0]);

    // scene/info adds new models once.
    node(&snap, "late/l/late_box");
    assert_eq!(snap.nodes.iter().filter(|n| n.path == "shapes").count(), 1);
}

// ─── Connection failures and arguments ──────────────────────────────────────

#[test]
fn correct_auth_key_connects() {
    let tmp = scratch("auth-ok");
    let log = tmp.join("client.log");
    let corpus = Corpus::load(&repo().join("tests/corpus/empty")).unwrap();
    let url = start_replay(corpus, Some(log.clone()), Some("sekrit"));
    let (output, snap) = snapshot(&url, &tmp.join("rs.json"), &["--key", "sekrit"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(snap.unwrap().world, "empty");
    let log = std::fs::read_to_string(log).unwrap();
    assert!(log.starts_with("auth,,,sekrit\nprotos,,,\n"), "{log}");
}

#[test]
fn rejected_auth_key_times_out_with_an_error() {
    let tmp = scratch("auth-bad");
    let corpus = Corpus::load(&repo().join("tests/corpus/empty")).unwrap();
    let url = start_replay(corpus, None, Some("sekrit"));
    let out = tmp.join("rs.json");
    let output = gzweb(&[
        "--url",
        &url,
        "--key",
        "wrong",
        "--snapshot",
        &out.to_string_lossy(),
        "--timeout-ms",
        "1500",
    ]);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    let err = stderr(&output);
    assert!(err.contains("auth key rejected"), "{err}");
    assert!(err.contains("timed out"), "{err}");
}

#[test]
fn unreachable_server_times_out_with_an_error() {
    // Bind then drop, so nothing listens on the port.
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let tmp = scratch("refused");
    let out = tmp.join("rs.json");
    let output = gzweb(&[
        "--port",
        &port.to_string(),
        "--snapshot",
        &out.to_string_lossy(),
        "--timeout-ms",
        "1000",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("websocket closed"),
        "{}",
        stderr(&output)
    );
    // A snapshot of the (empty) scene is still written.
    let snap: Snapshot = serde_json::from_str(&std::fs::read_to_string(out).unwrap()).unwrap();
    assert!(snap.nodes.is_empty());
}

#[test]
fn reconnects_after_the_server_drops_the_connection() {
    let dir = repo().join("tests/corpus/shapes");
    let tmp = scratch("reconnect");
    let log = tmp.join("client.log");
    let url = start_replay_with(
        Corpus::load(&dir).unwrap(),
        ReplayOptions {
            client_log: Some(log.clone()),
            connections: 2,
            // Right after the first scene, long before it settles.
            drop_first_at_sub: true,
            ..Default::default()
        },
    );
    let (output, snap) = snapshot(&url, &tmp.join("rs.json"), &["--reconnect"]);
    assert!(output.status.success(), "{}", stderr(&output));
    // Two sessions, and the scene spawned once (not twice) at the end.
    let log = std::fs::read_to_string(&log).unwrap();
    assert_eq!(log.matches("protos,,,").count(), 2, "{log}");
    let expected: Snapshot =
        serde_json::from_str(&std::fs::read_to_string(dir.join("gzweb.snapshot.json")).unwrap())
            .unwrap();
    assert_eq!(snap.unwrap().nodes.len(), expected.nodes.len());
}

#[test]
fn server_closing_mid_session_fails_pending_assets() {
    // A scene whose mesh is never answered: the replay server closes after
    // its stream, which must fail the pending load rather than hang.
    let scene = msgs::Scene {
        name: "w".into(),
        model: vec![model(
            "m",
            1,
            vec![link("l", vec![mesh("v", "/never/answered.obj")])],
        )],
        ..Default::default()
    };
    let mut corpus = Builder::new("w").scene(&scene).build();
    // Requests for this asset get no reply at all.
    corpus
        .manifest
        .assets
        .insert("/never/answered.obj".into(), usize::MAX);
    let tmp = scratch("closing");
    let url = start_replay(corpus, None, None);
    let (output, snap) = snapshot(&url, &tmp.join("rs.json"), &["--timeout-ms", "3000"]);
    // The connection is gone, so the scene never settles: exit 2, but the
    // snapshot of what arrived is still written.
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("connection closed"),
        "{}",
        stderr(&output)
    );
    let snap = snap.unwrap();
    let g = node(&snap, "m/l/v").geometry.as_ref();
    assert!(g.is_none_or(|g| g.vertex_count == 0), "{g:?}");
}

#[test]
fn help_and_bad_arguments() {
    let help = gzweb(&["--help"]);
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("usage: gzweb"));

    let bad = gzweb(&["--frobnicate"]);
    assert_eq!(bad.status.code(), Some(2));
    assert!(stderr(&bad).contains("unknown argument --frobnicate"));
}
