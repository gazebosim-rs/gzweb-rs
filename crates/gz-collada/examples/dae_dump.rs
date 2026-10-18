//! Prints a `.dae` file's node tree, meshes and materials.
//!
//! `cargo run -p gz-collada --example dae_dump -- model.dae`
fn main() {
    for path in std::env::args().skip(1) {
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) => {
                println!("{path}: can't read it: {e}");
                continue;
            }
        };
        match gz_collada::parse(&bytes) {
            Ok(scene) => {
                println!("{path}: unit {} up {:?}", scene.unit, scene.up_axis);
                fn walk(n: &gz_collada::Node, depth: usize) {
                    let t = n.transform;
                    println!(
                        "{:indent$}node {:?} t=({:.3},{:.3},{:.3}) prims={:?}",
                        "",
                        n.name,
                        t[0][3],
                        t[1][3],
                        t[2][3],
                        n.primitives,
                        indent = depth * 2
                    );
                    for c in &n.children {
                        walk(c, depth + 1);
                    }
                }
                for r in &scene.roots {
                    walk(r, 1);
                }
                for (i, m) in scene.meshes.iter().enumerate() {
                    println!(
                        "  mesh {i}: {} verts, normals {}, uvs {}",
                        m.positions.len(),
                        m.normals.len(),
                        m.texcoords.len()
                    );
                }
                for (i, m) in scene.materials.iter().enumerate() {
                    let path = |t: &Option<gz_collada::Texture>| t.as_ref().map(|t| t.path.clone());
                    println!(
                        "  material {i}: {:?} diffuse {:?} opacity {} tex {:?} normal {:?}",
                        m.name,
                        m.diffuse,
                        m.opacity,
                        path(&m.diffuse_texture),
                        path(&m.normal_texture)
                    );
                }
                for w in &scene.warnings {
                    println!("  warning: {w}");
                }
            }
            Err(e) => println!("{path}: error: {e}"),
        }
    }
}
