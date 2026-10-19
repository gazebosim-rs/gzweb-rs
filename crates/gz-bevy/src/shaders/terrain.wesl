// Heightmap terrain, forward and deferred: Bevy's StandardMaterial
// fragment (`bevy_pbr::render::pbr`) with the height-blended layers of
// `terrain_layers` laid over layer 0 before lighting.

import bevy_pbr::{
    render::{
        pbr_types,
        pbr_functions,
        pbr_functions::alpha_discard,
        pbr_fragment::pbr_input_from_standard_material,
    },
    decal::clustered::apply_decals,
};
import package::shaders::terrain_layers;

@if(PREPASS_PIPELINE)
import bevy_pbr::{
    prepass::io::{VertexOutput, FragmentOutput},
    deferred::functions::deferred_output,
};
@else
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
};

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
@if(VISIBILITY_RANGE_DITHER)
    pbr_functions::visibility_range_dither(in.position, in.visibility_range_dither);

    var pbr_input = pbr_input_from_standard_material(in, is_front);

@if(VERTEX_UVS_A) {
    let w = terrain_layers::weights(terrain_layers::height(in.instance_index, in.world_position));
    pbr_input.material.base_color =
        terrain_layers::blend_color(pbr_input.material.base_color, in.uv, w);
    // With a normal prepass the normal is read from it, already blended
    // (`terrain_prepass`).
@if(VERTEX_TANGENTS && !LOAD_PREPASS_NORMALS) {
    let flags = pbr_input.material.flags;
    let double_sided = (flags & pbr_types::STANDARD_MATERIAL_FLAGS_DOUBLE_SIDED_BIT) != 0u;
    let front = pbr_functions::winding_corrected_front_facing(pbr_input.flags, is_front);
    pbr_input.N = terrain_layers::blend_normal(
        pbr_input.N,
        pbr_input.world_normal,
        in.world_tangent,
        in.uv,
        w,
        flags,
        double_sided,
        front,
    );
}
}

    pbr_input.material.base_color = alpha_discard(
        pbr_input.material.flags,
        pbr_input.material.alpha_cutoff,
        pbr_input.material.base_color,
    );
    apply_decals(&pbr_input);

@if(PREPASS_PIPELINE) {
    // The G-buffer; lighting happens in the deferred lighting pass.
    return deferred_output(in, pbr_input);
}
@if(!PREPASS_PIPELINE) {
    var out: FragmentOutput;
    if (pbr_input.material.flags & pbr_types::STANDARD_MATERIAL_FLAGS_UNLIT_BIT) == 0u {
        out.color = apply_pbr_lighting(pbr_input);
    } else {
        out.color = pbr_input.material.base_color;
    }
    // Fog, alpha premultiplication, and tone mapping without HDR.
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
}
