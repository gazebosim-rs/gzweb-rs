//! Heightmap terrain.
//!
//! gzweb only draws heights carried in the message, which Gazebo never
//! sends, so its terrain is empty. Like Gazebo we load the heightmap image
//! (grayscale; black at `origin.z`, the brightest pixel at `origin.z +
//! size.z`), sample it `sampling` times per pixel with bilinear filtering,
//! and lay it over `size.x` × `size.y` centred on `origin`, the image's top
//! row at +Y. Heights in the message are used as they are.
//!
//! Texture layers follow Gazebo's ogre2 terrain (gz-rendering
//! `Ogre2Heightmap.cc` and its Terra shader pieces): layer 0 covers the
//! terrain; layer `i > 0` is laid over the layers below it where the
//! terrain is higher than `blends[i - 1]`'s `min_height` (metres above the
//! terrain's lowest point, `origin.z`), fading in over `fade_dist` with a
//! smoothstep. Each layer repeats every `size` metres. Up to
//! [`MAX_LAYERS`] are drawn, by [`TerrainMaterial`] (`shaders/terrain*.wesl`).

use bevy::asset::{embedded_asset, load_embedded_asset, LoadState, RenderAssetUsages};
use bevy::image::ImageLoaderSettings;
use bevy::math::Affine2;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use gz_core::scene::HeightmapDesc;

use crate::{assets, mesh_file};

/// Vertices along a side are capped here (about a million in all).
pub const MAX_VERTICES_PER_SIDE: usize = 1025;

/// Texture layers drawn at most: Gazebo's ogre2 limit (4; it takes a 5th
/// only when layer 0 has no normal map and one tile covers the terrain).
/// Each layer past the first costs two sampled textures of WebGPU's 16 per
/// shader stage, beside the StandardMaterial's six and the view's.
pub const MAX_LAYERS: usize = 4;

/// Waits for its image, then gets the terrain mesh.
#[derive(Component, Debug)]
pub struct GzHeightmap {
    pub desc: HeightmapDesc,
    pub image: Option<Handle<Image>>,
    /// Set once the terrain is built, or its image failed.
    pub built: bool,
    /// Vertices per row and column of the built terrain (0 when empty).
    pub grid_size: (usize, usize),
}

/// A grid of heights in metres, row 0 at +Y.
#[derive(Debug, Clone, PartialEq)]
pub struct Grid {
    pub width: usize,
    pub height: usize,
    pub heights: Vec<f32>,
}

/// The terrain's material: layer 0 in the StandardMaterial (so it reports
/// like any other), the blended layers in the extension.
pub type TerrainMaterial = ExtendedMaterial<StandardMaterial, TerrainExtension>;

const SHADER: &str = "embedded://gz_bevy/shaders/terrain.wesl";
const PREPASS_SHADER: &str = "embedded://gz_bevy/shaders/terrain_prepass.wesl";

/// Registers [`TerrainMaterial`] and its shaders.
pub struct TerrainMaterialPlugin;

/// Keeps the shared shader module loaded, so the terrain shaders' imports
/// of it resolve.
#[derive(Resource)]
struct TerrainShaderLibrary(#[allow(dead_code)] Handle<Shader>);

impl Plugin for TerrainMaterialPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/terrain.wesl");
        embedded_asset!(app, "shaders/terrain_prepass.wesl");
        embedded_asset!(app, "shaders/terrain_layers.wesl");
        // Without a renderer (unit tests) there are no shader assets.
        if app.world().contains_resource::<Assets<Shader>>() {
            let library: Handle<Shader> = load_embedded_asset!(app, "shaders/terrain_layers.wesl");
            app.insert_resource(TerrainShaderLibrary(library));
        }
        app.add_plugins(MaterialPlugin::<TerrainMaterial>::default());
    }
}

/// The layers over layer 0 (1 to 3), as `shaders/terrain_layers.wesl`
/// reads them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Reflect, ShaderType)]
pub struct TerrainLayers {
    /// Per layer: xy repeats over the terrain, z 1 with a diffuse map, w 1
    /// with a normal map.
    pub tiling: [Vec4; 3],
    /// Per layer: its blend's `min_height`, in metres above the base.
    pub min_height: Vec3,
    /// Layers in use (0 to 3).
    pub count: u32,
    /// Per layer: its blend's `fade_dist`.
    pub fade: Vec3,
    /// The terrain's lowest point (`origin.z`) in the mesh's frame.
    pub base_height: f32,
}

/// Layers 1 to 3 of a terrain, over the StandardMaterial's layer 0.
///
/// The textures are bound one by one, so layers can differ in size. They
/// share one sampler, `diffuse_1`'s: when layer 1 has no diffuse map that
/// slot holds another layer texture (unused but for its sampler), since
/// every layer texture repeats the same way.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct TerrainExtension {
    #[uniform(100)]
    pub layers: TerrainLayers,
    #[texture(101)]
    #[sampler(107)]
    pub diffuse_1: Option<Handle<Image>>,
    #[texture(102)]
    pub normal_1: Option<Handle<Image>>,
    #[texture(103)]
    pub diffuse_2: Option<Handle<Image>>,
    #[texture(104)]
    pub normal_2: Option<Handle<Image>>,
    #[texture(105)]
    pub diffuse_3: Option<Handle<Image>>,
    #[texture(106)]
    pub normal_3: Option<Handle<Image>>,
}

impl MaterialExtension for TerrainExtension {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }

    fn deferred_fragment_shader() -> ShaderRef {
        SHADER.into()
    }

    fn prepass_fragment_shader() -> ShaderRef {
        PREPASS_SHADER.into()
    }
}

/// How much of a layer covers the layers below it at `height` metres above
/// the terrain's lowest point: Gazebo ogre2's `smoothstep(min_height,
/// min_height + fade_dist, height)`. A fade of 0 is a hard step (ogre2
/// would never show such a layer; ogre1 steps).
///
/// Mirrors `layer_weight` in `shaders/terrain_layers.wesl`; keep the two
/// in sync.
pub fn layer_weight(height: f32, min_height: f32, fade: f32) -> f32 {
    if fade <= 1e-6 {
        return if height >= min_height { 1.0 } else { 0.0 };
    }
    let t = ((height - min_height) / fade).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Each of layers 1 to 3's own weight at `height` (0 for those not in
/// use). Mirrors `weights` in `shaders/terrain_layers.wesl`.
pub fn layer_weights(height: f32, layers: &TerrainLayers) -> [f32; 3] {
    let mut w = [0.0; 3];
    for (i, w) in w.iter_mut().enumerate().take(layers.count.min(3) as usize) {
        *w = layer_weight(height, layers.min_height[i], layers.fade[i]);
    }
    w
}

/// What share of the colour each layer (0 first) has at `height`, after
/// the shader lays them over each other in order. Sums to 1.
pub fn layer_shares(height: f32, layers: &TerrainLayers) -> [f32; MAX_LAYERS] {
    let mut shares = [1.0, 0.0, 0.0, 0.0];
    for (i, w) in layer_weights(height, layers).into_iter().enumerate() {
        shares.iter_mut().take(i + 1).for_each(|s| *s *= 1.0 - w);
        shares[i + 1] = w;
    }
    shares
}

/// The layers drawn: layer 0 and each later one with a blend, up to
/// [`MAX_LAYERS`]. As in Gazebo, a layer without a blend never shows and
/// blends past the last layer do nothing.
pub fn drawn_layers(desc: &HeightmapDesc) -> usize {
    let blended = desc.layers.len().saturating_sub(1).min(desc.blends.len());
    (desc.layers.len().min(1) + blended).min(MAX_LAYERS)
}

/// Starts loading the heightmap and its textures for a visual; returns
/// what it loads, for snapshots to wait on.
pub fn spawn(
    commands: &mut Commands,
    asset_server: &AssetServer,
    desc: &HeightmapDesc,
    parent: Entity,
) -> Vec<UntypedHandle> {
    // Only read on the CPU, never uploaded.
    let image: Option<Handle<Image>> = desc.image_uri.as_ref().map(|uri| {
        asset_server
            .load_builder()
            .with_settings(|s: &mut ImageLoaderSettings| {
                s.is_srgb = false;
                s.asset_usage = RenderAssetUsages::MAIN_WORLD;
            })
            .load(assets::asset_path(uri))
    });
    let textures = layer_textures(asset_server, desc);
    let loads: Vec<UntypedHandle> = image
        .iter()
        .map(|h| h.clone().untyped())
        .chain(
            textures
                .iter()
                .flat_map(|[d, n]| [d, n])
                .flatten()
                .map(|h| h.clone().untyped()),
        )
        .collect();
    let material = asset_server.add(material(desc, &textures));
    commands.spawn((
        GzHeightmap {
            desc: desc.clone(),
            image,
            built: false,
            grid_size: (0, 0),
        },
        MeshMaterial3d(material),
        Transform::IDENTITY,
        Visibility::default(),
        ChildOf(parent),
    ));
    loads
}

/// Starts loading the drawn layers' diffuse and normal maps.
fn layer_textures(
    asset_server: &AssetServer,
    desc: &HeightmapDesc,
) -> Vec<[Option<Handle<Image>>; 2]> {
    if desc.layers.len() > MAX_LAYERS {
        warn!(
            "heightmap: {} texture layers, only the first {MAX_LAYERS} are drawn",
            desc.layers.len()
        );
    }
    let texture = |uri: &Option<String>, srgb: bool| {
        uri.as_ref().map(|uri| {
            asset_server
                .load_builder()
                .with_settings(mesh_file::image_settings(
                    srgb,
                    mesh_file::sampler([true; 2], 16),
                ))
                .load(assets::asset_path(uri))
        })
    };
    desc.layers
        .iter()
        .take(drawn_layers(desc))
        .map(|l| [texture(&l.diffuse, true), texture(&l.normal, false)])
        .collect()
}

/// Layer 0 in the StandardMaterial, the rest in the extension; each
/// repeats every `size` metres.
fn material(desc: &HeightmapDesc, textures: &[[Option<Handle<Image>>; 2]]) -> TerrainMaterial {
    let repeats = |i: usize| {
        let size = desc.layers[i].size;
        Vec2::new((desc.size[0] / size) as f32, (desc.size[1] / size) as f32)
    };
    let base = match textures.first() {
        // gzweb's untextured terrain colour.
        None => StandardMaterial {
            base_color: Color::srgb_u8(0x55, 0x55, 0x55),
            perceptual_roughness: 1.0,
            ..default()
        },
        Some([diffuse, normal]) => StandardMaterial {
            base_color_texture: diffuse.clone(),
            normal_map_texture: normal.clone(),
            uv_transform: Affine2::from_scale(repeats(0)),
            perceptual_roughness: 1.0,
            ..default()
        },
    };
    let mut extension = TerrainExtension {
        layers: TerrainLayers {
            count: textures.len().saturating_sub(1) as u32,
            base_height: desc.origin[2] as f32,
            ..default()
        },
        ..default()
    };
    let slots = [
        (&mut extension.diffuse_1, &mut extension.normal_1),
        (&mut extension.diffuse_2, &mut extension.normal_2),
        (&mut extension.diffuse_3, &mut extension.normal_3),
    ];
    for (i, ((diffuse, normal), [d, n])) in
        slots.into_iter().zip(textures.iter().skip(1)).enumerate()
    {
        let flag = |h: &Option<Handle<Image>>| if h.is_some() { 1.0 } else { 0.0 };
        extension.layers.tiling[i] = repeats(i + 1).extend(flag(d)).extend(flag(n));
        let [min_height, fade] = desc.blends[i];
        extension.layers.min_height[i] = min_height as f32;
        extension.layers.fade[i] = fade as f32;
        *diffuse = d.clone();
        *normal = n.clone();
    }
    // The shared sampler comes from `diffuse_1`.
    if extension.diffuse_1.is_none() {
        extension.diffuse_1 = textures.iter().skip(1).flatten().flatten().next().cloned();
    }
    TerrainMaterial { base, extension }
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
        // Tangents only matter to a normal map.
        let tangents = terrain
            .desc
            .layers
            .iter()
            .take(drawn_layers(&terrain.desc))
            .any(|l| l.normal.is_some());
        let grid = grid.map(|g| resample(&g, terrain.desc.sampling as usize));
        // Without heights the terrain is an empty mesh, which still carries
        // its material (gzweb's terrain is always empty).
        let mesh = grid
            .as_ref()
            .and_then(|g| terrain_mesh(g, &terrain.desc, tangents));
        if mesh.is_some() {
            let g = grid.as_ref().expect("a mesh has a grid");
            terrain.grid_size = (g.width, g.height);
        }
        let mesh = mesh.unwrap_or_else(empty_mesh);
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

/// Image values as heights, or `None` if a pixel can't be read. As in
/// Gazebo, black is 0 and the brightest pixel `scale`: gz-rendering divides
/// by the image's maximum elevation (`ImageHeightmap::MaxElevation`, the red
/// value of the pixel with the largest R+G+B; the minimum is 0), so an image
/// that is never white still spans `size.z`. An all-black image is flat.
pub fn image_grid(image: &Image, scale: f32) -> Option<Grid> {
    let (width, height) = (image.width() as usize, image.height() as usize);
    let mut heights = Vec::with_capacity(width * height);
    let (mut brightest, mut max) = (f32::NEG_INFINITY, 0.0);
    for y in 0..height as u32 {
        for x in 0..width as u32 {
            let c = image.get_color_at(x, y).ok()?.to_linear();
            let sum = c.red + c.green + c.blue;
            if sum > brightest {
                (brightest, max) = (sum, c.red);
            }
            heights.push(c.red);
        }
    }
    let k = if max > 1e-6 { scale / max } else { scale };
    heights.iter_mut().for_each(|h| *h *= k);
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
/// [0, 1] (the material repeats them) and, with `tangents`, tangents.
/// `None` below 2×2.
pub fn terrain_mesh(grid: &Grid, desc: &HeightmapDesc, tangents: bool) -> Option<Mesh> {
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
    if tangents {
        let tangents = grid_tangents(grid, desc, &mesh);
        mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, tangents);
    }
    Some(mesh)
}

/// Tangents of the regular grid, without mikktspace (which takes seconds
/// on a 1025² terrain): ∂p/∂u by central differences, made orthogonal to
/// the normal, with Bevy's handedness (`w` is the sign that makes
/// `w · (n × t)` point against ∂p/∂v: Bevy's V runs down the texture).
fn grid_tangents(grid: &Grid, desc: &HeightmapDesc, mesh: &Mesh) -> Vec<[f32; 4]> {
    let (w, h) = (grid.width, grid.height);
    let [sx, sy, _] = desc.size.map(|v| v as f32);
    let Some(bevy::mesh::VertexAttributeValues::Float32x3(normals)) =
        mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
    else {
        return vec![[1.0, 0.0, 0.0, 1.0]; w * h];
    };
    let at = |r: usize, c: usize| grid.heights[r * w + c];
    let mut out = Vec::with_capacity(w * h);
    for r in 0..h {
        let (r0, r1) = (r.saturating_sub(1), (r + 1).min(h - 1));
        for c in 0..w {
            let (c0, c1) = (c.saturating_sub(1), (c + 1).min(w - 1));
            // Per unit of u (x grows with u) and of v (y shrinks with v).
            let du = (c1 - c0) as f32 / (w - 1) as f32;
            let dv = (r1 - r0) as f32 / (h - 1) as f32;
            let dp_du = Vec3::new(sx, 0.0, (at(r, c1) - at(r, c0)) / du);
            let dp_dv = Vec3::new(0.0, -sy, (at(r1, c) - at(r0, c)) / dv);
            let n = Vec3::from_array(normals[r * w + c]);
            let t = (dp_du - n * n.dot(dp_du)).normalize_or(Vec3::X);
            let sign = if n.cross(t).dot(dp_dv) > 0.0 {
                -1.0
            } else {
                1.0
            };
            out.push([t.x, t.y, t.z, sign]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;
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

    fn positions(mesh: &Mesh) -> &[[f32; 3]] {
        let Some(VertexAttributeValues::Float32x3(p)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("no positions");
        };
        p
    }

    #[test]
    fn terrain_is_centred_with_the_top_row_at_plus_y() {
        let grid = Grid {
            width: 3,
            height: 2,
            heights: vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0],
        };
        let mesh = terrain_mesh(&grid, &desc(), false).unwrap();
        let p = positions(&mesh);
        assert_eq!(p[0], [-1.0, 1.0, 0.5]);
        assert_eq!(p[2], [3.0, 1.0, 2.5]);
        assert_eq!(p[3], [-1.0, -1.0, 3.5]);
        assert_eq!(mesh.indices().unwrap().len(), 2 * 2 * 3);
        assert!(!mesh.contains_attribute(Mesh::ATTRIBUTE_TANGENT));
        // Flat terrain faces +Z.
        let flat = Grid {
            heights: vec![0.0; 6],
            ..grid
        };
        let mesh = terrain_mesh(&flat, &desc(), true).unwrap();
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
            &desc(),
            true
        )
        .is_none());
    }

    #[test]
    fn grid_tangents_match_mikktspace() {
        // A smooth bump, finely sampled: mikktspace averages face tangents,
        // so the two agree closely but not exactly.
        let n = 33;
        let heights = (0..n * n)
            .map(|i| {
                let (x, y) = ((i % n) as f32 / 8.0, (i / n) as f32 / 8.0);
                (x.sin() * y.cos()) * 0.5
            })
            .collect();
        let grid = Grid {
            width: n,
            height: n,
            heights,
        };
        let ours = terrain_mesh(&grid, &desc(), true).unwrap();
        let mut mikk = ours.clone();
        mikk.generate_tangents().unwrap();
        let tangents = |m: &Mesh| match m.attribute(Mesh::ATTRIBUTE_TANGENT) {
            Some(VertexAttributeValues::Float32x4(t)) => t.clone(),
            _ => panic!("no tangents"),
        };
        let (a, b) = (tangents(&ours), tangents(&mikk));
        for (i, (a, b)) in a.iter().zip(&b).enumerate() {
            assert_eq!(a[3], b[3], "handedness at {i}");
            let dot = Vec3::from_slice(a).dot(Vec3::from_slice(b));
            assert!(dot > 0.999, "vertex {i}: {a:?} vs {b:?}");
        }
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

    /// Spawns `desc` under a fresh entity in an app that reads PNGs, with
    /// `served` answered by the (absent) server, and runs until it is
    /// built. Returns what `spawn` loads too.
    fn built(desc: HeightmapDesc, served: &[(&str, assets::AssetResult)]) -> Built {
        let mut app = crate::test_util::scene_app(crate::GzConfig::default());
        app.register_asset_loader(bevy::image::ImageLoader::new(
            bevy::image::CompressedImageFormats::NONE,
        ));
        let store = app.world().resource::<assets::WsAssetStore>().clone();
        for (uri, result) in served {
            store.deliver(uri, result.clone());
        }
        let world = app.world_mut();
        let parent = world.spawn(Transform::IDENTITY).id();
        let loads = world
            .run_system_once(move |mut commands: Commands, server: Res<AssetServer>| {
                spawn(&mut commands, &server, &desc, parent)
            })
            .unwrap();
        for _ in 0..200 {
            app.update();
            let world = app.world_mut();
            if world.query::<&GzHeightmap>().iter(world).all(|t| t.built) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let world = app.world_mut();
        let entity = world
            .query_filtered::<Entity, With<GzHeightmap>>()
            .single(world)
            .unwrap();
        Built { app, entity, loads }
    }

    struct Built {
        app: App,
        entity: Entity,
        loads: Vec<UntypedHandle>,
    }

    impl Built {
        fn mesh(&self) -> &Mesh {
            let mesh = self
                .app
                .world()
                .get::<Mesh3d>(self.entity)
                .expect("a mesh once built");
            self.app
                .world()
                .resource::<Assets<Mesh>>()
                .get(&mesh.0)
                .unwrap()
        }

        fn terrain_material(&self) -> &TerrainMaterial {
            let mat = self
                .app
                .world()
                .get::<MeshMaterial3d<TerrainMaterial>>(self.entity)
                .unwrap();
            self.app
                .world()
                .resource::<Assets<TerrainMaterial>>()
                .get(&mat.0)
                .unwrap()
        }

        fn material(&self) -> &StandardMaterial {
            &self.terrain_material().base
        }

        fn terrain(&self) -> &GzHeightmap {
            self.app.world().get::<GzHeightmap>(self.entity).unwrap()
        }
    }

    #[test]
    fn message_heights_build_without_an_image_and_untextured_is_grey() {
        let b = built(
            HeightmapDesc {
                heights: vec![0.0, 1.0, 2.0, 3.0],
                width: 2,
                height: 2,
                sampling: 2,
                ..desc()
            },
            &[],
        );
        assert!(b.terrain().built);
        assert_eq!(b.terrain().grid_size, (3, 3));
        assert_eq!(b.mesh().count_vertices(), 9);
        assert!(b.loads.is_empty());
        assert_eq!(b.material().base_color, Color::srgb_u8(0x55, 0x55, 0x55));
    }

    fn layer() -> gz_core::scene::HeightmapLayer {
        gz_core::scene::HeightmapLayer {
            diffuse: Some("https://fuel.example/d.png".into()),
            normal: Some("https://fuel.example/n.png".into()),
            size: 2.0,
        }
    }

    #[test]
    fn a_missing_image_is_an_empty_textured_terrain() {
        let b = built(
            HeightmapDesc {
                image_uri: Some("does/not/exist.png".into()),
                layers: vec![layer()],
                ..desc()
            },
            &[("does/not/exist.png", Err("asset_not_found".into()))],
        );
        assert!(b.terrain().built);
        assert_eq!(b.mesh().count_vertices(), 0);
        assert_eq!(b.terrain().grid_size, (0, 0));
        let mat = b.material();
        assert_eq!(
            mat.base_color_texture
                .as_ref()
                .and_then(|h| h.path())
                .map(|p| p.to_string()),
            Some("gzhttps://fuel.example/d.png".into())
        );
        // 4 m by 2 m, one repeat every 2 m.
        assert_eq!(mat.uv_transform, Affine2::from_scale(Vec2::new(2.0, 1.0)));
        // Snapshots wait for the image and both layer textures.
        let paths: Vec<String> = b
            .loads
            .iter()
            .filter_map(|h| h.path().map(|p| p.to_string()))
            .collect();
        assert_eq!(
            paths,
            [
                "wsasset://rel/does/not/exist.png",
                "gzhttps://fuel.example/d.png",
                "gzhttps://fuel.example/n.png"
            ]
        );
    }

    fn path(h: &Option<Handle<Image>>) -> Option<String> {
        h.as_ref().and_then(|h| h.path()).map(|p| p.to_string())
    }

    fn named(name: &str, normal: bool, size: f64) -> gz_core::scene::HeightmapLayer {
        gz_core::scene::HeightmapLayer {
            diffuse: Some(format!("https://fuel.example/{name}.png")),
            normal: normal.then(|| "https://fuel.example/flat.png".into()),
            size,
        }
    }

    fn missing_image(layers: Vec<gz_core::scene::HeightmapLayer>, blends: Vec<[f64; 2]>) -> Built {
        built(
            HeightmapDesc {
                image_uri: Some("none.png".into()),
                layers,
                blends,
                ..desc()
            },
            &[("none.png", Err("asset_not_found".into()))],
        )
    }

    #[test]
    fn layer_weights_follow_gazebos_smoothstep() {
        // Gazebo's test world: dirt, grass above 1 m, fungus above 3 m,
        // each fading in over 1 m.
        let layers = TerrainLayers {
            count: 2,
            min_height: Vec3::new(1.0, 3.0, 0.0),
            fade: Vec3::new(1.0, 1.0, 0.0),
            ..default()
        };
        assert_eq!(layer_weights(0.0, &layers), [0.0, 0.0, 0.0]);
        assert_eq!(layer_weights(1.5, &layers), [0.5, 0.0, 0.0]);
        // smoothstep: 3t² − 2t³ at t = 0.25.
        let w = layer_weights(1.25, &layers)[0];
        assert!((w - 0.15625).abs() < 1e-6, "{w}");
        assert_eq!(layer_weights(2.0, &layers), [1.0, 0.0, 0.0]);
        assert_eq!(layer_weights(5.0, &layers), [1.0, 1.0, 0.0]);
        assert_eq!(layer_shares(0.5, &layers), [1.0, 0.0, 0.0, 0.0]);
        assert_eq!(layer_shares(2.5, &layers), [0.0, 1.0, 0.0, 0.0]);
        assert_eq!(layer_shares(3.5, &layers), [0.0, 0.5, 0.5, 0.0]);
        assert_eq!(layer_shares(4.0, &layers), [0.0, 0.0, 1.0, 0.0]);
        for h in [0.3, 1.1, 1.9, 3.2, 3.9] {
            let sum: f32 = layer_shares(h, &layers).iter().sum();
            assert!((sum - 1.0).abs() < 1e-6, "{h}: {sum}");
        }
        // Unused layers never show, whatever their heights.
        let one = TerrainLayers { count: 1, ..layers };
        assert_eq!(layer_weights(5.0, &one), [1.0, 0.0, 0.0]);
        // No fade: a hard step at min_height.
        assert_eq!(layer_weight(0.99, 1.0, 0.0), 0.0);
        assert_eq!(layer_weight(1.0, 1.0, 0.0), 1.0);
        assert_eq!(layer_weight(7.0, 1.0, -1.0), 1.0);
    }

    #[test]
    fn layers_drawn_need_a_blend_and_stop_at_the_limit() {
        let with = |layers: usize, blends: usize| HeightmapDesc {
            layers: vec![layer(); layers],
            blends: vec![[1.0, 1.0]; blends],
            ..desc()
        };
        assert_eq!(drawn_layers(&with(0, 0)), 0);
        assert_eq!(drawn_layers(&with(0, 2)), 0);
        assert_eq!(drawn_layers(&with(1, 0)), 1);
        assert_eq!(drawn_layers(&with(3, 2)), 3);
        // Layers without a blend never show (Gazebo's weight stays 0)…
        assert_eq!(drawn_layers(&with(3, 1)), 2);
        // …and blends past the last layer do nothing.
        assert_eq!(drawn_layers(&with(2, 4)), 2);
        assert_eq!(drawn_layers(&with(6, 5)), MAX_LAYERS);
    }

    #[test]
    fn one_layer_is_a_plain_standard_material() {
        let b = missing_image(vec![named("dirt", true, 2.0)], vec![[1.0, 1.0]]);
        let m = b.terrain_material();
        assert_eq!(
            path(&m.base.base_color_texture).unwrap(),
            "gzhttps://fuel.example/dirt.png"
        );
        assert_eq!(
            path(&m.base.normal_map_texture).unwrap(),
            "gzhttps://fuel.example/flat.png"
        );
        assert_eq!(m.extension.layers.count, 0);
        assert_eq!(m.extension.layers.base_height, 0.5);
        assert!(m.extension.diffuse_1.is_none() && m.extension.normal_1.is_none());
    }

    #[test]
    fn two_and_three_layers_tile_and_blend_on_their_own() {
        let b = missing_image(
            vec![named("dirt", true, 2.0), named("grass", false, 4.0)],
            vec![[1.0, 0.5]],
        );
        let e = &b.terrain_material().extension;
        assert_eq!(e.layers.count, 1);
        // 4 m × 2 m at 4 m a tile; a diffuse map but no normal map.
        assert_eq!(e.layers.tiling[0], Vec4::new(1.0, 0.5, 1.0, 0.0));
        assert_eq!((e.layers.min_height.x, e.layers.fade.x), (1.0, 0.5));
        assert_eq!(
            path(&e.diffuse_1).unwrap(),
            "gzhttps://fuel.example/grass.png"
        );
        assert!(e.normal_1.is_none() && e.diffuse_2.is_none());
        // The grass has no normal map, but the dirt's needs tangents.
        assert_eq!(b.loads.len(), 4);

        let b = missing_image(
            vec![
                named("dirt", true, 2.0),
                named("grass", true, 3.0),
                named("fungus", true, 5.0),
                named("ignored", true, 1.0),
            ],
            vec![[1.0, 1.0], [3.0, 1.0]],
        );
        let e = &b.terrain_material().extension;
        assert_eq!(e.layers.count, 2);
        assert_eq!(e.layers.tiling[1], Vec4::new(0.8, 0.4, 1.0, 1.0));
        assert_eq!(e.layers.min_height, Vec3::new(1.0, 3.0, 0.0));
        assert_eq!(e.layers.fade, Vec3::new(1.0, 1.0, 0.0));
        assert_eq!(
            path(&e.diffuse_2).unwrap(),
            "gzhttps://fuel.example/fungus.png"
        );
        assert_eq!(
            path(&e.normal_2).unwrap(),
            "gzhttps://fuel.example/flat.png"
        );
        assert!(e.diffuse_3.is_none());
        // Snapshots wait for the image and every drawn layer's textures,
        // not the layer without a blend.
        let paths: Vec<String> = b
            .loads
            .iter()
            .filter_map(|h| h.path().map(|p| p.to_string()))
            .collect();
        assert_eq!(paths.len(), 7);
        assert!(!paths.iter().any(|p| p.contains("ignored")), "{paths:?}");
    }

    #[test]
    fn the_shared_sampler_comes_from_another_layer_without_a_diffuse_map() {
        let mut bare = named("bare", true, 1.0);
        bare.diffuse = None;
        let b = missing_image(vec![named("dirt", false, 2.0), bare], vec![[1.0, 1.0]]);
        let e = &b.terrain_material().extension;
        // Bound for its sampler only: the diffuse flag stays off.
        assert_eq!(
            path(&e.diffuse_1).unwrap(),
            "gzhttps://fuel.example/flat.png"
        );
        assert_eq!(e.layers.tiling[0].z, 0.0);
        assert_eq!(e.layers.tiling[0].w, 1.0);
    }

    #[test]
    fn a_blended_layers_normal_map_needs_tangents() {
        let png = include_bytes!("../../gzweb/tests/data/terrain.png").to_vec();
        let mut base = named("dirt", false, 2.0);
        base.normal = None;
        let b = built(
            HeightmapDesc {
                image_uri: Some("t.png".into()),
                layers: vec![base, named("grass", true, 2.0)],
                blends: vec![[1.0, 1.0]],
                ..desc()
            },
            &[("t.png", Ok(png))],
        );
        assert!(b.mesh().contains_attribute(Mesh::ATTRIBUTE_TANGENT));
    }

    #[test]
    fn an_image_is_laid_out_top_row_at_plus_y_and_stays_on_the_cpu() {
        // 3×3 grey: white in the centre, 128 in the bottom-right corner.
        let png = include_bytes!("../../gzweb/tests/data/terrain.png").to_vec();
        let b = built(
            HeightmapDesc {
                image_uri: Some("t.png".into()),
                ..desc()
            },
            &[("t.png", Ok(png))],
        );
        assert_eq!(b.terrain().grid_size, (3, 3));
        let image = b.terrain().image.clone().unwrap();
        let images = b.app.world().resource::<Assets<Image>>();
        assert_eq!(
            images.get(&image).unwrap().asset_usage,
            RenderAssetUsages::MAIN_WORLD
        );
        let p = positions(b.mesh());
        // Origin (1, 0, 0.5), 4 × 2 × 10: the centre is the brightest, at
        // the top; the last pixel (bottom row, right) is at +X, -Y.
        assert_eq!(p[4], [1.0, 0.0, 10.5]);
        assert_eq!(p[8][..2], [3.0, -1.0]);
        assert!(
            (p[8][2] - (0.5 + 10.0 * 128.0 / 255.0)).abs() < 1e-4,
            "{p:?}"
        );
        assert_eq!(p[0], [-1.0, 1.0, 0.5]);
        // No normal map, no tangents.
        assert!(!b.mesh().contains_attribute(Mesh::ATTRIBUTE_TANGENT));
    }

    #[test]
    fn image_values_scale_so_the_brightest_is_size_z() {
        let image = |data: Vec<u8>| {
            Image::new(
                bevy::render::render_resource::Extent3d {
                    width: 3,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                bevy::render::render_resource::TextureDimension::D2,
                data,
                bevy::render::render_resource::TextureFormat::Rgba8Unorm,
                RenderAssetUsages::MAIN_WORLD,
            )
        };
        let white = image(vec![0, 0, 0, 255, 255, 255, 255, 255, 51, 51, 51, 255]);
        let grid = image_grid(&white, 10.0).unwrap();
        assert_eq!(grid.heights, vec![0.0, 10.0, 2.0]);
        // Never white: the brightest pixel still reaches size.z.
        let grey = image(vec![0, 0, 0, 255, 102, 102, 102, 255, 51, 51, 51, 255]);
        let grid = image_grid(&grey, 10.0).unwrap();
        assert!((grid.heights[1] - 10.0).abs() < 1e-5, "{:?}", grid.heights);
        assert!((grid.heights[2] - 5.0).abs() < 1e-5, "{:?}", grid.heights);
        // All black stays flat.
        let black = image(vec![0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255]);
        assert_eq!(image_grid(&black, 10.0).unwrap().heights, vec![0.0; 3]);
    }
}
