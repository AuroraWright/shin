#import types::{BlendVertex, BlendUniformParams}

@group(0) @binding(0)
var<uniform> params: BlendUniformParams;
@group(0) @binding(1)
var texture1_texture: texture_2d<f32>;
@group(0) @binding(2)
var texture1_sampler: sampler;
@group(0) @binding(3)
var texture2_texture: texture_2d<f32>;
@group(0) @binding(4)
var texture2_sampler: sampler;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) texture_position: vec4<f32>,
}

@vertex
fn vertex_main(input: BlendVertex) -> VertexOutput {
    var output: VertexOutput;
    output.clip_position = params.transform * vec4<f32>(input.position, 1.0);
    output.color = input.color;
    output.texture_position = input.texture_position;
    return output;
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let first = textureSample(texture1_texture, texture1_sampler, input.texture_position.xy);
    let second = textureSample(texture2_texture, texture2_sampler, input.texture_position.zw);
    // Native program6 subtracts first, then FFMA interpolates all four
    // channels. RGB flash uses the same order and preserves mixed alpha.
    let mixed = fma(second - first, params.blend, first);
    let flashed = fma(params.flash.rgb - mixed.rgb, vec3<f32>(params.flash.a), mixed.rgb);
    return vec4(flashed, mixed.a) * input.color;
}
