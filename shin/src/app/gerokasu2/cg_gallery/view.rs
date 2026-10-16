//! Native packet submission through the existing atlas/Picture block renderer.
use crate::{
    adv::{
        character_view::CharacterTextures,
        gerokasu2::{
            cg_gallery::{Controller, artwork, viewer::Viewer},
            reconstruction_mode::overlay::Overlay,
        },
    },
    asset::picture::Picture,
    layer::user::{PictureBlockParams, PictureBlockPassKind, render_block},
};
use glam::{Mat4, Vec2, Vec4};
use shin_core::primitives::color::{FloatColor4, UnormColor};
use shin_render::{
    ColorBlendType, LayerBlendType, LayerFragmentShader, RenderRequestBuilder,
    quad_vertices::QuadVertices, render_pass::RenderPass,
};
fn packet(q: &artwork::Packet, atlas: &CharacterTextures, pass: &mut RenderPass) {
    let c = q.color.to_le_bytes();
    let vertices = QuadVertices::new()
        .with_box(q.rect[0], q.rect[1], q.rect[2], q.rect[3])
        .with_tex_box(q.uv[0], q.uv[1], q.uv[2], q.uv[3])
        .with_color(UnormColor::from_rgba(c[2], c[1], c[0], c[3]));
    let builder = RenderRequestBuilder::new().color_blend_type(match q.blend {
        1 => ColorBlendType::Opaque,
        3 => ColorBlendType::Layer2,
        _ => ColorBlendType::Layer1,
    });
    let matrix = Mat4::from_cols_array(&artwork::projection());
    if q.texture.is_empty() {
        vertices.fill(pass, builder, matrix);
    } else if q.flash > 0. {
        vertices.render_button(
            pass,
            builder,
            atlas.get(&q.texture).as_source(),
            matrix,
            Vec4::ZERO,
            Vec4::new(1., 1., 1., q.flash),
        );
    } else {
        vertices.render_sprite(pass, builder, atlas.get(&q.texture).as_source(), matrix);
    }
}
pub(super) fn render_grid(m: &Controller, atlas: &CharacterTextures, pass: &mut RenderPass) {
    for q in artwork::packets(m) {
        packet(&q, atlas, pass);
    }
}
pub(super) fn render_viewer(
    m: &Viewer,
    picture: Option<&Picture>,
    capture: Option<&Overlay>,
    atlas: &CharacterTextures,
    pass: &mut RenderPass,
) {
    if let Some(picture) = picture {
        // Original4DF18 clears only stencil before its two Picture render passes.
        pass.clear(None, Some(0), None);
        let matrix = Mat4::from_cols_array(&artwork::projection());
        QuadVertices::new()
            .with_box(0., 0., 1920., 1080.)
            .with_color(UnormColor::BLACK)
            .fill(
                pass,
                RenderRequestBuilder::new().color_blend_type(ColorBlendType::Opaque),
                matrix,
            );
        let p = m.transform();
        let size = Vec2::new(
            picture.effective_width as f32,
            picture.effective_height as f32,
        );
        let transform = matrix
            * Mat4::from_translation(glam::vec3(960. + p[0], 540. + p[1], 0.))
            * Mat4::from_scale(glam::vec3(p[2] * 0.001, p[3] * 0.001, 1.))
            * Mat4::from_translation((-size * 0.5).extend(0.));
        for pass_kind in [
            PictureBlockPassKind::OpaqueOnly,
            PictureBlockPassKind::TransparentOnly,
        ] {
            let params = PictureBlockParams {
                pass_kind,
                color_multiplier: FloatColor4::WHITE,
                blend_type: LayerBlendType::Type1,
                fragment_shader: LayerFragmentShader::Default,
                fragment_shader_param: Vec4::ZERO,
            };
            for (_, position, block) in picture.blocks.iter_placements() {
                render_block(
                    block,
                    pass,
                    RenderRequestBuilder::new(),
                    params,
                    transform * Mat4::from_translation(position.extend(0.)),
                );
            }
        }
    }
    if let Some(c) = capture {
        c.render(pass);
    }
    if m.show_info && m.entry.pictures.len() >= 2 {
        let mut x = (1905 - 36 * m.entry.pictures.len() as i32) as f32;
        for (i, unlocked) in m.entry.unlocked.iter().enumerate() {
            let u = if m.variant == i as i32 {
                0.5
            } else if *unlocked {
                0.25
            } else {
                0.
            };
            packet(
                &artwork::Packet {
                    texture: "check".into(),
                    rect: [x, 1025., x + 52., 1077.],
                    uv: [u, 0., u + 0.25, 1.],
                    color: u32::MAX,
                    blend: 2,
                    flash: 0.,
                },
                atlas,
                pass,
            );
            x += 36.;
        }
    }
}
