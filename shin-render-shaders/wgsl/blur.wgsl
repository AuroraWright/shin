#import types::{PosTexVertex, BlurUniformParams}
@group(0) @binding(0)
var<uniform> params: BlurUniformParams;
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
    // Native program43: four symmetric pairs, each weighted by half
    // of the recovered pair weight. No central sample or alpha conversion.
    let uv = input.source_uv;
    let step = params.step.xy;
    let a = textureSampleLevel(source_texture, source_sampler, uv - step * 0.5, 0.0)
          + textureSampleLevel(source_texture, source_sampler, uv + step * 0.5, 0.0);
    let b = textureSampleLevel(source_texture, source_sampler, uv - step * 1.5, 0.0)
          + textureSampleLevel(source_texture, source_sampler, uv + step * 1.5, 0.0);
    let c = textureSampleLevel(source_texture, source_sampler, uv - step * 2.5, 0.0)
          + textureSampleLevel(source_texture, source_sampler, uv + step * 2.5, 0.0);
    let d = textureSampleLevel(source_texture, source_sampler, uv - step * 3.5, 0.0)
          + textureSampleLevel(source_texture, source_sampler, uv + step * 3.5, 0.0);
    return a * (0.5 * bitcast<f32>(0x3eceed6bu))
         + b * (0.5 * bitcast<f32>(0x3ea127beu))
         + c * (0.5 * bitcast<f32>(0x3e437dbeu))
         + d * (0.5 * bitcast<f32>(0x3db8afdfu));
}
