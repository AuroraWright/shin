#import types::{PosTexVertex, CharacterPortraitUniformParams}

@group(0) @binding(0)
var<uniform> params: CharacterPortraitUniformParams;
@group(0) @binding(1)
var portrait_texture: texture_2d<f32>;
@group(0) @binding(2)
var portrait_sampler: sampler;
@group(0) @binding(3)
var frame_texture: texture_2d<f32>;
@group(0) @binding(4)
var frame_sampler: sampler;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vertex_main(input: PosTexVertex) -> VertexOutput {
    var output: VertexOutput;
    output.clip_position = params.transform * vec4<f32>(input.position, 0.0, 1.0);
    output.uv = input.texture_position;
    return output;
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let portrait = textureSample(portrait_texture, portrait_sampler, input.uv);
    let frame = textureSample(frame_texture, frame_sampler, input.uv);
    var rgb = portrait.rgb;
    let mode = u32(params.effect.x);
    // Native programs48..51 ignore the frame alpha and output opaque RGB.
    if mode == 1u || (mode == 3u && input.uv.y > 1.0 - input.uv.x) {
        rgb = vec3<f32>(rgb.r, rgb.g * 0.1, rgb.b * 0.1);
    }
    if mode == 2u {
        let gray = dot(rgb, vec3<f32>(bitcast<f32>(0x3e991687u), 0.587, 0.114));
        rgb = vec3<f32>(gray);
    }
    return vec4<f32>(rgb * portrait.a + frame.rgb * (1.0 - portrait.a), 1.0);
}
