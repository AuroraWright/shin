#import types::{TextVertex, FontSdfUniformParams}

@group(0) @binding(0)
var<uniform> params: FontSdfUniformParams;
@group(0) @binding(1)
var glyph_texture: texture_2d<f32>;
@group(0) @binding(2)
var glyph_sampler: sampler;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: f32,
    @location(1) texture_position: vec2<f32>,
}

@vertex
fn vertex_main(input: TextVertex) -> VertexOutput {
    var output: VertexOutput;
    output.clip_position = params.transform * vec4<f32>(input.position.xy, 0.0, 1.0);
    output.color = input.color;
    output.texture_position = input.position.zw;
    return output;
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    // Gerokasu2 program5: signed R8 distance, rounded endpoint subtraction,
    // saturated normalization, FFMA(3-2t), t*t, then the cubic product.
    let distance = textureSample(glyph_texture, glyph_sampler, input.texture_position).x;
    let inverse_span = 1.0 / (params.thresholds.y - params.thresholds.x);
    let t = clamp((distance - params.thresholds.x) * inverse_span, 0.0, 1.0);
    let coverage = (t * t) * fma(-2.0, t, 3.0);
    // Literal c1[0] at168dcc, compared with coverage before tint alpha.
    if coverage < 0.001 {
        discard;
    }
    let tint = fma(vec4<f32>(input.color), params.color2_ - params.color1_, params.color1_);
    return tint * vec4<f32>(1.0, 1.0, 1.0, coverage);
}
