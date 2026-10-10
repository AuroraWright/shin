//! Top child passes, artwork and row UV arithmetic (original8cd70/updated8ccd0).
use super::*;

impl MurdererAccusationImpl {
    pub(super) fn render_top(
        &self,
        model: &MurdererModel,
        pass: &mut RenderPass,
        transform: &TransformParams,
        stencil_ref: u8,
        kind: PassKind,
    ) {
        // Native vertices stay in canvas coordinates. Centering belongs in the
        // composed matrix, preserving affine rounding under parent transforms.
        let matrix = transform.compute_top_left_transform();
        let builder = RenderRequestBuilder::new()
            .depth_stencil_shorthand(stencil_ref, true, true)
            .color_blend_type(ColorBlendType::Layer1);
        let quad = |rect: Vec4, uv: Vec4, color: UnormColor| {
            QuadVertices::new()
                .with_box(rect.x, rect.y, rect.x + rect.z, rect.y + rect.w)
                .with_tex_box(uv.x, uv.y, uv.z, uv.w)
                .with_color(color)
        };
        let full = vec4(0.0, 0.0, 1.0, 1.0);
        if kind == PassKind::Opaque {
            quad(vec4(0.0, 0.0, 1920.0, 1080.0), full, UnormColor::WHITE).render_sprite(
                pass,
                builder.color_blend_type(ColorBlendType::Opaque),
                self.textures._different_space_2d.as_source(),
                matrix,
            );
            for (texture, rect) in [
                (&self.textures._ber_body1, vec4(498.0, 256.0, 352.0, 160.0)),
                (&self.textures._ber_body2, vec4(386.0, 416.0, 208.0, 208.0)),
                (&self.textures._ber_body3, vec4(239.0, 624.0, 672.0, 456.0)),
                (
                    if model.top_accusation_art {
                        &self.textures._ber_akuwaraia1
                    } else {
                        &self.textures._ber_akuwarai2
                    },
                    vec4(594.0, 416.0, 256.0, 208.0),
                ),
            ] {
                quad(rect, full, UnormColor::WHITE).render_sprite(
                    pass,
                    builder,
                    texture.as_source(),
                    matrix,
                );
            }
            return;
        }
        for index in 0..6 {
            let rect = top_menu_rect(index).unwrap();
            let plate_v = (96 * index + 1) as f32 / 820.0;
            // Palette9df40 and vector lerp56810: retain float constants until
            // conversion to the native packed vertex color.
            let focus = model.top_focus_amount(index);
            let color = FloatColor4::from_rgba(
                0.2157 + (1.0 - 0.2157) * focus,
                0.1059 + (0.1608 - 0.1059) * focus,
                0.1059 + (0.1608 - 0.1059) * focus,
                1.0,
            );
            quad(
                rect,
                vec4(0.0, plate_v, 1.0, plate_v + 94.0 / 820.0),
                color.into_unorm(),
            )
            .render_sprite(pass, builder, self.textures._plate.as_source(), matrix);
            let text_v = (80 * index) as f32 / 480.0;
            quad(
                vec4(rect.x + 48.0, rect.y + 8.0, 640.0, 80.0),
                vec4(0.0, text_v, 1.0, text_v + 80.0 / 480.0),
                UnormColor::WHITE,
            )
            .render_sprite(pass, builder, self.textures._topmenu.as_source(), matrix);
            let flash = model.top_flash(index);
            if flash > 0.0 {
                quad(
                    rect,
                    full,
                    FloatColor4::from_rgba(1.0, 1.0, 1.0, flash).into_unorm(),
                )
                .fill(pass, builder, matrix);
            }
        }
    }
}
