//! Updated 6aaec: copy the completed target, then process it as one image.
//! This applies to capture contents. Native external Screen drawing uses
//! literal white and adds no second root opacity to these processed pixels.
use crate::{
    layer::{
        higurashi_properties::HigurashiProperties, render_params::higurashi::HigurashiTransform,
    },
    render::PreRenderContext,
};
use glam::{Mat4, Vec2, Vec3, Vec4};
use shin_core::primitives::color::FloatColor4;
use shin_render::render_texture::RenderTexture;
use shin_render::{
    DrawPrimitive, LayerFragmentShader, RenderProgramWithArguments, RenderRequestBuilder,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, texture::TextureSource, vertices::PosTexVertex},
};

#[derive(Clone, Debug)]
pub(crate) struct Packet {
    pub program: u32,
    pub operation: LayerFragmentShader,
    pub projection: Mat4,
    pub color: Vec4,
    pub param: Vec4,
    pub vertices: [PosTexVertex; 3],
}
impl Packet {
    pub fn new(properties: &HigurashiProperties) -> Option<Self> {
        let drawable = properties.drawable_params();
        let raw_color = drawable.color_multiplier.into_vec4();
        let param = drawable.shader_param;
        let operation = drawable.fragment_shader;
        // Original jump table14f38a preserves the operation's own fast path.
        // Even mathematically identity Negative still performs its copy/pass.
        let identity = raw_color.truncate() == Vec3::ONE
            && match operation {
                LayerFragmentShader::Default => true,
                LayerFragmentShader::Mono => param == Vec3::ONE.extend(0.0),
                LayerFragmentShader::Fill => param.w == 0.0,
                LayerFragmentShader::Fill2 => param.truncate() == Vec3::ZERO,
                LayerFragmentShader::Negative => false,
                LayerFragmentShader::Gamma => param.truncate() == Vec3::ONE,
            };
        if identity {
            return None;
        }
        // 6aaec tests raw RGB for its no-op branch. A real shader pass uses
        // the emitted material's premultiplied RGB and retained alpha, as the
        // full Screen program8 packet proves for tint plus partial opacity.
        let color = (raw_color.truncate() * raw_color.w).extend(raw_color.w);
        Some(Self {
            program: [6, 9, 7, 8, 10, 11][operation as usize],
            operation,
            projection: Mat4::from_cols_array(&[
                2., 0., 0., 0., 0., -2., 0., 0., 0., 0., -1., 0., -1., 1., -0., 1.,
            ]),
            color,
            param,
            vertices: [Vec2::ZERO, Vec2::new(2., 0.), Vec2::new(0., 2.)].map(|v| PosTexVertex {
                position: v,
                texture_position: v,
            }),
        })
    }
    pub fn render(&self, pass: &mut RenderPass, texture: TextureSource) {
        pass.run(RenderRequestBuilder::new().build(
            RenderProgramWithArguments::HigurashiPostprocess {
                vertices: VertexSource::VertexData {
                    vertices: &self.vertices,
                },
                texture,
                transform: HigurashiTransform::convert_clip_depth(self.projection),
                color: FloatColor4::from_vec4(self.color),
                operation: self.operation,
                param: self.param,
            },
            DrawPrimitive::TrianglesStrip,
        ));
    }
}

pub(crate) fn apply(
    context: &mut PreRenderContext,
    target: &mut RenderTexture,
    properties: &HigurashiProperties,
) {
    if let Some(packet) = Packet::new(properties) {
        let snapshot = target.copy_snapshot(&mut context.render_clone_ctx());
        let mut pass = context.begin_pass(
            target.as_texture_target(),
            None,
            "Higurashi framebuffer postprocessing",
        );
        packet.render(&mut pass, snapshot.as_texture_source());
    }
}

#[cfg(test)]
mod color_combination_tests;
