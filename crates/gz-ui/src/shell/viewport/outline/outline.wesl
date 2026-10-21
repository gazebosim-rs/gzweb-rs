// The selection outline (spec §6.4): `selection_outline` just outside the
// silhouettes in the mask (red: the selection, green: the Scene tree's
// hover), antialiased over the last texel. Inside a silhouette, nothing.

import bevy_ui_render::ui_vertex_output::UiVertexOutput;

struct OutlineParams {
    selected: vec4<f32>,
    hovered: vec4<f32>,
    // x, y: one mask texel in uv; z, w: the selection's and the hover's
    // widths in texels.
    texel_widths: vec4<f32>,
};

@group(1) @binding(0) var<uniform> params: OutlineParams;
@group(1) @binding(1) var mask: texture_2d<f32>;
@group(1) @binding(2) var mask_sampler: sampler;

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    let texel = params.texel_widths.xy;
    let here = textureSampleLevel(mask, mask_sampler, in.uv, 0.0);
    if (here.r > 0.5) {
        return vec4<f32>(0.0);
    }
    let reach = i32(ceil(max(params.texel_widths.z, params.texel_widths.w))) + 1;
    var to_selected = 1e3;
    var to_hovered = 1e3;
    for (var y = -reach; y <= reach; y++) {
        for (var x = -reach; x <= reach; x++) {
            let o = vec2<f32>(f32(x), f32(y));
            let m = textureSampleLevel(mask, mask_sampler, in.uv + o * texel, 0.0);
            let d = length(o);
            if (m.r > 0.5) {
                to_selected = min(to_selected, d);
            }
            if (m.g > 0.5) {
                to_hovered = min(to_hovered, d);
            }
        }
    }
    let selected = clamp(params.texel_widths.z + 0.5 - to_selected, 0.0, 1.0);
    if (selected > 0.0) {
        return vec4<f32>(params.selected.rgb, params.selected.a * selected);
    }
    if (here.g > 0.5) {
        return vec4<f32>(0.0);
    }
    let hovered = clamp(params.texel_widths.w + 0.5 - to_hovered, 0.0, 1.0);
    return vec4<f32>(params.hovered.rgb, params.hovered.a * hovered);
}
