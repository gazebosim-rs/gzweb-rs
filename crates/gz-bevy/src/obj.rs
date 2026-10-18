//! Wavefront OBJ with MTL materials for
//! [`SceneFileLoader`](crate::mesh_file::SceneFileLoader), parsed by `tobj`
//! and built like the COLLADA loader's [`Scene`]:
//!
//! - Every `o`/`g` object is an entity with its `Name` (for `<submesh>`),
//!   with a child per material it uses (three.js builds one mesh per object
//!   with a group per material; gz-common a submesh per material).
//! - `map_Kd` paths are rewritten the way gzweb's `GzObjLoader` does, so
//!   textures are found where Gazebo models keep them ([`texture_path`]).
//! - A missing or broken `.mtl` leaves the default material (gzweb shows
//!   nothing at all).
//!
//! Materials map onto PBR like the COLLADA loader's: `Kd` × `map_Kd`,
//! alpha from `d` (or `1 - Tr`), roughness from `Ns` unless `Pr` is given,
//! `Pm` metallic, `Ke`/`map_Ke` emission, `map_Bump`/`bump`/`norm` as a
//! normal map (as gz-common reads them). Keys are matched in any case, as
//! three.js's MTLLoader does, and the diffuse map's `-s`/`-o` options scale
//! and offset the material's UVs.

use std::collections::HashMap;
use std::io::BufReader;

use bevy::asset::{AssetPath, LoadContext};
use bevy::math::Affine2;
use bevy::prelude::*;
use bevy::render::render_resource::Face;

use crate::mesh_file::{self, MeshParts};
use crate::render::units;

const OPTIONS: tobj::LoadOptions = tobj::LoadOptions {
    single_index: true,
    triangulate: true,
    ignore_points: true,
    ignore_lines: true,
};

/// Parses an `.obj` file and its material libraries into a scene.
pub async fn load(bytes: &[u8], cx: &mut LoadContext<'_>) -> Result<Scene, tobj::LoadError> {
    let obj_path = cx.path().clone_owned();
    // tobj's material callback is synchronous, so read the libraries first.
    let mut libraries = HashMap::new();
    for name in mtllibs(bytes) {
        let Ok(path) = obj_path.resolve_embed(&name) else {
            continue;
        };
        match cx.read_asset_bytes(path.clone()).await {
            Ok(text) => {
                libraries.insert(name, normalize_mtl(&text));
            }
            Err(e) => warn!("OBJ '{obj_path}': material library '{path}': {e}"),
        }
    }
    let (models, materials) = tobj::load_obj_buf(&mut BufReader::new(bytes), &OPTIONS, |p| {
        let name = p.to_string_lossy();
        let text = libraries
            .get(name.as_ref())
            .ok_or(tobj::LoadError::OpenFileFailed)?;
        tobj::load_mtl_buf(&mut BufReader::new(text.as_slice()))
    })?;
    let materials = materials.unwrap_or_else(|e| {
        warn!("OBJ '{obj_path}': materials: {e}");
        Vec::new()
    });
    Ok(build_scene(&models, &materials, cx))
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

/// MTL keys tobj (or this loader) reads, in tobj's spelling.
const MTL_KEYS: &[&str] = &[
    "newmtl", "Ka", "Kd", "Ks", "Ke", "Ns", "Ni", "d", "Tr", "illum", "map_Ka", "map_Kd", "map_Ks",
    "map_Ke", "map_Ns", "map_d", "map_Bump", "bump", "norm", "Pr", "Pm",
];

/// The MTL text with each line's key in tobj's spelling, so `kd` and
/// `MAP_KD` work, as three.js lower-cases keys.
fn normalize_mtl(text: &[u8]) -> Vec<u8> {
    let mut out = String::with_capacity(text.len());
    for line in String::from_utf8_lossy(text).lines() {
        let body = line.trim_start();
        let key_len = body.find(char::is_whitespace).unwrap_or(body.len());
        let key = &body[..key_len];
        match MTL_KEYS.iter().find(|k| k.eq_ignore_ascii_case(key)) {
            Some(canonical) => {
                out.push_str(canonical);
                out.push_str(&body[key_len..]);
            }
            None => out.push_str(line),
        }
        out.push('\n');
    }
    out.into_bytes()
}

fn build_scene(models: &[tobj::Model], mtls: &[tobj::Material], cx: &mut LoadContext<'_>) -> Scene {
    let obj_path = cx.path().clone_owned();
    let materials: Vec<Handle<StandardMaterial>> = mtls
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let mut mat = material(m);
            // three.js's MTLLoader repeats textures.
            let texture =
                |cx: &mut LoadContext<'_>, tex: Option<AssetPath<'static>>, srgb: bool| {
                    mesh_file::load_texture(cx, tex?, srgb, mesh_file::sampler([true; 2], 1))
                };
            if let Some(spec) = m.diffuse_texture.as_deref() {
                mat.base_color_texture = texture(cx, texture_path(&obj_path, spec, true), true);
                mat.uv_transform = uv_transform(&TextureSpec::parse(spec));
            }
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
    let fallback = cx.add_labeled_asset(
        "MaterialDefault".into(),
        material(&tobj::Material::default()),
    );

    let mut world = World::new();
    let root = world
        .spawn((Transform::IDENTITY, Visibility::default()))
        .id();
    // tobj splits an object at each `usemtl`; consecutive pieces with the
    // same name are one object, as in three.js. A later object with the
    // same name is another entity; `<submesh>` keeps both, as Gazebo does.
    let mut node: Option<(&str, Entity)> = None;
    for (i, model) in models.iter().enumerate() {
        let name = match model.name.as_str() {
            "unnamed_object" => "",
            n => n,
        };
        let parent = match node {
            Some((n, e)) if n == name => e,
            _ => {
                let mut e =
                    world.spawn((Transform::IDENTITY, Visibility::default(), ChildOf(root)));
                if !name.is_empty() {
                    e.insert(Name::new(name.to_string()));
                }
                let e = e.id();
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
    Scene::new(world)
}

fn normal_map(m: &tobj::Material) -> Option<&str> {
    m.normal_texture
        .as_deref()
        .or_else(|| m.unknown_param.get("norm").map(String::as_str))
}

/// An OBJ mesh as a Bevy mesh ([`MeshParts::build`]); `None` without
/// triangles.
fn mesh(m: &tobj::Mesh, tangents: bool) -> Option<Mesh> {
    if m.indices.is_empty() || m.positions.is_empty() {
        return None;
    }
    let parts = MeshParts {
        positions: m
            .positions
            .chunks_exact(3)
            .map(|p| [p[0], p[1], p[2]])
            .collect(),
        normals: m
            .normals
            .chunks_exact(3)
            .map(|v| [v[0], v[1], v[2]])
            .collect(),
        uvs: m.texcoords.chunks_exact(2).map(|t| [t[0], t[1]]).collect(),
        colors: m
            .vertex_color
            .chunks_exact(3)
            .map(|c| [c[0], c[1], c[2], 1.0])
            .collect(),
        indices: Some(m.indices.clone()),
    };
    Some(parts.build(tangents, "OBJ mesh"))
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
    let roughness = match param("Pr") {
        Some(pr) => pr.clamp(0.089, 1.0),
        None => mesh_file::phong_roughness(m.shininess.unwrap_or(30.0)),
    };
    let [er, eg, eb] = m.emissive.unwrap_or_default();
    StandardMaterial {
        base_color: Color::srgba(r, g, b, alpha),
        alpha_mode: if alpha < 1.0 {
            AlphaMode::Blend
        } else {
            AlphaMode::Opaque
        },
        perceptual_roughness: roughness,
        metallic: param("Pm").unwrap_or(0.0).clamp(0.0, 1.0),
        emissive: units::emissive(Color::srgb(er, eg, eb)),
        cull_mode: Some(Face::Back),
        ..default()
    }
}

/// An MTL texture statement's arguments: the file and three.js's two UV
/// options (`-s u v [w]` repeats, `-o u v [w]` offsets; the rest are
/// dropped).
#[derive(Debug, Clone, PartialEq)]
struct TextureSpec<'a> {
    file: &'a str,
    scale: [f32; 2],
    offset: [f32; 2],
}

impl<'a> TextureSpec<'a> {
    fn parse(spec: &'a str) -> Self {
        let mut out = TextureSpec {
            file: "",
            scale: [1.0; 2],
            offset: [0.0; 2],
        };
        let mut rest = spec.trim();
        while rest.starts_with('-') {
            let (opt, after) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            let max_args = match opt {
                "-o" | "-s" | "-t" => 3,
                "-mm" => 2,
                _ => 1,
            };
            let mut after = after.trim_start();
            let mut args = Vec::new();
            for i in 0..max_args {
                let (arg, next) = after.split_once(char::is_whitespace).unwrap_or((after, ""));
                // Optional numeric arguments (`-s u [v [w]]`) end at the
                // first non-number.
                let number = arg.parse::<f32>();
                if i > 0 && number.is_err() {
                    break;
                }
                args.push(number.unwrap_or(0.0));
                after = next.trim_start();
            }
            let uv = |default: f32| [args[0], args.get(1).copied().unwrap_or(default)];
            match opt {
                "-s" => out.scale = uv(1.0),
                "-o" => out.offset = uv(0.0),
                _ => {}
            }
            rest = after;
        }
        out.file = rest;
        out
    }
}

/// three.js's `repeat`/`offset` for a texture as Bevy's `uv_transform`.
/// three applies them to OBJ UVs (V up) and samples with V flipped; Bevy's
/// UVs are already flipped, so V's offset becomes `1 - s - o`.
fn uv_transform(spec: &TextureSpec) -> Affine2 {
    let [su, sv] = spec.scale;
    let [ou, ov] = spec.offset;
    if spec.scale == [1.0; 2] && spec.offset == [0.0; 2] {
        return Affine2::IDENTITY;
    }
    Affine2::from_scale_angle_translation(Vec2::new(su, sv), 0.0, Vec2::new(ou, 1.0 - sv - ov))
}

/// Where a texture an `.mtl` names lives. Diffuse maps follow gzweb's
/// `GzObjLoader.loadMTL`:
///
/// - `model://other/x.png` is looked up beside this model's directory
///   (the `.obj` must be in a `meshes` directory);
/// - a path with `/materials/textures` in it is kept;
/// - a URI or absolute path is used as it is (gzweb would prefix it);
/// - anything else is a file in `<model>/materials/textures/`, `<model>`
///   being the `.obj`'s path up to `meshes`.
///
/// Other maps, and kept paths, resolve like a COLLADA texture
/// ([`mesh_file::texture_path`]): through the usual sources when a URI or
/// absolute, else relative to the `.obj`.
pub fn texture_path(
    obj: &AssetPath<'_>,
    spec: &str,
    gzweb_rules: bool,
) -> Option<AssetPath<'static>> {
    let file = TextureSpec::parse(spec).file;
    if file.is_empty() {
        return None;
    }
    let obj_str = obj.to_string();
    let dir = &obj_str[..obj_str.rfind('/').map_or(0, |i| i + 1)];
    if let Some(rest) = file.strip_prefix("model://") {
        // gzweb: the models directory is the one holding this model.
        let models = dir
            .rfind("/meshes")
            .map(|i| &dir[..i])
            .and_then(|m| m.rfind('/').map(|j| &m[..=j]));
        return match models {
            Some(models) => AssetPath::try_parse(&format!("{models}{rest}"))
                .ok()
                .map(AssetPath::into_owned),
            None => obj.resolve_embed(rest).ok(),
        };
    }
    let absolute = file.contains("://") || file.starts_with('/');
    // gzweb keeps any line with `/materials/textures` after its start,
    // which is always the case after the `map_Kd` key.
    if gzweb_rules && !absolute && !file.contains("/materials/textures") {
        return match dir.rfind("meshes") {
            Some(i) => AssetPath::try_parse(&format!("{}materials/textures/{file}", &dir[..i]))
                .ok()
                .map(AssetPath::into_owned),
            None => obj
                .resolve_embed(&format!("materials/textures/{file}"))
                .ok(),
        };
    }
    mesh_file::texture_path(obj, file)
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
    fn texture_options_are_parsed() {
        let p = |s| TextureSpec::parse(s);
        assert_eq!(p("tex.png").file, "tex.png");
        assert_eq!(p("-bm 0.5 my normal.png").file, "my normal.png");
        let both = p("-s 1 2 3 -o 0.5 0.25 t.png");
        assert_eq!(
            both,
            TextureSpec {
                file: "t.png",
                scale: [1.0, 2.0],
                offset: [0.5, 0.25]
            }
        );
        let one = p("-s 2 t.png");
        assert_eq!((one.file, one.scale), ("t.png", [2.0, 1.0]));
        assert_eq!(p("-clamp on -mm 0 1 t.png").file, "t.png");
        assert_eq!(uv_transform(&p("t.png")), Affine2::IDENTITY);
        // three: uv' = s * uv + o on V-up UVs, sampled V-flipped.
        let t = uv_transform(&both);
        let three = |u: f32, v: f32| Vec2::new(u * 1.0 + 0.5, 1.0 - (v * 2.0 + 0.25));
        for (u, v) in [(0.0, 0.0), (1.0, 0.5), (0.25, 1.0)] {
            let bevy = t.transform_point2(Vec2::new(u, 1.0 - v));
            assert!(
                (bevy - three(u, v)).length() < 1e-6,
                "{bevy} {}",
                three(u, v)
            );
        }
    }

    #[test]
    fn mtl_keys_match_in_any_case() {
        let mtl = normalize_mtl(b"NEWMTL m\n  kd 1 0 0\nMAP_KD a.png\nmap_ke e.png\nfoo 1\n");
        let (mats, _) = tobj::load_mtl_buf(&mut BufReader::new(&mtl[..])).unwrap();
        assert_eq!(mats[0].name, "m");
        assert_eq!(mats[0].diffuse, Some([1.0, 0.0, 0.0]));
        assert_eq!(mats[0].diffuse_texture.as_deref(), Some("a.png"));
        assert_eq!(mats[0].unknown_param["map_Ke"], "e.png");
        assert_eq!(mats[0].unknown_param["foo"], "1");
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
        // gzweb keeps a path starting with /materials/textures; it is
        // absolute, so it goes to the server.
        assert_eq!(
            p("/materials/textures/x.png", true),
            "wsasset://abs/materials/textures/x.png"
        );
        // Not a diffuse map: relative to the .obj.
        assert_eq!(
            p("n.png", false),
            "gzhttps://fuel.example/o/models/m/1/files/meshes/n.png"
        );
        // URIs and absolute paths through the usual sources.
        assert_eq!(
            p("/home/u/m/materials/textures/a.png", true),
            "wsasset://abs/home/u/m/materials/textures/a.png"
        );
        assert_eq!(p("/home/u/t/a.png", true), "wsasset://abs/home/u/t/a.png");
        assert_eq!(
            p("https://fuel.example/x/a.png", true),
            "gzhttps://fuel.example/x/a.png"
        );
        assert_eq!(p("file:///tmp/a.png", false), "wsasset://abs/tmp/a.png");
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
        assert_eq!(m.perceptual_roughness, mesh_file::phong_roughness(20.0));
        assert_eq!(m.metallic, 0.25);
        assert_eq!(m.emissive, units::emissive(Color::srgb(0.0, 1.0, 0.0)));
        let t = material(&mats[1]);
        assert_eq!(t.base_color.alpha(), 0.75);
        assert_eq!(t.perceptual_roughness, 0.6);
        // No MTL at all: opaque white.
        let d = material(&tobj::Material::default());
        assert_eq!(d.base_color, Color::srgb(1.0, 1.0, 1.0));
        assert_eq!(d.alpha_mode, AlphaMode::Opaque);
    }

    #[test]
    fn meshes_keep_their_index_buffer_and_srgb_colours() {
        let tri = tobj::Mesh {
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            texcoords: vec![0.0, 0.25, 1.0, 0.0, 0.0, 1.0],
            vertex_color: vec![0.5; 9],
            normals: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            indices: vec![0, 1, 2],
            ..Default::default()
        };
        let m = mesh(&tri, true).unwrap();
        assert!(m.indices().is_some());
        assert!(m.contains_attribute(Mesh::ATTRIBUTE_TANGENT));
        let Some(bevy::mesh::VertexAttributeValues::Float32x4(colors)) =
            m.attribute(Mesh::ATTRIBUTE_COLOR)
        else {
            panic!("no colours");
        };
        // sRGB 0.5 is linear 0.214, as three.js reads OBJ colours.
        assert!((colors[0][0] - 0.214).abs() < 1e-3, "{:?}", colors[0]);
        assert_eq!(colors[0][3], 1.0);
        assert!(mesh(&tobj::Mesh::default(), false).is_none());
    }
}
