#import types::{PosTexVertex, ZoomBlurUniformParams}
@group(0) @binding(0)
var<uniform> params: ZoomBlurUniformParams;
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
    let center = params.center.xy;
    let delta = input.source_uv - center;
    let scales = array<f32, 8>(params.scales_first.x, params.scales_first.y, params.scales_first.z, params.scales_first.w,
                              params.scales_last.x, params.scales_last.y, params.scales_last.z, params.scales_last.w);
    let weights = array<f32, 8>(bitcast<f32>(0x3e638e39u), bitcast<f32>(0x3e471c72u),
                               bitcast<f32>(0x3e2aaaabu), bitcast<f32>(0x3e0e38e4u),
                               bitcast<f32>(0x3de38e39u), bitcast<f32>(0x3daaaaabu),
                               bitcast<f32>(0x3d638e39u), bitcast<f32>(0x3ce38e39u));
    var samples: array<vec4<f32>, 8>;
    for (var i = 0u; i < 8u; i += 1u) {
        samples[i] = textureSampleLevel(source_texture, source_sampler, center + delta * scales[i], 0.0);
    }
    // Native accumulates adjacent weighted pairs with FMA, then two groups of four.
    let a = fma(samples[0], vec4<f32>(weights[0]), samples[1] * weights[1]);
    let b = fma(samples[2], vec4<f32>(weights[2]), samples[3] * weights[3]);
    let c = fma(samples[4], vec4<f32>(weights[4]), samples[5] * weights[5]);
    let d = fma(samples[6], vec4<f32>(weights[6]), samples[7] * weights[7]);
    return (a + b) + (c + d);
}
