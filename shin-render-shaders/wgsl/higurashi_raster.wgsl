#import types::{PosTexVertex, HigurashiRasterUniformParams}
@group(0) @binding(0)
var<uniform> params: HigurashiRasterUniformParams;
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
    let y = params.transform[1] * input.position.y;
    output.clip_position = fma(params.transform[0], vec4<f32>(input.position.x), y)
                         + params.transform[3];
    output.source_uv = input.texture_position;
    return output;
}
@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    // Original updated program24,1c3f58: crossed axes, literal angular factor,
    // displaced sample, then straight RGBA tint. Later program45 omits tint.
    let cycles = fma(input.source_uv.yx, params.amplitude_frequency.zw, params.phase.xy);
    let wave = sin(cycles * 6.28318548);
    let uv = fma(wave, params.amplitude_frequency.xy, input.source_uv);
    return textureSample(source_texture, source_sampler, uv) * params.color;
}
