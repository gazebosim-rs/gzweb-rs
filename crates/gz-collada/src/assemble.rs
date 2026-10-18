//! Turns a parsed COLLADA document into [`crate::Scene`]: the node tree of
//! the instantiated visual scene, one mesh per primitive and the materials
//! each instance binds. Replaces mesh-loader's `instance.rs`, which
//! multiplied transforms child-first, post-multiplied the unit, merged a
//! geometry's primitives under one material and emitted geometries rather
//! than instances.
//!
//! As in three.js, a reference the scene follows (geometry, material,
//! effect, library node) must resolve, and only what the scene uses is
//! built and validated; a missing image just drops the texture.

use std::collections::HashMap;

use crate::validate;
use crate::vendor::collada::{
    iter, Document, Effect, Geometry, ImageSource, Node, ShadeType, Texture,
};

/// Nodes plus primitives one document may expand to. `<instance_node>`s
/// that each instance the next one twice grow exponentially.
const BUDGET: usize = 1_000_000;

/// Deepest node chain, counting `<instance_node>` expansions (the assembly
/// recurses). Literal nesting is capped lower, by the parser.
const MAX_DEPTH: usize = 1024;

struct Builder<'d, 'a> {
    doc: &'d Document<'a>,
    meshes: Vec<crate::Mesh>,
    materials: Vec<crate::Material>,
    /// (geometry id, primitive index) → mesh index, or `None` if it has no
    /// triangles.
    mesh_index: HashMap<(&'a str, usize), Option<usize>>,
    /// material id → material index.
    material_index: HashMap<&'a str, usize>,
    /// Node id → index, for `<instance_node>` (the first node with the id).
    library: HashMap<&'a str, usize>,
    /// Indices of the nodes being built, root first.
    path: Vec<usize>,
    /// Nodes and primitives emitted so far.
    emitted: usize,
    /// `<instance_controller>`s in the instantiated nodes.
    controllers: usize,
    /// `<instance_node>`s skipped because they instance an ancestor.
    cycles: usize,
}

pub(crate) fn assemble(doc: &Document<'_>) -> Result<crate::Scene, String> {
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
    let mut library = HashMap::new();
    for (i, node) in nodes.iter().enumerate() {
        if let (false, Some(id)) = (node.visual_scene, node.id) {
            library.entry(id).or_insert(i);
        }
    }
    let mut b = Builder {
        doc,
        meshes: Vec::new(),
        materials: Vec::new(),
        mesh_index: HashMap::new(),
        material_index: HashMap::new(),
        library,
        path: Vec::new(),
        emitted: 0,
        controllers: 0,
        cycles: 0,
    };
    let roots = match scene {
        Some(i) => nodes[i]
            .children
            .iter()
            .map(|&c| b.node(c))
            .collect::<Result<_, _>>()?,
        None => Vec::new(),
    };
    let mut warnings = Vec::new();
    if b.controllers > 0 {
        warnings.push(format!(
            "{} <instance_controller>s (skinned meshes) are not supported and were skipped",
            b.controllers
        ));
    }
    if b.cycles > 0 {
        warnings.push(format!(
            "{} <instance_node>s instance their own ancestor and were skipped",
            b.cycles
        ));
    }
    Ok(crate::Scene {
        unit: doc.asset.unit,
        up_axis: doc.asset.up_axis,
        roots,
        meshes: b.meshes,
        materials: b.materials,
        warnings,
    })
}

impl<'a> Builder<'_, 'a> {
    fn node(&mut self, index: usize) -> Result<crate::Node, String> {
        if self.path.len() >= MAX_DEPTH {
            return Err(format!(
                "nodes nest more than {MAX_DEPTH} deep through <instance_node>"
            ));
        }
        self.path.push(index);
        let node = self.build_node(index);
        self.path.pop();
        node
    }

    fn build_node(&mut self, index: usize) -> Result<crate::Node, String> {
        let doc = self.doc;
        let node: &Node<'a> = &doc.library_visual_scenes.nodes[index];
        self.spend(1)?;
        self.controllers += node.instance_controllers;
        let mut primitives = Vec::new();
        for instance in &node.instance_geometry {
            let url = instance.url.as_str();
            let geometry = doc
                .get(&instance.url)
                .ok_or_else(|| format!("<instance_geometry>: no geometry #{url}"))?;
            for (i, prim) in geometry.mesh.primitives.iter().enumerate() {
                let Some(mesh) = self.mesh(geometry, i)? else {
                    continue;
                };
                // The material this instance binds to the primitive's symbol;
                // an unbound symbol gets the default material.
                let binding = prim
                    .material
                    .and_then(|symbol| instance.materials.get(symbol));
                let material = match binding {
                    Some(binding) => Some(self.material(binding.target.as_str())?),
                    None => None,
                };
                self.spend(1)?;
                primitives.push(crate::Primitive { mesh, material });
            }
        }
        let mut children = Vec::with_capacity(node.children.len() + node.instance_node.len());
        for &child in &node.children {
            children.push(self.node(child)?);
        }
        for id in &node.instance_node {
            let library = *self
                .library
                .get(id)
                .ok_or_else(|| format!("<instance_node>: no node #{id}"))?;
            if self.path.contains(&library) {
                self.cycles += 1;
                continue;
            }
            children.push(self.node(library)?);
        }
        Ok(crate::Node {
            name: node.name.map(str::to_string),
            id: node.id.map(str::to_string),
            transform: node.transform.rows(),
            primitives,
            children,
        })
    }

    fn spend(&mut self, n: usize) -> Result<(), String> {
        self.emitted += n;
        if self.emitted > BUDGET {
            return Err(format!(
                "the scene expands to more than {BUDGET} nodes and primitives"
            ));
        }
        Ok(())
    }

    /// The mesh of primitive `index` of `geometry`, validated and built
    /// once; `None` if it has no triangles.
    fn mesh(&mut self, geometry: &Geometry<'a>, index: usize) -> Result<Option<usize>, String> {
        if let Some(&m) = self.mesh_index.get(&(geometry.id, index)) {
            return Ok(m);
        }
        validate::primitive(self.doc, geometry, index)?;
        let mesh = build_mesh(self.doc, geometry, index);
        let m = (!mesh.positions.is_empty()).then(|| {
            self.meshes.push(mesh);
            self.meshes.len() - 1
        });
        self.mesh_index.insert((geometry.id, index), m);
        Ok(m)
    }

    fn material(&mut self, id: &'a str) -> Result<usize, String> {
        if let Some(&m) = self.material_index.get(id) {
            return Ok(m);
        }
        let doc = self.doc;
        let material = doc
            .library_materials
            .materials
            .get(id)
            .ok_or_else(|| format!("<instance_material>: no material #{id}"))?;
        let effect = doc.get(&material.instance_effect.url).ok_or_else(|| {
            format!(
                "material #{id}: no effect #{}",
                material.instance_effect.url.as_str()
            )
        })?;
        self.materials
            .push(build_material(doc, material.name, effect));
        let m = self.materials.len() - 1;
        self.material_index.insert(id, m);
        Ok(m)
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
    let texture = |tex: &Texture<'_>| texture(doc, effect, tex);

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
        transparent_texture: texture(&t.transparent.texture),
    }
}

/// The image a texture slot refers to (through a sampler and surface, or
/// directly by image id), as a cleaned-up path, with its wrapping.
fn texture(
    doc: &Document<'_>,
    effect: &Effect<'_>,
    texture: &Texture<'_>,
) -> Option<crate::Texture> {
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
    let path = match &image.source {
        ImageSource::InitFrom(path) => clean_path(path)?,
        // Embedded image data isn't supported.
        _ => return None,
    };
    Some(crate::Texture {
        path,
        repeat: texture.wrap.unwrap_or([true; 2]),
    })
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
