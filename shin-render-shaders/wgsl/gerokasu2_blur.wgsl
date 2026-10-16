#import types::{PosTexVertex, HigurashiBlurUniformParams}
@group(0) @binding(0)
var<uniform> params: HigurashiBlurUniformParams;
@group(0) @binding(1)
var source_texture: texture_2d<f32>;
@group(0) @binding(2)
var source_sampler: sampler;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv0: vec2<f32>,
    @location(1) uv1: vec2<f32>,
    @location(2) uv2: vec2<f32>,
    @location(3) uv3: vec2<f32>,
    @location(4) uv4: vec2<f32>,
    @location(5) uv5: vec2<f32>,
    @location(6) uv6: vec2<f32>,
    @location(7) uv7: vec2<f32>,
}
@vertex
fn vertex_main(input: PosTexVertex) -> VertexOutput {
    var output: VertexOutput;
    // Ger44 VS1a69e4: multiply Y, FMA X, then add translation.
    output.clip_position = fma(vec4<f32>(input.position.x), params.transform[0],
                               input.position.y * params.transform[1])
                         + params.transform[3];
    // The native vertex stage adds the four explicit offsets before interpolation.
    output.uv0 = input.texture_position - params.distances_first.xy;
    output.uv1 = input.texture_position + params.distances_first.xy;
    output.uv2 = input.texture_position - params.distances_first.zw;
    output.uv3 = input.texture_position + params.distances_first.zw;
    output.uv4 = input.texture_position - params.distances_last.xy;
    output.uv5 = input.texture_position + params.distances_last.xy;
    output.uv6 = input.texture_position - params.distances_last.zw;
    output.uv7 = input.texture_position + params.distances_last.zw;
    return output;
}
@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    // Ger44 FS1a7fa4: positive + negative within each pair; all four RGBA
    // channels use the same half weights. Seed pair1, FMA pair0, pair2, pair3.
    let pair0 = textureSample(source_texture, source_sampler, input.uv1)
              + textureSample(source_texture, source_sampler, input.uv0);
    let pair1 = textureSample(source_texture, source_sampler, input.uv3)
              + textureSample(source_texture, source_sampler, input.uv2);
    let pair2 = textureSample(source_texture, source_sampler, input.uv5)
              + textureSample(source_texture, source_sampler, input.uv4);
    let pair3 = textureSample(source_texture, source_sampler, input.uv7)
              + textureSample(source_texture, source_sampler, input.uv6);
    let weights = params.weights * 0.5;
    var color = pair1 * weights.y;
    color = fma(vec4<f32>(weights.x), pair0, color);
    color = fma(vec4<f32>(weights.z), pair2, color);
    color = fma(vec4<f32>(weights.w), pair3, color);
    return color;
}
