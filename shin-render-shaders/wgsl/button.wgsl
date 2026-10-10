#import types::{PosColTexVertex, ButtonUniformParams}

@group(0) @binding(0)
var<uniform> params: ButtonUniformParams;

// this will be detected as out build script as a single texture
// the binding names should have the same prefix and end with _texture and _sampler
@group(0) @binding(1)
var button_texture: texture_2d<f32>;
@group(0) @binding(2)
var button_sampler: sampler;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) texture_position: vec2<f32>,
}

@vertex
fn vertex_main(input: PosColTexVertex) -> VertexOutput {
    var output: VertexOutput;

    output.clip_position = params.transform * vec4<f32>(input.position, 1.0);
    // UnormColor stores RGBA bytes, matching the Unorm8x4 vertex attribute.
    output.color = input.color;
    output.texture_position = input.texture_position;

    return output;
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let sampled = textureSample(button_texture, button_sampler, input.texture_position);

    // Native program5 applies two FADD/FFMA RGB flash stages before tint.
    let first = fma(params.flash1_.rgb - sampled.rgb, vec3<f32>(params.flash1_.a), sampled.rgb);
    let second = fma(params.flash2_.rgb - first, vec3<f32>(params.flash2_.a), first);
    return vec4(second * input.color.rgb, sampled.a * input.color.a);
}
