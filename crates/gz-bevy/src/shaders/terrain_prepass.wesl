// Heightmap terrain, prepass: Bevy's StandardMaterial prepass
// (`bevy_pbr::render::pbr_prepass`) writing the height-blended normal of
// `terrain_layers`, which SSAO and the forward pass then read.

import bevy_pbr::{
    render::{
        pbr_prepass_functions,
        pbr_bindings,
        pbr_types,
        pbr_functions,
        mesh_bindings::mesh,
        mesh_view_bindings::view,
    },
    prepass::io as prepass_io,
};
import package::shaders::terrain_layers;

@if(PREPASS_FRAGMENT)
@fragment
fn fragment(
    in: prepass_io::VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> prepass_io::FragmentOutput {
@if(VISIBILITY_RANGE_DITHER)
    pbr_functions::visibility_range_dither(in.position, in.visibility_range_dither);

    pbr_prepass_functions::prepass_sample_color_and_alpha_discard(in);

    var out: prepass_io::FragmentOutput;

@if(UNCLIPPED_DEPTH_ORTHO_EMULATION)
    out.frag_depth = in.unclipped_depth;

@if(NORMAL_PREPASS) {
    let flags = pbr_bindings::material.flags;
    let double_sided = (flags & pbr_types::STANDARD_MATERIAL_FLAGS_DOUBLE_SIDED_BIT) != 0u;
    let front = pbr_functions::winding_corrected_front_facing(mesh[in.instance_index].flags, is_front);
    let world_normal = pbr_functions::prepare_world_normal(in.world_normal, double_sided, front);
    var normal = normalize(world_normal);

@if(VERTEX_UVS_A && VERTEX_TANGENTS) {
@if(STANDARD_MATERIAL_NORMAL_MAP) {
    // Layer 0, as `pbr_input_from_standard_material` maps it.
    let uv = (pbr_bindings::material.uv_transform * vec3(in.uv, 1.0)).xy;
    let Nt = textureSampleBias(
        pbr_bindings::normal_map_texture,
        pbr_bindings::normal_map_sampler,
        uv,
        view.mip_bias,
    ).rgb;
    let TBN = pbr_functions::calculate_tbn_mikktspace(world_normal, in.world_tangent);
    normal = pbr_functions::apply_normal_mapping(flags, TBN, double_sided, front, Nt);
}
    let w = terrain_layers::weights(terrain_layers::height(in.instance_index, in.world_position));
    normal = terrain_layers::blend_normal(
        normal,
        world_normal,
        in.world_tangent,
        in.uv,
        w,
        flags,
        double_sided,
        front,
    );
}

    out.normal = vec4(normal * 0.5 + vec3(0.5), 1.0);
}

@if(MOTION_VECTOR_PREPASS)
    out.motion_vector = pbr_prepass_functions::calculate_motion_vector(
        in.world_position,
        in.previous_world_position,
    );

    return out;
}
@else
@fragment
fn fragment(in: prepass_io::VertexOutput) {
    pbr_prepass_functions::prepass_sample_color_and_alpha_discard(in);
}
