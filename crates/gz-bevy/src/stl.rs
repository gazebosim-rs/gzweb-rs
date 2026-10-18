//! A Bevy `AssetLoader` for ASCII and binary STL, parsed by `stl_io`.
//!
//! Faces are flat shaded. A face's stored normal is used when it has one;
//! many exporters write zeros, so those faces get the normal of their
//! winding instead.

use std::io::Cursor;

use bevy::asset::io::Reader;
use bevy::asset::{AssetLoader, LoadContext, RenderAssetUsages};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

#[derive(Default, TypePath)]
pub struct StlLoader;

/// Why an `.stl` failed to load.
#[derive(Debug)]
pub struct StlError(std::io::Error);

impl std::fmt::Display for StlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid STL: {}", self.0)
    }
}

impl std::error::Error for StlError {}

impl AssetLoader for StlLoader {
    type Asset = Mesh;
    type Settings = ();
    type Error = StlError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        _cx: &mut LoadContext<'_>,
    ) -> Result<Mesh, StlError> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await.map_err(StlError)?;
        let stl = stl_io::read_stl(&mut Cursor::new(bytes)).map_err(StlError)?;
        Ok(mesh(&stl))
    }

    fn extensions(&self) -> &[&str] {
        &["stl"]
    }
}

fn mesh(stl: &stl_io::IndexedMesh) -> Mesh {
    let n = stl.faces.len() * 3;
    let mut positions = Vec::with_capacity(n);
    let mut normals = Vec::with_capacity(n);
    for face in &stl.faces {
        let v = face
            .vertices
            .map(|i| Vec3::from_array(stl.vertices[i].into()));
        let stored = Vec3::from_array(face.normal.into());
        let normal = if stored.length_squared() > 0.0 {
            stored.normalize()
        } else {
            (v[1] - v[0]).cross(v[2] - v[0]).normalize_or_zero()
        };
        positions.extend(v.map(|p| p.to_array()));
        normals.extend([normal.to_array(); 3]);
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32, 0.0]; n])
    .with_inserted_indices(Indices::U32((0..n as u32).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;

    fn normals(mesh: &Mesh) -> Vec<[f32; 3]> {
        match mesh.attribute(Mesh::ATTRIBUTE_NORMAL) {
            Some(VertexAttributeValues::Float32x3(n)) => n.clone(),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn ascii_faces_keep_stored_normals_or_use_their_winding() {
        let text = "solid t
facet normal 0 0 2
outer loop
vertex 0 0 0
vertex 1 0 0
vertex 0 1 0
endloop
endfacet
facet normal 0 0 0
outer loop
vertex 0 0 0
vertex 0 1 0
vertex 0 0 1
endloop
endfacet
endsolid t
";
        let stl = stl_io::read_stl(&mut Cursor::new(text.as_bytes())).unwrap();
        let m = mesh(&stl);
        assert_eq!(m.count_vertices(), 6);
        assert_eq!(m.indices().unwrap().len(), 6);
        let n = normals(&m);
        assert_eq!(n[0], [0.0, 0.0, 1.0]);
        assert_eq!(n[3], [1.0, 0.0, 0.0]);
    }

    #[test]
    fn loads_binary_stl_through_the_asset_server() {
        use bevy::asset::io::memory::{Dir, MemoryAssetReader};
        use bevy::asset::io::{AssetSourceBuilder, AssetSourceId};

        let mut bytes = vec![0u8; 80];
        bytes.extend(1u32.to_le_bytes());
        for f in [
            0.0f32, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0,
        ] {
            bytes.extend(f.to_le_bytes());
        }
        bytes.extend([0, 0]);
        let dir = Dir::default();
        dir.insert_asset(std::path::Path::new("t.stl"), bytes);
        dir.insert_asset(
            std::path::Path::new("bad.stl"),
            b"solid x\nnonsense".to_vec(),
        );

        let mut app = App::new();
        app.register_asset_source(
            AssetSourceId::Default,
            AssetSourceBuilder::new(move || Box::new(MemoryAssetReader { root: dir.clone() })),
        )
        .add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Mesh>()
        .init_asset_loader::<StlLoader>();
        let server = app.world().resource::<AssetServer>().clone();
        let good: Handle<Mesh> = server.load("t.stl");
        let bad: Handle<Mesh> = server.load("bad.stl");
        for _ in 0..200 {
            app.update();
            if server.is_loaded(&good) && server.load_state(&bad).is_failed() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let meshes = app.world().resource::<Assets<Mesh>>();
        assert_eq!(meshes.get(&good).unwrap().count_vertices(), 3);
        assert!(server.load_state(&bad).is_failed());
        assert!(StlError(std::io::Error::other("x"))
            .to_string()
            .contains("invalid STL"));
    }
}
