//! Actual401e0 composition. Picture art and retained capture are owned by the
//! concrete Viewer; neither is synthesized by this packet planner.
use super::{Model, Picture};
use crate::{
    adv::{character_view::CharacterTextures, higurashi::reconstruction_mode::overlay::DrawPacket},
    layer::render_params::higurashi::HigurashiTransform,
};
use anyhow::{Result, ensure};
use glam::{Mat4, Vec4};
use shin_core::primitives::color::{FloatColor4, UnormColor};
use shin_render::{
    ColorBlendType, DrawPrimitive, LayerFragmentShader, LayerShaderOutputKind, PassKind,
    RenderProgramWithArguments, RenderRequestBuilder,
    quad_vertices::QuadVertices,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, texture::TextureSource},
};
#[derive(Clone, Copy, Debug)]
pub(super) enum Op {
    Black,
    Picture(PassKind),
    Overlay { opacity: f32 },
    Badge { rect: [f32; 4], color: u32 },
}
fn projection() -> Mat4 {
    Mat4::from_cols_array(&[
        2. / 1920.,
        0.,
        0.,
        0.,
        0.,
        -2. / 1080.,
        0.,
        0.,
        0.,
        0.,
        -1.,
        0.,
        -1.,
        1.,
        -0.,
        1.,
    ])
}
pub(super) fn plan(model: &Model) -> Vec<Op> {
    let mut out = vec![];
    if model.picture.is_some() {
        out.push(Op::Black);
        out.push(Op::Picture(PassKind::Opaque));
        out.push(Op::Picture(PassKind::Transparent));
    }
    if let Some(DrawPacket::Plain { opacity, .. }) = model.capture.draw_packet(None) {
        out.push(Op::Overlay { opacity });
    }
    if model.show_info && model.entry.pictures.len() >= 2 {
        let mut x = (1905 - 30 * model.entry.pictures.len() as i32) as f32;
        for (i, unlocked) in model.entry.unlocked.iter().enumerate() {
            out.push(Op::Badge {
                rect: [x, 1025., x + 32., 1065.],
                color: if i as i32 == model.variant {
                    0xff80ff90
                } else if *unlocked {
                    u32::MAX
                } else {
                    0xff808080
                },
            });
            x += 30.;
        }
    }
    out
}
pub(super) struct View;
impl View {
    pub fn new() -> Self {
        Self
    }
    /// draw_picture must render this ONE properties owner at original stencil1
    /// for each supplied pass, using identity root context (observed default global words).
    /// Do not advance it in this callback; Model::advance_clocks owns its tick.
    pub fn render(
        &self,
        model: &Model,
        atlas: &CharacterTextures,
        snapshot: Option<TextureSource<'_>>,
        pass: &mut RenderPass,
        mut draw_picture: impl FnMut(&Picture, PassKind, u8, &mut RenderPass) -> Result<()>,
    ) -> Result<()> {
        for op in plan(model) {
            match op {
                Op::Black => QuadVertices::new()
                    .with_box(0., 0., 1920., 1080.)
                    .with_color(UnormColor::from_rgba(0, 0, 0, 255))
                    .fill(
                        pass,
                        RenderRequestBuilder::new().color_blend_type(ColorBlendType::Opaque),
                        HigurashiTransform::convert_clip_depth(projection()),
                    ),
                Op::Picture(kind) => draw_picture(model.picture.as_ref().unwrap(), kind, 1, pass)?,
                Op::Overlay { opacity } => {
                    ensure!(
                        snapshot.is_some(),
                        "Native Viewer visible retained capture is missing"
                    );
                    let Some(DrawPacket::Plain { vertices, .. }) = model.capture.draw_packet(None)
                    else {
                        unreachable!()
                    };
                    pass.run(
                        RenderRequestBuilder::new()
                            .color_blend_type(ColorBlendType::LayerPremultiplied1)
                            .build(
                                RenderProgramWithArguments::Layer {
                                    output_kind: LayerShaderOutputKind::Layer,
                                    fragment_shader: LayerFragmentShader::Default,
                                    vertices: VertexSource::VertexData {
                                        vertices: &vertices,
                                    },
                                    texture: snapshot.unwrap(),
                                    transform: crate::render::normalized_projection_matrix(),
                                    color_multiplier: FloatColor4::from_rgba(
                                        opacity, opacity, opacity, opacity,
                                    ),
                                    fragment_shader_param: Vec4::ZERO,
                                },
                                DrawPrimitive::TrianglesStrip,
                            ),
                    );
                }
                Op::Badge { rect, color } => {
                    let c = color.to_le_bytes();
                    QuadVertices::new()
                        .with_box(rect[0], rect[1], rect[2], rect[3])
                        .with_tex_box(0., 0., 1., 1.)
                        .with_color(UnormColor::from_rgba(c[2], c[1], c[0], c[3]))
                        .render_sprite(
                            pass,
                            RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1),
                            atlas.get("check").as_source(),
                            HigurashiTransform::convert_clip_depth(projection()),
                        );
                }
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests;
