#import types::{PosTexVertex, WiperWaveUniformParams}

@group(0) @binding(0)
var<uniform> params: WiperWaveUniformParams;

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
    let delta = (input.texture_position - params.center_scale.xy) * params.center_scale.zw;
    let radius = length(delta);
    let phase = (params.front_width.x - radius) / params.front_width.y;
    if phase <= 0.0 {
        return textureSampleLevel(source_texture, source_sampler, input.texture_position, 0.0);
    }
    if phase >= 1.0 {
        return textureSampleLevel(target_texture, target_sampler, input.texture_position, 0.0);
    }
    // Original program38 distorts only the advancing transition band.
    let displacement = sin(phase * bitcast<f32>(0x40c90fdbu))
                     * params.front_width.y * params.front_width.z * 0.25;
    let uv = params.center_scale.xy
           + (delta / radius) * (radius + displacement) / params.center_scale.zw;
    let source = textureSampleLevel(source_texture, source_sampler, uv, 0.0);
    let incoming = textureSampleLevel(target_texture, target_sampler, uv, 0.0);
    let blended = mix(source, incoming, phase);
    let highlight = sin(phase * bitcast<f32>(0x40490fdbu)) * 0.1;
    return vec4<f32>(blended.rgb + vec3<f32>(highlight), blended.a);
}
