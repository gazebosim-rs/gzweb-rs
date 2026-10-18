//! Headless: every corpus .dae converts into valid Bevy meshes/materials.
use std::path::Path;

use bevy_mesh::{Mesh, VertexAttributeValues};
use collada_spike::{bevy_conv, summary};

fn dae_files() -> Vec<std::path::PathBuf> {
    let mut out = vec![];
    // The probes live with gz-collada's tests.
    for dir in ["corpus", "../../crates/gz-collada/tests/data"] {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(dir);
        let mut stack = vec![root];
        while let Some(d) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&d) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|e| e == "dae") {
                    out.push(p);
                }
            }
        }
    }
    out.sort();
    out
}

#[test]
fn converts_corpus_to_bevy_meshes() {
    let files = dae_files();
    assert!(!files.is_empty(), "run ./fetch.sh first");
    for f in files {
        let scene = summary::parse(&std::fs::read(&f).unwrap()).unwrap();
        let t0 = std::time::Instant::now();
        let prims = bevy_conv::convert_scene(&scene);
        let conv_ms = t0.elapsed().as_secs_f64() * 1e3;
        assert!(!prims.is_empty(), "{}: no meshes", f.display());
        let mut tris = 0;
        for p in &prims {
            let n = p.mesh.count_vertices();
            let Some(VertexAttributeValues::Float32x3(_)) = p.mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
            else {
                panic!("{}: {} has no normals", f.display(), p.name)
            };
            if let Some(uv) = p.mesh.attribute(Mesh::ATTRIBUTE_UV_0) {
                assert_eq!(uv.len(), n);
            }
            let idx = p.mesh.indices().unwrap();
            assert!(idx.iter().all(|i| i < n));
            tris += idx.len() / 3;
            let Some(VertexAttributeValues::Float32x3(pos)) = p.mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!() };
            assert!(pos.iter().flatten().all(|v| v.is_finite()));
            // StandardMaterial construction with dummy handles.
            let _m = p.material.to_standard(|_| Default::default());
        }
        eprintln!("{}: {} bevy meshes, {} tris, convert {:.2} ms", f.display(), prims.len(), tris, conv_ms);
    }
}
