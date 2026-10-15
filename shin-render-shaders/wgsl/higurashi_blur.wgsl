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
}
@vertex
fn vertex_main(input: PosTexVertex) -> VertexOutput {
    var output: VertexOutput;
    // Program23 computes offsets at vertices, before interpolation.
    output.clip_position = params.transform[0] * input.position.x
                         + params.transform[1] * input.position.y
                         + params.transform[3];
    output.uv0 = input.texture_position;
    output.uv1 = input.texture_position + params.distances_first.xy;
    output.uv2 = input.texture_position - params.distances_first.xy;
    output.uv3 = input.texture_position + params.distances_first.zw;
    output.uv4 = input.texture_position - params.distances_first.zw;
    output.uv5 = input.texture_position + params.distances_last.xy;
    output.uv6 = input.texture_position - params.distances_last.xy;
    return output;
}
@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    // Original fragment order: +1,center,-1,+2,-2,+3,-3. No alpha conversion.
    var color = textureSample(source_texture, source_sampler, input.uv1) * params.weights.y;
    color = textureSample(source_texture, source_sampler, input.uv0) * params.weights.x + color;
    color = textureSample(source_texture, source_sampler, input.uv2) * params.weights.y + color;
    color = textureSample(source_texture, source_sampler, input.uv3) * params.weights.z + color;
    color = textureSample(source_texture, source_sampler, input.uv4) * params.weights.z + color;
    color = textureSample(source_texture, source_sampler, input.uv5) * params.weights.w + color;
    color = textureSample(source_texture, source_sampler, input.uv6) * params.weights.w + color;
    return color * params.color;
}
