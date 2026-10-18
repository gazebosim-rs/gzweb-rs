//! A Bevy `AssetLoader` for Wavefront OBJ with MTL materials, parsed by
//! `tobj`, built like the COLLADA loader's [`Scene`]:
//!
//! - Every `o`/`g` object is an entity with its `Name` (for `<submesh>`),
//!   and a child per material it uses.
//! - `map_Kd`/`map_Ka` paths are rewritten the way gzweb's `GzObjLoader`
//!   does, so textures are found where Gazebo models keep them
//!   ([`texture_path`]).
//! - A missing or broken `.mtl` leaves the default material (gzweb shows
//!   nothing at all).
//!
//! Materials map onto PBR like [`crate::collada::material`]: `Kd` × `map_Kd`,
//! alpha from `d` (or `1 - Tr`), roughness from `Ns` unless `Pr` is given,
//! `Pm` metallic, `Ke`/`map_Ke` emission, `map_Bump`/`bump`/`norm` as a
//! normal map.

use std::collections::HashMap;
use std::io::BufReader;

use bevy::asset::io::Reader;
use bevy::asset::{AssetLoader, AssetPath, LoadContext, RenderAssetUsages};
use bevy::image::ImageLoaderSettings;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::Face;

use crate::render::units;

#[derive(Default, TypePath)]
pub struct ObjLoader;

/// Why an `.obj` failed to load.
#[derive(Debug)]
pub enum ObjError {
    Io(std::io::Error),
    Parse(tobj::LoadError),
}

impl std::fmt::Display for ObjError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ObjError::Io(e) => write!(f, "reading the file: {e}"),
            ObjError::Parse(e) => write!(f, "invalid OBJ: {e}"),
        }
    }
}

impl std::error::Error for ObjError {}

const OPTIONS: tobj::LoadOptions = tobj::LoadOptions {
    single_index: true,
    triangulate: true,
    ignore_points: true,
    ignore_lines: true,
};

impl AssetLoader for ObjLoader {
    type Asset = WorldAsset;
    type Settings = ();
    type Error = ObjError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        cx: &mut LoadContext<'_>,
    ) -> Result<WorldAsset, ObjError> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await.map_err(ObjError::Io)?;
        let obj_path = cx.path().clone_owned();

        // tobj asks for material libraries synchronously, so read them first.
        let mut libraries = HashMap::new();
        for name in mtllibs(&bytes) {
            let Ok(path) = obj_path.resolve_embed_str(&name) else {
                continue;
            };
            match cx.read_asset_bytes(path.clone()).await {
                Ok(text) => {
                    libraries.insert(name, text);
                }
                Err(e) => warn!("OBJ '{obj_path}': material library '{path}': {e}"),
            }
        }
        let (models, materials) =
            tobj::load_obj_buf(&mut BufReader::new(bytes.as_slice()), &OPTIONS, |p| {
                let name = p.to_string_lossy();
                let text = libraries
                    .get(name.as_ref())
                    .ok_or(tobj::LoadError::OpenFileFailed)?;
                tobj::load_mtl_buf(&mut BufReader::new(text.as_slice()))
            })
            .map_err(ObjError::Parse)?;
        let materials = materials.unwrap_or_else(|e| {
            warn!("OBJ '{obj_path}': materials: {e}");
            Vec::new()
        });
        Ok(build_scene(&models, &materials, cx))
    }

    fn extensions(&self) -> &[&str] {
        &["obj"]
    }
}

/// The `mtllib` names in an OBJ file, in order (one per line; names may
/// contain spaces).
fn mtllibs(obj: &[u8]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in String::from_utf8_lossy(obj).lines() {
        if let Some(name) = line.trim().strip_prefix("mtllib ") {
            let name = name.trim().to_string();
            if !name.is_empty() && !out.contains(&name) {
                out.push(name);
            }
        }
    }
    out
}

fn build_scene(
    models: &[tobj::Model],
    mtls: &[tobj::Material],
    cx: &mut LoadContext<'_>,
) -> WorldAsset {
    let obj_path = cx.path().clone_owned();
    let materials: Vec<Handle<StandardMaterial>> = mtls
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let mut mat = material(m);
            let texture =
                |cx: &mut LoadContext<'_>, tex: Option<AssetPath<'static>>, srgb: bool| {
                    let path = tex?;
                    Some(
                        cx.load_builder()
                            .with_settings(move |s: &mut ImageLoaderSettings| s.is_srgb = srgb)
                            .load(path),
                    )
                };
            let diffuse = m
                .diffuse_texture
                .as_deref()
                .or(m.ambient_texture.as_deref());
            mat.base_color_texture = texture(
                cx,
                diffuse.and_then(|t| texture_path(&obj_path, t, true)),
                true,
            );
            mat.normal_map_texture = texture(
                cx,
                normal_map(m).and_then(|t| texture_path(&obj_path, t, false)),
                false,
            );
            mat.emissive_texture = texture(
                cx,
                m.unknown_param
                    .get("map_Ke")
                    .and_then(|t| texture_path(&obj_path, t, false)),
                true,
            );
            cx.add_labeled_asset(format!("Material{i}"), mat)
        })
        .collect();
    let fallback = cx.add_labeled_asset("MaterialDefault", material(&tobj::Material::default()));

    let mut world = World::new();
    let root = world
        .spawn((Name::new("obj"), Transform::IDENTITY, Visibility::default()))
        .id();
    // tobj splits an object at each `usemtl`; consecutive pieces with the
    // same name are one object, as in three.js.
    let mut node: Option<(&str, Entity)> = None;
    for (i, model) in models.iter().enumerate() {
        let name = match model.name.as_str() {
            "unnamed_object" => "",
            n => n,
        };
        let parent = match node {
            Some((n, e)) if n == name => e,
            _ => {
                let e = world
                    .spawn((
                        Name::new(name.to_string()),
                        Transform::IDENTITY,
                        Visibility::default(),
                        ChildOf(root),
                    ))
                    .id();
                node = Some((name, e));
                e
            }
        };
        let m = model.mesh.material_id.and_then(|id| materials.get(id));
        let tangents = model
            .mesh
            .material_id
            .and_then(|id| mtls.get(id))
            .is_some_and(|m| normal_map(m).is_some());
        let Some(bevy_mesh) = mesh(&model.mesh, tangents) else {
            continue;
        };
        let handle = cx.add_labeled_asset(format!("Mesh{i}"), bevy_mesh);
        world.spawn((
            Mesh3d(handle),
            MeshMaterial3d(m.unwrap_or(&fallback).clone()),
            Transform::IDENTITY,
            Visibility::default(),
            ChildOf(parent),
        ));
    }
    WorldAsset::new(world)
}

fn normal_map(m: &tobj::Material) -> Option<&str> {
    m.normal_texture
        .as_deref()
        .or_else(|| m.unknown_param.get("norm").map(String::as_str))
}

/// An OBJ mesh as a Bevy mesh with flat normals when it has none (three.js
/// computes them on unindexed triangles, so they are flat too). OBJ's V
/// points up, Bevy's down. `None` without triangles.
fn mesh(m: &tobj::Mesh, tangents: bool) -> Option<Mesh> {
    let positions: Vec<[f32; 3]> = m
        .positions
        .chunks_exact(3)
        .map(|p| [p[0], p[1], p[2]])
        .collect();
    if m.indices.is_empty() || positions.is_empty() {
        return None;
    }
    let n = positions.len();
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_indices(Indices::U32(m.indices.clone()));
    if m.texcoords.len() == 2 * n {
        let uvs: Vec<[f32; 2]> = m
            .texcoords
            .chunks_exact(2)
            .map(|t| [t[0], 1.0 - t[1]])
            .collect();
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    }
    if m.vertex_color.len() == 3 * n {
        let colors: Vec<[f32; 4]> = m
            .vertex_color
            .chunks_exact(3)
            .map(|c| [c[0], c[1], c[2], 1.0])
            .collect();
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    }
    if m.normals.len() == 3 * n {
        let normals: Vec<[f32; 3]> = m
            .normals
            .chunks_exact(3)
            .map(|v| [v[0], v[1], v[2]])
            .collect();
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    } else {
        mesh.duplicate_vertices();
        mesh.compute_flat_normals();
    }
    if tangents && mesh.contains_attribute(Mesh::ATTRIBUTE_UV_0) {
        if let Err(e) = mesh.generate_tangents() {
            warn!("OBJ mesh: no tangents for its normal map: {e}");
        }
    }
    Some(mesh)
}

/// An MTL material as PBR (textures are added by the loader).
pub fn material(m: &tobj::Material) -> StandardMaterial {
    let [r, g, b] = m.diffuse.unwrap_or([1.0; 3]);
    let param = |k: &str| {
        m.unknown_param
            .get(k)
            .and_then(|v| v.trim().parse::<f32>().ok())
    };
    let alpha = m
        .dissolve
        .or_else(|| param("Tr").map(|tr| 1.0 - tr))
        .unwrap_or(1.0)
        .clamp(0.0, 1.0);
    let roughness = param("Pr").unwrap_or_else(|| {
        let ns = m.shininess.unwrap_or(30.0).max(0.0);
        (2.0 / (ns + 2.0)).sqrt()
    });
    let [er, eg, eb] = m.emissive.unwrap_or_default();
    StandardMaterial {
        base_color: Color::srgba(r, g, b, alpha),
        alpha_mode: if alpha < 1.0 {
            AlphaMode::Blend
        } else {
            AlphaMode::Opaque
        },
        perceptual_roughness: roughness.clamp(0.089, 1.0),
        metallic: param("Pm").unwrap_or(0.0).clamp(0.0, 1.0),
        emissive: LinearRgba::from(Color::srgb(er, eg, eb)) * units::UNIT_LUMINANCE,
        cull_mode: Some(Face::Back),
        ..default()
    }
}

/// Where a texture an `.mtl` names lives. Options (`-bm 1`, `-s 1 1 1`, …)
/// are dropped. Diffuse/ambient maps follow gzweb's `GzObjLoader.loadMTL`:
///
/// - `model://other/x.png` is looked up beside this model's directory
///   (the `.obj` must be in a `meshes` directory);
/// - a path with `/materials/textures` in it is kept;
/// - anything else is a file in `<model>/materials/textures/`, `<model>`
///   being the `.obj`'s path up to `meshes`.
///
/// Other maps, and anything without those rules, are relative to the `.obj`.
pub fn texture_path(
    obj: &AssetPath<'_>,
    spec: &str,
    gzweb_rules: bool,
) -> Option<AssetPath<'static>> {
    let file = strip_options(spec);
    if file.is_empty() {
        return None;
    }
    let obj_str = obj.to_string();
    let dir = &obj_str[..obj_str.rfind('/').map_or(0, |i| i + 1)];
    let rewritten = if let Some(rest) = file.strip_prefix("model://") {
        // gzweb: the models directory is the one holding this model.
        let models = dir
            .rfind("/meshes")
            .map(|i| &dir[..i])
            .and_then(|m| m.rfind('/').map(|j| &m[..=j]));
        match models {
            Some(models) => format!("{models}{rest}"),
            None => rest.to_string(),
        }
    } else if gzweb_rules && file.find("/materials/textures").is_none_or(|i| i == 0) {
        let model = dir.rfind("meshes").map_or("", |i| &dir[..i]);
        format!("{model}materials/textures/{file}")
    } else {
        file.to_string()
    };
    if rewritten.contains("://") {
        AssetPath::try_parse(&rewritten)
            .ok()
            .map(AssetPath::into_owned)
    } else {
        obj.resolve_embed_str(&rewritten).ok()
    }
}

/// The file name in an MTL texture statement's arguments.
fn strip_options(spec: &str) -> &str {
    let mut rest = spec.trim();
    while rest.starts_with('-') {
        let (opt, after) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        let max_args = match opt {
            "-o" | "-s" | "-t" => 3,
            "-mm" => 2,
            _ => 1,
        };
        let mut after = after.trim_start();
        for i in 0..max_args {
            let (arg, next) = after.split_once(char::is_whitespace).unwrap_or((after, ""));
            // Optional numeric arguments (`-s u [v [w]]`) end at the first
            // non-number.
            if i > 0 && arg.parse::<f32>().is_err() {
                break;
            }
            after = next.trim_start();
        }
        rest = after;
    }
    rest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mtllib_names_keep_spaces() {
        let obj = b"# x\nmtllib a b.mtl\nmtllib c.mtl\nmtllib a b.mtl\nv 0 0 0\n";
        assert_eq!(mtllibs(obj), vec!["a b.mtl", "c.mtl"]);
    }

    #[test]
    fn texture_options_are_dropped() {
        assert_eq!(strip_options("tex.png"), "tex.png");
        assert_eq!(strip_options("-bm 0.5 my normal.png"), "my normal.png");
        assert_eq!(strip_options("-s 1 2 3 -o 0.5 t.png"), "t.png");
        assert_eq!(strip_options("-s 2 t.png"), "t.png");
        assert_eq!(strip_options("-clamp on -mm 0 1 t.png"), "t.png");
    }

    #[test]
    fn textures_follow_gzweb() {
        let fuel = AssetPath::parse("gzhttps://fuel.example/o/models/m/1/files/meshes/a.obj");
        let p = |spec, gz| texture_path(&fuel, spec, gz).unwrap().to_string();
        assert_eq!(
            p("a.png", true),
            "gzhttps://fuel.example/o/models/m/1/files/materials/textures/a.png"
        );
        assert_eq!(
            p("../materials/textures/a.png", true),
            "gzhttps://fuel.example/o/models/m/1/files/materials/textures/a.png"
        );
        // Not a diffuse map: relative to the .obj.
        assert_eq!(
            p("n.png", false),
            "gzhttps://fuel.example/o/models/m/1/files/meshes/n.png"
        );
        let local = AssetPath::parse("wsasset://abs/home/u/models/m/meshes/a.obj");
        assert_eq!(
            texture_path(&local, "model://other/materials/textures/b.png", true)
                .unwrap()
                .to_string(),
            "wsasset://abs/home/u/models/other/materials/textures/b.png"
        );
        // No meshes directory: materials/textures beside the .obj.
        let flat = AssetPath::parse("wsasset://rel/pkg/a.obj");
        assert_eq!(
            texture_path(&flat, "a.png", true).unwrap().to_string(),
            "wsasset://rel/pkg/materials/textures/a.png"
        );
        assert_eq!(
            texture_path(&flat, "model://x/y.png", true)
                .unwrap()
                .to_string(),
            "wsasset://rel/pkg/x/y.png"
        );
        assert!(texture_path(&flat, "  ", true).is_none());
    }

    #[test]
    fn mtl_becomes_pbr() {
        let mtl = b"newmtl m\nKd 1 0 0\nd 0.5\nNs 20\nKe 0 1 0\nPm 0.25\n\
            newmtl t\nTr 0.25\nPr 0.6\n";
        let (mats, _) = tobj::load_mtl_buf(&mut BufReader::new(&mtl[..])).unwrap();
        let m = material(&mats[0]);
        assert_eq!(m.base_color, Color::srgba(1.0, 0.0, 0.0, 0.5));
        assert_eq!(m.alpha_mode, AlphaMode::Blend);
        assert!((m.perceptual_roughness - (2.0f32 / 22.0).sqrt()).abs() < 1e-6);
        assert_eq!(m.metallic, 0.25);
        assert!(m.emissive.green > 0.0 && m.emissive.red == 0.0);
        let t = material(&mats[1]);
        assert_eq!(t.base_color.alpha(), 0.75);
        assert_eq!(t.perceptual_roughness, 0.6);
        // No MTL at all: opaque white.
        let d = material(&tobj::Material::default());
        assert_eq!(d.base_color, Color::srgb(1.0, 1.0, 1.0));
        assert_eq!(d.alpha_mode, AlphaMode::Opaque);
    }

    #[test]
    fn meshes_flip_v_and_fill_in_normals() {
        let tri = tobj::Mesh {
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            texcoords: vec![0.0, 0.25, 1.0, 0.0, 0.0, 1.0],
            vertex_color: vec![1.0; 9],
            indices: vec![0, 1, 2],
            ..Default::default()
        };
        let m = mesh(&tri, true).unwrap();
        assert!(m.contains_attribute(Mesh::ATTRIBUTE_NORMAL));
        assert!(m.contains_attribute(Mesh::ATTRIBUTE_TANGENT));
        assert!(m.contains_attribute(Mesh::ATTRIBUTE_COLOR));
        let Some(bevy::mesh::VertexAttributeValues::Float32x2(uv)) =
            m.attribute(Mesh::ATTRIBUTE_UV_0)
        else {
            panic!("no UVs");
        };
        assert_eq!(uv[0], [0.0, 0.75]);
        let lit = tobj::Mesh {
            normals: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            texcoords: vec![],
            ..tri
        };
        assert!(!mesh(&lit, true)
            .unwrap()
            .contains_attribute(Mesh::ATTRIBUTE_TANGENT));
        assert!(mesh(&tobj::Mesh::default(), false).is_none());
    }
}
