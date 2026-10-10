fn map(value: f32, minmax: vec2<f32>) -> f32 {
    let min = minmax.x;
    let max = minmax.y;

    return fma(value, max - min, min);
}

fn evaluate_fragment_shader(sampled: vec3<f32>, tint: vec3<f32>, operation: u32, param: vec4<f32>) -> vec3<f32> {
    let color = sampled * tint;
    if operation == 0 {
        // default
        return color;
    } else if operation == 1 {
        // mono
        // Native programs11/17/23/29 round the first luma multiply, then
        // accumulate green and blue with FFMA. Their delta subtracts the
        // original sampled*tint product inside FFMA, before interpolation.
        let red_luma = color.x * 0.299;
        let green_luma = fma(color.y, 0.587, red_luma);
        let luma = fma(color.z, 0.114, green_luma);
        let delta = fma(sampled, -tint, vec3<f32>(luma));
        return fma(delta, vec3<f32>(param.w), color) * param.xyz;
    } else if operation == 2 {
        // fill
        // Native programs9/15/21/27 preserve both fused steps, including the
        // sampled*tint subtraction rather than subtracting rounded color.
        let delta = fma(sampled, -tint, param.xyz);
        return fma(delta, vec3<f32>(param.w), color);
    } else if operation == 3 {
        // fill2
        // Native texture programs10/16/22 have the same executable code as
        // Default8/14/20. CPU drawable Fill2 has a separate interpolation rule.
        return color;
    } else if operation == 4 {
        // negative
        // Native programs12/18/24 multiply by the RGB parameter gains after
        // weighted inversion. Preserve their FMUL, FFMA, FADD, FMUL order.
        let weighted = color * param.w;
        let delta = fma(weighted, vec3<f32>(-2.0), vec3<f32>(param.w));
        return (color + delta) * param.xyz;
    } else if operation == 5 {
        // gamma
        // Native programs13/19/25/31 compute parameter reciprocals before
        // multiplying LG2(tinted RGB), then apply EX2. Alpha and param.w
        // do not participate in the color operation.
        let reciprocal = vec3<f32>(1.0) / param.xyz;
        return exp2(log2(color) * reciprocal);
    } else {
        return color;
    }
}
