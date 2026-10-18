//! COLLADA (`.dae`) loading for gzweb-rs.
//!
//! [`parse`] returns the instantiated visual scene as a node tree, the way
//! Gazebo models need it: node names (for `<submesh>`), local transforms in
//! the right order, one mesh per primitive with its own material, and the
//! document's `<unit>` as a scale for the whole model.
//!
//! The XML, geometry and effect parsers are vendored from
//! [mesh-loader](https://github.com/openrr/mesh-loader) (Apache-2.0, see
//! `src/vendor`); its scene assembly flattened the tree with four bugs that
//! broke real Fuel models (`.agent/WORKLOG.md`, Collada spike), so
//! `assemble.rs` replaces it, and `validate.rs` checks what it reads.
//!
//! Like gz-common and gzweb, `<up_axis>` is reported but not applied.
//!
//! A malformed document is an [`Error`], never a panic.

mod assemble;
mod validate;

// Vendored: none of the workspace's lints apply.
#[allow(
    clippy::all,
    clippy::pedantic,
    clippy::restriction,
    unreachable_pub,
    unused_qualifications,
    rust_2018_idioms,
    dead_code,
    unused_imports
)]
mod vendor;

/// A `.dae` document's visual scene.
#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    /// `<unit meter>`: metres per document unit. Apply it as a scale above
    /// [`Scene::roots`], so node translations scale too (as gz-common does).
    pub unit: f32,
    /// `<up_axis>`. Gazebo ignores it, and so should renderers.
    pub up_axis: UpAxis,
    /// Top-level nodes of the instantiated `<visual_scene>`.
    pub roots: Vec<Node>,
    /// Indexed by [`Primitive::mesh`].
    pub meshes: Vec<Mesh>,
    /// Indexed by [`Primitive::material`].
    pub materials: Vec<Material>,
    /// What was skipped (unsupported or cyclic content), once per kind.
    pub warnings: Vec<String>,
}

impl Scene {
    /// Every primitive instance with its world transform (the unit scale,
    /// then each node's transform, parent first), row-major.
    pub fn instances(&self) -> Vec<([[f32; 4]; 4], Primitive)> {
        fn walk(node: &Node, parent: &[[f32; 4]; 4], out: &mut Vec<([[f32; 4]; 4], Primitive)>) {
            let world = mul(parent, &node.transform);
            out.extend(node.primitives.iter().map(|p| (world, *p)));
            for child in &node.children {
                walk(child, &world, out);
            }
        }
        let u = self.unit;
        let root = [
            [u, 0.0, 0.0, 0.0],
            [0.0, u, 0.0, 0.0],
            [0.0, 0.0, u, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let mut out = Vec::new();
        for node in &self.roots {
            walk(node, &root, &mut out);
        }
        out
    }

    /// Triangles over all instances.
    pub fn triangle_count(&self) -> usize {
        self.instances()
            .iter()
            .map(|(_, p)| self.meshes[p.mesh].positions.len() / 3)
            .sum()
    }

    /// World-space bounds (metres) over all instances, or `None` if empty.
    pub fn bounds(&self) -> Option<([f32; 3], [f32; 3])> {
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for (m, p) in self.instances() {
            for v in &self.meshes[p.mesh].positions {
                for (i, row) in m.iter().take(3).enumerate() {
                    let w = row[0] * v[0] + row[1] * v[1] + row[2] * v[2] + row[3];
                    min[i] = min[i].min(w);
                    max[i] = max[i].max(w);
                }
            }
        }
        (min[0] <= max[0]).then_some((min, max))
    }
}

/// Row-major 4×4 product `a · b`.
fn mul(a: &[[f32; 4]; 4], b: &[[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut out = [[0.0; 4]; 4];
    for (r, row) in out.iter_mut().enumerate() {
        for (c, cell) in row.iter_mut().enumerate() {
            *cell = (0..4).map(|k| a[r][k] * b[k][c]).sum();
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpAxis {
    X,
    Y,
    Z,
}

/// A `<node>`, with library nodes from `<instance_node>` inlined as children.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub name: Option<String>,
    pub id: Option<String>,
    /// Local transform, row-major (`[row][column]`), translation in column 3.
    pub transform: [[f32; 4]; 4],
    /// One per `<triangles>`/`<polylist>`/… of each `<instance_geometry>`.
    pub primitives: Vec<Primitive>,
    pub children: Vec<Node>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Primitive {
    pub mesh: usize,
    /// `None` when the instance binds no material to the primitive's symbol.
    pub material: Option<usize>,
}

/// Unindexed triangles (every three vertices are a face), in document units.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    pub positions: Vec<[f32; 3]>,
    /// Empty, or one per position.
    pub normals: Vec<[f32; 3]>,
    /// First UV set: empty, or one per position. V points up (COLLADA's
    /// convention).
    pub texcoords: Vec<[f32; 2]>,
    /// Empty, or one per position.
    pub colors: Vec<[f32; 4]>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Shading {
    Constant,
    Lambert,
    #[default]
    Phong,
    Blinn,
}

impl Default for Material {
    /// COLLADA's defaults (a grey Phong effect).
    fn default() -> Self {
        Self {
            name: None,
            shading: Shading::Phong,
            emission: [0.0, 0.0, 0.0, 1.0],
            ambient: [0.1, 0.1, 0.1, 1.0],
            diffuse: [0.6, 0.6, 0.6, 1.0],
            specular: [0.4, 0.4, 0.4, 1.0],
            shininess: 10.0,
            opacity: 1.0,
            double_sided: false,
            diffuse_texture: None,
            normal_texture: None,
            emission_texture: None,
            specular_texture: None,
            transparent_texture: None,
        }
    }
}

/// A `<material>`'s `profile_COMMON` effect.
#[derive(Debug, Clone, PartialEq)]
pub struct Material {
    pub name: Option<String>,
    pub shading: Shading,
    pub emission: [f32; 4],
    pub ambient: [f32; 4],
    /// The colour; COLLADA's default grey when the slot holds a texture.
    /// Its alpha is as written; three.js ignores it (opacity comes from
    /// `<transparent>`/`<transparency>` only).
    pub diffuse: [f32; 4],
    pub specular: [f32; 4],
    pub shininess: f32,
    /// 1 is opaque (assimp's rules for `<transparent>`/`<transparency>`).
    pub opacity: f32,
    /// `<double_sided>` in the technique or in any `<extra>` of the effect.
    pub double_sided: bool,
    pub diffuse_texture: Option<Texture>,
    /// `<bump>`, in the technique or an `<extra>`.
    pub normal_texture: Option<Texture>,
    pub emission_texture: Option<Texture>,
    pub specular_texture: Option<Texture>,
    /// `<transparent>`'s texture: the material is a cut-out, its alpha taken
    /// from the diffuse texture (three.js sets no alpha map either).
    pub transparent_texture: Option<Texture>,
}

/// A texture slot's image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Texture {
    /// As written (`init_from`), `file://` and backslashes removed;
    /// relative to the `.dae`.
    pub path: String,
    /// Whether U and V repeat (else clamp): repeat unless the texture's
    /// `<extra>` says otherwise, as in three.js.
    pub repeat: [bool; 2],
}

/// Why a document couldn't be parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

/// Parses a `.dae` document (UTF-8, or UTF-16 with a BOM).
pub fn parse(bytes: &[u8]) -> Result<Scene, Error> {
    vendor::collada::parse(bytes).map_err(|e| Error(e.to_string()))
}
