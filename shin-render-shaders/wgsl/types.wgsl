struct GlassVertex {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) texture_position: vec2<f32>,
}

struct PosVertex {
    @location(0) position: vec3<f32>,
}

struct PosTexVertex {
    @location(0) position: vec2<f32>,
    @location(1) texture_position: vec2<f32>,
}

struct PosColVertex {
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
}

struct PosColTexVertex {
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) texture_position: vec2<f32>,
}

struct TextVertex {
    @location(0) position: vec4<f32>,
    @location(1) color: f32,
}

struct BlendVertex {
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) texture_position: vec4<f32>,
}

struct WindowVertex {
    @location(0) position: vec4<f32>,
    @location(1) texture_position: vec4<f32>,
}

struct MaskVertex {
    @location(0) position: vec2<f32>,
    @location(1) texture_position: vec2<f32>,
    @location(2) mask_position: vec2<f32>,
}

struct ClearUniformParams {
    color: vec4<f32>,
}

struct FillUniformParams {
    transform: mat4x4<f32>,
}

struct SpriteUniformParams {
    transform: mat4x4<f32>,
}

struct ButtonUniformParams {
    transform: mat4x4<f32>,
    flash1_: vec4<f32>,
    flash2_: vec4<f32>,
}

struct BlendUniformParams {
    transform: mat4x4<f32>,
    blend: vec4<f32>,
    flash: vec4<f32>,
}

struct MosaicUniformParams {
    transform: mat4x4<f32>,
    step: vec4<f32>,
}

struct RasterUniformParams {
    transform: mat4x4<f32>,
    amplitude_frequency: vec4<f32>,
    phase: vec4<f32>,
}

struct RippleUniformParams {
    transform: mat4x4<f32>,
    wave: vec4<f32>,
}

struct TapRippleUniformParams {
    transform: mat4x4<f32>,
    center_scale: vec4<f32>,
    wave_params: vec4<f32>,
}

struct HigurashiBlurUniformParams {
    transform: mat4x4<f32>,
    distances_first: vec4<f32>,
    distances_last: vec4<f32>,
    color: vec4<f32>,
    weights: vec4<f32>,
}

struct HigurashiRasterUniformParams {
    transform: mat4x4<f32>,
    color: vec4<f32>,
    amplitude_frequency: vec4<f32>,
    phase: vec4<f32>,
}

struct BlurUniformParams {
    transform: mat4x4<f32>,
    step: vec4<f32>,
}

struct ZoomBlurUniformParams {
    transform: mat4x4<f32>,
    center: vec4<f32>,
    scales_first: vec4<f32>,
    scales_last: vec4<f32>,
}

struct WiperWhirlUniformParams {
    transform: mat4x4<f32>,
    effect: vec4<f32>,
}

struct WiperWaveUniformParams {
    transform: mat4x4<f32>,
    center_scale: vec4<f32>,
    front_width: vec4<f32>,
}

struct WiperRippleUniformParams {
    transform: mat4x4<f32>,
    center_scale: vec4<f32>,
    wave_params: vec4<f32>,
}

struct CharacterPortraitUniformParams {
    transform: mat4x4<f32>,
    effect: vec4<f32>,
}

struct WiperGlassUniformParams {
    transform: mat4x4<f32>,
    model: mat4x4<f32>,
    effect: vec4<f32>,
}

struct FontUniformParams {
    transform: mat4x4<f32>,
    color1_: vec4<f32>,
    color2_: vec4<f32>,
}

struct FontBorderUniformParams {
    transform: mat4x4<f32>,
    dist: array<vec4<f32>, 4>,
    color: vec4<f32>,
}

struct LayerUniformParams {
    transform: mat4x4<f32>,
    color: vec4<f32>,
    fragment_param: vec4<f32>,
    output_type: u32,
    fragment_operation: u32,
}

struct MaskUniformParams {
    transform: mat4x4<f32>,
    color: vec4<f32>,
    fragment_param: vec4<f32>,
    minmax: vec4<f32>,
    fragment_operation: u32,
}

struct MovieUniformParams {
    transform: mat4x4<f32>,
    color_bias: vec4<f32>,
    color_transform: array<vec4<f32>, 3>,
    color_multiplier: vec4<f32>,
    alpha_mode: u32,
    texture_size: vec4<f32>,
}

struct WiperDefaultUniformParams {
    transform: mat4x4<f32>,
    alpha: vec4<f32>,
}

struct WiperMaskUniformParams {
    transform: mat4x4<f32>,
    minmax: vec4<f32>,
}

