#import types::{MaskVertex, SpriteUniformParams}

@group(0) @binding(0)
var<uniform> params: SpriteUniformParams;
@group(0) @binding(1)
var source_texture: texture_2d<f32>;
@group(0) @binding(2)
var source_sampler: sampler;
@group(0) @binding(3)
var breakup_texture: texture_2d<f32>;
@group(0) @binding(4)
var breakup_sampler: sampler;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) source_uv: vec2<f32>,
    @location(1) breakup_uv: vec2<f32>,
}
@vertex
fn vertex_main(input: MaskVertex) -> VertexOutput {
    var output: VertexOutput;
    output.clip_position = params.transform * vec4<f32>(input.position, 0.0, 1.0);
    output.source_uv = input.texture_position;
    output.breakup_uv = input.mask_position;
    return output;
}
@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    // Native Maxwell program47 samples only the second texture's alpha.
    let source = textureSample(source_texture, source_sampler, input.source_uv);
    let breakup = textureSample(breakup_texture, breakup_sampler, input.breakup_uv).a;
    return source * breakup;
}
