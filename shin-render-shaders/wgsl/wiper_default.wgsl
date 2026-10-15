#import types::{PosTexVertex, WiperDefaultUniformParams}

@group(0) @binding(0)
var<uniform> params: WiperDefaultUniformParams;

@group(0) @binding(1)
var source_texture: texture_2d<f32>;
@group(0) @binding(2)
var source_sampler: sampler;
@group(0) @binding(3)
var target_texture: texture_2d<f32>;
@group(0) @binding(4)
var target_sampler: sampler;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) texture_position: vec2<f32>,
}

@vertex
fn vertex_main(input: PosTexVertex) -> VertexOutput {
    var output: VertexOutput;

    output.clip_position = params.transform * vec4<f32>(input.position, 0.0, 1.0);
    output.texture_position = input.texture_position;

    return output;
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let sampled_source = textureSample(source_texture, source_sampler, input.texture_position);
    let sampled_target = textureSample(target_texture, target_sampler, input.texture_position);

    let alpha = params.alpha.x;

    if params.alpha.y != 0.0 {
        // Updated Higurashi program21's embedded GLSL (1c50ff..1c5405):
        // incoming * alpha + outgoing * (1 - alpha), including RGBA.
        let incoming = sampled_target * alpha;
        let outgoing = sampled_source * (1.0 - alpha);
        return outgoing + incoming;
    }

    // Native program36 weights outgoing RGBA with FFMA(-alpha, source,
    // source), then adds the incoming RGBA in a second fused stage.
    let outgoing = fma(vec4<f32>(-alpha), sampled_source, sampled_source);
    return fma(vec4<f32>(alpha), sampled_target, outgoing);
}
