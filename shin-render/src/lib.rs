#![expect(clippy::derivable_impls)] // want to be explicit

//! Rendering framework for the shin engine.

// here we create an abstraction over wgpu which makes it look more like shin's render abstraction over nvn.
// an important departure is not using global variables, but making all the arguments explicit (helped by a builder pattern with typestates (maybe))

mod depth_stencil;
pub mod dynamic_buffer;
pub mod gpu_texture;
pub mod init;
pub mod pipelines;
pub mod quad_vertices;
pub mod render_pass;
pub mod render_texture;
pub mod resize;
pub mod resizeable_texture;

use enum_iterator::Sequence;
use glam::{Mat4, Vec2, Vec3, Vec4, vec3, vec4};
use shin_primitives::color::FloatColor4;
use shin_render_shader_types::{
    buffer::VertexSource,
    texture::TextureSource,
    vertices::{
        BlendVertex, GlassVertex, MaskVertex, PosColTexVertex, PosColVertex, PosTexVertex,
        PosVertex, TextVertex, WindowVertex,
    },
};
pub use shin_render_shaders as shaders;
use shin_render_shaders::ShaderName;

pub const TEXTURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
pub const DEPTH_STENCIL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth24PlusStencil8;

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum PassKind {
    Opaque,
    Transparent,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash, Sequence)]
#[repr(u32)]
pub enum LayerShaderOutputKind {
    Layer,
    LayerPremultiply,
    LayerDiscard,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash, Sequence)]
#[repr(u32)]
pub enum LayerFragmentShader {
    Default = 0,
    Mono = 1,
    Fill = 2,
    Fill2 = 3,
    Negative = 4,
    Gamma = 5,
}

impl LayerFragmentShader {
    pub fn is_equivalent_to_default(self, param: Vec4) -> bool {
        match self {
            LayerFragmentShader::Default => true,
            LayerFragmentShader::Mono => param == vec4(1.0, 1.0, 1.0, 0.0),
            LayerFragmentShader::Fill => param.w == 0.0,
            LayerFragmentShader::Fill2 => param.truncate() == Vec3::ZERO,
            LayerFragmentShader::Negative => false,
            LayerFragmentShader::Gamma => param.truncate() == Vec3::ONE,
        }
    }

    /// If the shader is equivalent to the default shader with the given parameters, downgrades to the default shader.
    pub fn simplify(self, param: Vec4) -> Self {
        if self.is_equivalent_to_default(param) {
            LayerFragmentShader::Default
        } else {
            self
        }
    }

    pub fn evaluate(self, color: FloatColor4, param: Vec4) -> FloatColor4 {
        let color = color.into_vec4();

        let color = match self {
            LayerFragmentShader::Default => color,
            LayerFragmentShader::Mono => {
                let luma = color.truncate().dot(vec3(0.299, 0.587, 0.114));

                Vec3::splat(luma).extend(color.w) * param.truncate().extend(1.0)
            }
            LayerFragmentShader::Fill => {
                // Native736bc/56810 execute difference, multiply and add for
                // all four channels, with alpha weight0. Retain that arithmetic
                // for signed zero and overflowing alpha differences too.
                color + (param - color) * Vec3::splat(param.w).extend(0.0)
            }
            LayerFragmentShader::Fill2 => {
                // Native736e4 interpolates toward the scalar param.w with
                // independent RGB weights, unlike the textured GPU operation.
                color + (Vec4::splat(param.w) - color) * param.truncate().extend(0.0)
            }
            LayerFragmentShader::Negative => {
                // Native73720 inverts RGB, then multiplies by param.xyz.
                // CPU drawable colors preserve alpha and do not use param.w.
                ((Vec3::ONE - color.truncate()) * param.truncate()).extend(color.w)
            }
            LayerFragmentShader::Gamma => {
                // Native73640 dispatches Gamma directly to the epilogue736ac,
                // preserving unspecified incoming s0..s3. It skips the color
                // loads736a4/736a8, so this is not a CPU gamma formula.
                // See reverse-engineering/native-cpu-color.c for the oracle.
                todo!()
            }
        };

        FloatColor4::from_vec4(color)
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash, Sequence)]
pub enum WiperKind {
    Default,
    Mask,
    Wave,
    Ripple,
    Whirl,
    Glass,
}

#[derive(Debug)]
pub enum RenderProgramWithArguments<'a> {
    // TODO: specify all arguments required for all programs
    Clear {
        vertices: VertexSource<'a, PosVertex>,
        // implementation note:
        // a lot of the params passed by-value here would go into a Params struct and bound as a uniform buffer for the shaders to use.
        // us accepting these params by-value instead limits the potential performance of the renderer (since with our current design we will have to create a new buffer every time we want to change the color).
        // but allowing to pass direct GPU buffers here would be a detriment for usability and won't lead to better performance, since most of these parameters are computed on the fly either way.
        // this is unlike vertex and index buffers, which can be re-used between frames for some components (and are accepting GPU-side buffers instead).
        color: FloatColor4,
    },
    Fill {
        vertices: VertexSource<'a, PosColVertex>,
        transform: Mat4,
    },
    Sprite {
        vertices: VertexSource<'a, PosColTexVertex>,
        sprite: TextureSource<'a>,
        transform: Mat4,
    },
    Font {
        vertices: VertexSource<'a, TextVertex>,
        glyph: TextureSource<'a>,
        transform: Mat4,
        color1: FloatColor4,
        color2: FloatColor4,
    },
    FontSdf {
        vertices: VertexSource<'a, TextVertex>,
        glyph: TextureSource<'a>,
        transform: Mat4,
        color1: FloatColor4,
        color2: FloatColor4,
        thresholds: Vec2,
    },
    FontBorder {
        vertices: VertexSource<'a, TextVertex>,
        glyph: TextureSource<'a>,
        transform: Mat4,
        distances: [Vec2; 8],
        color: FloatColor4,
    },
    Button {
        vertices: VertexSource<'a, PosColTexVertex>,
        texture: TextureSource<'a>,
        transform: Mat4,
        // IDK what is the difference between those, seems like they do the same thing repeatedly?
        // higurashi and dc4 only have one of those
        flash1: Vec4,
        flash2: Vec4,
    },
    Blend {
        vertices: VertexSource<'a, BlendVertex>,
        texture1: TextureSource<'a>,
        texture2: TextureSource<'a>,
        transform: Mat4,
        blend: Vec4,
        flash: Vec4,
    },
    /// Later-engine program8: independently sampled border, mask and backdrop.
    Window {
        vertices: VertexSource<'a, WindowVertex>,
        border: TextureSource<'a>,
        mask: TextureSource<'a>,
        background: TextureSource<'a>,
        transform: Mat4,
    },

    Layer {
        output_kind: LayerShaderOutputKind,
        fragment_shader: LayerFragmentShader,
        vertices: VertexSource<'a, PosTexVertex>,
        texture: TextureSource<'a>,
        transform: Mat4,
        color_multiplier: FloatColor4,
        fragment_shader_param: Vec4,
    },
    HigurashiPostprocess {
        vertices: VertexSource<'a, PosTexVertex>,
        texture: TextureSource<'a>,
        transform: Mat4,
        color: FloatColor4,
        operation: LayerFragmentShader,
        param: Vec4,
    },
    HigurashiOverlay {
        vertices: VertexSource<'a, MaskVertex>,
        texture: TextureSource<'a>,
        mask: TextureSource<'a>,
        transform: Mat4,
        minmax: Vec2,
    },
    Mask {
        fragment_shader: LayerFragmentShader,
        vertices: VertexSource<'a, MaskVertex>,
        texture: TextureSource<'a>,
        mask: TextureSource<'a>,
        transform: Mat4,
        color_multiplier: FloatColor4,
        fragment_shader_param: Vec4,
        minmax: Vec2,
    },

    Mosaic {
        vertices: VertexSource<'a, PosTexVertex>,
        texture: TextureSource<'a>,
        transform: Mat4,
        step: Vec4,
    },
    Dissolve {
        vertices: VertexSource<'a, MaskVertex>,
        texture: TextureSource<'a>,
        breakup: TextureSource<'a>,
        transform: Mat4,
    },
    TapEffect {},
    CharacterPortrait {
        vertices: VertexSource<'a, PosTexVertex>,
        portrait: TextureSource<'a>,
        frame: TextureSource<'a>,
        transform: Mat4,
        mode: u8,
    },
    Movie {
        vertices: VertexSource<'a, PosTexVertex>,
        texture_luma: TextureSource<'a>,
        texture_chroma: TextureSource<'a>,
        transform: Mat4,
        color_bias: Vec4,
        color_transform: [Vec4; 3],
        color_multiplier: Vec4,
        alpha_mode: bool,
        texture_size: Vec4,
    },
    MovieAlpha {},

    WiperDefault {
        vertices: VertexSource<'a, PosTexVertex>,
        texture_source: TextureSource<'a>,
        texture_target: TextureSource<'a>,
        transform: Mat4,
        alpha: f32,
    },
    WiperHigurashiDefault {
        vertices: VertexSource<'a, PosTexVertex>,
        texture_source: TextureSource<'a>,
        texture_target: TextureSource<'a>,
        transform: Mat4,
        alpha: f32,
    },
    WiperMask {
        vertices: VertexSource<'a, MaskVertex>,
        texture_source: TextureSource<'a>,
        texture_target: TextureSource<'a>,
        texture_mask: TextureSource<'a>,
        transform: Mat4,
        minmax: Vec2,
    },
    WiperHigurashiMask {
        vertices: VertexSource<'a, MaskVertex>,
        texture_source: TextureSource<'a>,
        texture_target: TextureSource<'a>,
        texture_mask: TextureSource<'a>,
        transform: Mat4,
        minmax: Vec2,
    },
    ScreenCurtain {
        vertices: VertexSource<'a, MaskVertex>,
        texture: TextureSource<'a>,
        mask: TextureSource<'a>,
        transform: Mat4,
        minmax: Vec2,
    },
    WiperWave {
        vertices: VertexSource<'a, PosTexVertex>,
        texture_source: TextureSource<'a>,
        texture_target: TextureSource<'a>,
        transform: Mat4,
        center_scale: Vec4,
        front_width: Vec4,
    },
    WiperRipple {
        vertices: VertexSource<'a, PosTexVertex>,
        texture_source: TextureSource<'a>,
        texture_target: TextureSource<'a>,
        transform: Mat4,
        center_scale: Vec4,
        wave_params: Vec4,
    },
    WiperWhirl {
        vertices: VertexSource<'a, PosTexVertex>,
        texture_source: TextureSource<'a>,
        texture_target: TextureSource<'a>,
        transform: Mat4,
        effect: Vec4,
    },
    WiperGlass {
        vertices: VertexSource<'a, GlassVertex>,
        texture: TextureSource<'a>,
        transform: Mat4,
        model: Mat4,
        effect: Vec4,
    },

    HigurashiRaster {
        vertices: VertexSource<'a, PosTexVertex>,
        texture: TextureSource<'a>,
        transform: Mat4,
        color: Vec4,
        amplitude_frequency: Vec4,
        phase: Vec4,
    },
    Gerokasu2Blur {
        vertices: VertexSource<'a, PosTexVertex>,
        texture: TextureSource<'a>,
        transform: Mat4,
        distances: [Vec4; 2],
        weights: Vec4,
    },
    HigurashiBlur {
        vertices: VertexSource<'a, PosTexVertex>,
        texture: TextureSource<'a>,
        transform: Mat4,
        distances: [Vec4; 2],
        color: Vec4,
        weights: Vec4,
    },
    Blur {
        vertices: VertexSource<'a, PosTexVertex>,
        texture: TextureSource<'a>,
        transform: Mat4,
        step: Vec4,
    },
    ZoomBlur {
        vertices: VertexSource<'a, PosTexVertex>,
        texture: TextureSource<'a>,
        transform: Mat4,
        center: Vec4,
        scales: [Vec4; 2],
    },
    Raster {
        vertices: VertexSource<'a, PosTexVertex>,
        texture: TextureSource<'a>,
        transform: Mat4,
        amplitude_frequency: Vec4,
        phase: Vec4,
    },
    TapRipple {
        vertices: VertexSource<'a, PosTexVertex>,
        texture: TextureSource<'a>,
        transform: Mat4,
        center_scale: Vec4,
        wave_params: Vec4,
    },
    Ripple {
        vertices: VertexSource<'a, PosTexVertex>,
        texture: TextureSource<'a>,
        transform: Mat4,
        wave: Vec4,
    },
    Breakup {},

    Charicon0 {},
    Charicon1 {},
    Charicon2 {},
    Charicon3 {},
    Test {},
}

impl RenderProgramWithArguments<'_> {
    pub fn get_shader_name(&self) -> ShaderName {
        match *self {
            RenderProgramWithArguments::Clear { .. } => ShaderName::Clear,
            RenderProgramWithArguments::Fill { .. } => ShaderName::Fill,
            RenderProgramWithArguments::Sprite { .. } => ShaderName::Sprite,
            RenderProgramWithArguments::Button { .. } => ShaderName::Button,
            RenderProgramWithArguments::Blend { .. } => ShaderName::Blend,
            RenderProgramWithArguments::Window { .. } => ShaderName::Window,
            RenderProgramWithArguments::Font { .. } => ShaderName::Font,
            RenderProgramWithArguments::FontSdf { .. } => ShaderName::FontSdf,
            RenderProgramWithArguments::FontBorder { .. } => ShaderName::FontBorder,
            RenderProgramWithArguments::Layer { .. } => ShaderName::Layer,
            RenderProgramWithArguments::Mask { .. } => ShaderName::Mask,
            RenderProgramWithArguments::HigurashiOverlay { .. } => ShaderName::HigurashiOverlay,
            RenderProgramWithArguments::HigurashiPostprocess { .. } => {
                ShaderName::HigurashiPostprocess
            }
            RenderProgramWithArguments::Dissolve { .. } => ShaderName::Dissolve,
            RenderProgramWithArguments::Mosaic { .. } => ShaderName::Mosaic,
            RenderProgramWithArguments::Raster { .. } => ShaderName::Raster,
            RenderProgramWithArguments::Ripple { .. } => ShaderName::Ripple,
            RenderProgramWithArguments::TapRipple { .. } => ShaderName::TapRipple,
            RenderProgramWithArguments::HigurashiBlur { .. } => ShaderName::HigurashiBlur,
            RenderProgramWithArguments::Gerokasu2Blur { .. } => ShaderName::Gerokasu2Blur,
            RenderProgramWithArguments::HigurashiRaster { .. } => ShaderName::HigurashiRaster,
            RenderProgramWithArguments::Blur { .. } => ShaderName::Blur,
            RenderProgramWithArguments::ZoomBlur { .. } => ShaderName::ZoomBlur,
            RenderProgramWithArguments::Movie { .. } => ShaderName::Movie,
            RenderProgramWithArguments::WiperDefault { .. } => ShaderName::WiperDefault,
            RenderProgramWithArguments::WiperHigurashiDefault { .. } => ShaderName::WiperDefault,
            RenderProgramWithArguments::WiperWhirl { .. } => ShaderName::WiperWhirl,
            RenderProgramWithArguments::WiperWave { .. } => ShaderName::WiperWave,
            RenderProgramWithArguments::WiperRipple { .. } => ShaderName::WiperRipple,
            RenderProgramWithArguments::CharacterPortrait { .. } => ShaderName::CharacterPortrait,
            RenderProgramWithArguments::WiperGlass { .. } => ShaderName::WiperGlass,
            RenderProgramWithArguments::WiperMask { .. }
            | RenderProgramWithArguments::WiperHigurashiMask { .. } => ShaderName::WiperMask,
            RenderProgramWithArguments::ScreenCurtain { .. } => ShaderName::ScreenCurtain,

            ref program => todo!("Implement shader for {:?}", program),
        }
    }
}

#[derive(Debug, Default, Copy, Clone, Eq, PartialEq, Hash, Sequence)]
pub enum DepthFunction {
    Never,
    Less,
    Equal,
    LessOrEqual,
    Greater,
    NotEqual,
    GreaterOrEqual,
    #[default]
    Always,
}

#[derive(Debug, Default, Copy, Clone, Eq, PartialEq, Hash, Sequence)]
pub struct DepthState {
    pub function: DepthFunction,
    pub write_enable: bool,
}

#[derive(Debug, Default, Copy, Clone, Eq, PartialEq, Hash, Sequence)]
pub enum StencilFunction {
    Never,
    Less,
    Equal,
    LessOrEqual,
    Greater,
    NotEqual,
    GreaterOrEqual,
    #[default]
    Always,
}

#[derive(Debug, Default, Copy, Clone, Eq, PartialEq, Hash, Sequence)]
pub enum StencilOperation {
    #[default]
    Keep,
    Zero,
    Replace,
    Increment,
    Decrement,
    Invert,
    IncrementWrap,
    DecrementWrap,
}

#[derive(Default, Debug, Copy, Clone, Eq, PartialEq, Hash, Sequence)]
#[repr(u8)]
pub enum StencilMask {
    #[default]
    All = 0xff,
    SignOnly = 0x80,
    ClipOnly = 0x40,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash, Sequence)]
pub struct StencilPipelineState {
    pub function: StencilFunction,
    pub stencil_fail_operation: StencilOperation,
    pub depth_fail_operation: StencilOperation,
    pub pass_operation: StencilOperation,
    pub stencil_read_mask: StencilMask,
    pub stencil_write_mask: StencilMask,
}

impl Default for StencilPipelineState {
    fn default() -> Self {
        Self {
            function: StencilFunction::default(),
            stencil_fail_operation: StencilOperation::default(),
            depth_fail_operation: StencilOperation::default(),
            pass_operation: StencilOperation::default(),
            stencil_read_mask: StencilMask::All,
            stencil_write_mask: StencilMask::All,
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct StencilState {
    pub pipeline: StencilPipelineState,
    pub stencil_reference: u8,
}

impl Default for StencilState {
    fn default() -> Self {
        Self {
            pipeline: StencilPipelineState::default(),
            stencil_reference: 0,
        }
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash, Sequence)]
pub struct DepthStencilPipelineState {
    pub depth: DepthState,
    pub stencil: StencilPipelineState,
}

#[derive(Debug, Default, Copy, Clone)]
pub struct DepthStencilState {
    pub depth: DepthState,
    pub stencil: StencilState,
}

impl DepthStencilState {
    pub fn shorthand(stencil_ref: u8, allow_eq_stencil: bool, test_depth: bool) -> Self {
        let depth = if test_depth {
            DepthState {
                function: DepthFunction::Less,
                write_enable: false,
            }
        } else {
            DepthState::default()
        };
        let stencil = StencilState {
            pipeline: StencilPipelineState {
                function: if allow_eq_stencil {
                    StencilFunction::GreaterOrEqual
                } else {
                    StencilFunction::Greater
                },
                pass_operation: StencilOperation::Replace,
                ..Default::default()
            },
            stencil_reference: stencil_ref,
        };

        Self { depth, stencil }
    }

    pub fn into_pipeline_parts(self) -> (DepthStencilPipelineState, u8) {
        (
            DepthStencilPipelineState {
                depth: self.depth,
                stencil: self.stencil.pipeline,
            },
            self.stencil.stencil_reference,
        )
    }
}

#[derive(Debug, Default, Copy, Clone, Eq, PartialEq, Hash, Sequence)]
pub enum ColorBlendType {
    NoColor,
    #[default]
    Opaque,
    // I don't understand the meaning of each of these (yet)
    Layer1,
    Layer2,
    Layer3,
    // those do the same thing as above, but operate on premultiplied alpha (QUESTION: is it on the input or output or both?)
    LayerPremultiplied1,
    LayerPremultiplied2,
    LayerPremultiplied3,
    /// Updated Higurashi a8630: straight alpha with reverse subtraction.
    HigurashiLayer3,
}

impl ColorBlendType {
    pub fn from_higurashi_layer(layer: LayerBlendType) -> Self {
        match layer {
            LayerBlendType::Type1 => Self::Layer1,
            LayerBlendType::Type2 => Self::Layer2,
            LayerBlendType::Type3 => Self::HigurashiLayer3,
        }
    }

    pub fn from_regular_layer(layer: LayerBlendType) -> Self {
        match layer {
            LayerBlendType::Type1 => ColorBlendType::Layer1,
            LayerBlendType::Type2 => ColorBlendType::Layer2,
            LayerBlendType::Type3 => ColorBlendType::Layer3,
        }
    }

    pub fn from_premultiplied_layer(layer: LayerBlendType) -> Self {
        match layer {
            LayerBlendType::Type1 => ColorBlendType::LayerPremultiplied1,
            LayerBlendType::Type2 => ColorBlendType::LayerPremultiplied2,
            LayerBlendType::Type3 => ColorBlendType::LayerPremultiplied3,
        }
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum LayerBlendType {
    Type1,
    Type2,
    Type3,
}

#[derive(Debug, Default, Copy, Clone, Eq, PartialEq, Hash, Sequence)]
pub enum CullFace {
    #[default]
    None,
    Back,
    Front,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash, Sequence)]
pub enum DrawPrimitive {
    Triangles,
    TrianglesStrip,
}

#[derive(Default, Copy, Clone)]
#[must_use]
pub struct RenderRequestBuilder {
    depth_stencil: DepthStencilState,
    color_blend_type: ColorBlendType,
    cull_faces: CullFace,
}

impl RenderRequestBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn depth_stencil(mut self, depth_stencil: DepthStencilState) -> Self {
        self.depth_stencil = depth_stencil;
        self
    }

    pub fn depth_stencil_shorthand(
        mut self,
        stencil_ref: u8,
        allow_eq_stencil: bool,
        test_depth: bool,
    ) -> Self {
        self.depth_stencil =
            DepthStencilState::shorthand(stencil_ref, allow_eq_stencil, test_depth);
        self
    }

    pub fn cull_faces(mut self, cull_faces: CullFace) -> Self {
        self.cull_faces = cull_faces;
        self
    }

    pub fn color_blend_type(mut self, color_blend_type: ColorBlendType) -> Self {
        self.color_blend_type = color_blend_type;
        self
    }

    pub fn layer_color_blend(mut self, color_blend_type: LayerBlendType) -> Self {
        self.color_blend_type = ColorBlendType::from_regular_layer(color_blend_type);
        self
    }

    pub fn layer_color_blend_premultiplied(mut self, color_blend_type: LayerBlendType) -> Self {
        self.color_blend_type = ColorBlendType::from_premultiplied_layer(color_blend_type);
        self
    }

    pub fn build(
        self,
        program: RenderProgramWithArguments,
        primitive: DrawPrimitive,
    ) -> RenderRequest {
        RenderRequest {
            depth_stencil: self.depth_stencil,
            color_blend_type: self.color_blend_type,
            cull_faces: self.cull_faces,
            primitive,
            program,
        }
    }
}

#[must_use]
pub struct RenderRequest<'a> {
    depth_stencil: DepthStencilState,
    color_blend_type: ColorBlendType,
    cull_faces: CullFace,
    primitive: DrawPrimitive,
    program: RenderProgramWithArguments<'a>,
}

pub fn shin_orthographic_projection_matrix(
    left: f32,
    right: f32,
    bottom: f32,
    top: f32,
    near: f32,
    far: f32,
) -> Mat4 {
    // The caller supplies top < bottom to map the game's downward Y axis to
    // clip space. Flipping Y here as well would invert every rendered layer.
    // Only map Z from OpenGL's [-1; 1] to wgpu's [0; 1].
    #[rustfmt::skip]
    const SHIN_TO_WGPU_MATRIX: Mat4 = Mat4::from_cols_array(&[
        1.0, 0.0, 0.0, 0.0,
        0.0, 1.0, 0.0, 0.0,
        0.0, 0.0, 0.5, 0.0,
        0.0, 0.0, 0.5, 1.0,
    ]);

    SHIN_TO_WGPU_MATRIX
        * glam::camera::rh::proj::opengl::orthographic(left, right, bottom, top, near, far)
}

#[cfg(test)]
mod projection_tests {
    use super::*;

    #[test]
    fn downward_y_maps_top_to_top_of_wgpu_viewport() {
        let projection = shin_orthographic_projection_matrix(0.0, 1920.0, 1080.0, 0.0, -1.0, 1.0);
        assert_eq!(
            projection.transform_point3(vec3(0.0, 0.0, 0.0)),
            vec3(-1.0, 1.0, 0.5)
        );
        assert_eq!(
            projection.transform_point3(vec3(1920.0, 1080.0, 0.0)),
            vec3(1.0, -1.0, 0.5)
        );
    }
}

// /// A trait for elements that can be rendered
// ///
// /// Most elements will be containers, containing other elements to render.
// pub trait Renderable {
//     /// Renders an element on the screen
//     ///
//     /// # Arguments
//     ///
//     /// * `resources` - The common resources used by the renderer
//     /// * `render_pass` - The render pass to encode commands to
//     /// * `transform` - The transform matrix to apply to the element
//     /// * `projection` - The projection matrix to apply to the element
//     ///
//     /// # Notes
//     ///
//     /// The `projection` matrix is used to convert from virtual screen space to real screen space.
//     /// The `transform` matrix represents inherited transformations from parent elements.
//     ///
//     /// This distinction is needed to allow for rendering using intermediate render targets.
//     /// In this case the `transform` matrix is preserved and passed into inner elements.
//     /// The `projection` matrix is used only to render the intermediate render target to the screen.
//     fn render<'enc>(
//         &'enc self,
//         resources: &'enc GpuCommonResources,
//         render_pass: &mut wgpu::RenderPass<'enc>,
//         transform: Mat4,
//         projection: Mat4,
//     );
//
//     /// Notifies of window resize
//     ///
//     /// If a renderable element has an intermediate render target, it should resize it here.
//     fn resize(&mut self, resources: &GpuCommonResources);
// }

#[cfg(test)]
mod native_cpu_fragment_tests {
    use super::*;
    #[test]
    fn negative_preserves_alpha_uses_rgb_gain_and_ignores_w() {
        let color = FloatColor4::from_vec4(vec4(0.25, 0.5, 0.75, 0.125));
        for w in [0.0, 0.5, 1.0, -3.0] {
            let actual = LayerFragmentShader::Negative
                .evaluate(color, vec4(0.5, 0.25, 0.75, w))
                .into_vec4();
            assert_eq!(actual, vec4(0.375, 0.125, 0.1875, 0.125));
        }
    }
    #[test]
    fn fill_preserves_alpha_and_extrapolates_without_clamping() {
        let color = FloatColor4::from_vec4(vec4(0.25, 0.5, 0.75, 0.125));
        assert_eq!(
            LayerFragmentShader::Fill
                .evaluate(color, vec4(0.75, 0.25, 0.5, 0.5))
                .into_vec4(),
            vec4(0.5, 0.375, 0.625, 0.125)
        );
        assert_eq!(
            LayerFragmentShader::Fill
                .evaluate(color, vec4(0.75, 0.25, 0.5, 2.0))
                .into_vec4(),
            vec4(1.25, 0.0, 0.25, 0.125)
        );
    }
    #[test]
    fn fill2_uses_independent_rgb_weights_toward_scalar_w() {
        let color = FloatColor4::from_vec4(vec4(0.25, 0.5, 0.75, 0.125));
        assert_eq!(
            LayerFragmentShader::Fill2
                .evaluate(color, vec4(0.5, 0.25, 0.75, 0.125))
                .into_vec4(),
            vec4(0.1875, 0.40625, 0.28125, 0.125)
        );
    }
    #[test]
    fn fill_alpha_retains_native_zero_weight_arithmetic() {
        for operation in [LayerFragmentShader::Fill, LayerFragmentShader::Fill2] {
            let color = FloatColor4::from_vec4(vec4(0.25, 0.5, 0.75, -0.0));
            // Original56810 returns positive zero for this alpha channel.
            assert_eq!(
                operation
                    .evaluate(color, vec4(0.5, 0.25, 0.75, 0.0))
                    .into_vec4()
                    .w
                    .to_bits(),
                0
            );
            let color = FloatColor4::from_vec4(vec4(0.25, 0.5, 0.75, f32::MAX));
            // Finite opposite alpha endpoints overflow their difference;
            // original FMUL(infinity,0) then FADD yields NaN.
            assert!(
                operation
                    .evaluate(color, vec4(0.5, 0.25, 0.75, -f32::MAX))
                    .into_vec4()
                    .w
                    .is_nan()
            );
        }
    }
    #[test]
    #[ignore = "requires supplied executable and compiled native-cpu-color.c oracle"]
    fn reference_native_cpu_color_bit_patterns() {
        use std::io::{Read, Write};
        use std::process::{Command, Stdio};
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let executable = std::path::Path::new(&rom)
            .parent()
            .unwrap()
            .join("uncompressed_executable");
        let oracle = std::env::var("SHIN_REFERENCE_NATIVE_CPU_COLOR").unwrap();
        let mut child = Command::new(oracle)
            .arg(executable)
            .arg("--stream")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut input = child.stdin.take().unwrap();
        let mut output = child.stdout.take().unwrap();
        input.write_all(b"NCL1").unwrap();
        let operations = [
            LayerFragmentShader::Default,
            LayerFragmentShader::Mono,
            LayerFragmentShader::Fill,
            LayerFragmentShader::Fill2,
            LayerFragmentShader::Negative,
        ];
        let mut compared = 0;
        let mut compare = |operation: usize, color: [f32; 4], param: [f32; 4]| {
            input.write_all(&(operation as u32).to_le_bytes()).unwrap();
            for value in color.into_iter().chain(param) {
                input.write_all(&value.to_bits().to_le_bytes()).unwrap();
            }
            input.flush().unwrap();
            let mut bytes = [0u8; 16];
            output.read_exact(&mut bytes).unwrap();
            let native: [u32; 4] = std::array::from_fn(|c| {
                u32::from_le_bytes(bytes[c * 4..c * 4 + 4].try_into().unwrap())
            });
            let actual = operations[operation]
                .evaluate(
                    FloatColor4::from_vec4(Vec4::from_array(color)),
                    Vec4::from_array(param),
                )
                .into_array()
                .map(f32::to_bits);
            assert_eq!(
                actual,
                native,
                "Native CPU {:?}, colorbits={:?}, parambits={:?}",
                operations[operation],
                color.map(f32::to_bits),
                param.map(f32::to_bits)
            );
            compared += 1;
        };
        // Native Fill/Fill2 still execute alpha's difference, zero-weight
        // multiply and add; bypassing these loses signed-zero/overflow results.
        for alpha in [
            0.0,
            -0.0,
            0.125,
            -0.125,
            f32::MAX,
            -f32::MAX,
            f32::INFINITY,
            f32::NEG_INFINITY,
        ] {
            for weight in [0.0, -0.0, 0.125, -0.125, 1.0, -1.0, f32::MAX, -f32::MAX] {
                for operation in 0..5 {
                    compare(
                        operation,
                        [0.25, 0.5, 0.75, alpha],
                        [0.5, 0.25, 0.75, weight],
                    );
                }
            }
        }
        let mut state = 0x654b9c17u32;
        for _ in 0..4096 {
            let mut value = || {
                state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                // Finite signed values spanning binary exponents -20..20.
                f32::from_bits((state & 0x807fffff) | ((107 + ((state >> 23) % 41)) << 23))
            };
            let color = std::array::from_fn(|_| value());
            let param = std::array::from_fn(|_| value());
            for operation in 0..5 {
                compare(operation, color, param);
            }
        }
        drop(compare);
        drop(input);
        assert!(child.wait().unwrap().success());
        eprintln!(
            "Native CPU color: {compared} exact RGBA bit-pattern comparisons across Default/Mono/Fill/Fill2/Negative; original pure AArch64 evaluator, host FP environment; Gamma excluded"
        );
    }
    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
    fn reference_native_font_fused_rgba_and_outline_coverage() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let executable = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let relocations: std::collections::HashMap<_, _> = (0x11e040..0x152eb8)
            .step_by(24)
            .map(|offset| {
                (
                    u64::from_le_bytes(executable[offset + 256..offset + 264].try_into().unwrap()),
                    u64::from_le_bytes(executable[offset + 272..offset + 280].try_into().unwrap()),
                )
            })
            .collect();
        for (program, blob, bytes, expected) in [
            (3, 0x1edd70, 0xf0, 0xdadf36bfe6e02071u64),
            (4, 0x1ec1f0, 0x1b8, 0x420c9943b7d09a62),
        ] {
            assert_eq!(
                relocations.get(&(0x1dc1c0 + program * 48 + 8)),
                Some(&(blob as u64))
            );
            let code = &executable[blob + 0xb40 + 256..blob + 0xb40 + 256 + bytes];
            let fingerprint = code.iter().fold(0xcbf29ce484222325u64, |hash, byte| {
                (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
            });
            assert_eq!(fingerprint, expected, "Native font program{program}");
        }
        // Font rounds endpoint differences, fuses four channel interpolations,
        // then multiplies glyph coverage into alpha only. Outline takes the
        // maximum of eight taps, multiplies1.5, clamps1, then applies tint alpha.
        for (address, expected) in [
            (0x1ee938, 0x4c59101000370404u64),
            (0x1ee940, 0x4c59101000170201u64),
            (0x1ee948, 0x4c59101000270602u64),
            (0x1ee958, 0x4c59101000070000u64),
            (0x1ee960, 0x51a0038c00370404u64),
            (0x1ee968, 0x51a0038c00070000u64),
            (0x1ee978, 0x51a0038c00170101u64),
            (0x1ee980, 0x51a0038c00270202u64),
            (0x1ee988, 0x5c68100000470303u64),
            (0x1ece60, 0x5c60178000e7050eu64),
            (0x1ece68, 0x5c60178000970b0bu64),
            (0x1ece78, 0x5c60178000470303u64),
            (0x1ece80, 0x5c60178000e7030eu64),
            (0x1ece88, 0x5c60178000170005u64),
            (0x1ecea8, 0x5c60178000b70505u64),
            (0x1eceb8, 0x5c60178000e70505u64),
            (0x1ecec0, 0x3868103fc0070503u64),
            (0x1ecec8, 0x386013bf80070303u64),
            (0x1eced8, 0x4c68100c00370303u64),
        ] {
            assert_eq!(
                u64::from_le_bytes(executable[address + 256..address + 264].try_into().unwrap()),
                expected
            );
        }
    }
    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
    fn reference_native_textured_gamma_reciprocal_before_log_product() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let executable = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let relocations: std::collections::HashMap<_, _> = (0x11e040..0x152eb8)
            .step_by(24)
            .map(|offset| {
                (
                    u64::from_le_bytes(executable[offset + 256..offset + 264].try_into().unwrap()),
                    u64::from_le_bytes(executable[offset + 272..offset + 280].try_into().unwrap()),
                )
            })
            .collect();
        // Complete reviewed ordinary/discard/premultiplied/masked fragments.
        for (table, program, blob, bytes, expected) in [
            (0x16fce8, 13, 0x1f7a70, 0x130, 0x9705ec79fd52f6aeu64),
            (0x16fd18, 19, 0x1f6cb0, 0x140, 0x10c59c4455550e1e),
            (0x16fd00, 25, 0x1f8730, 0x150, 0x03cb126d0b553253),
            (0x16fd30, 31, 0x201770, 0x198, 0xe5ec056a02b3b59c),
        ] {
            assert_eq!(
                u32::from_le_bytes(
                    executable[table + 20 + 256..table + 24 + 256]
                        .try_into()
                        .unwrap()
                ),
                program
            );
            assert_eq!(
                relocations.get(&(0x1dc1c0 + u64::from(program) * 48 + 8)),
                Some(&(blob as u64))
            );
            let code = &executable[blob + 0xb40 + 256..blob + 0xb40 + 256 + bytes];
            let fingerprint = code.iter().fold(0xcbf29ce484222325u64, |hash, byte| {
                (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
            });
            assert_eq!(fingerprint, expected, "Native Gamma program {program}");
        }
        // c4 RGB loads, RCPs, tint multiplies, LG2, product with reciprocal,
        // range-reduced EX2, and the independent tinted alpha channel.
        for (offset, expected) in [
            (0x08, 0x4c9807900007000au64),
            (0x48, 0x4c9807900017000b),
            (0x50, 0x5080000000470a07),
            (0x58, 0x4c98079000270003),
            (0x68, 0x5080000000470b08),
            (0x70, 0x5080000000470303),
            (0x78, 0x4c68100c00070006),
            (0x90, 0x5080000000370606),
            (0xa8, 0x5080000000370202),
            (0xb0, 0x5080000000370404),
            (0xb8, 0x5c68100000770600),
            (0xc8, 0x5c68100000870201),
            (0xd0, 0x5c68100000370407),
            (0xd8, 0x4c68100c00370503),
            (0xe8, 0x5c90008000070000),
            (0xf8, 0x5080000000270000),
            (0x110, 0x5080000000270101),
            (0x118, 0x5080000000270602),
        ] {
            let address = 0x1f85b0 + offset + 256;
            assert_eq!(
                u64::from_le_bytes(executable[address..address + 8].try_into().unwrap()),
                expected
            );
        }
    }
    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
    fn reference_native_textured_mono_fill_fused_arithmetic_all_modes() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let executable = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let relocations: std::collections::HashMap<_, _> = (0x11e040..0x152eb8)
            .step_by(24)
            .map(|offset| {
                (
                    u64::from_le_bytes(executable[offset + 256..offset + 264].try_into().unwrap()),
                    u64::from_le_bytes(executable[offset + 272..offset + 280].try_into().unwrap()),
                )
            })
            .collect();
        for (table, operation, program, blob, bytes, expected) in [
            (0x16fce8, 1, 11, 0x1fa1b0, 0xf0, 0x65693f610ddd543fu64),
            (0x16fd18, 1, 17, 0x1f93f0, 0x100, 0x631d59d5ec45ba4d),
            (0x16fd00, 1, 23, 0x1faf70, 0x110, 0x21f127245d4dc31a),
            (0x16fd30, 1, 29, 0x202530, 0x158, 0xb937a2ffb8fbfc7c),
            (0x16fce8, 2, 9, 0x1f5330, 0xe0, 0xc90ffd765fd2e2d3),
            (0x16fd18, 2, 15, 0x1f4570, 0xe0, 0x4d6463d541cb2cb5),
            (0x16fd00, 2, 21, 0x1f5ff0, 0xf0, 0xfe49ed20dfe9238e),
            (0x16fd30, 2, 27, 0x200ab0, 0x138, 0x22a69f085d70976c),
        ] {
            let selector = table + operation * 4 + 256;
            assert_eq!(
                u32::from_le_bytes(executable[selector..selector + 4].try_into().unwrap()),
                program
            );
            assert_eq!(
                relocations.get(&(0x1dc1c0 + u64::from(program) * 48 + 8)),
                Some(&(blob as u64))
            );
            let code = &executable[blob + 0xb40 + 256..blob + 0xb40 + 256 + bytes];
            let fingerprint = code.iter().fold(0xcbf29ce484222325u64, |hash, byte| {
                (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
            });
            assert_eq!(
                fingerprint, expected,
                "Native operation{operation} program{program}"
            );
        }
        // c1 constants follow shader code. Discard adds its alpha cutoff at
        // c1[0], shifting the green and blue luma constants by one word.
        for address in [0x1fae70, 0x1fa0b4, 0x1fbc30, 0x2031f0] {
            assert_eq!(
                u32::from_le_bytes(executable[address + 256..address + 260].try_into().unwrap()),
                0x3f1645a2
            );
            assert_eq!(
                u32::from_le_bytes(executable[address + 260..address + 264].try_into().unwrap()),
                0x3de978d5
            );
        }
        // Mono: rounded red multiply, green/blue FFMA, original sample*tint
        // fused subtraction, fused interpolation, then RGB gains.
        // Fill: fused target-sample*tint delta and fused interpolation.
        for (address, expected) in [
            (0x1facf0 + 0x68, 0x1e23e99168770407u64),
            (0x1facf0 + 0x70, 0x49a0038400070507),
            (0x1facf0 + 0x78, 0x49a0038400170607),
            (0x1facf0 + 0x88, 0x49a1038c00070009),
            (0x1facf0 + 0xa8, 0x49a0021000370900),
            (0x1facf0 + 0xc8, 0x4c68101000070000),
            (0x1f5e70 + 0x70, 0x51a1031000070006),
            (0x1f5e70 + 0x88, 0x51a1039000170107),
            (0x1f5e70 + 0x98, 0x51a1041000270208),
            (0x1f5e70 + 0xb8, 0x49a0001000370600),
            (0x1f5e70 + 0xc8, 0x49a0021000370701),
            (0x1f5e70 + 0xd0, 0x49a0029000370802),
            (0x203070 + 0x68, 0x4c59101400170404),
            (0x203070 + 0x70, 0x51a0029400070404),
        ] {
            assert_eq!(
                u64::from_le_bytes(executable[address + 256..address + 264].try_into().unwrap()),
                expected
            );
        }
    }
    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
    fn reference_native_textured_negative_rgb_gains_all_output_modes() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let executable = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let relocations: std::collections::HashMap<_, _> = (0x11e040..0x152eb8)
            .step_by(24)
            .map(|offset| {
                (
                    u64::from_le_bytes(executable[offset + 256..offset + 264].try_into().unwrap()),
                    u64::from_le_bytes(executable[offset + 272..offset + 280].try_into().unwrap()),
                )
            })
            .collect();
        for (table, program, blob, bytes, expected) in [
            (0x16fce8, 12, 0x1fcaf0, 0xf8, 0x0c2ca97617fbe33bu64),
            (0x16fd18, 18, 0x1fbd30, 0x110, 0x9cd473dc0aad2415),
            (0x16fd00, 24, 0x1fd7b0, 0x118, 0x18856a3e53fe0942),
            (0x16fd30, 30, 0x2032f0, 0x160, 0x8ca9164d63913eaf),
        ] {
            assert_eq!(
                u32::from_le_bytes(
                    executable[table + 16 + 256..table + 20 + 256]
                        .try_into()
                        .unwrap()
                ),
                program
            );
            assert_eq!(
                relocations.get(&(0x1dc1c0 + u64::from(program) * 48 + 8)),
                Some(&(blob as u64))
            );
            // Entire reviewed fragment through EXIT. All four variants apply
            // RGB c4 gain after FMUL/FFMA/FADD weighted inversion; alpha is
            // preserved, with the corresponding discard/premultiply/mask path.
            let code = &executable[blob + 0xb40 + 256..blob + 0xb40 + 256 + bytes];
            let fingerprint = code.iter().fold(0xcbf29ce484222325u64, |hash, byte| {
                (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
            });
            assert_eq!(fingerprint, expected, "Native Negative program {program}");
        }
        for (address, expected) in [
            (0x1fd630 + 0x70, 0x4c68101000370005u64),
            (0x1fd630 + 0x90, 0x51a1061000370505),
            (0x1fd630 + 0xb0, 0x5c58100000570000),
            (0x1fd630 + 0xd0, 0x4c68101000070000),
            (0x1fd630 + 0xd8, 0x4c68101000170101),
            (0x1fd630 + 0xe8, 0x4c68101000270202),
        ] {
            assert_eq!(
                u64::from_le_bytes(executable[address + 256..address + 264].try_into().unwrap()),
                expected
            );
        }
    }

    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
    fn reference_native_default_wiper_fused_rgba() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let executable = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let fragment_pointer = 0x1dc1c0 + 36 * 48 + 8;
        let relocation = (0x11e040..0x152eb8)
            .step_by(24)
            .find(|offset| {
                u64::from_le_bytes(executable[offset + 256..offset + 264].try_into().unwrap())
                    == fragment_pointer
            })
            .unwrap();
        assert_eq!(
            u64::from_le_bytes(
                executable[relocation + 272..relocation + 280]
                    .try_into()
                    .unwrap()
            ),
            0x212930
        );
        let code = &executable[0x213470 + 256..0x213528 + 256];
        let fingerprint = code.iter().fold(0xcbf29ce484222325u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        });
        assert_eq!(fingerprint, 0x0cf117ab247559ed);
        // All four outgoing channels use FFMA(source,-progress,source),
        // followed by FFMA(incoming,progress,outgoing); no clamp or RGB-only path.
        for (offset, expected) in [
            (0x58, 0x49a1030c00070606u64),
            (0x68, 0x49a1038c00070707),
            (0x70, 0x49a1020c00070404),
            (0x78, 0x49a1028c00070505),
            (0x88, 0x49a0030c00070000),
            (0x90, 0x49a0038c00070101),
            (0x98, 0x49a0020c00070202),
            (0xa8, 0x49a0028c00070303),
            (0xb0, 0xe30000000007000f),
        ] {
            assert_eq!(
                u64::from_le_bytes(code[offset..offset + 8].try_into().unwrap()),
                expected,
                "Native Default wipe instruction +{offset:x}"
            );
        }
    }

    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
    fn reference_native_button_fragment_arithmetic() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let executable = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let fragment_pointer = 0x1dc1c0 + 5 * 48 + 8;
        let relocation = (0x11e040..0x152eb8)
            .step_by(24)
            .find(|offset| {
                u64::from_le_bytes(executable[offset + 256..offset + 264].try_into().unwrap())
                    == fragment_pointer
            })
            .unwrap();
        assert_eq!(
            u64::from_le_bytes(
                executable[relocation + 272..relocation + 280]
                    .try_into()
                    .unwrap()
            ),
            0x1e1c30
        );
        let code = &executable[0x1e2770 + 256..0x1e2888 + 256];
        let fingerprint = code.iter().fold(0xcbf29ce484222325u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        });
        assert_eq!(fingerprint, 0xe6b31b992883ddcb);
        // Reviewed TEXS/FADD/FFMA/FMULT body, RGB flash1 then flash2;
        // alpha only samples and multiplies vertex alpha, before EXIT.
        for (offset, expected) in [
            (0x30, 0xd832008020170000u64),
            (0x68, 0x4c59100c00070007),
            (0x70, 0x4c59100c0017010a),
            (0x78, 0x4c59100c00270209),
            (0x88, 0x5c68100000870303),
            (0x90, 0x49a0000c00370707),
            (0x98, 0x49a0008c00370a0a),
            (0xa8, 0x49a0010c00370909),
            (0xb0, 0x4c59101000070700),
            (0xb8, 0x4c59101000170a01),
            (0xc8, 0x4c5910100027090b),
            (0xd0, 0x49a0039000370000),
            (0xd8, 0x49a0051000370102),
            (0xe8, 0x49a0049000370b07),
            (0xf0, 0x5c68100000670000),
            (0xf8, 0x5c68100000570201),
            (0x108, 0x5c68100000470702),
            (0x110, 0xe30000000007000f),
        ] {
            assert_eq!(
                u64::from_le_bytes(code[offset..offset + 8].try_into().unwrap()),
                expected,
                "Native Button instruction +{offset:x}"
            );
        }
    }

    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
    fn reference_native_blend_fragment_arithmetic() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let executable = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let fragment_pointer = 0x1dc1c0 + 6 * 48 + 8;
        let relocation = (0x11e040..0x152eb8)
            .step_by(24)
            .find(|offset| {
                u64::from_le_bytes(executable[offset + 256..offset + 264].try_into().unwrap())
                    == fragment_pointer
            })
            .unwrap();
        assert_eq!(
            u64::from_le_bytes(
                executable[relocation + 272..relocation + 280]
                    .try_into()
                    .unwrap()
            ),
            0x1dcbb0
        );
        // Complete reviewed Maxwell body through EXIT: independent xy/zw UV
        // samples, FADD/FFMA interpolation, RGB-only flash, then RGBA tint.
        let code = &executable[0x1dd6f0 + 256..0x1dd840 + 256];
        let fingerprint = code.iter().fold(0xcbf29ce484222325u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        });
        assert_eq!(fingerprint, 0x3a27bb7403a365b5);
        for (offset, expected) in [
            (0x48, 0xd832008040570406u64),
            (0x50, 0xd83200a020370100),
            (0x88, 0x5c5910000017070c),
            (0x90, 0x5c59100000070600),
            (0x98, 0x5c5910000027040d),
            (0xa8, 0x5c59100000370503),
            (0xb0, 0x49a0038c00170c02),
            (0xb8, 0x49a0030c00070000),
            (0xc8, 0x49a0020c00270d0d),
            (0xd0, 0x49a0028c00370305),
            (0x108, 0x49a0011000370402),
            (0x110, 0x49a0001000370100),
            (0x118, 0x49a0069000370606),
            (0xf8, 0x5c68100000b70503),
            (0x128, 0x5c68100000970201),
            (0x130, 0x5c68100000a70000),
            (0x138, 0x5c68100000870602),
            (0x148, 0xe30000000007000f),
        ] {
            assert_eq!(
                u64::from_le_bytes(code[offset..offset + 8].try_into().unwrap()),
                expected,
                "Native Blend instruction +{offset:x}"
            );
        }
    }

    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
    fn reference_native_texture_fill2_matches_default_in_all_output_modes() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let executable = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let relocations: std::collections::HashMap<_, _> = (0x11e040..0x152eb8)
            .step_by(24)
            .map(|offset| {
                (
                    u64::from_le_bytes(executable[offset + 256..offset + 264].try_into().unwrap()),
                    u64::from_le_bytes(executable[offset + 272..offset + 280].try_into().unwrap()),
                )
            })
            .collect();
        for (table, default, fill2, default_blob, fill2_blob) in [
            (0x16fce8, 8, 10, 0x1f04b0, 0x1f2bf0),
            (0x16fd18, 14, 16, 0x1ef6f0, 0x1f1e30),
            (0x16fd00, 20, 22, 0x1f1170, 0x1f38b0),
        ] {
            let word = |address: usize| {
                u32::from_le_bytes(executable[address + 256..address + 260].try_into().unwrap())
            };
            assert_eq!(word(table), default);
            assert_eq!(word(table + 3 * 4), fill2);
            for (program, blob) in [(default, default_blob), (fill2, fill2_blob)] {
                assert_eq!(
                    relocations.get(&(0x1dc1c0 + u64::from(program) * 48 + 8)),
                    Some(&(blob as u64))
                );
            }
            // Complete executable body through EXIT and its dead self-loop.
            // All three pairs contain sampling, RGBA tint and their respective
            // output handling; none applies the Fill2 parameter uniform.
            let code = |blob: usize| &executable[blob + 256 + 0xb40..blob + 256 + 0xb40 + 0x100];
            assert_eq!(code(default_blob), code(fill2_blob));
        }
    }

    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
    fn reference_native_tile_cpu_fragment_evaluation() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let executable = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        assert_eq!(
            &executable[0x16fcc4 + 256..0x16fcc9 + 256],
            &[0, 16, 26, 41, 12]
        );
        // Gamma's table byte12 selects7367c+12*4=736ac: the
        // epilogue, bypassing both FP loads in the default return path.
        assert_eq!(
            0x7367c + usize::from(executable[0x16fcc8 + 256]) * 4,
            0x736ac
        );
        for (address, expected) in [
            (0x106598, 0x97fdb42a),
            (0x736a4, 0x2d4407e0),
            (0x736a8, 0x2d450fe2),
            (0x736ac, 0xa9447bfd),
            (0x736b0, 0xa9434ff4),
            (0x736b4, 0x910143ff),
            (0x736b8, 0xd65f03c0),
            (0x736c4, 0x97ff8c1f),
            (0x736dc, 0x97ff8c4d),
            (0x736e8, 0x97ff8c12),
            (0x73718, 0x97ff8c3e),
            (0x73728, 0x1e203860),
            (0x73734, 0x1e203860),
            (0x73740, 0x1e203860),
            (0x73748, 0xb9400e68),
            (0x7376c, 0x97ff8c09),
            (0x56798, 0x6e21dc00),
            (0x5681c, 0x4ea0d421),
            (0x56820, 0x6e22dc21),
            (0x56824, 0x4e21d400),
        ] {
            assert_eq!(
                u32::from_le_bytes(executable[address + 256..address + 260].try_into().unwrap()),
                expected,
                "Native instruction {address:x}"
            );
        }
    }
}
