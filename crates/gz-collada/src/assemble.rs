//! Turns a parsed COLLADA document into [`crate::Scene`]: the node tree of
//! the instantiated visual scene, one mesh per primitive and the materials
//! each instance binds. Replaces mesh-loader's `instance.rs`, which
//! multiplied transforms child-first, post-multiplied the unit, merged a
//! geometry's primitives under one material and emitted geometries rather
//! than instances.
//!
//! This is a child of the vendored `collada` module (see its `#[path]`), so
//! it can read the parser's types.

use std::collections::HashMap;

use super::iter;
use super::{Document, Effect, Geometry, ImageSource, Node, ShadeType, Texture};

/// Deeper `instance_node` chains are treated as cycles.
const MAX_DEPTH: usize = 64;

struct Builder<'d, 'a> {
    doc: &'d Document<'a>,
    meshes: Vec<crate::Mesh>,
    materials: Vec<crate::Material>,
    /// (geometry id, primitive index) → mesh index.
    mesh_index: HashMap<(&'a str, usize), usize>,
    /// material id → material index.
    material_index: HashMap<&'a str, usize>,
}

pub(super) fn assemble(doc: &Document<'_>) -> crate::Scene {
    let nodes = &doc.library_visual_scenes.nodes;
    // The scene `<scene>` instantiates, else the first visual scene.
    let wanted = doc
        .scene
        .instance_visual_scene
        .as_ref()
        .map(|v| v.url.as_str());
    let scene = nodes
        .iter()
        .position(|n| n.visual_scene && wanted.is_none_or(|w| n.id == Some(w)))
        .or_else(|| nodes.iter().position(|n| n.visual_scene));
    let mut b = Builder {
        doc,
        meshes: Vec::new(),
        materials: Vec::new(),
        mesh_index: HashMap::new(),
        material_index: HashMap::new(),
    };
    let roots = match scene {
        Some(i) => nodes[i]
            .children
            .iter()
            .filter_map(|&c| b.node(c, 0))
            .collect(),
        None => Vec::new(),
    };
    crate::Scene {
        unit: doc.asset.unit,
        up_axis: doc.asset.up_axis,
        roots,
        meshes: b.meshes,
        materials: b.materials,
    }
}

impl<'a> Builder<'_, 'a> {
    fn node(&mut self, index: usize, depth: usize) -> Option<crate::Node> {
        if depth > MAX_DEPTH {
            return None;
        }
        let doc = self.doc;
        let node: &Node<'a> = &doc.library_visual_scenes.nodes[index];
        let mut primitives = Vec::new();
        for instance in &node.instance_geometry {
            let Some(geometry) = doc.get(&instance.url) else {
                continue;
            };
            for (i, prim) in geometry.mesh.primitives.iter().enumerate() {
                let Some(mesh) = self.mesh(geometry, i) else {
                    continue;
                };
                // The material this instance binds to the primitive's symbol.
                let material = prim
                    .material
                    .and_then(|symbol| instance.materials.get(symbol))
                    .and_then(|binding| self.material(binding.target.as_str()));
                primitives.push(crate::Primitive { mesh, material });
            }
        }
        let mut children: Vec<crate::Node> = node
            .children
            .iter()
            .filter_map(|&c| self.node(c, depth + 1))
            .collect();
        for id in &node.instance_node {
            let library = doc
                .library_visual_scenes
                .nodes
                .iter()
                .position(|n| !n.visual_scene && n.id == Some(*id));
            if let Some(child) = library.and_then(|l| self.node(l, depth + 1)) {
                children.push(child);
            }
        }
        Some(crate::Node {
            name: node.name.map(str::to_string),
            id: node.id.map(str::to_string),
            transform: node.transform.rows(),
            primitives,
            children,
        })
    }

    /// The mesh of primitive `index` of `geometry`, built once; `None` if
    /// it has no triangles.
    fn mesh(&mut self, geometry: &Geometry<'a>, index: usize) -> Option<usize> {
        if let Some(&m) = self.mesh_index.get(&(geometry.id, index)) {
            return Some(m);
        }
        let mesh = build_mesh(self.doc, geometry, index);
        if mesh.positions.is_empty() {
            return None;
        }
        self.meshes.push(mesh);
        let m = self.meshes.len() - 1;
        self.mesh_index.insert((geometry.id, index), m);
        Some(m)
    }

    fn material(&mut self, id: &'a str) -> Option<usize> {
        if let Some(&m) = self.material_index.get(id) {
            return Some(m);
        }
        let doc = self.doc;
        let material = doc.library_materials.materials.get(id)?;
        let effect = doc.get(&material.instance_effect.url)?;
        self.materials
            .push(build_material(doc, material.name, effect));
        let m = self.materials.len() - 1;
        self.material_index.insert(id, m);
        Some(m)
    }
}

/// Triangle soup for one primitive. Faces with an index past the end of a
/// source are dropped; an attribute whose indices run out is dropped
/// entirely (mesh-loader panicked on both).
fn build_mesh(doc: &Document<'_>, geometry: &Geometry<'_>, index: usize) -> crate::Mesh {
    let prim = iter::Primitive {
        mesh: iter::Mesh { doc, xml: geometry },
        xml: &geometry.mesh.primitives[index],
    };
    let p: Vec<_> = prim.positions().collect();
    let n: Vec<_> = prim.normals().collect();
    let t: Vec<_> = prim.texcoords(0).collect();
    let c: Vec<_> = prim.colors().collect();
    let mut normal_faces = prim.normal_indices();
    let mut uv_faces = prim.texcoord_indices(0);
    let mut color_faces = prim.color_indices();
    let (mut use_n, mut use_t, mut use_c) = (!n.is_empty(), !t.is_empty(), !c.is_empty());

    let mut mesh = crate::Mesh::default();
    let mut normals = Vec::new();
    let mut texcoords = Vec::new();
    let mut colors = Vec::new();
    for face in prim.vertex_indices() {
        let nf = use_n.then(|| normal_faces.next()).flatten();
        let tf = use_t.then(|| uv_faces.next()).flatten();
        let cf = use_c.then(|| color_faces.next()).flatten();
        use_n &= nf.is_some();
        use_t &= tf.is_some();
        use_c &= cf.is_some();
        let Some(positions) = gather(&p, face) else {
            continue;
        };
        mesh.positions.extend(positions);
        // A bad attribute index drops that attribute, not the face.
        match nf.and_then(|f| gather(&n, f)) {
            Some(v) => normals.extend(v),
            None => use_n = false,
        }
        match tf.and_then(|f| gather(&t, f)) {
            Some(v) => texcoords.extend(v),
            None => use_t = false,
        }
        match cf.and_then(|f| gather(&c, f)) {
            Some(v) => colors.extend(v.map(|[r, g, b]| [r, g, b, 1.0])),
            None => use_c = false,
        }
    }
    if use_n {
        mesh.normals = normals;
    }
    if use_t {
        mesh.texcoords = texcoords;
    }
    if use_c {
        mesh.colors = colors;
    }
    mesh
}

/// The three values `face` indexes, or `None` if any index is out of range.
fn gather<T: Copy>(values: &[T], face: [u32; 3]) -> Option<[T; 3]> {
    let get = |i: u32| values.get(i as usize).copied();
    Some([get(face[0])?, get(face[1])?, get(face[2])?])
}

fn build_material(doc: &Document<'_>, name: Option<&str>, effect: &Effect<'_>) -> crate::Material {
    let t = &effect.profile.technique;
    let texture = |tex: &Texture<'_>| texture_path(doc, effect, tex);

    // Refs: https://github.com/assimp/assimp/blob/v5.3.1/code/AssetLib/Collada/ColladaLoader.cpp#L1588
    let mut opacity = 1.0;
    let mut transparency = t.transparency;
    if (0.0..=1.0).contains(&transparency) {
        let tc = t.transparent.color;
        if t.rgb_transparency {
            transparency *= 0.212_671 * tc[0] + 0.715_160 * tc[1] + 0.072_169 * tc[2];
        } else {
            transparency *= tc[3];
        }
        if t.invert_transparency {
            transparency = 1.0 - transparency;
        }
        if t.has_transparency || transparency < 1.0 {
            opacity = transparency;
        }
    }
    // SketchUp writes `<transparency>0` with `opaque="A_ONE"` for opaque
    // materials; nobody means fully invisible.
    if opacity <= 0.0 {
        opacity = 1.0;
    }

    crate::Material {
        name: name.map(str::to_string),
        shading: match t.ty {
            ShadeType::Constant => crate::Shading::Constant,
            ShadeType::Lambert => crate::Shading::Lambert,
            ShadeType::Phong => crate::Shading::Phong,
            ShadeType::Blinn => crate::Shading::Blinn,
        },
        emission: t.emission.color,
        ambient: t.ambient.color,
        diffuse: t.diffuse.color,
        specular: t.specular.color,
        shininess: t.shininess,
        opacity,
        double_sided: t.double_sided,
        diffuse_texture: texture(&t.diffuse.texture),
        normal_texture: texture(&t.bump),
        emission_texture: texture(&t.emission.texture),
        specular_texture: texture(&t.specular.texture),
    }
}

/// The image a texture slot refers to (through a sampler and surface, or
/// directly by image id), as a cleaned-up path.
fn texture_path(doc: &Document<'_>, effect: &Effect<'_>, texture: &Texture<'_>) -> Option<String> {
    if texture.texture.is_empty() {
        return None;
    }
    let via_sampler = effect
        .profile
        .samplers
        .get(texture.texture)
        .and_then(|s| effect.profile.surfaces.get(s.source))
        .and_then(|surface| doc.get(&surface.init_from));
    let image = via_sampler.or_else(|| doc.library_images.images.get(texture.texture))?;
    match &image.source {
        ImageSource::InitFrom(path) => clean_path(path),
        // Embedded image data isn't supported.
        _ => None,
    }
}

/// `file://` removed, backslashes as slashes, and `/../x` as `../x`; `None`
/// when empty (some exporters write `<init_from/>`).
fn clean_path(path: &str) -> Option<String> {
    let path = path
        .strip_prefix("file://")
        .unwrap_or(path)
        .replace('\\', "/");
    let path = if path.starts_with("/..") {
        path[1..].to_string()
    } else {
        path
    };
    (!path.is_empty()).then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gather_checks_every_index() {
        let v = [10, 20, 30];
        assert_eq!(gather(&v, [2, 0, 1]), Some([30, 10, 20]));
        assert_eq!(gather(&v, [0, 1, 3]), None);
    }

    #[test]
    fn paths_are_cleaned() {
        assert_eq!(
            clean_path("file://textures/a.png").as_deref(),
            Some("textures/a.png")
        );
        assert_eq!(
            clean_path("..\\textures\\a.png").as_deref(),
            Some("../textures/a.png")
        );
        assert_eq!(
            clean_path("/../textures/a.png").as_deref(),
            Some("../textures/a.png")
        );
        assert_eq!(clean_path(""), None);
    }
}
