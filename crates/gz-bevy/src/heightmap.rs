//! Heightmap terrain.
//!
//! gzweb only draws heights carried in the message, which Gazebo never
//! sends, so its terrain is empty. Like Gazebo we load the heightmap image
//! (grayscale, black = 0, white = `size.z`), sample it `sampling` times per
//! pixel with bilinear filtering, and lay it over `size.x` × `size.y` centred
//! on `origin`, the image's top row at +Y. Heights in the message are used
//! as they are.
//!
//! The first texture layer covers the terrain, repeating every `size`
//! metres (as in gzweb). Height blending between layers is not done yet.

use bevy::asset::{LoadState, RenderAssetUsages};
use bevy::image::{ImageAddressMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};
use bevy::math::Affine2;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use gz_core::scene::HeightmapDesc;

use crate::assets;

/// Vertices along a side are capped here (about a million in all).
pub const MAX_VERTICES_PER_SIDE: usize = 1025;

/// Waits for its image, then gets the terrain mesh and material.
#[derive(Component, Debug)]
pub struct GzHeightmap {
    pub desc: HeightmapDesc,
    pub image: Option<Handle<Image>>,
    /// Set once the terrain is built, or its image failed.
    pub built: bool,
}

/// A grid of heights in metres, row 0 at +Y.
#[derive(Debug, Clone, PartialEq)]
pub struct Grid {
    pub width: usize,
    pub height: usize,
    pub heights: Vec<f32>,
}

/// Starts loading the heightmap (and its textures) for a visual.
pub fn spawn(
    commands: &mut Commands,
    asset_server: &AssetServer,
    materials: &mut Assets<StandardMaterial>,
    desc: &HeightmapDesc,
    parent: Entity,
) -> Option<UntypedHandle> {
    let image: Option<Handle<Image>> = desc.image_uri.as_ref().map(|uri| {
        asset_server.load_with_settings(assets::asset_path(uri), |s: &mut ImageLoaderSettings| {
            s.is_srgb = false;
        })
    });
    let material = materials.add(material(asset_server, desc));
    commands.spawn((
        GzHeightmap {
            desc: desc.clone(),
            image: image.clone(),
            built: false,
        },
        MeshMaterial3d(material),
        Transform::IDENTITY,
        Visibility::default(),
        ChildOf(parent),
    ));
    image.map(|h| h.untyped())
}

/// The first layer's textures, repeating every `size` metres.
fn material(asset_server: &AssetServer, desc: &HeightmapDesc) -> StandardMaterial {
    let Some(layer) = desc.layers.first() else {
        // gzweb's untextured terrain colour.
        return StandardMaterial {
            base_color: Color::srgb_u8(0x55, 0x55, 0x55),
            perceptual_roughness: 1.0,
            ..default()
        };
    };
    let texture = |uri: &Option<String>, srgb: bool| {
        uri.as_ref().map(|uri| {
            asset_server.load_with_settings(
                assets::asset_path(uri),
                move |s: &mut ImageLoaderSettings| {
                    s.is_srgb = srgb;
                    s.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                        address_mode_u: ImageAddressMode::Repeat,
                        address_mode_v: ImageAddressMode::Repeat,
                        anisotropy_clamp: 16,
                        ..ImageSamplerDescriptor::linear()
                    });
                },
            )
        })
    };
    let repeats = Vec2::new(
        (desc.size[0] / layer.size) as f32,
        (desc.size[1] / layer.size) as f32,
    );
    StandardMaterial {
        base_color_texture: texture(&layer.diffuse, true),
        normal_map_texture: texture(&layer.normal, false),
        uv_transform: Affine2::from_scale(repeats),
        perceptual_roughness: 1.0,
        ..default()
    }
}

/// Builds terrains whose heights are ready.
pub fn build(
    mut commands: Commands,
    mut terrains: Query<(Entity, &mut GzHeightmap)>,
    images: Res<Assets<Image>>,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    for (entity, mut terrain) in &mut terrains {
        if terrain.built {
            continue;
        }
        let grid = match &terrain.image {
            None => Some(Grid {
                width: terrain.desc.width as usize,
                height: terrain.desc.height as usize,
                heights: terrain.desc.heights.clone(),
            }),
            Some(handle) => match asset_server.load_state(handle) {
                LoadState::Loaded => images.get(handle).and_then(|image| {
                    let grid = image_grid(image, terrain.desc.size[2] as f32);
                    if grid.is_none() {
                        warn!(
                            "heightmap: unsupported image format {:?}",
                            image.texture_descriptor.format
                        );
                    }
                    grid
                }),
                LoadState::Failed(e) => {
                    warn!("heightmap: {e}");
                    None
                }
                _ => continue,
            },
        };
        terrain.built = true;
        // Without heights the terrain is an empty mesh, which still carries
        // its material (gzweb's terrain is always empty).
        let mesh = grid
            .and_then(|g| {
                terrain_mesh(&resample(&g, terrain.desc.sampling as usize), &terrain.desc)
            })
            .unwrap_or_else(empty_mesh);
        commands.entity(entity).insert(Mesh3d(meshes.add(mesh)));
    }
}

fn empty_mesh() -> Mesh {
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, Vec::<[f32; 3]>::new())
}

/// Image values as heights (0..=`scale`), or `None` if a pixel can't be read.
pub fn image_grid(image: &Image, scale: f32) -> Option<Grid> {
    let (width, height) = (image.width() as usize, image.height() as usize);
    let mut heights = Vec::with_capacity(width * height);
    for y in 0..height as u32 {
        for x in 0..width as u32 {
            let c = image.get_color_at(x, y).ok()?.to_linear();
            heights.push(c.red * scale);
        }
    }
    Some(Grid {
        width,
        height,
        heights,
    })
}

/// `sampling` vertices per input step along each side (bilinear), capped at
/// [`MAX_VERTICES_PER_SIDE`].
pub fn resample(grid: &Grid, sampling: usize) -> Grid {
    let side = |n: usize| {
        if n < 2 {
            n
        } else {
            ((n - 1) * sampling.max(1) + 1).min(MAX_VERTICES_PER_SIDE)
        }
    };
    let (w, h) = (side(grid.width), side(grid.height));
    if (w, h) == (grid.width, grid.height) {
        return grid.clone();
    }
    let at = |x: usize, y: usize| grid.heights[y * grid.width + x];
    let mut heights = Vec::with_capacity(w * h);
    for r in 0..h {
        let fy = r as f32 * (grid.height - 1) as f32 / (h - 1).max(1) as f32;
        let (y0, ty) = (fy.floor() as usize, fy.fract());
        let y1 = (y0 + 1).min(grid.height - 1);
        for c in 0..w {
            let fx = c as f32 * (grid.width - 1) as f32 / (w - 1).max(1) as f32;
            let (x0, tx) = (fx.floor() as usize, fx.fract());
            let x1 = (x0 + 1).min(grid.width - 1);
            let top = at(x0, y0) * (1.0 - tx) + at(x1, y0) * tx;
            let bottom = at(x0, y1) * (1.0 - tx) + at(x1, y1) * tx;
            heights.push(top * (1.0 - ty) + bottom * ty);
        }
    }
    Grid {
        width: w,
        height: h,
        heights,
    }
}

/// The terrain in the visual's Gazebo frame, with smooth normals, UVs over
/// [0, 1] (the material repeats them) and tangents. `None` below 2×2.
pub fn terrain_mesh(grid: &Grid, desc: &HeightmapDesc) -> Option<Mesh> {
    let (w, h) = (grid.width, grid.height);
    if w < 2 || h < 2 || grid.heights.len() != w * h {
        return None;
    }
    let [sx, sy, _] = desc.size.map(|v| v as f32);
    let [ox, oy, oz] = desc.origin.map(|v| v as f32);
    let mut positions = Vec::with_capacity(w * h);
    let mut uvs = Vec::with_capacity(w * h);
    for r in 0..h {
        let v = r as f32 / (h - 1) as f32;
        for c in 0..w {
            let u = c as f32 / (w - 1) as f32;
            positions.push([
                ox + (u - 0.5) * sx,
                oy + (0.5 - v) * sy,
                oz + grid.heights[r * w + c],
            ]);
            uvs.push([u, v]);
        }
    }
    let mut indices = Vec::with_capacity((w - 1) * (h - 1) * 6);
    for r in 0..h - 1 {
        for c in 0..w - 1 {
            let i = (r * w + c) as u32;
            let (right, down) = (i + 1, i + w as u32);
            // Counter-clockwise seen from +Z.
            indices.extend([i, down, right, right, down, down + 1]);
        }
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(indices));
    mesh.compute_smooth_normals();
    if let Err(e) = mesh.generate_tangents() {
        warn!("heightmap: no tangents: {e}");
    }
    Some(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;

    fn desc() -> HeightmapDesc {
        HeightmapDesc {
            image_uri: None,
            heights: vec![],
            width: 0,
            height: 0,
            size: [4.0, 2.0, 10.0],
            origin: [1.0, 0.0, 0.5],
            sampling: 1,
            layers: vec![],
            blends: vec![],
        }
    }

    #[test]
    fn terrain_is_centred_with_the_top_row_at_plus_y() {
        let grid = Grid {
            width: 3,
            height: 2,
            heights: vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0],
        };
        let mesh = terrain_mesh(&grid, &desc()).unwrap();
        let Some(VertexAttributeValues::Float32x3(p)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("no positions");
        };
        assert_eq!(p[0], [-1.0, 1.0, 0.5]);
        assert_eq!(p[2], [3.0, 1.0, 2.5]);
        assert_eq!(p[3], [-1.0, -1.0, 3.5]);
        assert_eq!(mesh.indices().unwrap().len(), 2 * 2 * 3);
        // Flat terrain faces +Z.
        let flat = Grid {
            heights: vec![0.0; 6],
            ..grid
        };
        let mesh = terrain_mesh(&flat, &desc()).unwrap();
        let Some(VertexAttributeValues::Float32x3(n)) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
        else {
            panic!("no normals");
        };
        assert!(n.iter().all(|n| (n[2] - 1.0).abs() < 1e-6), "{n:?}");
        assert!(mesh.contains_attribute(Mesh::ATTRIBUTE_TANGENT));
        assert!(terrain_mesh(
            &Grid {
                width: 1,
                height: 1,
                heights: vec![0.0]
            },
            &desc()
        )
        .is_none());
    }

    #[test]
    fn resampling_is_bilinear_and_capped() {
        let grid = Grid {
            width: 2,
            height: 2,
            heights: vec![0.0, 2.0, 4.0, 6.0],
        };
        assert_eq!(resample(&grid, 1), grid);
        let up = resample(&grid, 2);
        assert_eq!((up.width, up.height), (3, 3));
        assert_eq!(
            up.heights,
            vec![0.0, 1.0, 2.0, 2.0, 3.0, 4.0, 4.0, 5.0, 6.0]
        );
        let big = resample(&grid, 4096);
        assert_eq!(
            (big.width, big.height),
            (MAX_VERTICES_PER_SIDE, MAX_VERTICES_PER_SIDE)
        );
        let huge = Grid {
            width: 2000,
            height: 2,
            heights: vec![1.0; 4000],
        };
        assert_eq!(resample(&huge, 1).width, MAX_VERTICES_PER_SIDE);
    }

    #[test]
    fn image_values_scale_to_size_z() {
        let image = Image::new(
            bevy::render::render_resource::Extent3d {
                width: 2,
                height: 1,
                depth_or_array_layers: 1,
            },
            bevy::render::render_resource::TextureDimension::D2,
            vec![0, 0, 0, 255, 255, 255, 255, 255],
            bevy::render::render_resource::TextureFormat::Rgba8Unorm,
            RenderAssetUsages::default(),
        );
        let grid = image_grid(&image, 10.0).unwrap();
        assert_eq!(grid.heights, vec![0.0, 10.0]);
    }
}
