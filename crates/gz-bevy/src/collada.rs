//! COLLADA (`.dae`) for [`SceneFileLoader`](crate::mesh_file::SceneFileLoader):
//! [`gz_collada`]'s node tree becomes a [`Scene`], like the glTF loader's.
//!
//! - The root entity (unnamed) scales by the document's `<unit>`;
//!   `<up_axis>` is ignored, as in Gazebo and gzweb.
//! - Every COLLADA node is an entity with its local `Transform` and, if it
//!   has a `name` attribute, that `Name` (gz-common names submeshes after
//!   it, never the `id`); each primitive is a child with its mesh and
//!   material.
//! - Textures load through the same sources as the `.dae` (`gzhttps://`,
//!   `wsasset://`), relative to it, and repeat unless the file says not to.
//!   One that fails to load is dropped from its material
//!   ([`drop_failed_textures`]).
//!
//! Materials are COLLADA's Phong/Blinn/Lambert effects mapped onto Bevy's
//! PBR `StandardMaterial` (see `material` for the rules).

use bevy::asset::{AssetLoadFailedEvent, LoadContext};
use bevy::color::Luminance;
use bevy::prelude::*;
use bevy::render::render_resource::Face;

use crate::mesh_file::{self, MeshParts};
use crate::render::units;

/// Parses a `.dae` file into a scene.
pub fn load(bytes: &[u8], cx: &mut LoadContext<'_>) -> Result<Scene, gz_collada::Error> {
    let dae = gz_collada::parse(bytes)?;
    for warning in &dae.warnings {
        warn!("{}: {warning}", cx.path());
    }
    Ok(build_scene(dae, cx))
}

fn build_scene(dae: gz_collada::Scene, cx: &mut LoadContext<'_>) -> Scene {
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
        .into_iter()
        .zip(needs_tangents)
        .enumerate()
        .map(|(i, (m, tangents))| cx.add_labeled_asset(format!("Mesh{i}"), mesh(m, tangents)))
        .collect();
    let dae_path = cx.path().clone_owned();
    let materials: Vec<Handle<StandardMaterial>> = dae
        .materials
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let mut mat = material(m);
            let texture =
                |cx: &mut LoadContext<'_>, tex: &Option<gz_collada::Texture>, srgb: bool| {
                    let tex = tex.as_ref()?;
                    let path = mesh_file::texture_path(&dae_path, &tex.path)?;
                    let sampler = mesh_file::sampler(tex.repeat, 1);
                    mesh_file::load_texture(cx, path, srgb, sampler)
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
    let mut entity = world.spawn((
        transform(&node.transform),
        Visibility::default(),
        ChildOf(parent),
    ));
    if let Some(name) = &node.name {
        entity.insert(Name::new(name.clone()));
    }
    let entity = entity.id();
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
fn transform(rows: &[[f32; 4]; 4]) -> Transform {
    let m = Mat4::from_cols_array_2d(rows).transpose();
    Transform::from_matrix(m)
}

/// Unindexed triangles as a Bevy mesh (flat normals and tangents work
/// without an index buffer); see [`MeshParts::build`].
fn mesh(m: gz_collada::Mesh, tangents: bool) -> Mesh {
    MeshParts {
        positions: m.positions,
        normals: m.normals,
        uvs: m.texcoords,
        colors: m.colors,
        indices: None,
    }
    .build(tangents, "COLLADA mesh")
}

/// A COLLADA effect as a PBR material (textures are added by the loader).
///
/// - Base colour is the diffuse colour, or white under a diffuse texture
///   (COLLADA's default grey would darken it). Alpha is the opacity alone,
///   as in three.js, which ignores the diffuse colour's alpha; below 1 it
///   blends. A `<transparent>` texture with full opacity is a cut-out: the
///   diffuse texture's alpha is masked at 0.5 (three.js blends; a mask
///   sorts and casts shadows correctly).
/// - Phong/Blinn: roughness follows the exponent
///   ([`mesh_file::phong_roughness`]). Reflectance (F0, 4% by default) scales with the specular colour's
///   luminance. Lambert has no highlight: roughness 1.
/// - Nothing is metallic. Emission (white under an emission texture) uses
///   [`units::emissive`]; `constant` shading is unlit.
fn material(m: &gz_collada::Material) -> StandardMaterial {
    let [r, g, b, _] = if m.diffuse_texture.is_some() {
        [1.0; 4]
    } else {
        m.diffuse
    };
    let alpha = m.opacity.clamp(0.0, 1.0);
    let alpha_mode = if alpha < 1.0 {
        AlphaMode::Blend
    } else if m.transparent_texture.is_some() {
        AlphaMode::Mask(0.5)
    } else {
        AlphaMode::Opaque
    };
    let lambert = m.shading == gz_collada::Shading::Lambert;
    let [sr, sg, sb, _] = m.specular;
    // Bevy's F0 is 0.16 × reflectance²; its default 0.5 is 4%.
    let specular = LinearRgba::from(Color::srgb(sr, sg, sb)).luminance();
    let [er, eg, eb, _] = m.emission;
    let emission = if m.emission_texture.is_some() {
        Color::WHITE
    } else {
        Color::srgb(er, eg, eb)
    };
    StandardMaterial {
        base_color: Color::srgba(r, g, b, alpha),
        alpha_mode,
        perceptual_roughness: if lambert {
            1.0
        } else {
            mesh_file::phong_roughness(m.shininess)
        },
        reflectance: if lambert {
            0.5
        } else {
            0.5 * specular.clamp(0.0, 1.0).sqrt()
        },
        metallic: 0.0,
        emissive: units::emissive(emission),
        unlit: m.shading == gz_collada::Shading::Constant,
        double_sided: m.double_sided,
        cull_mode: (!m.double_sided).then_some(Face::Back),
        ..default()
    }
}

/// Drops textures that failed to load from every `StandardMaterial` (the
/// COLLADA, OBJ and glTF loaders' alike). Bevy waits for a texture before it
/// draws the material, forever if the texture failed, so the mesh would
/// never appear; without it, the mesh renders untextured.
///
/// Checks every material when an image fails, and each new material
/// (its texture may have failed before the material was added).
pub fn drop_failed_textures(
    mut failed: MessageReader<AssetLoadFailedEvent<Image>>,
    mut events: MessageReader<AssetEvent<StandardMaterial>>,
    server: Res<AssetServer>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let added = events.read().filter_map(|e| match e {
        AssetEvent::Added { id } => Some(*id),
        _ => None,
    });
    let check: Vec<AssetId<StandardMaterial>> = if failed.read().count() > 0 {
        materials.ids().collect()
    } else {
        added.collect()
    };
    let is_failed =
        |t: &Option<Handle<Image>>| t.as_ref().is_some_and(|h| server.load_state(h).is_failed());
    for id in check {
        let any = materials.get(id).is_some_and(|m| {
            [
                &m.base_color_texture,
                &m.normal_map_texture,
                &m.emissive_texture,
                &m.metallic_roughness_texture,
                &m.occlusion_texture,
            ]
            .into_iter()
            .any(is_failed)
        });
        let Some(m) = materials.get_mut(id).filter(|_| any) else {
            continue;
        };
        for texture in [
            &mut m.base_color_texture,
            &mut m.normal_map_texture,
            &mut m.emissive_texture,
            &mut m.metallic_roughness_texture,
            &mut m.occlusion_texture,
        ] {
            if is_failed(texture) {
                *texture = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::image::{ImageAddressMode, ImageSampler};

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
    fn effects_become_pbr_materials() {
        let probe = dae("probe.dae");
        let red = material(&probe.materials[0]);
        assert_eq!(red.base_color, Color::srgba(1.0, 0.0, 0.0, 1.0));
        assert_eq!(red.alpha_mode, AlphaMode::Opaque);
        assert_eq!(red.cull_mode, Some(Face::Back));
        // A Lambert effect: no highlight.
        assert_eq!(probe.materials[0].shading, gz_collada::Shading::Lambert);
        assert_eq!((red.perceptual_roughness, red.reflectance), (1.0, 0.5));
        let textured = material(&probe.materials[1]);
        // White under the texture, half transparent.
        assert_eq!(textured.base_color, Color::srgba(1.0, 1.0, 1.0, 0.5));
        assert_eq!(textured.alpha_mode, AlphaMode::Blend);
        // Phong exponent 20: GGX α = sqrt(2/22), perceptual = sqrt(α).
        let shiny = (2.0f32 / 22.0).powf(0.25);
        assert!((textured.perceptual_roughness - shiny).abs() < 1e-6);
        let unlit = material(&gz_collada::Material {
            shading: gz_collada::Shading::Constant,
            double_sided: true,
            ..default()
        });
        assert!(unlit.unlit && unlit.double_sided && unlit.cull_mode.is_none());
    }

    #[test]
    fn opacity_specular_and_emission() {
        // Diffuse alpha is ignored, as in three.js.
        let m = material(&gz_collada::Material {
            diffuse: [1.0, 1.0, 1.0, 0.2],
            ..default()
        });
        assert_eq!(m.alpha_mode, AlphaMode::Opaque);
        let texture = Some(gz_collada::Texture {
            path: "a.png".into(),
            repeat: [true; 2],
        });
        let cutout = material(&gz_collada::Material {
            transparent_texture: texture.clone(),
            ..default()
        });
        assert_eq!(cutout.alpha_mode, AlphaMode::Mask(0.5));
        // Black specular: no reflectance; white: Bevy's default 4%.
        let black = material(&gz_collada::Material {
            specular: [0.0, 0.0, 0.0, 1.0],
            ..default()
        });
        let white = material(&gz_collada::Material {
            specular: [1.0; 4],
            ..default()
        });
        assert_eq!((black.reflectance, white.reflectance), (0.0, 0.5));
        // An emission texture glows white × the texture; a colour of 1
        // renders at 1.
        let glow = material(&gz_collada::Material {
            emission_texture: texture,
            ..default()
        });
        assert_eq!(glow.emissive, units::emissive(Color::WHITE));
        let back = units::emission_color(glow.emissive);
        assert!((back.red - 1.0).abs() < 1e-6, "{back:?}");
        assert_eq!(material(&default()).emissive, LinearRgba::BLACK);
    }

    #[test]
    fn meshes_are_triangle_soups() {
        let soup = gz_collada::Mesh {
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            ..default()
        };
        let m = mesh(soup, false);
        assert!(m.indices().is_none());
        assert!(m.contains_attribute(Mesh::ATTRIBUTE_NORMAL));
    }

    #[test]
    fn nodes_are_named_by_name_never_id() {
        let mut probe = dae("probe.dae");
        probe.roots[0].name = None;
        let id = probe.roots[0].id.clone().unwrap();
        let mut world = World::new();
        let root = world.spawn_empty().id();
        let handles = vec![Handle::default(); probe.meshes.len()];
        let mats = vec![Handle::default(); probe.materials.len()];
        spawn_node(
            &mut world,
            &probe.roots[0],
            root,
            &handles,
            &mats,
            &Handle::default(),
        );
        let names: Vec<String> = world
            .query::<&Name>()
            .iter(&world)
            .map(|n| n.to_string())
            .collect();
        assert!(!names.contains(&id), "{names:?}");
        assert!(names.contains(&"ChildNode".to_string()), "{names:?}");
    }

    /// A 1×1 white PNG.
    const PNG: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f,
        0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0b, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8,
        0x0f, 0x04, 0x00, 0x09, 0xfb, 0x03, 0xfd, 0xfb, 0x5e, 0x6b, 0x2b, 0x00, 0x00, 0x00, 0x00,
        0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];

    /// An app that loads `.dae` files and PNGs from `dir`.
    fn loader_app(dir: &std::path::Path) -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin {
                file_path: dir.to_string_lossy().into(),
                ..default()
            },
        ))
        .init_asset::<Mesh>()
        .init_asset::<Image>()
        .init_asset::<StandardMaterial>()
        .init_asset::<Scene>()
        .register_asset_loader(bevy::image::ImageLoader::new(
            bevy::image::CompressedImageFormats::NONE,
        ))
        .init_asset_loader::<mesh_file::SceneFileLoader>()
        .add_systems(Update, drop_failed_textures);
        app
    }

    fn update_until(app: &mut App, done: impl Fn(&World) -> bool) {
        for _ in 0..500 {
            app.update();
            if done(app.world()) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        panic!("timed out");
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("gz-bevy-collada-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_missing_texture_leaves_the_material_drawable() {
        let dir = scratch("missing");
        std::fs::write(dir.join("ok.png"), PNG).unwrap();
        // Two textured materials; one image is missing.
        let probe = dae("probe.dae");
        assert!(probe.materials[1].diffuse_texture.is_some());
        let text = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../gz-collada/tests/data/probe.dae"),
        )
        .unwrap()
        .replace("../materials/textures/probe.png", "ok.png")
        .replace("../materials/textures/probe_normal.png", "missing.png");
        std::fs::write(dir.join("m.dae"), text).unwrap();

        let mut app = loader_app(&dir);
        let scene: Handle<Scene> = app.world().resource::<AssetServer>().load("m.dae");
        update_until(&mut app, |w| {
            let server = w.resource::<AssetServer>();
            server.is_loaded_with_dependencies(&scene)
                || server
                    .get_recursive_dependency_load_state(&scene)
                    .is_some_and(|s| s.is_failed())
        });
        // The failure reaches the material a frame or two later.
        let textured = |w: &World| {
            let handle = w
                .resource::<AssetServer>()
                .get_handle::<StandardMaterial>("m.dae#Material1")
                .unwrap();
            w.resource::<Assets<StandardMaterial>>()
                .get(&handle)
                .unwrap()
                .clone()
        };
        update_until(&mut app, |w| textured(w).normal_map_texture.is_none());
        let m = textured(app.world());
        let image = m.base_color_texture.unwrap();
        let images = app.world().resource::<Assets<Image>>();
        let ImageSampler::Descriptor(d) = &images.get(&image).unwrap().sampler else {
            panic!("default sampler");
        };
        assert_eq!(d.address_mode_u, ImageAddressMode::Repeat);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_material_added_after_its_texture_failed_is_fixed_too() {
        let dir = scratch("late");
        let mut app = loader_app(&dir);
        let image: Handle<Image> = app.world().resource::<AssetServer>().load("gone.png");
        update_until(&mut app, |w| {
            w.resource::<AssetServer>().load_state(&image).is_failed()
        });
        let material = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial {
                base_color_texture: Some(image),
                ..default()
            });
        update_until(&mut app, |w| {
            w.resource::<Assets<StandardMaterial>>()
                .get(&material)
                .unwrap()
                .base_color_texture
                .is_none()
        });
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
