#import types::{PosTexVertex, LayerUniformParams}

@group(0) @binding(0)
var<uniform> params: LayerUniformParams;
@group(0) @binding(1)
var source_texture: texture_2d<f32>;
@group(0) @binding(2)
var source_sampler: sampler;

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

// Updated executable programs6/9/7/8/10/11. The older Fill2 shader retains
// its interpolation operation; the later shared texture shader is different.
@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let sampled = textureSample(source_texture, source_sampler, input.texture_position);
    let tinted = sampled * params.color;
    let p = params.fragment_param;
    let operation = params.fragment_operation;
    if operation == 1u {
        let red_luma = tinted.x * 0.299;
        let green_luma = fma(tinted.y, 0.587, red_luma);
        let luma = fma(tinted.z, 0.114, green_luma);
        let delta = fma(sampled.xyz, -params.color.xyz, vec3<f32>(luma));
        return vec4<f32>(fma(delta, vec3<f32>(p.w), tinted.xyz) * p.xyz, tinted.w);
    } else if operation == 2u {
        let delta = fma(sampled.xyz, -params.color.xyz, p.xyz);
        return vec4<f32>(fma(delta, vec3<f32>(p.w), tinted.xyz), tinted.w);
    } else if operation == 3u {
        let delta = fma(sampled.xyz, -params.color.xyz, vec3<f32>(p.w));
        return vec4<f32>(fma(delta, p.xyz, tinted.xyz), tinted.w);
    } else if operation == 4u {
        // Program10 keeps this vec4 expression order. Rewriting the two
        // interpolation steps as explicit fma changes UNORM byte boundaries
        // for filtered Movie RGBA, even when the algebra is equivalent.
        let inverted = tinted * vec4<f32>(-1.0, -1.0, -1.0, 1.0)
            + vec4<f32>(1.0, 1.0, 1.0, 0.0);
        let delta = (-sampled) * params.color + inverted;
        let interpolated = vec4<f32>(p.w, p.w, p.w, 1.0) * delta + tinted;
        return vec4<f32>(interpolated.xyz * p.xyz, interpolated.w);
    } else if operation == 5u {
        let reciprocal = vec3<f32>(1.0) / p.xyz;
        return exp2(log2(tinted) * vec4<f32>(reciprocal, 1.0));
    }
    return tinted;
}
