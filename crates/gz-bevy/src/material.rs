//! A visual's Gazebo material (`gz.msgs.Material`, SDF `<material>`) as a
//! Bevy [`StandardMaterial`], by Gazebo's rules rather than gzweb's (see
//! `.agent/DESIGN.md` "Materials"):
//!
//! - **Colours are linear.** Gazebo's ogre2 PBS shaders use the message's
//!   floats as they are and render to sRGB targets; colour textures
//!   (albedo, emissive, light map) are sRGB, data textures linear.
//! - **Base colour** is the diffuse colour (ogre2 ignores ambient; gzweb
//!   mixes `0.4 * ambient + 0.6 * diffuse`), times the albedo map.
//! - **Opacity** is `diffuse.a * (1 - transparency)` (`Ogre2Material::
//!   UpdateTransparency`). Below 1 it blends like Ogre's `Transparent`
//!   mode, which fades the diffuse light (by alpha²) but keeps reflections
//!   (by alpha): Bevy's premultiplied blending with the colour and F0 scaled
//!   to match. Under an albedo map it is plain alpha blending, so the
//!   texture's alpha still works. An opaque albedo map is a 0.5 alpha mask
//!   (Gazebo's alpha test), which also cuts its shadow.
//! - **Specular** scales the specular reflection (Ogre's `kS`): dielectric
//!   F0 is Ogre's 3% times the specular colour. Bevy can't scale a metal's
//!   reflection, so metals keep theirs.
//! - **PBR** (metal workflow): metalness, roughness and their maps (red
//!   channel, as Ogre samples them), combined on the CPU into one glTF-layout
//!   image (G roughness, B metalness) once both load ([`combine_metal_rough`]).
//!   The specular workflow keeps the maps but not the scalars, as in Gazebo.
//!   Normal, emissive, ambient-occlusion and light maps; the environment map
//!   is not rendered.
//! - **Defaults**: a primitive without a material is Gazebo's `gz-grey`
//!   (diffuse 0.7, specular 1, roughness 0.2, metalness 1); a mesh keeps its
//!   file's materials, and an STL (which has none) gets ogre2's default
//!   (white, specular 0.2, roughness 0.5).
//!
//! A mesh visual's SDF material replaces **every** submesh's material
//! (gz-sim `SceneManager::CreateVisual` → `BaseMesh::SetMaterial`); without
//! one, the file's materials stay and only the visual's transparency and
//! `cast_shadows` apply ([`apply_to_scene`]). Overrides are new assets per
//! visual: shared file materials are never changed.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::pbr::Lightmap;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, Face, TextureDimension, TextureFormat};
use bevy::world_serialization::WorldInstanceReady;
use gz_core::scene::{MaterialDesc, PbrWorkflow};

use crate::render::units;
use crate::{assets, mesh_file};

/// Ogre's dielectric F0 in the metal workflow (Bevy's default is 4%).
pub const DIELECTRIC_F0: f32 = 0.03;

/// The alpha-test threshold Gazebo uses for an albedo map's alpha.
pub const ALPHA_CUTOFF: f32 = 0.5;

/// What a visual without a material looks like.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fallback {
    /// gz-sim's `gz-grey`, for primitives.
    GzGrey,
    /// ogre2's default material, for a mesh without its own (STL).
    Mesh,
}

/// A material's scalar parameters as Bevy wants them.
struct Params {
    diffuse: [f32; 4],
    specular: Option<[f32; 3]>,
    metallic: f32,
    roughness: f32,
}

fn params(mat: &MaterialDesc, fallback: Fallback) -> Params {
    if !mat.present {
        return match fallback {
            Fallback::GzGrey => Params {
                diffuse: [0.7, 0.7, 0.7, 1.0],
                specular: Some([1.0; 3]),
                metallic: 1.0,
                roughness: 0.2,
            },
            Fallback::Mesh => Params {
                diffuse: [1.0; 4],
                specular: Some([0.2; 3]),
                metallic: 0.0,
                roughness: 0.5,
            },
        };
    }
    let (metallic, roughness) = match &mat.pbr {
        Some(p) if p.workflow == PbrWorkflow::Metal => (p.metalness as f32, p.roughness as f32),
        // ogre2's defaults (gz-common `Pbr`).
        _ => (0.0, 0.5),
    };
    Params {
        diffuse: mat.diffuse.unwrap_or([1.0; 4]),
        specular: mat.specular,
        metallic,
        roughness,
    }
}

/// The material's colours, scalars, culling and alpha mode; textures are
/// loaded by [`build`]. `has_albedo_map` picks the alpha mode.
pub fn standard_material(
    mat: &MaterialDesc,
    transparency: f64,
    fallback: Fallback,
    has_albedo_map: bool,
) -> StandardMaterial {
    let p = params(mat, fallback);
    let [r, g, b, a] = p.diffuse;
    let opacity = a.clamp(0.0, 1.0) * (1.0 - transparency.clamp(0.0, 1.0) as f32);
    let [er, eg, eb] = mat.emissive;
    let (reflectance, specular_tint) = match p.specular {
        Some(ks) => {
            // F0 = 0.16 (reflectance · tint)² = DIELECTRIC_F0 · kS.
            let [sr, sg, sb] = ks.map(|c| c.max(0.0).sqrt());
            ((DIELECTRIC_F0 / 0.16).sqrt(), Color::linear_rgb(sr, sg, sb))
        }
        None => (0.5, Color::WHITE),
    };
    let mut m = StandardMaterial {
        base_color: Color::linear_rgba(r, g, b, 1.0),
        emissive: units::emissive(Color::linear_rgb(er, eg, eb)),
        metallic: p.metallic.clamp(0.0, 1.0),
        perceptual_roughness: p.roughness.clamp(0.089, 1.0),
        reflectance,
        specular_tint,
        double_sided: mat.double_sided,
        cull_mode: (!mat.double_sided).then_some(Face::Back),
        // Gazebo scales it by 100 for its reverse depth, as wgpu wants
        // for a float depth buffer.
        depth_bias: mat.render_order * 100.0,
        lightmap_exposure: units::UNIT_LUMINANCE,
        alpha_mode: if has_albedo_map {
            AlphaMode::Mask(ALPHA_CUTOFF)
        } else {
            AlphaMode::Opaque
        },
        ..default()
    };
    set_opacity(&mut m, opacity);
    m
}

/// Makes `m` `opacity` times as opaque as it is, with Gazebo's blending
/// (see the module docs). Used for SDF materials and, for a visual's
/// `transparency`, on a mesh file's own materials.
pub fn set_opacity(m: &mut StandardMaterial, opacity: f32) {
    let opacity = opacity.clamp(0.0, 1.0);
    if opacity >= 1.0 {
        return;
    }
    let base = m.base_color.to_linear();
    if m.base_color_texture.is_some() || matches!(m.alpha_mode, AlphaMode::Mask(_)) {
        m.base_color = Color::LinearRgba(base.with_alpha(base.alpha * opacity));
        m.alpha_mode = AlphaMode::Blend;
        return;
    }
    let a = base.alpha * opacity;
    let fade = if matches!(m.alpha_mode, AlphaMode::Premultiplied) {
        // Already premultiplied by the old alpha squared.
        opacity * opacity
    } else {
        a * a
    };
    m.base_color = Color::linear_rgba(base.red * fade, base.green * fade, base.blue * fade, a);
    m.reflectance *= opacity.sqrt();
    m.alpha_mode = AlphaMode::Premultiplied;
}

/// The colour and opacity a material was given, undoing [`set_opacity`]'s
/// premultiplication (for snapshots).
pub fn reported_color(m: &StandardMaterial) -> [f32; 4] {
    let c = m.base_color.to_linear();
    let fade = match m.alpha_mode {
        AlphaMode::Premultiplied if c.alpha > 0.0 => c.alpha * c.alpha,
        _ => 1.0,
    };
    [c.red / fade, c.green / fade, c.blue / fade, c.alpha]
}

/// A built material and what its mesh needs.
#[derive(Debug, Clone)]
pub struct VisualMaterial {
    pub material: Handle<StandardMaterial>,
    /// It has a normal map, so its mesh needs tangents.
    pub tangents: bool,
    /// Its light map and the UV set it uses.
    pub lightmap: Option<(Handle<Image>, u32)>,
    /// The textures it loads, for snapshots to wait on.
    pub loads: Vec<UntypedHandle>,
}

/// Separate metalness and roughness maps waiting to be combined.
#[derive(Debug)]
struct PendingCombine {
    material: Handle<StandardMaterial>,
    metalness: Option<Handle<Image>>,
    roughness: Option<Handle<Image>>,
}

type CombineKey = (Option<AssetId<Image>>, Option<AssetId<Image>>);

/// Metalness/roughness pairs being combined, and the images already made
/// (many visuals share one pair).
#[derive(Resource, Default, Debug)]
pub struct GzMaterials {
    pending: Vec<PendingCombine>,
    combined: HashMap<CombineKey, Option<Handle<Image>>>,
}

impl GzMaterials {
    /// Whether every combine has finished (snapshots wait for it).
    pub fn settled(&self) -> bool {
        self.pending.is_empty()
    }
}

/// Builds a visual's material and starts loading its textures through the
/// usual sources (HTTP, then the websocket server).
pub fn build(
    asset_server: &AssetServer,
    materials: &mut Assets<StandardMaterial>,
    state: &mut GzMaterials,
    mat: &MaterialDesc,
    transparency: f64,
    fallback: Fallback,
) -> VisualMaterial {
    let pbr = mat.pbr.as_ref();
    let load = |uri: Option<&str>, srgb: bool| -> Option<Handle<Image>> {
        uri.map(|uri| {
            asset_server
                .load_builder()
                .with_settings(mesh_file::image_settings(
                    srgb,
                    mesh_file::sampler([true; 2], 16),
                ))
                .load(assets::asset_path(uri))
        })
    };
    let albedo = load(pbr.and_then(|p| p.albedo_map.as_deref()), true);
    let normal = load(mat.normal_map(), false);
    let emissive = load(pbr.and_then(|p| p.emissive_map.as_deref()), true);
    let occlusion = load(pbr.and_then(|p| p.ambient_occlusion_map.as_deref()), false);
    let light = load(pbr.and_then(|p| p.light_map.as_deref()), true);
    let metal_workflow = pbr.is_some_and(|p| p.workflow == PbrWorkflow::Metal);
    let metalness = load(
        pbr.filter(|_| metal_workflow)
            .and_then(|p| p.metalness_map.as_deref()),
        false,
    );
    let roughness = load(
        pbr.filter(|_| metal_workflow)
            .and_then(|p| p.roughness_map.as_deref()),
        false,
    );

    let mut m = standard_material(mat, transparency, fallback, albedo.is_some());
    if emissive.is_some() && mat.emissive == [0.0; 3] {
        // Ogre multiplies the map by the colour only when there is one.
        m.emissive = units::emissive(Color::WHITE);
    }
    m.base_color_texture = albedo.clone();
    m.normal_map_texture = normal.clone();
    m.emissive_texture = emissive.clone();
    m.occlusion_texture = occlusion.clone();

    let loads = [
        &albedo, &normal, &emissive, &occlusion, &light, &metalness, &roughness,
    ]
    .into_iter()
    .flatten()
    .map(|h| h.clone().untyped())
    .collect();
    let material = materials.add(m);
    if metalness.is_some() || roughness.is_some() {
        state.pending.push(PendingCombine {
            material: material.clone(),
            metalness,
            roughness,
        });
    }
    VisualMaterial {
        material,
        tangents: normal.is_some(),
        lightmap: light.map(|l| (l, pbr.map_or(0, |p| p.light_map_uv))),
        loads,
    }
}

/// glTF's metallic-roughness layout from Gazebo's separate maps: G is the
/// roughness map's red channel, B the metalness map's, each 1 (the scalar
/// alone) without its map. The larger map's size; the other is sampled
/// nearest. `None` if neither can be read (block-compressed formats).
pub fn combine(metalness: Option<&Image>, roughness: Option<&Image>) -> Option<Image> {
    let readable = |i: &&Image| i.get_color_at(0, 0).is_ok();
    let (metalness, roughness) = (metalness.filter(readable), roughness.filter(readable));
    let (w, h) = [metalness, roughness]
        .into_iter()
        .flatten()
        .map(|i| (i.width(), i.height()))
        .max_by_key(|(w, h)| w * h)?;
    let red = |img: Option<&Image>, x: u32, y: u32| -> u8 {
        let Some(img) = img else { return 255 };
        let sx = (x as u64 * img.width() as u64 / w as u64) as u32;
        let sy = (y as u64 * img.height() as u64 / h as u64) as u32;
        let c = img.get_color_at(sx, sy).unwrap_or(Color::WHITE);
        (c.to_linear().red.clamp(0.0, 1.0) * 255.0).round() as u8
    };
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            data.extend_from_slice(&[255, red(roughness, x, y), red(metalness, x, y), 255]);
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::default(),
    );
    image.sampler = mesh_file::sampler([true; 2], 16);
    Some(image)
}

/// Combines each material's metalness and roughness maps once they have
/// loaded (or failed).
pub fn combine_metal_rough(
    mut state: ResMut<GzMaterials>,
    server: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if state.pending.is_empty() {
        return;
    }
    let waiting = |h: &Option<Handle<Image>>| {
        h.as_ref()
            .is_some_and(|h| !images.contains(h) && !server.load_state(h).is_failed())
    };
    let (ready, pending): (Vec<_>, Vec<_>) = std::mem::take(&mut state.pending)
        .into_iter()
        .partition(|p| !waiting(&p.metalness) && !waiting(&p.roughness));
    state.pending = pending;
    for p in ready {
        let key = (
            p.metalness.as_ref().map(Handle::id),
            p.roughness.as_ref().map(Handle::id),
        );
        let texture = match state.combined.get(&key) {
            Some(t) => t.clone(),
            None => {
                let get = |h: &Option<Handle<Image>>| h.as_ref().and_then(|h| images.get(h));
                let image = combine(get(&p.metalness), get(&p.roughness));
                if image.is_none() {
                    warn!("metalness/roughness maps could not be read; using the scalars");
                }
                let t = image.map(|i| images.add(i));
                state.combined.insert(key, t.clone());
                t
            }
        };
        if let (Some(t), Some(mut m)) = (texture, materials.get_mut(&p.material)) {
            m.metallic_roughness_texture = Some(t);
        }
    }
}

/// On a mesh file's `WorldAssetRoot`: what the visual does to the meshes
/// once the scene is instanced ([`apply_to_scene`]).
#[derive(Component, Debug, Clone)]
pub struct GzMeshMaterial {
    /// The visual's SDF material, replacing every submesh's.
    pub replace: Option<VisualMaterial>,
    /// The visual's `transparency`, applied to the file's own materials.
    pub transparency: f32,
    pub cast_shadows: bool,
}

/// Gives `mesh` tangents (for a normal map) and a second UV set (for a
/// light map) if it lacks them. Changes the mesh asset itself: extra
/// attributes don't change how it renders elsewhere.
pub fn prepare_mesh(mesh: &mut Mesh, tangents: bool, lightmap_uv: Option<u32>) {
    let has_uv = mesh.contains_attribute(Mesh::ATTRIBUTE_UV_0);
    if tangents && has_uv && !mesh.contains_attribute(Mesh::ATTRIBUTE_TANGENT) {
        if let Err(e) = mesh.generate_tangents() {
            warn!("no tangents for a normal map: {e}");
        }
    }
    if let Some(set) = lightmap_uv {
        if !mesh.contains_attribute(Mesh::ATTRIBUTE_UV_1) {
            if set != 0 {
                warn!("light map UV set {set} missing; using the first");
            }
            if let Some(uv) = mesh.attribute(Mesh::ATTRIBUTE_UV_0).cloned() {
                mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, uv);
            }
        }
    }
}

/// The `Lightmap` component for a light map.
pub fn lightmap(image: &Handle<Image>) -> Lightmap {
    Lightmap {
        image: image.clone(),
        uv_rect: Rect::new(0.0, 0.0, 1.0, 1.0),
        bicubic_sampling: false,
    }
}

/// Observer for [`WorldInstanceReady`] on an entity with
/// [`GzMeshMaterial`]: applies the visual's material rules to every mesh
/// below it. Runs again on the fresh entities of a respawned scene.
pub fn apply_to_scene(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    roots: Query<&GzMeshMaterial>,
    children: Query<&Children>,
    meshes_q: Query<(&Mesh3d, Option<&MeshMaterial3d<StandardMaterial>>)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Ok(rule) = roots.get(ready.entity) else {
        return;
    };
    let opacity = 1.0 - rule.transparency.clamp(0.0, 1.0);
    // One clone per shared file material.
    let mut faded: HashMap<AssetId<StandardMaterial>, Handle<StandardMaterial>> = HashMap::new();
    for e in children.iter_descendants(ready.entity) {
        let Ok((mesh3d, own)) = meshes_q.get(e) else {
            continue;
        };
        // `try_`: the submesh observer may despawn it first.
        let mut entity = commands.entity(e);
        if !rule.cast_shadows {
            entity.try_insert(NotShadowCaster);
        }
        match &rule.replace {
            Some(v) => {
                if let Some(mut mesh) = meshes.get_mut(&mesh3d.0) {
                    prepare_mesh(&mut mesh, v.tangents, v.lightmap.as_ref().map(|l| l.1));
                }
                entity.try_insert(MeshMaterial3d(v.material.clone()));
                if let Some((image, _)) = &v.lightmap {
                    entity.try_insert(lightmap(image));
                }
            }
            None if opacity < 1.0 => {
                let Some(own) = own else { continue };
                let id = own.0.id();
                let handle = match faded.get(&id) {
                    Some(h) => h.clone(),
                    None => {
                        let Some(mut m) = materials.get(id).cloned() else {
                            continue;
                        };
                        set_opacity(&mut m, opacity);
                        let h = materials.add(m);
                        faded.insert(id, h.clone());
                        h
                    }
                };
                entity.try_insert(MeshMaterial3d(handle));
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gz_core::scene::PbrDesc;

    fn mat() -> MaterialDesc {
        MaterialDesc {
            diffuse: Some([0.5, 0.25, 1.0, 1.0]),
            ambient: Some([1.0, 0.0, 0.0, 1.0]),
            present: true,
            ..MaterialDesc::absent()
        }
    }

    fn pbr() -> PbrDesc {
        PbrDesc {
            workflow: PbrWorkflow::Metal,
            metalness: 0.8,
            roughness: 0.3,
            albedo_map: None,
            normal_map: None,
            metalness_map: None,
            roughness_map: None,
            emissive_map: None,
            ambient_occlusion_map: None,
            light_map: None,
            light_map_uv: 0,
            environment_map: None,
        }
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn colours_are_linear_diffuse_without_ambient() {
        let m = standard_material(&mat(), 0.0, Fallback::GzGrey, false);
        assert_eq!(m.base_color, Color::linear_rgba(0.5, 0.25, 1.0, 1.0));
        assert_eq!(m.alpha_mode, AlphaMode::Opaque);
        assert_eq!(m.cull_mode, Some(Face::Back));
        // No specular in the message: Bevy's default 4%.
        assert_eq!((m.reflectance, m.specular_tint), (0.5, Color::WHITE));
        // Emission is linear too (and scaled to render 1 at full).
        let glow = MaterialDesc {
            emissive: [0.5, 0.0, 0.0],
            ..mat()
        };
        let e = standard_material(&glow, 0.0, Fallback::GzGrey, false).emissive;
        assert!(close(units::emission_color(e).red, 0.5));
        assert_eq!(reported_color(&m), [0.5, 0.25, 1.0, 1.0]);
    }

    #[test]
    fn specular_scales_ogres_three_percent() {
        let m = standard_material(
            &MaterialDesc {
                specular: Some([1.0, 0.25, 0.0]),
                ..mat()
            },
            0.0,
            Fallback::GzGrey,
            false,
        );
        // Bevy's F0 = 0.16 (reflectance · tint)².
        let tint = m.specular_tint.to_linear();
        let f0 = |t: f32| 0.16 * (m.reflectance * t).powi(2);
        assert!(close(f0(tint.red), 0.03));
        assert!(close(f0(tint.green), 0.0075));
        assert!(close(f0(tint.blue), 0.0));
    }

    #[test]
    fn defaults_follow_gazebo() {
        let grey = standard_material(&MaterialDesc::absent(), 0.0, Fallback::GzGrey, false);
        assert_eq!(grey.base_color, Color::linear_rgba(0.7, 0.7, 0.7, 1.0));
        assert_eq!((grey.metallic, grey.perceptual_roughness), (1.0, 0.2));
        let mesh = standard_material(&MaterialDesc::absent(), 0.0, Fallback::Mesh, false);
        assert_eq!(mesh.base_color, Color::linear_rgba(1.0, 1.0, 1.0, 1.0));
        assert_eq!((mesh.metallic, mesh.perceptual_roughness), (0.0, 0.5));
        // No PBR block, or the specular workflow: ogre2's 0 and 0.5.
        let plain = standard_material(&mat(), 0.0, Fallback::GzGrey, false);
        assert_eq!((plain.metallic, plain.perceptual_roughness), (0.0, 0.5));
        let specular = MaterialDesc {
            pbr: Some(PbrDesc {
                workflow: PbrWorkflow::Specular,
                ..pbr()
            }),
            ..mat()
        };
        let s = standard_material(&specular, 0.0, Fallback::GzGrey, false);
        assert_eq!((s.metallic, s.perceptual_roughness), (0.0, 0.5));
        let metal = MaterialDesc {
            pbr: Some(pbr()),
            render_order: 1.5,
            double_sided: true,
            ..mat()
        };
        let m = standard_material(&metal, 0.0, Fallback::GzGrey, false);
        assert!(close(m.metallic, 0.8) && close(m.perceptual_roughness, 0.3));
        assert!(m.double_sided && m.cull_mode.is_none());
        assert_eq!(m.depth_bias, 150.0);
    }

    #[test]
    fn opacity_is_diffuse_alpha_times_transparency() {
        let half = MaterialDesc {
            diffuse: Some([0.5, 0.25, 1.0, 0.5]),
            ..mat()
        };
        let m = standard_material(&half, 0.5, Fallback::GzGrey, false);
        // Ogre's Transparent mode: diffuse × α², reflections × α.
        assert_eq!(m.alpha_mode, AlphaMode::Premultiplied);
        let c = m.base_color.to_linear();
        assert!(close(c.alpha, 0.25) && close(c.red, 0.5 * 0.0625));
        let f0 = 0.16 * m.reflectance.powi(2);
        assert!(close(f0, 0.04 * 0.25), "{f0}");
        let r = reported_color(&m);
        assert!(
            close(r[0], 0.5) && close(r[1], 0.25) && close(r[3], 0.25),
            "{r:?}"
        );
        // Fully transparent stays premultiplied (and reports as given).
        let gone = standard_material(&mat(), 1.0, Fallback::GzGrey, false);
        assert_eq!(gone.alpha_mode, AlphaMode::Premultiplied);
        assert_eq!(reported_color(&gone)[3], 0.0);

        // An albedo map is a mask when opaque, blended when not.
        let cut = standard_material(&mat(), 0.0, Fallback::GzGrey, true);
        assert_eq!(cut.alpha_mode, AlphaMode::Mask(ALPHA_CUTOFF));
        let blend = standard_material(&mat(), 0.5, Fallback::GzGrey, true);
        assert_eq!(blend.alpha_mode, AlphaMode::Blend);
        assert!(close(reported_color(&blend)[3], 0.5));

        // Fading an already premultiplied material compounds.
        let mut twice = m.clone();
        set_opacity(&mut twice, 0.5);
        let r = reported_color(&twice);
        assert!(close(r[3], 0.125) && close(r[0], 0.5), "{r:?}");
        // Opaque leaves it alone.
        let mut same = m.clone();
        set_opacity(&mut same, 1.0);
        assert_eq!(same.base_color, m.base_color);
    }

    fn grey(w: u32, h: u32, v: u8) -> Image {
        Image::new_fill(
            Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &[v, 0, 0, 255],
            TextureFormat::Rgba8Unorm,
            RenderAssetUsages::default(),
        )
    }

    #[test]
    fn metalness_and_roughness_combine_into_green_and_blue() {
        let mut rough = grey(4, 2, 100);
        // The left half of the roughness map is darker.
        for y in 0..2 {
            for x in 0..2 {
                rough
                    .set_color_at(x, y, Color::linear_rgb(0.0, 1.0, 1.0))
                    .unwrap();
            }
        }
        let metal = grey(2, 1, 200);
        let c = combine(Some(&metal), Some(&rough)).unwrap();
        assert_eq!((c.width(), c.height()), (4, 2));
        let px = |x: u32, y: u32| {
            let i = ((y * 4 + x) * 4) as usize;
            c.data.as_ref().unwrap()[i..i + 4].to_vec()
        };
        assert_eq!(px(0, 0), [255, 0, 200, 255]);
        assert_eq!(px(3, 1), [255, 100, 200, 255]);
        // One map alone: the other channel is 1, so its scalar applies.
        let only = combine(None, Some(&grey(1, 1, 7))).unwrap();
        assert_eq!(only.data.unwrap(), [255, 7, 255, 255]);
        assert!(combine(None, None).is_none());
        // An unreadable (compressed) map counts as missing.
        let mut bc = grey(4, 4, 0);
        bc.texture_descriptor.format = TextureFormat::Bc1RgbaUnorm;
        assert!(combine(Some(&bc), None).is_none());
    }

    /// A scene of two meshes sharing one textured file material; returns
    /// the app, the root and that material.
    fn scene_app(
        rule: impl FnOnce(&mut World) -> GzMeshMaterial,
    ) -> (App, Entity, Handle<StandardMaterial>) {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            TransformPlugin,
            bevy::world_serialization::WorldSerializationPlugin,
        ))
        .init_asset::<Mesh>()
        .init_asset::<Image>()
        .init_asset::<StandardMaterial>()
        .register_type::<MeshMaterial3d<StandardMaterial>>();
        let world = app.world_mut();
        let rule = rule(world);
        let cube = world.resource_mut::<Assets<Mesh>>().add(Cuboid::default());
        let image = world.resource_mut::<Assets<Image>>().add(grey(1, 1, 9));
        let own = world
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial {
                base_color_texture: Some(image),
                ..default()
            });
        let mut scene = World::new();
        let root = scene.spawn(Transform::IDENTITY).id();
        for _ in 0..2 {
            scene.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(own.clone()),
                Transform::IDENTITY,
                ChildOf(root),
            ));
        }
        let scene = world
            .resource_mut::<Assets<WorldAsset>>()
            .add(WorldAsset::new(scene));
        let e = world
            .spawn((WorldAssetRoot(scene), Transform::IDENTITY, rule))
            .observe(apply_to_scene)
            .id();
        for _ in 0..3 {
            app.update();
        }
        (app, e, own)
    }

    fn mesh_entities(app: &mut App) -> Vec<Entity> {
        let world = app.world_mut();
        let mut q = world.query_filtered::<Entity, With<Mesh3d>>();
        q.iter(world).collect()
    }

    fn material_of(app: &App, e: Entity) -> Handle<StandardMaterial> {
        app.world()
            .get::<MeshMaterial3d<StandardMaterial>>(e)
            .unwrap()
            .0
            .clone()
    }

    #[test]
    fn an_sdf_material_replaces_every_submesh_material() {
        let (mut app, _, own) = scene_app(|world| {
            let sdf = world
                .resource_mut::<Assets<StandardMaterial>>()
                .add(StandardMaterial::default());
            let light = world.resource_mut::<Assets<Image>>().add(grey(1, 1, 1));
            GzMeshMaterial {
                replace: Some(VisualMaterial {
                    material: sdf,
                    tangents: true,
                    lightmap: Some((light, 0)),
                    loads: vec![],
                }),
                transparency: 0.5,
                cast_shadows: false,
            }
        });
        let meshes = mesh_entities(&mut app);
        assert_eq!(meshes.len(), 2);
        for e in meshes {
            let m = material_of(&app, e);
            assert_ne!(m.id(), own.id());
            assert!(app.world().get::<NotShadowCaster>(e).is_some());
            assert!(app.world().get::<Lightmap>(e).is_some());
            let mesh = app.world().get::<Mesh3d>(e).unwrap().0.clone();
            let meshes = app.world().resource::<Assets<Mesh>>();
            let mesh = meshes.get(&mesh).unwrap();
            assert!(mesh.contains_attribute(Mesh::ATTRIBUTE_TANGENT));
            assert!(mesh.contains_attribute(Mesh::ATTRIBUTE_UV_1));
        }
    }

    #[test]
    fn transparency_fades_a_copy_of_the_file_materials() {
        let (mut app, _, own) = scene_app(|_| GzMeshMaterial {
            replace: None,
            transparency: 0.25,
            cast_shadows: true,
        });
        let meshes = mesh_entities(&mut app);
        let faded: Vec<_> = meshes.iter().map(|e| material_of(&app, *e)).collect();
        // One copy, shared like the original; the original is untouched.
        assert_eq!(faded[0].id(), faded[1].id());
        assert_ne!(faded[0].id(), own.id());
        let materials = app.world().resource::<Assets<StandardMaterial>>();
        let m = materials.get(&faded[0]).unwrap();
        assert_eq!(m.alpha_mode, AlphaMode::Blend);
        assert!(close(m.base_color.alpha(), 0.75));
        assert_eq!(materials.get(&own).unwrap().alpha_mode, AlphaMode::Opaque);
        assert!(app.world().get::<NotShadowCaster>(meshes[0]).is_none());

        // Opaque, shadowed and no SDF material: nothing changes.
        let (mut app, _, own) = scene_app(|_| GzMeshMaterial {
            replace: None,
            transparency: 0.0,
            cast_shadows: true,
        });
        for e in mesh_entities(&mut app) {
            assert_eq!(material_of(&app, e).id(), own.id());
        }
    }

    #[test]
    fn maps_load_and_metal_rough_combine_once_per_pair() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            crate::GzAssetSourcesPlugin::default(),
            AssetPlugin::default(),
        ))
        .init_asset::<Image>()
        .init_asset::<StandardMaterial>()
        .init_resource::<GzMaterials>()
        .add_systems(Update, combine_metal_rough);
        let server = app.world().resource::<AssetServer>().clone();
        let desc = MaterialDesc {
            emissive: [0.0; 3],
            pbr: Some(PbrDesc {
                albedo_map: Some("model/a.png".into()),
                normal_map: Some("https://fuel.example/n.png".into()),
                metalness_map: Some("/abs/m.png".into()),
                roughness_map: Some("/abs/r.png".into()),
                emissive_map: Some("e.png".into()),
                ambient_occlusion_map: Some("ao.png".into()),
                light_map: Some("l.png".into()),
                light_map_uv: 1,
                ..pbr()
            }),
            ..mat()
        };
        let mut built = Vec::new();
        for _ in 0..2 {
            let world = app.world_mut();
            world.resource_scope(|world, mut state: Mut<GzMaterials>| {
                let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
                built.push(build(
                    &server,
                    &mut materials,
                    &mut state,
                    &desc,
                    0.0,
                    Fallback::GzGrey,
                ));
            });
        }
        let v = &built[0];
        assert!(v.tangents);
        assert_eq!(v.loads.len(), 7);
        assert_eq!(v.lightmap.as_ref().unwrap().1, 1);
        let materials = app.world().resource::<Assets<StandardMaterial>>();
        let m = materials.get(&v.material).unwrap();
        let path = |h: &Option<Handle<Image>>| h.as_ref().unwrap().path().unwrap().to_string();
        assert_eq!(path(&m.base_color_texture), "wsasset://rel/model/a.png");
        assert_eq!(path(&m.normal_map_texture), "gzhttps://fuel.example/n.png");
        assert_eq!(path(&m.emissive_texture), "wsasset://rel/e.png");
        assert_eq!(path(&m.occlusion_texture), "wsasset://rel/ao.png");
        assert_eq!(m.alpha_mode, AlphaMode::Mask(ALPHA_CUTOFF));
        // An emissive map without a colour shines at full.
        assert!(close(units::emission_color(m.emissive).red, 1.0));
        assert!(!app.world().resource::<GzMaterials>().settled());

        // Nothing serves the maps here: stand in for the loads.
        let (metal, rough) = {
            let world = app.world_mut();
            let mut images = world.resource_mut::<Assets<Image>>();
            let metal = server
                .get_handle::<Image>("wsasset://abs/abs/m.png")
                .unwrap();
            let rough = server
                .get_handle::<Image>("wsasset://abs/abs/r.png")
                .unwrap();
            images.insert(&metal, grey(1, 1, 50)).unwrap();
            images.insert(&rough, grey(1, 1, 60)).unwrap();
            (metal, rough)
        };
        app.update();
        assert!(app.world().resource::<GzMaterials>().settled());
        let materials = app.world().resource::<Assets<StandardMaterial>>();
        let t: Vec<_> = built
            .iter()
            .map(|v| {
                materials
                    .get(&v.material)
                    .unwrap()
                    .metallic_roughness_texture
                    .clone()
                    .unwrap()
            })
            .collect();
        assert_eq!(t[0].id(), t[1].id());
        let image = app.world().resource::<Assets<Image>>().get(&t[0]).unwrap();
        assert_eq!(image.data.as_deref(), Some(&[255, 60, 50, 255][..]));
        drop((metal, rough));

        // The specular workflow loads no metalness/roughness maps.
        let spec = MaterialDesc {
            pbr: Some(PbrDesc {
                workflow: PbrWorkflow::Specular,
                ..desc.pbr.clone().unwrap()
            }),
            ..desc
        };
        let world = app.world_mut();
        world.resource_scope(|world, mut state: Mut<GzMaterials>| {
            let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
            let v = build(
                &server,
                &mut materials,
                &mut state,
                &spec,
                0.0,
                Fallback::GzGrey,
            );
            assert_eq!(v.loads.len(), 5);
            assert!(state.settled());
        });
    }

    /// A 1×1 uncompressed A8R8G8B8 DDS (legacy header) of `bgra`.
    fn dds(bgra: [u8; 4]) -> Vec<u8> {
        let mut d = b"DDS ".to_vec();
        let mut put = |v: u32| d.extend_from_slice(&v.to_le_bytes());
        // Header size, flags (caps | height | width | pixel format), h, w,
        // pitch, depth, mip count, 11 reserved.
        for v in [124, 0x1007, 1, 1, 4, 0, 1] {
            put(v);
        }
        for _ in 0..11 {
            put(0);
        }
        // Pixel format: size, RGB | alpha, no FourCC, 32 bits, masks.
        for v in [
            32,
            0x41,
            0,
            32,
            0x00ff_0000,
            0x0000_ff00,
            0x0000_00ff,
            0xff00_0000,
        ] {
            put(v);
        }
        // Caps (texture), caps2-4, reserved.
        for v in [0x1000, 0, 0, 0, 0] {
            put(v);
        }
        d.extend_from_slice(&bgra);
        d
    }

    /// A 1×1 uncompressed 32-bit TGA of `bgra`.
    fn tga(bgra: [u8; 4]) -> Vec<u8> {
        let mut t = vec![0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 32, 0x28];
        t.extend_from_slice(&bgra);
        t
    }

    #[test]
    fn dds_and_tga_textures_load() {
        let dir = std::env::temp_dir().join(format!("gz-bevy-dds-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("t.dds"), dds([0, 0, 255, 255])).unwrap();
        std::fs::write(dir.join("t.tga"), tga([0, 255, 0, 255])).unwrap();
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin {
                file_path: dir.to_string_lossy().into(),
                ..default()
            },
        ))
        .init_asset::<Image>()
        .register_asset_loader(bevy::image::ImageLoader::new(
            bevy::image::CompressedImageFormats::NONE,
        ));
        let server = app.world().resource::<AssetServer>().clone();
        let handles: Vec<Handle<Image>> = ["t.dds", "t.tga"]
            .into_iter()
            .map(|p| {
                server
                    .load_builder()
                    .with_settings(mesh_file::image_settings(
                        true,
                        mesh_file::sampler([true; 2], 1),
                    ))
                    .load(p)
            })
            .collect();
        crate::test_util::wait_until(|| {
            app.update();
            handles
                .iter()
                .all(|h| server.is_loaded(h) || server.load_state(h).is_failed())
        });
        let images = app.world().resource::<Assets<Image>>();
        let red = images.get(&handles[0]).expect("DDS loads");
        assert_eq!(red.get_color_at(0, 0).unwrap().to_srgba().red, 1.0);
        let green = images.get(&handles[1]).expect("TGA loads");
        assert_eq!(green.get_color_at(0, 0).unwrap().to_srgba().green, 1.0);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn meshes_get_tangents_and_light_map_uvs() {
        let mut cube = Mesh::from(Cuboid::default());
        assert!(!cube.contains_attribute(Mesh::ATTRIBUTE_TANGENT));
        prepare_mesh(&mut cube, true, Some(1));
        assert!(cube.contains_attribute(Mesh::ATTRIBUTE_TANGENT));
        assert!(cube.contains_attribute(Mesh::ATTRIBUTE_UV_1));
        // Without UVs: nothing to do, and no failure.
        let mut bare = Mesh::new(
            bevy::mesh::PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; 3]);
        prepare_mesh(&mut bare, true, Some(0));
        assert!(!bare.contains_attribute(Mesh::ATTRIBUTE_TANGENT));
        assert!(!bare.contains_attribute(Mesh::ATTRIBUTE_UV_1));
    }
}
