#import types::{WindowVertex, FillUniformParams}

@group(0) @binding(0)
var<uniform> params: FillUniformParams;
@group(0) @binding(1)
var border_texture: texture_2d<f32>;
@group(0) @binding(2)
var border_sampler: sampler;
@group(0) @binding(3)
var mask_texture: texture_2d<f32>;
@group(0) @binding(4)
var mask_sampler: sampler;
@group(0) @binding(5)
var background_texture: texture_2d<f32>;
@group(0) @binding(6)
var background_sampler: sampler;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) border_position: vec2<f32>,
    @location(1) background_position: vec2<f32>,
}

@vertex
fn vertex_main(input: WindowVertex) -> VertexOutput {
    var output: VertexOutput;
    // Ger program8: one multiply, one FMA, then translation. The final
    // normalized UV pair is written by the CPU but unused by its fragment.
    let y = params.transform[1] * input.position.y;
    output.clip_position = fma(params.transform[0], vec4<f32>(input.position.x), y) + params.transform[3];
    output.border_position = input.position.zw;
    output.background_position = input.texture_position.xy;
    return output;
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    // Original fragment16fd54..16fe3c samples both border and mask with
    // the nine-slice UV. Background uses the centered 1920x1080 crop.
    let background = textureSample(background_texture, background_sampler, input.background_position);
    let mask = textureSample(mask_texture, mask_sampler, input.border_position);
    let border = textureSample(border_texture, border_sampler, input.border_position);
    let masked = background * mask;
    let behind = fma(masked.rgb, vec3<f32>(-border.a), masked.rgb);
    let overlap = fma(-masked.a, border.a, border.a);
    return vec4<f32>(fma(border.rgb, vec3<f32>(border.a), behind), fma(background.a, mask.a, overlap));
}
