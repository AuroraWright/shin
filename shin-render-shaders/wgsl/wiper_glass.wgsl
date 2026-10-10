#import types::{GlassVertex, WiperGlassUniformParams}

@group(0) @binding(0)
var<uniform> params: WiperGlassUniformParams;
@group(0) @binding(1)
var source_texture: texture_2d<f32>;
@group(0) @binding(2)
var source_sampler: sampler;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) texture_position: vec2<f32>,
    @location(1) brightness: f32,
    @location(2) opacity: f32,
}

@vertex
fn vertex_main(input: GlassVertex) -> VertexOutput {
    var output: VertexOutput;
    output.clip_position = params.transform * vec4<f32>(input.position, 1.0);
    let normal = params.model * vec4<f32>(input.normal, 0.0);
    let light = bitcast<f32>(0xbe3504f3u);
    output.brightness = fma(normal.x, light, normal.y * light) + params.effect.y;
    output.opacity = params.effect.x;
    output.texture_position = input.texture_position;
    return output;
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let source = textureSample(source_texture, source_sampler, input.texture_position).rgb;
    return vec4<f32>(source + vec3<f32>(input.brightness), input.opacity);
}
