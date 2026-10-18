// Height-blended texture layers of a heightmap terrain (`gz_bevy::heightmap`).
//
// Layer 0 is the StandardMaterial's own base colour and normal map. Up to
// three more layers are laid over it in order, each where the terrain is
// higher than its blend's `min_height`, fading in over `fade_dist`, as
// Gazebo's ogre2 terrain does (`100.gz_CustomWeights_piece_ps.any`).

import bevy_pbr::render::{
    mesh_functions::get_local_from_world,
    mesh_view_bindings::view,
    pbr_functions::{apply_normal_mapping, calculate_tbn_mikktspace},
    pbr_types::STANDARD_MATERIAL_FLAGS_TWO_COMPONENT_NORMAL_MAP,
};

// `heightmap::TerrainLayers`; the layout must match.
struct TerrainLayers {
    // Per blended layer (1, 2, 3): xy repeats over the terrain, z 1 when it
    // has a diffuse map, w 1 when it has a normal map.
    tiling: array<vec4<f32>, 3>,
    // Per blended layer: its blend's min_height (metres above the base).
    min_height: vec3<f32>,
    // Blended layers in use (0 to 3).
    count: u32,
    // Per blended layer: its blend's fade_dist.
    fade: vec3<f32>,
    // The terrain's lowest point (origin.z) in the mesh's frame, Z up.
    base_height: f32,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> terrain: TerrainLayers;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var diffuse_1: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var normal_1: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var diffuse_2: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var normal_2: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(105) var diffuse_3: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(106) var normal_3: texture_2d<f32>;
// One repeating sampler for all of them (WebGPU allows 16 per stage).
@group(constants::MATERIAL_BIND_GROUP) @binding(107) var layer_sampler: sampler;

// Height above the terrain's base: Gazebo's `localHeight`, the world
// height less the terrain's lowest point, taken here in the mesh's own
// frame (the visual's, Z up).
fn height(instance_index: u32, world_position: vec4<f32>) -> f32 {
    let local = get_local_from_world(instance_index) * vec4(world_position.xyz, 1.0);
    return local.z - terrain.base_height;
}

// How much of one layer covers what is below it. Keep in sync with
// `heightmap::layer_weight`.
fn layer_weight(h: f32, min_height: f32, fade: f32) -> f32 {
    if fade <= 1e-6 {
        return select(0.0, 1.0, h >= min_height);
    }
    return smoothstep(min_height, min_height + fade, h);
}

// Each blended layer's own weight (0 for those not in use). Keep in sync
// with `heightmap::layer_weights`.
fn weights(h: f32) -> vec3<f32> {
    var w = vec3(0.0);
    for (var i = 0u; i < min(terrain.count, 3u); i += 1u) {
        w[i] = layer_weight(h, terrain.min_height[i], terrain.fade[i]);
    }
    return w;
}

fn sample_layer(t: texture_2d<f32>, uv: vec2<f32>) -> vec4<f32> {
    return textureSampleBias(t, layer_sampler, uv, view.mip_bias);
}

// Lays the layers' colours over `color` (layer 0's), in order. A layer
// without a diffuse map leaves the colour below it, as in Gazebo.
fn blend_color(color: vec4<f32>, uv: vec2<f32>, w: vec3<f32>) -> vec4<f32> {
    var rgb = color.rgb;
    if terrain.tiling[0].z > 0.5 {
        rgb = mix(rgb, sample_layer(diffuse_1, uv * terrain.tiling[0].xy).rgb, w.x);
    }
    if terrain.tiling[1].z > 0.5 {
        rgb = mix(rgb, sample_layer(diffuse_2, uv * terrain.tiling[1].xy).rgb, w.y);
    }
    if terrain.tiling[2].z > 0.5 {
        rgb = mix(rgb, sample_layer(diffuse_3, uv * terrain.tiling[2].xy).rgb, w.z);
    }
    return vec4(rgb, color.a);
}

// A layer's world-space normal: its normal map, or the surface's own normal.
fn layer_normal(
    t: texture_2d<f32>,
    i: u32,
    uv: vec2<f32>,
    TBN: mat3x3<f32>,
    flags: u32,
    double_sided: bool,
    is_front: bool,
) -> vec3<f32> {
    // The layers' maps are three-component, whatever layer 0's is.
    let layer_flags = flags & ~STANDARD_MATERIAL_FLAGS_TWO_COMPONENT_NORMAL_MAP;
    var Nt = vec3(0.5, 0.5, 1.0);
    if terrain.tiling[i].w > 0.5 {
        Nt = sample_layer(t, uv * terrain.tiling[i].xy).rgb;
    }
    return apply_normal_mapping(layer_flags, TBN, double_sided, is_front, Nt);
}

// Lays the layers' normals over `N` (layer 0's, world space) with the same
// weights as the colours. A layer without a normal map is flat.
fn blend_normal(
    N: vec3<f32>,
    world_normal: vec3<f32>,
    world_tangent: vec4<f32>,
    uv: vec2<f32>,
    w: vec3<f32>,
    flags: u32,
    double_sided: bool,
    is_front: bool,
) -> vec3<f32> {
    let TBN = calculate_tbn_mikktspace(world_normal, world_tangent);
    let n1 = layer_normal(normal_1, 0u, uv, TBN, flags, double_sided, is_front);
    let n2 = layer_normal(normal_2, 1u, uv, TBN, flags, double_sided, is_front);
    let n3 = layer_normal(normal_3, 2u, uv, TBN, flags, double_sided, is_front);
    var n = N;
    n = normalize(mix(n, n1, w.x));
    n = normalize(mix(n, n2, w.y));
    n = normalize(mix(n, n3, w.z));
    return n;
}
