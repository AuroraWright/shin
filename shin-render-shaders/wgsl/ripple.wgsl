#import types::{PosTexVertex, RippleUniformParams}
@group(0) @binding(0)
var<uniform> params: RippleUniformParams;
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
    // Native program46 normalizes UV distance, without aspect correction.
    let delta = input.source_uv - vec2<f32>(0.5);
    let squared_radius = dot(delta, delta);
    let radius = sqrt(squared_radius);
    let direction = delta * inverseSqrt(squared_radius);
    let cycles = (radius * 2.0 - params.wave.z) * params.wave.x;
    let shifted_radius = radius + sin(cycles * 6.283185307179586) * params.wave.y;
    let uv = direction * shifted_radius + vec2<f32>(0.5);
    return textureSampleLevel(source_texture, source_sampler, uv, 0.0);
}
