#import types::{PosTexVertex, TapRippleUniformParams}
@group(0) @binding(0)
var<uniform> params: TapRippleUniformParams;
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
    // Native program33 (original Maxwell20e4f0): diagonal UV distortion,
    // a sine highlight on all RGBA channels, then premultiplied radial fade.
    let delta = input.source_uv - params.center_scale.xy;
    let metric = delta * params.center_scale.zw;
    let radius = length(metric);
    let wave = sin((radius - params.wave_params.x) * 6.283185307179586);
    let uv = input.source_uv + wave * params.wave_params.zw;
    let source = textureSampleLevel(source_texture, source_sampler, uv, 0.0);
    let r = min(radius, 1.0);
    // Native r9 loads age, then multiplies by -age before the opacity FFMA.
    let opacity = (1.0 - r * r * r) * (1.0 - params.wave_params.x * params.wave_params.x);
    return (source + vec4<f32>(wave * 0.2)) * opacity;
}
