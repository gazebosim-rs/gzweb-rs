//! What the COLLADA and OBJ loaders share: the asset loader that picks
//! between them, texture loading, the Phong roughness rule and the mesh
//! attributes every format ends with.
//!
//! [`SceneFileLoader`] is the only loader of [`Scene`]s from files, so Bevy
//! picks it by asset type whatever the extension's case (`.DAE`, `.Obj`),
//! and it picks the format from the lower-cased extension. Two loaders for
//! one asset type would be chosen by extension, which Bevy matches
//! case-sensitively: an upper-case `.DAE` went to the OBJ loader.

use bevy::asset::io::Reader;
use bevy::asset::{AssetLoader, AssetPath, LoadContext, RenderAssetUsages};
use bevy::image::{ImageAddressMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::{assets, collada, obj};

/// Loads `.dae` and `.obj` files (any case) as scenes.
#[derive(Default, TypePath)]
pub struct SceneFileLoader;

/// Why a mesh file failed to load.
#[derive(Debug)]
pub enum SceneFileError {
    Io(std::io::Error),
    Collada(gz_collada::Error),
    Obj(tobj::LoadError),
    /// An extension this loader doesn't read.
    Format(String),
}

impl std::fmt::Display for SceneFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SceneFileError::Io(e) => write!(f, "reading the file: {e}"),
            SceneFileError::Collada(e) => write!(f, "invalid COLLADA: {e}"),
            SceneFileError::Obj(e) => write!(f, "invalid OBJ: {e}"),
            SceneFileError::Format(ext) => write!(f, "not a COLLADA or OBJ file ('{ext}')"),
        }
    }
}

impl std::error::Error for SceneFileError {}

impl AssetLoader for SceneFileLoader {
    type Asset = Scene;
    type Settings = ();
    type Error = SceneFileError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        cx: &mut LoadContext<'_>,
    ) -> Result<Scene, SceneFileError> {
        let ext = gz_core::uri::extension(&cx.path().path().to_string_lossy());
        if !matches!(ext.as_str(), "dae" | "obj") {
            return Err(SceneFileError::Format(ext));
        }
        let mut bytes = Vec::new();
        reader
            .read_to_end(&mut bytes)
            .await
            .map_err(SceneFileError::Io)?;
        if ext == "dae" {
            collada::load(&bytes, cx).map_err(SceneFileError::Collada)
        } else {
            obj::load(&bytes, cx).await.map_err(SceneFileError::Obj)
        }
    }

    /// Both cases, for when another `Scene` loader is registered and Bevy
    /// has to choose by extension; mixed case relies on this being the only
    /// one.
    fn extensions(&self) -> &[&str] {
        &["dae", "DAE", "obj", "OBJ"]
    }
}

/// Linear filtering, repeating or clamped per axis, with up to
/// `anisotropy` samples (1 = none).
pub fn sampler(repeat: [bool; 2], anisotropy: u16) -> ImageSampler {
    let mode = |repeat| {
        if repeat {
            ImageAddressMode::Repeat
        } else {
            ImageAddressMode::ClampToEdge
        }
    };
    ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: mode(repeat[0]),
        address_mode_v: mode(repeat[1]),
        anisotropy_clamp: anisotropy.max(1),
        ..ImageSamplerDescriptor::linear()
    })
}

/// Image loader settings: sRGB (colour) or linear (normals, heights), and
/// the sampler.
pub fn image_settings(
    srgb: bool,
    sampler: ImageSampler,
) -> impl Fn(&mut ImageLoaderSettings) + Send + Sync + 'static {
    move |s: &mut ImageLoaderSettings| {
        s.is_srgb = srgb;
        s.sampler = sampler.clone();
    }
}

/// Loads a texture a mesh file names, as a dependency of the file. `None`
/// for a path that climbs out of its source (`../` past the top), which
/// Bevy refuses (and would panic on inside a loader).
pub fn load_texture(
    cx: &mut LoadContext<'_>,
    path: AssetPath<'static>,
    srgb: bool,
    sampler: ImageSampler,
) -> Option<Handle<Image>> {
    if path.is_unapproved() {
        warn!("{}: texture '{path}' is outside its source", cx.path());
        return None;
    }
    Some(
        cx.loader()
            .with_settings(image_settings(srgb, sampler))
            .load(path),
    )
}

/// Where a texture a mesh file names lives: a URI or absolute path through
/// the usual sources ([`gz_core::uri::mesh_load_uri`], then
/// [`assets::asset_path`]), else relative to the file itself.
pub fn texture_path(file: &AssetPath<'_>, texture: &str) -> Option<AssetPath<'static>> {
    let absolute = texture.contains("://") || texture.starts_with('/');
    if absolute {
        let load_uri = gz_core::uri::mesh_load_uri(texture);
        return AssetPath::try_parse(&assets::asset_path(&load_uri))
            .ok()
            .map(AssetPath::into_owned);
    }
    file.resolve_embed(texture).ok()
}

/// Perceptual roughness for a Phong/Blinn exponent: `sqrt(2 / (n + 2))` is
/// the GGX α of the usual Blinn-Phong mapping, and Bevy squares perceptual
/// roughness to get α, so this is its square root. Clamped to Bevy's range.
pub fn phong_roughness(shininess: f32) -> f32 {
    (2.0 / (shininess.max(0.0) + 2.0))
        .powf(0.25)
        .clamp(0.089, 1.0)
}

/// A mesh file's triangles, before they become a Bevy [`Mesh`]. An
/// attribute whose length differs from `positions` is ignored.
#[derive(Debug, Default, Clone)]
pub struct MeshParts {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    /// V pointing up, as in COLLADA, OBJ and three.js.
    pub uvs: Vec<[f32; 2]>,
    /// sRGB, as three.js reads COLLADA and OBJ vertex colours.
    pub colors: Vec<[f32; 4]>,
    /// `None` for a triangle soup.
    pub indices: Option<Vec<u32>>,
}

impl MeshParts {
    /// The Bevy mesh: V flipped (Bevy's points down), colours linear, flat
    /// normals when there are none (three.js computes them on unindexed
    /// triangles, so they are flat too), and tangents when `tangents` and
    /// there are UVs. `what` names the mesh in warnings.
    pub fn build(self, tangents: bool, what: &str) -> Mesh {
        let n = self.positions.len();
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions);
        let indexed = self.indices.is_some();
        if let Some(indices) = self.indices {
            mesh.insert_indices(Indices::U32(indices));
        }
        if self.uvs.len() == n {
            let uvs: Vec<[f32; 2]> = self.uvs.into_iter().map(|[u, v]| [u, 1.0 - v]).collect();
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
        }
        if self.colors.len() == n {
            let linear: Vec<[f32; 4]> = self
                .colors
                .into_iter()
                .map(|[r, g, b, a]| LinearRgba::from(Color::srgba(r, g, b, a)).to_f32_array())
                .collect();
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, linear);
        }
        if self.normals.len() == n {
            mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals);
        } else {
            if indexed {
                mesh.duplicate_vertices();
            }
            mesh.compute_flat_normals();
        }
        if tangents && mesh.contains_attribute(Mesh::ATTRIBUTE_UV_0) {
            if let Err(e) = mesh.generate_tangents() {
                warn!("{what}: no tangents for its normal map: {e}");
            }
        }
        mesh
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;

    fn tri() -> MeshParts {
        MeshParts {
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            uvs: vec![[0.0, 0.25], [1.0, 0.0], [0.0, 1.0]],
            colors: vec![[0.5, 1.0, 0.0, 1.0]; 3],
            ..default()
        }
    }

    #[test]
    fn meshes_flip_v_linearise_colours_and_fill_in_normals() {
        for indices in [None, Some(vec![0, 1, 2])] {
            let m = MeshParts {
                indices: indices.clone(),
                ..tri()
            }
            .build(true, "t");
            assert!(m.contains_attribute(Mesh::ATTRIBUTE_NORMAL));
            assert!(m.contains_attribute(Mesh::ATTRIBUTE_TANGENT));
            let Some(VertexAttributeValues::Float32x4(colors)) = m.attribute(Mesh::ATTRIBUTE_COLOR)
            else {
                panic!("no colours");
            };
            // sRGB 0.5 is linear 0.214.
            assert!((colors[0][0] - 0.214).abs() < 1e-3, "{:?}", colors[0]);
            assert_eq!(colors[0][1..], [1.0, 0.0, 1.0]);
            let Some(VertexAttributeValues::Float32x2(uv)) = m.attribute(Mesh::ATTRIBUTE_UV_0)
            else {
                panic!("no UVs");
            };
            assert_eq!(uv[0], [0.0, 0.75]);
        }
        // No UVs: no tangents, and no failure. Given normals are kept.
        let bare = MeshParts {
            positions: tri().positions,
            normals: vec![[0.0, 0.0, 1.0]; 3],
            indices: Some(vec![0, 1, 2]),
            ..default()
        }
        .build(true, "t");
        assert!(!bare.contains_attribute(Mesh::ATTRIBUTE_TANGENT));
        assert!(bare.indices().is_some());
        // Wrong-length attributes are ignored.
        let odd = MeshParts {
            uvs: vec![[0.0; 2]],
            ..tri()
        }
        .build(false, "t");
        assert!(!odd.contains_attribute(Mesh::ATTRIBUTE_UV_0));
    }

    #[test]
    fn any_case_of_dae_and_obj_loads_with_the_right_parser() {
        let dir = std::env::temp_dir().join(format!("gz-bevy-case-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dae = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../gz-collada/tests/data/probe.dae"),
        )
        .unwrap();
        // probe.dae's textures are in `../materials`, above this source's
        // root: refused (Bevy would panic in the loader), not loaded.
        let obj = b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";
        let files: [(&str, &[u8], usize); 6] = [
            ("a.dae", &dae, 5),
            ("b.DAE", &dae, 5),
            ("c.Dae", &dae, 5),
            ("d.OBJ", obj, 1),
            ("e.Obj", obj, 1),
            ("f.stl", obj, 0),
        ];
        for (name, bytes, _) in &files {
            std::fs::write(dir.join(name), bytes).unwrap();
        }
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
        .init_asset_loader::<SceneFileLoader>();
        let server = app.world().resource::<AssetServer>().clone();
        let handles: Vec<Handle<Scene>> = files.iter().map(|(n, ..)| server.load(*n)).collect();
        for _ in 0..500 {
            app.update();
            let done = handles
                .iter()
                .all(|h| server.is_loaded(h) || server.load_state(h).is_failed());
            if done {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let mut scenes = app.world_mut().resource_mut::<Assets<Scene>>();
        for ((name, _, meshes), h) in files.iter().zip(&handles) {
            match scenes.get_mut(h) {
                Some(scene) => {
                    let n = scene.world.query::<&Mesh3d>().iter(&scene.world).count();
                    assert_eq!(n, *meshes, "{name}");
                }
                None => assert_eq!(*meshes, 0, "{name} failed"),
            }
        }
        assert!(server.load_state(&handles[5]).is_failed());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn phong_roughness_is_clamped() {
        assert!((phong_roughness(20.0) - (2.0f32 / 22.0).powf(0.25)).abs() < 1e-6);
        assert_eq!(phong_roughness(0.0), 1.0);
        assert_eq!(phong_roughness(-5.0), 1.0);
        assert_eq!(phong_roughness(1e9), 0.089);
    }

    #[test]
    fn samplers_repeat_unless_told_not_to() {
        let ImageSampler::Descriptor(d) = sampler([true, false], 1) else {
            panic!("no descriptor");
        };
        assert_eq!(d.address_mode_u, ImageAddressMode::Repeat);
        assert_eq!(d.address_mode_v, ImageAddressMode::ClampToEdge);
        assert_eq!(d.mag_filter, bevy::image::ImageFilterMode::Linear);
        assert_eq!(d.anisotropy_clamp, 1);
        let ImageSampler::Descriptor(d) = sampler([true; 2], 16) else {
            panic!("no descriptor");
        };
        assert_eq!(d.anisotropy_clamp, 16);
        let mut s = ImageLoaderSettings::default();
        image_settings(false, sampler([true; 2], 1))(&mut s);
        assert!(!s.is_srgb);
        assert!(matches!(s.sampler, ImageSampler::Descriptor(_)));
    }

    #[test]
    fn textures_resolve_relative_to_the_file_or_through_the_sources() {
        let dae = AssetPath::parse("gzhttps://fuel.example/m/1/files/meshes/a.dae");
        assert_eq!(
            texture_path(&dae, "../materials/textures/a.png").unwrap(),
            AssetPath::parse("gzhttps://fuel.example/m/1/files/materials/textures/a.png")
        );
        assert_eq!(
            texture_path(&dae, "https://fuel.example/x.png").unwrap(),
            AssetPath::parse("gzhttps://fuel.example/x.png")
        );
        assert_eq!(
            texture_path(&dae, "/abs/x.png").unwrap(),
            AssetPath::parse("wsasset://abs/abs/x.png")
        );
        assert_eq!(
            texture_path(&dae, "file:///home/u/x.png").unwrap(),
            AssetPath::parse("wsasset://abs/home/u/x.png")
        );
        assert_eq!(
            texture_path(&dae, "http://localhost:8000/x.png").unwrap(),
            AssetPath::parse("gzhttps://http:/localhost:8000/x.png")
        );
    }
}
