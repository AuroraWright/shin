//! Replay native child passes and thumbnail transition (original8e710/updated8e670).
use super::*;

impl MurdererAccusationImpl {
    pub(super) fn render_replay(
        &self,
        model: &MurdererModel,
        pass: &mut RenderPass,
        transform: &TransformParams,
        stencil_ref: u8,
        kind: PassKind,
    ) {
        let matrix = transform.compute_top_left_transform();
        let mut builder = RenderRequestBuilder::new()
            .depth_stencil_shorthand(stencil_ref, true, true)
            .color_blend_type(if kind == PassKind::Opaque {
                ColorBlendType::Opaque
            } else {
                ColorBlendType::Layer1
            });
        let quad = |rect: Vec4, color: UnormColor| {
            QuadVertices::new()
                .with_box(rect.x, rect.y, rect.x + rect.z, rect.y + rect.w)
                .with_tex_box(0.0, 0.0, 1.0, 1.0)
                .with_color(color)
        };
        let thumbs = [
            &self.textures._thumb_1_s,
            &self.textures._thumb_2_s,
            &self.textures._thumb_4_s,
            &self.textures._thumb_56_s,
            &self.textures._thumb_7_s,
            &self.textures._thumb_8_s,
        ];
        if kind == PassKind::Opaque {
            quad(vec4(0.0, 0.0, 1920.0, 1080.0), UnormColor::WHITE).render_sprite(
                pass,
                builder,
                self.textures._different_space_2d.as_source(),
                matrix,
            );
            for (index, texture) in thumbs.into_iter().enumerate() {
                let rect = replay_rect(index).unwrap();
                let (focus, flash) = model.replay_feedback(index);
                quad(rect, UnormColor::WHITE).render_button(
                    pass,
                    builder,
                    texture.as_source(),
                    matrix,
                    Vec4::ZERO,
                    vec4(1.0, 1.0, 1.0, flash),
                );
                if focus > 0.0 {
                    // Native8e914 switches to normal alpha blending. The state
                    // persists for subsequent thumbnails in the opaque pass.
                    builder = builder.color_blend_type(ColorBlendType::Layer1);
                    quad(
                        rect,
                        FloatColor4::from_rgba(1.0, 1.0, 1.0, focus * 0.5).into_unorm(),
                    )
                    .fill(pass, builder, matrix);
                }
            }
            return;
        }
        quad(BACK_RECT, UnormColor::WHITE).render_button(
            pass,
            builder,
            self.textures._back.as_source(),
            matrix,
            Vec4::ZERO,
            vec4(1.0, 1.0, 1.0, model.back_flash()),
        );
        if let Some((index, rect, amount)) = model.replay_expansion() {
            let full_textures = [
                &self.textures._thumb_1,
                &self.textures._thumb_2,
                &self.textures._thumb_4,
                &self.textures._thumb_56,
                &self.textures._thumb_7,
                &self.textures._thumb_8,
            ];
            let vertices = shin_render::quad_vertices::build_quad_vertices(|uv| {
                shin_render::shaders::types::vertices::BlendVertex {
                    position: glam::vec3(rect.x + rect.z * uv.x, rect.y + rect.w * uv.y, 0.0),
                    color: UnormColor::WHITE,
                    texture_position: vec4(uv.x, uv.y, uv.x, uv.y),
                }
            });
            pass.run(builder.color_blend_type(ColorBlendType::Opaque).build(
                shin_render::RenderProgramWithArguments::Blend {
                    vertices: shin_render::shaders::types::buffer::VertexSource::VertexData {
                        vertices: &vertices,
                    },
                    texture1: thumbs[index].as_source(),
                    texture2: full_textures[index].as_source(),
                    transform: matrix,
                    blend: Vec4::splat(amount),
                    flash: vec4(1.0, 1.0, 1.0, 0.0),
                },
                shin_render::DrawPrimitive::TrianglesStrip,
            ));
        }
    }
}
