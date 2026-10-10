#import types::{PosTexVertex, WiperRippleUniformParams}

@group(0) @binding(0)
var<uniform> params: WiperRippleUniformParams;

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
    let angle = (2.0 * radius - params.wave_params.z) * params.wave_params.x
              * bitcast<f32>(0x40c90fdbu);
    let displaced_radius = radius + sin(angle) * params.wave_params.y;
    let uv = params.center_scale.xy
           + (delta / radius) * displaced_radius / params.center_scale.zw;
    return mix(textureSampleLevel(source_texture, source_sampler, uv, 0.0),
               textureSampleLevel(target_texture, target_sampler, uv, 0.0), params.wave_params.w);
}
