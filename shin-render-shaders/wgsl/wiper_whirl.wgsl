#import types::{PosTexVertex, WiperWhirlUniformParams}

@group(0) @binding(0)
var<uniform> params: WiperWhirlUniformParams;

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
    let progress = clamp(params.effect.x, 0.0, 1.0);
    let fade = progress * progress * (3.0 - 2.0 * progress);
    let pulse = clamp(1.0 - 2.0 * abs(params.effect.x - 0.5), 0.0, 1.0);
    let envelope = pulse * pulse * (3.0 - 2.0 * pulse);
    let metric = params.effect.zw;
    let delta = (input.texture_position - vec2<f32>(0.5)) * metric;
    let radius = length(delta);
    let turns = fade * params.effect.y + 0.3 * envelope * sin(radius * bitcast<f32>(0x41fb53d2u));
    let angle = turns * bitcast<f32>(0x40c90fdbu);
    let rotated = vec2<f32>(delta.x * cos(angle) - delta.y * sin(angle),
                            delta.x * sin(angle) + delta.y * cos(angle));
    let uv = rotated / metric + vec2<f32>(0.5);
    return mix(textureSampleLevel(source_texture, source_sampler, uv, 0.0),
               textureSampleLevel(target_texture, target_sampler, uv, 0.0), fade);
}
