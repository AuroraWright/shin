//! Native Accusation child drawing (original98a10/1.0398970).
use super::*;

impl MurdererAccusationImpl {
    pub(super) fn render_accusation(
        &self,
        model: &MurdererModel,
        pass: &mut RenderPass,
        transform: &TransformParams,
        stencil_ref: u8,
        kind: PassKind,
    ) {
        let matrix = transform.compute_top_left_transform();
        let builder = RenderRequestBuilder::new()
            .depth_stencil_shorthand(stencil_ref, true, true)
            .color_blend_type(ColorBlendType::Layer1);
        let full = vec4(0., 0., 1., 1.);
        let quad = |rect: Vec4, uv: Vec4| {
            QuadVertices::new()
                .with_box(rect.x, rect.y, rect.x + rect.z, rect.y + rect.w)
                .with_tex_box(uv.x, uv.y, uv.z, uv.w)
                .with_color(UnormColor::WHITE)
        };
        let draw = |pass: &mut RenderPass, texture: &GpuTexture, rect: Vec4, uv: Vec4| {
            quad(rect, uv).render_sprite(pass, builder, texture.as_source(), matrix);
        };
        let button =
            |pass: &mut RenderPass, texture: &GpuTexture, rect: Vec4, focus: f32, flash: f32| {
                let vertices = quad(rect, full);
                if focus > 0. || flash > 0. {
                    vertices.render_button(
                        pass,
                        builder,
                        texture.as_source(),
                        matrix,
                        vec4(1., 1., 1., focus * 0.5),
                        vec4(1., 1., 1., flash),
                    );
                } else {
                    vertices.render_sprite(pass, builder, texture.as_source(), matrix);
                }
            };
        if kind == PassKind::Opaque {
            quad(vec4(0., 0., 1920., 1080.), full).render_sprite(
                pass,
                builder.color_blend_type(ColorBlendType::Opaque),
                self.textures.background.as_source(),
                matrix,
            );
            // Native switches back to Layer1 within the opaque child pass,
            // draws body/hands/face, and preserves that state for decorations.
            let state = model.battler_sprite_state();
            draw(
                pass,
                if state == 0 {
                    &self.textures._but_bodya
                } else {
                    &self.textures._but_bodyb
                },
                vec4(928., 384., 812., 696.),
                full,
            );
            if state != 0 {
                draw(
                    pass,
                    &self.textures._but_fingerb1,
                    vec4(864., 272., 160., 112.),
                    full,
                );
                draw(
                    pass,
                    &self.textures._but_fingerb2,
                    vec4(800., 384., 128., 160.),
                    full,
                );
            }
            draw(
                pass,
                match state {
                    0 => &self.textures._but_majime3,
                    1 => &self.textures._but_futekib2,
                    _ => &self.textures._but_komarub1,
                },
                vec4(1182., 0., 384., 384.),
                full,
            );
            return;
        }
        let xs = [304., 640., 780., 812.];
        let ys = [184., 584., 926., 958.];
        let us = [0., 0.875, 352. / 384., 1.];
        let vs = [0., 400. / 448., 416. / 448., 1.];
        for y in 0..3 {
            for x in 0..3 {
                draw(
                    pass,
                    &self.textures.left,
                    vec4(xs[x], ys[y], xs[x + 1] - xs[x], ys[y + 1] - ys[y]),
                    vec4(us[x], vs[y], us[x + 1], vs[y + 1]),
                );
            }
        }
        draw(
            pass,
            &self.textures.sentaku,
            vec4(320., 120., 460., 64.),
            full,
        );
        let line_colors = [
            UnormColor(0),
            UnormColor::from_rgba(160, 20, 60, 255),
            UnormColor::from_rgba(60, 22, 176, 255),
            UnormColor::from_rgba(208, 192, 20, 255),
        ];
        for [column, row, flags] in CONNECTIONS {
            let u = (2. + 32. * (flags & 3) as f32) * (1.0 / 128.0);
            let v = (3. + 36. * ((flags >> 2) & 3) as f32) / 144.;
            // Native uses top + atlas span; folding this into a second
            // division changes UV rounding under fractional transforms.
            quad(
                vec4(
                    340. + 28. * column as f32,
                    218. + 30. * row as f32,
                    28.,
                    30.,
                ),
                vec4(u, v, u + 0.21875, v + 30. / 144.),
            )
            .with_color(line_colors[((flags >> 4) & 3) as usize])
            .render_sprite(pass, builder, self.textures.line.as_source(), matrix);
        }
        for entry in FIRST_SUSPECT_ENTRY..=SUBMIT_ENTRY {
            let rect = accusation_rect(entry).unwrap();
            let (focus, flash) = model.accusation_feedback(entry);
            if entry == SUBMIT_ENTRY {
                button(pass, &self.textures.tokutei, rect, focus, flash);
            } else {
                draw(pass, &self.textures.non, rect, full);
                draw(
                    pass,
                    self.textures.suspects()[entry - FIRST_SUSPECT_ENTRY],
                    rect,
                    full,
                );
                if model.suspect_selected(entry) {
                    draw(pass, &self.textures.murderer, rect, full);
                }
                for amount in [focus * 0.5, flash] {
                    if amount > 0. {
                        quad(rect, full)
                            .with_color(UnormColor::from_rgba(255, 255, 255, (amount * 255.) as u8))
                            .fill(pass, builder, matrix);
                    }
                }
            }
        }
        button(
            pass,
            &self.textures._back,
            BACK_RECT,
            0.,
            model.back_flash(),
        );
        let opacity = model.white_flash_opacity();
        if opacity > 0. {
            quad(vec4(0., 0., 1920., 1080.), full)
                .with_color(UnormColor::from_rgba(255, 255, 255, (opacity * 255.) as u8))
                .fill(pass, builder, matrix);
        }
    }
}
