//! Prints a `.dae` file's node tree, meshes and materials.
//!
//! `cargo run -p gz-collada --example dae_dump -- model.dae`
fn main() {
    for path in std::env::args().skip(1) {
        let bytes = std::fs::read(&path).unwrap_or_default();
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
                    println!(
                        "  material {i}: {:?} diffuse {:?} opacity {} tex {:?} normal {:?}",
                        m.name, m.diffuse, m.opacity, m.diffuse_texture, m.normal_texture
                    );
                }
            }
            Err(e) => println!("{path}: error: {e}"),
        }
    }
}
