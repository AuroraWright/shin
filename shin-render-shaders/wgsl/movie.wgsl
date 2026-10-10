#import types::{PosTexVertex, MovieUniformParams}

@group(0) @binding(0)
var<uniform> params: MovieUniformParams;

// this will be detected as out build script as a single texture bind group
// we can only bind these in an isolated bind group!
@group(0) @binding(1)
var luma_texture: texture_2d<f32>;
@group(0) @binding(2)
var luma_sampler: sampler;
@group(0) @binding(3)
var chroma_texture: texture_2d<f32>;
@group(0) @binding(4)
var chroma_sampler: sampler;

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
    var luma_uv = input.texture_position;
    var chroma_uv = input.texture_position;
    var alpha = 1.0;
    if params.alpha_mode != 0u {
        let luma_height = params.texture_size.y;
        let chroma_height = params.texture_size.w;
        let region_height = 0.5 - 4.0 / luma_height;
        let mask_start = 0.5 + 4.0 / luma_height;
        luma_uv.y = min(input.texture_position.y * region_height, region_height - 0.5 / luma_height);
        chroma_uv.y = min(input.texture_position.y * region_height, region_height - 0.5 / chroma_height);
        let alpha_uv = vec2<f32>(input.texture_position.x,
            max(input.texture_position.y * region_height + mask_start, mask_start + 0.5 / luma_height));
        let mask = textureSample(luma_texture, luma_sampler, alpha_uv).x;
        // The grayscale mask is encoded as video-range luma: black16, white235.
        alpha = clamp((mask - 16.0 / 255.0) * (255.0 / 219.0), 0.0, 1.0);
    }
    let luma = textureSample(luma_texture, luma_sampler, luma_uv).x;
    let chroma = textureSample(chroma_texture, chroma_sampler, chroma_uv).xy;

    let biased_yuv = vec3<f32>(luma, chroma.x, chroma.y) - params.color_bias.xyz;

    let r = dot(biased_yuv, params.color_transform[0].xyz);
    let g = dot(biased_yuv, params.color_transform[1].xyz);
    let b = dot(biased_yuv, params.color_transform[2].xyz);

    return vec4<f32>(r, g, b, alpha) * params.color_multiplier;
}
