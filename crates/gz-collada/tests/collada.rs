//! Scene assembly on hand-written probes (always) and on real Fuel meshes
//! checked against gzweb's three.js loader (when the spike corpus is
//! present: `spikes/collada/fetch.sh`, then `cargo test -p gz-collada -- --ignored`).
#![allow(clippy::unwrap_used)]

use std::path::Path;

use gz_collada::{parse, Scene, Shading, UpAxis};

fn data(name: &str) -> Scene {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name);
    parse(&std::fs::read(path).unwrap()).unwrap()
}

fn near(a: [f32; 3], b: [f32; 3], tol: f32) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() <= tol)
}

#[test]
fn hierarchy_composes_parent_first_with_the_unit_on_top() {
    let s = data("hierarchy.dae");
    assert_eq!((s.unit, s.up_axis), (0.01, UpAxis::Y));
    let parent = &s.roots[0];
    assert_eq!(parent.name.as_deref(), Some("ParentNode"));
    assert_eq!(parent.children[0].name.as_deref(), Some("ChildNode"));
    // three.js: the triangle lands at y = 1 m (mesh-loader put it at x = 100).
    let (min, max) = s.bounds().unwrap();
    assert!(near(min, [-0.01, 1.0, 0.0], 1e-5), "{min:?}");
    assert!(near(max, [0.0, 1.01, 0.0], 1e-5), "{max:?}");
}

#[test]
fn instances_materials_and_library_nodes() {
    let s = data("probe.dae");
    let names: Vec<_> = s.roots.iter().map(|n| n.name.as_deref().unwrap()).collect();
    assert_eq!(
        names,
        ["ParentNode", "SecondInstance", "MultiMat", "RefNode"]
    );
    // One triangle mesh shared by three instances (one through
    // <instance_node>), and a two-material geometry split per primitive.
    assert_eq!(s.meshes.len(), 3);
    assert_eq!(s.instances().len(), 5);
    assert_eq!(s.triangle_count(), 6);
    let multi = &s.roots[2];
    assert_eq!(multi.primitives.len(), 2);
    assert_ne!(multi.primitives[0].material, multi.primitives[1].material);
    let lib = &s.roots[3].children[0];
    assert_eq!(lib.name.as_deref(), Some("LibNode"));
    assert_eq!(lib.id.as_deref(), Some("LibNode"));

    let textured = &s.materials[multi.primitives[1].material.unwrap()];
    assert_eq!(textured.name.as_deref(), Some("Textured"));
    assert_eq!(textured.shading, Shading::Phong);
    assert_eq!(textured.opacity, 0.5);
    assert_eq!(textured.shininess, 20.0);
    assert_eq!(
        textured.diffuse_texture.as_deref(),
        Some("../materials/textures/probe.png")
    );
    // FCOLLADA <extra><technique><bump>.
    assert_eq!(
        textured.normal_texture.as_deref(),
        Some("../materials/textures/probe_normal.png")
    );
    let (min, max) = s.bounds().unwrap();
    assert!(near(min, [-1.0, 0.0, 0.0], 1e-5), "{min:?}");
    assert!(near(max, [0.03, 3.01, 2.0], 1e-5), "{max:?}");
}

#[test]
fn unused_geometries_are_not_emitted() {
    // textured.dae has two geometries nobody instances.
    let s = data("textured.dae");
    assert_eq!(s.instances().len(), 1);
    let (min, max) = s.bounds().unwrap();
    assert!(near(min, [0.02, 0.0, 0.0], 1e-5), "{min:?}");
    assert!(near(max, [0.03, 0.01, 0.0], 1e-5), "{max:?}");
}

#[test]
fn bad_documents_are_errors() {
    assert!(parse(b"not xml").is_err());
    assert!(parse(b"<root/>")
        .unwrap_err()
        .to_string()
        .contains("COLLADA"));
    let empty =
        br#"<COLLADA xmlns="http://www.collada.org/2005/11/COLLADASchema" version="1.4.1"/>"#;
    let s = parse(empty).unwrap();
    assert!(s.roots.is_empty() && s.bounds().is_none());
    assert_eq!(s.triangle_count(), 0);
}

/// gzweb's three.js loader output for the spike corpus, one JSON line per file.
#[test]
#[ignore = "needs spikes/collada/fetch.sh"]
fn fuel_corpus_matches_three_js() {
    let spike = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spikes/collada");
    let reference = std::fs::read_to_string(spike.join("out/js.jsonl")).unwrap();
    let mut checked = 0;
    for line in reference.lines() {
        let field = |key: &str| -> &str {
            let start = line.find(&format!("\"{key}\":")).unwrap() + key.len() + 3;
            &line[start..]
        };
        let file = field("file")
            .trim_start_matches('"')
            .split('"')
            .next()
            .unwrap();
        let Ok(bytes) = std::fs::read(spike.join(file)) else {
            continue;
        };
        let s = parse(&bytes).unwrap();
        let tris: usize = field("total_triangles")
            .split(|c: char| !c.is_ascii_digit())
            .next()
            .unwrap()
            .parse()
            .unwrap();
        // three collapses `instance_node` scale in the probe; skip it.
        if !file.contains("synthetic") {
            assert_eq!(s.triangle_count(), tris, "{file}");
        }
        let nums = |key: &str| -> [f32; 3] {
            let inner = field("aabb");
            let at = inner.find(&format!("\"{key}\":[")).unwrap() + key.len() + 4;
            let v: Vec<f32> = inner[at..]
                .split(']')
                .next()
                .unwrap()
                .split(',')
                .map(|x| x.parse().unwrap())
                .collect();
            [v[0], v[1], v[2]]
        };
        let (min, max) = s.bounds().unwrap();
        let tol = 1e-3 * (1.0 + max.iter().fold(0.0f32, |m, x| m.max(x.abs())));
        assert!(
            near(min, nums("min"), tol),
            "{file}: min {min:?} vs {:?}",
            nums("min")
        );
        assert!(
            near(max, nums("max"), tol),
            "{file}: max {max:?} vs {:?}",
            nums("max")
        );
        checked += 1;
    }
    assert!(checked >= 14, "only {checked} files checked");
}
