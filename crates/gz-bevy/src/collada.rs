//! A Bevy `AssetLoader` for COLLADA (`.dae`): [`gz_collada`]'s node tree
//! becomes a [`Scene`], like the glTF loader's.
//!
//! - The root entity scales by the document's `<unit>`; `<up_axis>` is
//!   ignored, as in Gazebo and gzweb.
//! - Every COLLADA node is an entity with its `Name` and local `Transform`,
//!   so `<submesh>` can find it; each primitive is a child with its mesh and
//!   material.
//! - Textures load through the same sources as the `.dae` (`gzhttps://`,
//!   `wsasset://`), relative to it.
//!
//! Materials are COLLADA's Phong/Blinn/Lambert effects mapped onto Bevy's
//! PBR `StandardMaterial` (see [`material`]).

use bevy::asset::io::Reader;
use bevy::asset::{AssetLoader, AssetPath, LoadContext, RenderAssetUsages};
use bevy::image::ImageLoaderSettings;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::Face;

use crate::assets;
use crate::render::units;

#[derive(Default, TypePath)]
pub struct ColladaLoader;

/// Why a `.dae` failed to load.
#[derive(Debug)]
pub enum ColladaError {
    Io(std::io::Error),
    Parse(gz_collada::Error),
}

impl std::fmt::Display for ColladaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ColladaError::Io(e) => write!(f, "reading the file: {e}"),
            ColladaError::Parse(e) => write!(f, "invalid COLLADA: {e}"),
        }
    }
}

impl std::error::Error for ColladaError {}

impl AssetLoader for ColladaLoader {
    type Asset = Scene;
    type Settings = ();
    type Error = ColladaError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        load_context: &mut LoadContext<'_>,
    ) -> Result<Scene, ColladaError> {
        let mut bytes = Vec::new();
        reader
            .read_to_end(&mut bytes)
            .await
            .map_err(ColladaError::Io)?;
        let dae = gz_collada::parse(&bytes).map_err(ColladaError::Parse)?;
        Ok(build_scene(&dae, load_context))
    }

    fn extensions(&self) -> &[&str] {
        &["dae"]
    }
}

fn build_scene(dae: &gz_collada::Scene, cx: &mut LoadContext<'_>) -> Scene {
    // A primitive with a normal map needs tangents on its mesh.
    let mut needs_tangents = vec![false; dae.meshes.len()];
    for (_, p) in dae.instances() {
        if p.material
            .is_some_and(|m| dae.materials[m].normal_texture.is_some())
        {
            needs_tangents[p.mesh] = true;
        }
    }
    let meshes: Vec<Handle<Mesh>> = dae
        .meshes
        .iter()
        .zip(&needs_tangents)
        .enumerate()
        .map(|(i, (m, &tangents))| cx.add_labeled_asset(format!("Mesh{i}"), mesh(m, tangents)))
        .collect();
    let dae_path = cx.path().clone_owned();
    let materials: Vec<Handle<StandardMaterial>> = dae
        .materials
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let mut mat = material(m);
            let texture = |cx: &mut LoadContext<'_>, tex: &Option<String>, srgb: bool| {
                let path = texture_path(&dae_path, tex.as_deref()?)?;
                Some(
                    cx.loader()
                        .with_settings(move |s: &mut ImageLoaderSettings| s.is_srgb = srgb)
                        .load(path),
                )
            };
            mat.base_color_texture = texture(cx, &m.diffuse_texture, true);
            mat.normal_map_texture = texture(cx, &m.normal_texture, false);
            mat.emissive_texture = texture(cx, &m.emission_texture, true);
            cx.add_labeled_asset(format!("Material{i}"), mat)
        })
        .collect();
    // Primitives without a material get COLLADA's default grey.
    let fallback = cx.add_labeled_asset(
        "MaterialDefault".into(),
        material(&gz_collada::Material::default()),
    );

    let mut world = World::new();
    let root = world
        .spawn((
            Name::new("collada"),
            Transform::from_scale(Vec3::splat(dae.unit)),
            Visibility::default(),
        ))
        .id();
    for node in &dae.roots {
        spawn_node(&mut world, node, root, &meshes, &materials, &fallback);
    }
    Scene::new(world)
}

fn spawn_node(
    world: &mut World,
    node: &gz_collada::Node,
    parent: Entity,
    meshes: &[Handle<Mesh>],
    materials: &[Handle<StandardMaterial>],
    fallback: &Handle<StandardMaterial>,
) {
    let name = node
        .name
        .clone()
        .or_else(|| node.id.clone())
        .unwrap_or_default();
    let entity = world
        .spawn((
            Name::new(name),
            transform(&node.transform),
            Visibility::default(),
            ChildOf(parent),
        ))
        .id();
    for p in &node.primitives {
        let material = p.material.map_or(fallback, |m| &materials[m]);
        world.spawn((
            Mesh3d(meshes[p.mesh].clone()),
            MeshMaterial3d(material.clone()),
            Transform::IDENTITY,
            ChildOf(entity),
        ));
    }
    for child in &node.children {
        spawn_node(world, child, entity, meshes, materials, fallback);
    }
}

/// A row-major COLLADA matrix as a Bevy transform (skew is dropped).
pub fn transform(rows: &[[f32; 4]; 4]) -> Transform {
    let m = Mat4::from_cols_array_2d(rows).transpose();
    Transform::from_matrix(m)
}

/// Unindexed triangles as a Bevy mesh. COLLADA's V points up, Bevy's down.
pub fn mesh(m: &gz_collada::Mesh, tangents: bool) -> Mesh {
    let n = m.positions.len();
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, m.positions.clone())
    .with_inserted_indices(Indices::U32((0..n as u32).collect()));
    if m.normals.len() == n {
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, m.normals.clone());
    } else {
        mesh.duplicate_vertices();
        mesh.compute_flat_normals();
    }
    if m.texcoords.len() == n {
        let uvs: Vec<[f32; 2]> = m.texcoords.iter().map(|&[u, v]| [u, 1.0 - v]).collect();
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    }
    if m.colors.len() == n {
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, m.colors.clone());
    }
    if tangents && mesh.contains_attribute(Mesh::ATTRIBUTE_UV_0) {
        if let Err(e) = mesh.generate_tangents() {
            warn!("COLLADA mesh: no tangents for its normal map: {e}");
        }
    }
    mesh
}

/// A COLLADA effect as a PBR material (textures are added by the loader).
///
/// - Base colour is the diffuse colour, or white under a diffuse texture
///   (COLLADA's default grey would darken it); alpha is diffuse alpha ×
///   opacity, blended below 1.
/// - Roughness follows the Phong exponent (`sqrt(2 / (n + 2))`, the usual
///   Blinn-Phong to GGX mapping); nothing is metallic.
/// - Emission is scaled so 1 glows like a white surface in full sun.
/// - `constant` shading is unlit.
pub fn material(m: &gz_collada::Material) -> StandardMaterial {
    let [r, g, b, a] = if m.diffuse_texture.is_some() {
        [1.0, 1.0, 1.0, m.diffuse[3]]
    } else {
        m.diffuse
    };
    let alpha = (a * m.opacity).clamp(0.0, 1.0);
    let [er, eg, eb, _] = m.emission;
    StandardMaterial {
        base_color: Color::srgba(r, g, b, alpha),
        alpha_mode: if alpha < 1.0 {
            AlphaMode::Blend
        } else {
            AlphaMode::Opaque
        },
        perceptual_roughness: (2.0 / (m.shininess.max(0.0) + 2.0))
            .sqrt()
            .clamp(0.089, 1.0),
        metallic: 0.0,
        emissive: LinearRgba::from(Color::srgb(er, eg, eb)) * units::UNIT_LUMINANCE,
        unlit: m.shading == gz_collada::Shading::Constant,
        double_sided: m.double_sided,
        cull_mode: (!m.double_sided).then_some(Face::Back),
        ..default()
    }
}

/// Where a texture the `.dae` names lives: a URI or absolute path through
/// the usual sources, else relative to the `.dae` itself.
pub fn texture_path(dae: &AssetPath<'_>, texture: &str) -> Option<AssetPath<'static>> {
    let absolute = texture.contains("://") || texture.starts_with('/');
    if absolute {
        let load_uri = gz_core::uri::mesh_load_uri(texture);
        return AssetPath::try_parse(&assets::asset_path(&load_uri))
            .ok()
            .map(AssetPath::into_owned);
    }
    dae.resolve_embed(texture).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dae(name: &str) -> gz_collada::Scene {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../gz-collada/tests/data")
            .join(name);
        gz_collada::parse(&std::fs::read(path).unwrap()).unwrap()
    }

    #[test]
    fn transforms_from_rows() {
        let rows = [
            [0.0, -1.0, 0.0, 1.0],
            [1.0, 0.0, 0.0, 2.0],
            [0.0, 0.0, 1.0, 3.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let t = transform(&rows);
        assert_eq!(t.translation, Vec3::new(1.0, 2.0, 3.0));
        // 90° about Z: X goes to Y.
        assert!((t.rotation * Vec3::X - Vec3::Y).length() < 1e-6);
    }

    #[test]
    fn meshes_flip_v_and_fill_in_normals() {
        let soup = gz_collada::Mesh {
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            texcoords: vec![[0.0, 0.25], [1.0, 0.0], [0.0, 1.0]],
            colors: vec![[1.0; 4]; 3],
            ..default()
        };
        let m = mesh(&soup, true);
        assert!(m.contains_attribute(Mesh::ATTRIBUTE_NORMAL));
        assert!(m.contains_attribute(Mesh::ATTRIBUTE_TANGENT));
        assert!(m.contains_attribute(Mesh::ATTRIBUTE_COLOR));
        let Some(bevy::mesh::VertexAttributeValues::Float32x2(uv)) =
            m.attribute(Mesh::ATTRIBUTE_UV_0)
        else {
            panic!("no UVs");
        };
        assert_eq!(uv[0], [0.0, 0.75]);
        // No UVs: no tangents, and no failure.
        let bare = gz_collada::Mesh {
            positions: soup.positions,
            normals: vec![[0.0, 0.0, 1.0]; 3],
            ..default()
        };
        assert!(!mesh(&bare, true).contains_attribute(Mesh::ATTRIBUTE_TANGENT));
    }

    #[test]
    fn effects_become_pbr_materials() {
        let probe = dae("probe.dae");
        let red = material(&probe.materials[0]);
        assert_eq!(red.base_color, Color::srgba(1.0, 0.0, 0.0, 1.0));
        assert_eq!(red.alpha_mode, AlphaMode::Opaque);
        assert_eq!(red.cull_mode, Some(Face::Back));
        let textured = material(&probe.materials[1]);
        // White under the texture, half transparent.
        assert_eq!(textured.base_color, Color::srgba(1.0, 1.0, 1.0, 0.5));
        assert_eq!(textured.alpha_mode, AlphaMode::Blend);
        let shiny = (2.0f32 / 22.0).sqrt();
        assert!((textured.perceptual_roughness - shiny).abs() < 1e-6);
        let unlit = material(&gz_collada::Material {
            shading: gz_collada::Shading::Constant,
            double_sided: true,
            ..default()
        });
        assert!(unlit.unlit && unlit.double_sided && unlit.cull_mode.is_none());
    }

    #[test]
    fn textures_resolve_relative_to_the_dae() {
        let dae = AssetPath::parse("gzhttps://fuel.example/m/1/files/meshes/a.dae");
        assert_eq!(
            texture_path(&dae, "../materials/textures/a.png").unwrap(),
            AssetPath::parse("gzhttps://fuel.example/m/1/files/materials/textures/a.png")
        );
        assert_eq!(
            texture_path(&dae, "https://fuel.example/x.png").unwrap(),
            AssetPath::parse("gzhttps://fuel.example/x.png")
        );
        let ws = texture_path(&dae, "/abs/x.png").unwrap();
        assert_eq!(ws, AssetPath::parse("wsasset://abs/abs/x.png"));
    }
}
