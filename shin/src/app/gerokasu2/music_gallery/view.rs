use crate::adv::{
    character_view::CharacterTextures,
    gerokasu2::music_gallery::{Controller, artwork},
};
use glam::{Mat4, Vec2, Vec3, Vec4};
use shin_core::primitives::color::UnormColor;
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    quad_vertices::QuadVertices,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, vertices::PosColTexVertex},
};
pub(super) fn render(
    model: &Controller,
    atlas: &CharacterTextures,
    system: &CharacterTextures,
    pass: &mut RenderPass,
) {
    let matrix = Mat4::from_cols_array(&artwork::PROJECTION);
    for p in artwork::packets(model, false) {
        if p.texture == "top" {
            for bar in artwork::scrollbar(model) {
                let vertices = bar.vertices.map(|v| PosColTexVertex {
                    position: Vec3::from_array(v.position),
                    color: UnormColor::WHITE,
                    texture_position: Vec2::from_array(v.uv),
                });
                pass.run(
                    RenderRequestBuilder::new()
                        .color_blend_type(ColorBlendType::Layer1)
                        .build(
                            RenderProgramWithArguments::Sprite {
                                vertices: VertexSource::VertexData {
                                    vertices: &vertices,
                                },
                                sprite: system.get("scroll").as_source(),
                                transform: Mat4::from_cols_array(&bar.matrix),
                            },
                            DrawPrimitive::TrianglesStrip,
                        ),
                );
            }
        }
        let bytes = p.color.to_le_bytes();
        let quad = QuadVertices::new()
            .with_box(p.rect[0], p.rect[1], p.rect[2], p.rect[3])
            .with_tex_box(p.uv[0], p.uv[1], p.uv[2], p.uv[3])
            .with_color(UnormColor::from_rgba(
                bytes[2], bytes[1], bytes[0], bytes[3],
            ));
        let builder = RenderRequestBuilder::new().color_blend_type(match p.blend {
            1 => ColorBlendType::Opaque,
            3 => ColorBlendType::Layer2,
            _ => ColorBlendType::Layer1,
        });
        let texture = atlas.get(&p.texture);
        if p.flash > 0. {
            quad.render_button(
                pass,
                builder,
                texture.as_source(),
                matrix,
                Vec4::ZERO,
                Vec4::new(1., 1., 1., p.flash),
            );
        } else {
            quad.render_sprite(pass, builder, texture.as_source(), matrix);
        }
    }
}
