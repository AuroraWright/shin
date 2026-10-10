#import types::{PosTexVertex, MosaicUniformParams}

@group(0) @binding(0)
var<uniform> params: MosaicUniformParams;
@group(0) @binding(1)
var source_texture: texture_2d<f32>;
@group(0) @binding(2)
var source_sampler: sampler;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) source_uv: vec2<f32>,
}
@vertex
fn vertex_main(input: PosTexVertex) -> VertexOutput {
    var output: VertexOutput;
    output.clip_position = params.transform * vec4<f32>(input.position, 0.0, 1.0);
    output.source_uv = input.texture_position;
    return output;
}
@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    // Native program42: centered grid, floor, then sample the cell center.
    let cell = floor((input.source_uv - vec2<f32>(0.5)) * params.step.zw);
    let uv = (cell + vec2<f32>(0.5)) * params.step.xy + vec2<f32>(0.5);
    // Derivatives after floor can straddle cells; source has only mip level0.
    return textureSampleLevel(source_texture, source_sampler, uv, 0.0);
}
