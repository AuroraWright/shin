//! Rules child's native passes and nine-slice artwork (original8fd70/updated8fcd0).
use super::*;

impl MurdererAccusationImpl {
    pub(super) fn render_rules(
        &self,
        model: &MurdererModel,
        pass: &mut RenderPass,
        transform: &TransformParams,
        stencil_ref: u8,
        kind: PassKind,
    ) {
        // The native child receives the parent's composed top-left matrix and
        // submits uncentered vertices. Keep translation in the matrix to retain
        // affine rounding, including fractional scales and rotations.
        let matrix = transform.compute_top_left_transform();
        let builder = RenderRequestBuilder::new()
            .depth_stencil_shorthand(stencil_ref, true, true)
            .color_blend_type(if kind == PassKind::Opaque {
                ColorBlendType::Opaque
            } else {
                ColorBlendType::Layer1
            });
        let quad = |rect: Vec4, uv: Vec4| {
            QuadVertices::new()
                .with_box(rect.x, rect.y, rect.x + rect.z, rect.y + rect.w)
                .with_tex_box(uv.x, uv.y, uv.z, uv.w)
                .with_color(UnormColor::WHITE)
        };
        let full = vec4(0.0, 0.0, 1.0, 1.0);
        if kind == PassKind::Opaque {
            quad(vec4(0.0, 0.0, 1920.0, 1080.0), full).render_sprite(
                pass,
                builder,
                self.textures._different_space_2d.as_source(),
                matrix,
            );
            return;
        }
        // Rules uses "right", with thin left/top borders and large decorative
        // right/bottom pieces. Hints uses a separate "left" layout.
        let xs = [264.0, 280.0, 1320.0, 1672.0];
        let ys = [164.0, 180.0, 516.0, 932.0];
        let us = [0.0, 16.0 / 384.0, 32.0 / 384.0, 1.0];
        let vs = [0.0, 16.0 / 448.0, 32.0 / 448.0, 1.0];
        for y in 0..3 {
            for x in 0..3 {
                quad(
                    vec4(xs[x], ys[y], xs[x + 1] - xs[x], ys[y + 1] - ys[y]),
                    vec4(us[x], vs[y], us[x + 1], vs[y + 1]),
                )
                .render_sprite(
                    pass,
                    builder,
                    self.textures._right.as_source(),
                    matrix,
                );
            }
        }
        quad(vec4(300.0, 180.0, 1120.0, 720.0), full).render_sprite(
            pass,
            builder,
            self.textures._rule_text.as_source(),
            matrix,
        );
        quad(BACK_RECT, full).render_button(
            pass,
            builder,
            self.textures._back.as_source(),
            matrix,
            Vec4::ZERO,
            vec4(1.0, 1.0, 1.0, model.back_flash()),
        );
    }
}
