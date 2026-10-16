//! Shared renderer submission of the native Manual packet model.
use crate::adv::{
    characters::view::CharacterTextures,
    gerokasu2::manual::{
        Controller,
        artwork::{self, CLIP, PAGE},
    },
};
use glam::{Mat4, Vec4};
use shin_core::primitives::color::UnormColor;
use shin_render::{
    ColorBlendType, DepthStencilState, RenderRequestBuilder, StencilFunction, StencilOperation,
    StencilPipelineState, StencilState, quad_vertices::QuadVertices, render_pass::RenderPass,
};
pub(super) fn render(
    model: &Controller,
    atlas: &CharacterTextures,
    pass: &mut RenderPass,
    game: crate::cli::Game,
) {
    // Desktop uses the shared fixed A/B controller contract; native key layout0.
    for q in artwork::packets(model, false, game) {
        let c = q.color.to_le_bytes();
        let quad = QuadVertices::new()
            .with_box(q.rect[0], q.rect[1], q.rect[2], q.rect[3])
            .with_tex_box(q.uv[0], q.uv[1], q.uv[2], q.uv[3])
            .with_color(UnormColor::from_rgba(c[2], c[1], c[0], c[3]));
        let pipeline = if q.stencil == CLIP {
            StencilPipelineState {
                pass_operation: StencilOperation::Increment,
                ..Default::default()
            }
        } else if q.stencil == PAGE {
            StencilPipelineState {
                function: StencilFunction::LessOrEqual,
                ..Default::default()
            }
        } else {
            StencilPipelineState::default()
        };
        let builder = RenderRequestBuilder::new()
            .color_blend_type(if q.texture.is_empty() {
                ColorBlendType::NoColor
            } else if q.blend == 3 {
                ColorBlendType::Layer2
            } else {
                ColorBlendType::Layer1
            })
            .depth_stencil(DepthStencilState {
                stencil: StencilState {
                    pipeline,
                    stencil_reference: (q.stencil[4] >> 16) as u8,
                },
                ..Default::default()
            });
        let matrix = Mat4::from_cols_array(&q.matrix);
        if q.texture.is_empty() {
            pass.clear(None, Some(0), None);
            quad.fill(pass, builder, matrix);
        } else if q.flash > 0. {
            quad.render_button(
                pass,
                builder,
                atlas.get(&q.texture).as_source(),
                matrix,
                Vec4::ZERO,
                Vec4::new(1., 1., 1., q.flash),
            );
        } else {
            quad.render_sprite(pass, builder, atlas.get(&q.texture).as_source(), matrix);
        }
    }
}
