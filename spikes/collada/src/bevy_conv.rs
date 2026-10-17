//! Prototype: mesh-loader `Scene` -> Bevy 0.18 `Mesh` + `StandardMaterial`.
//!
//! mesh-loader gives one triangle-soup `Mesh` per `<geometry>` (all of its
//! primitives merged) and exactly one `Material` per mesh (the first bound
//! primitive's), so here "one Bevy mesh per primitive/material" degenerates to
//! one per geometry. See REPORT.md for what a production loader needs instead.

use bevy_asset::{Asset, AssetLoader, Handle, LoadContext, RenderAssetUsages, io::Reader};
use bevy_color::{Color, LinearRgba};
use bevy_image::Image;
use bevy_mesh::{Indices, Mesh, PrimitiveTopology};
use bevy_pbr::StandardMaterial;
use bevy_reflect::TypePath;
use bevy_render::alpha::AlphaMode;

/// Renderer-independent material description (texture paths unresolved).
#[derive(Debug, Clone)]
pub struct MaterialDesc {
    pub name: String,
    /// Collada colours are used as-is by gzweb (three r141, linear output,
    /// no colour management) which looks the same as treating them as sRGB
    /// in Bevy's linear pipeline + sRGB output.
    pub base_color: Color,
    pub base_color_texture: Option<String>,
    pub normal_map_texture: Option<String>,
    pub emissive: LinearRgba,
    pub emissive_texture: Option<String>,
    pub perceptual_roughness: f32,
    pub alpha_mode: AlphaMode,
}

#[derive(Debug)]
pub struct ConvertedPrimitive {
    /// Geometry id (mesh-loader exposes nothing else; gz `<submesh>` wants
    /// the node name — see REPORT.md).
    pub name: String,
    pub mesh: Mesh,
    pub material: MaterialDesc,
}

fn tex(p: &Option<std::path::PathBuf>) -> Option<String> {
    p.as_ref().map(|p| p.to_string_lossy().replace('\\', "/"))
}

pub fn material_desc(m: &mesh_loader::Material) -> MaterialDesc {
    // mesh-loader reports assimp's default grey (0.6) diffuse when the
    // diffuse slot is a texture; three.js (gzweb) uses white so the texture
    // is not darkened. Match gzweb.
    let d = if m.texture.diffuse.is_some() {
        [1.0, 1.0, 1.0, 1.0]
    } else {
        m.color.diffuse.unwrap_or([0.8, 0.8, 0.8, 1.0])
    };
    // SketchUp-style exports (Coke Can, Cordless Drill) write
    // <transparent>0 0 0 1</transparent><transparency>0</transparency>, i.e.
    // opacity 0 if taken literally (both mesh-loader and three do). Treat a
    // fully transparent material as opaque, as Gazebo users expect.
    let opacity = match m.opacity.unwrap_or(1.0).clamp(0.0, 1.0) {
        o if o <= 0.0 => 1.0,
        o => o,
    };
    let alpha = d[3] * opacity;
    let e = m.color.emissive.unwrap_or([0.0, 0.0, 0.0, 1.0]);
    // Blinn-Phong exponent -> roughness (common approximation).
    let roughness = m
        .shininess
        .filter(|s| *s > 0.0)
        .map(|s| (2.0 / (s + 2.0)).sqrt())
        .unwrap_or(0.9);
    MaterialDesc {
        name: m.name.clone(),
        base_color: Color::srgba(d[0], d[1], d[2], alpha),
        base_color_texture: tex(&m.texture.diffuse),
        normal_map_texture: tex(&m.texture.normal),
        emissive: LinearRgba::rgb(e[0], e[1], e[2]),
        emissive_texture: tex(&m.texture.emissive),
        perceptual_roughness: roughness.clamp(0.089, 1.0),
        alpha_mode: if alpha < 1.0 { AlphaMode::Blend } else { AlphaMode::Opaque },
    }
}

impl MaterialDesc {
    /// Build a `StandardMaterial`; `load` turns a Collada texture path into a
    /// handle (e.g. `LoadContext::load` after resolving relative to the .dae).
    pub fn to_standard(&self, mut load: impl FnMut(&str) -> Handle<Image>) -> StandardMaterial {
        StandardMaterial {
            base_color: self.base_color,
            base_color_texture: self.base_color_texture.as_deref().map(&mut load),
            normal_map_texture: self.normal_map_texture.as_deref().map(&mut load),
            emissive: self.emissive,
            emissive_texture: self.emissive_texture.as_deref().map(&mut load),
            perceptual_roughness: self.perceptual_roughness,
            metallic: 0.0,
            alpha_mode: self.alpha_mode,
            // gzweb renders Collada single-sided (FrontSide) unless told otherwise.
            ..Default::default()
        }
    }
}

/// mesh-loader mesh -> Bevy mesh. Returns `None` for empty meshes.
pub fn mesh_to_bevy(m: &mesh_loader::Mesh) -> Option<Mesh> {
    let n = m.vertices.len();
    if n == 0 || m.faces.is_empty() {
        return None;
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, m.vertices.clone());
    // mesh-loader emits per-corner attributes, but if some primitives of a
    // geometry lack normals/uvs the arrays are shorter than `vertices`; Bevy
    // requires equal lengths, so drop (normals: recompute) in that case.
    let has_normals = m.normals.len() == n;
    if has_normals {
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, m.normals.clone());
    }
    if m.texcoords[0].len() == n {
        // Collada V points up (OpenGL); Bevy/wgpu images have V pointing down.
        let uv: Vec<[f32; 2]> = m.texcoords[0].iter().map(|[u, v]| [*u, 1.0 - *v]).collect();
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    }
    if m.colors[0].len() == n {
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, m.colors[0].clone());
    }
    let idx: Vec<u32> = m.faces.iter().flatten().copied().collect();
    mesh.insert_indices(Indices::U32(idx));
    if !has_normals {
        mesh.compute_normals();
    }
    Some(mesh)
}

pub fn convert_scene(scene: &mesh_loader::Scene) -> Vec<ConvertedPrimitive> {
    scene
        .meshes
        .iter()
        .zip(&scene.materials)
        .filter_map(|(m, mat)| {
            Some(ConvertedPrimitive {
                name: m.name.clone(),
                mesh: mesh_to_bevy(m)?,
                material: material_desc(mat),
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// AssetLoader sketch (compiles; exercised only in tests via convert_scene).
// A production version should emit a `bevy_scene::Scene` with one entity per
// Collada node (Name + Transform) so gz `<submesh>` selection can work.

/// One entry per converted primitive; parallel vectors so the derive can
/// track handle dependencies.
#[derive(Asset, TypePath, Debug)]
pub struct ColladaAsset {
    pub names: Vec<String>,
    #[dependency]
    pub meshes: Vec<Handle<Mesh>>,
    #[dependency]
    pub materials: Vec<Handle<StandardMaterial>>,
}

#[derive(Default, TypePath)]
pub struct ColladaLoader;

impl AssetLoader for ColladaLoader {
    type Asset = ColladaAsset;
    type Settings = ();
    type Error = std::io::Error;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        cx: &mut LoadContext<'_>,
    ) -> Result<ColladaAsset, std::io::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let scene = mesh_loader::collada::from_slice(&bytes)?;
        let base = cx.path().clone();
        let mut out = ColladaAsset { names: vec![], meshes: vec![], materials: vec![] };
        for (i, p) in convert_scene(&scene).into_iter().enumerate() {
            let mesh = cx.add_labeled_asset(format!("Mesh{i}"), p.mesh);
            // Gazebo meshes reference textures as "../materials/textures/x.png"
            // relative to the .dae (gz-common also searches a few fallbacks).
            let mat = p.material.to_standard(|t| {
                let t = t.strip_prefix("file://").unwrap_or(t);
                match base.resolve_embed(t) {
                    Ok(path) => cx.load(path),
                    Err(_) => Handle::default(),
                }
            });
            let material = cx.add_labeled_asset(format!("Material{i}"), mat);
            out.names.push(p.name);
            out.meshes.push(mesh);
            out.materials.push(material);
        }
        Ok(out)
    }

    fn extensions(&self) -> &[&str] {
        &["dae"]
    }
}
