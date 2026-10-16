//! Purple transcript child renderer (native950c0/1.0395020).
use super::*;
use crate::layer::render_params::multiply_transforms;
use shin_render::shaders::types::{buffer::VertexSource, vertices::PosColTexVertex};
use shin_render::{
    DepthFunction, DepthState, DepthStencilState, DrawPrimitive, RenderProgramWithArguments,
};

impl MurdererAccusationImpl {
    pub(super) fn render_purple(
        &self,
        model: &MurdererModel,
        pass: &mut RenderPass,
        transform: &TransformParams,
        stencil_ref: u8,
        kind: PassKind,
    ) {
        let matrix = transform.compute_top_left_transform();
        let stencil = DepthStencilState::shorthand(stencil_ref, true, false);
        let builder = RenderRequestBuilder::new()
            .depth_stencil(DepthStencilState {
                depth: DepthState {
                    function: DepthFunction::Less,
                    write_enable: false,
                },
                ..stencil
            })
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
        let full = vec4(0., 0., 1., 1.);
        if kind == PassKind::Opaque {
            quad(vec4(0., 0., 1920., 1080.), full).render_sprite(
                pass,
                builder,
                self.textures._different_space_2d.as_source(),
                matrix,
            );
            return;
        }
        let button = |pass: &mut RenderPass,
                      texture: &GpuTexture,
                      rect: Vec4,
                      uv: Vec4,
                      z: f32,
                      focus: f32,
                      flash: f32,
                      builder: RenderRequestBuilder| {
            let vertices = quad(rect, uv).with_z(z);
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
        for (texture, xs, ys, us, vs) in [(
            &self.textures.left,
            [130., 466., 666., 698.],
            [164., 564., 900., 932.],
            [0., 0.875, 11. / 12., 1.],
            [0., 25. / 28., 13. / 14., 1.],
        )] {
            for y in 0..3 {
                for x in 0..3 {
                    quad(
                        vec4(xs[x], ys[y], xs[x + 1] - xs[x], ys[y + 1] - ys[y]),
                        vec4(us[x], vs[y], us[x + 1], vs[y + 1]),
                    )
                    .render_sprite(pass, builder, texture.as_source(), matrix);
                }
            }
        }
        // Native uses transformed depth masks, retaining the full row UVs.
        pass.clear(None, None, Some(0.));
        let mask = builder
            .color_blend_type(ColorBlendType::NoColor)
            .depth_stencil(DepthStencilState {
                depth: DepthState {
                    function: DepthFunction::Always,
                    write_enable: true,
                },
                ..stencil
            });
        quad(vec4(146., 180., 1628., 720.), full)
            .with_z(-1.)
            .with_color(UnormColor::from_rgba(255, 255, 255, 128))
            .fill(pass, mask, matrix);
        let chapter = model.menu == MurdererMenu::PurpleByChapter;
        if chapter {
            quad(vec4(146., 180., 520., 90.), full)
                .with_z(-0.9)
                .with_color(UnormColor::from_rgba(255, 255, 255, 128))
                .fill(pass, mask, matrix);
            let origin = model.purple_list_scroll - 180.;
            for entry in crate::layer::user::purple_text::CHAPTERS
                .iter()
                .filter(|entry| entry.heading().is_none())
            {
                let y = (entry.original_index * 90) as f32 - origin;
                let (focus, flash) = model.purple_chapter_feedback(entry.original_index);
                button(
                    pass,
                    &self.textures._chap_menu,
                    vec4(166., y + 1., 480., 88.),
                    full,
                    -0.99,
                    focus,
                    flash,
                    builder,
                );
                let v = entry.label_row().unwrap() as f32 / 23.;
                button(
                    pass,
                    &self.textures._chap_text,
                    vec4(166., (y + 12.) + 1., 480., 64.),
                    vec4(0., v, 1., v + 1. / 23.),
                    -0.99,
                    focus,
                    flash,
                    builder,
                );
            }
            for heading in 0..6 {
                let y = (model.purple_heading_offset(heading).unwrap() + 1.) + 180.;
                let v = heading as f32 / 6.;
                button(
                    pass,
                    &self.textures._chapter,
                    vec4(166., y, 480., 88.),
                    vec4(0., v, 1., v + 1. / 6.),
                    0.,
                    0.,
                    model.purple_heading_flash(heading),
                    builder,
                );
            }
            self.render_purple_scrollbar(
                pass,
                builder,
                matrix,
                654.,
                2610.,
                model.purple_list_scroll,
            );
        } else {
            for (i, [column, row, flags]) in CONNECTIONS.into_iter().enumerate() {
                let u = (flags & 3) as f32 * 0.25;
                let v = ((flags >> 2) & 3) as f32 * 0.25;
                quad(
                    vec4(
                        166. + 32. * column as f32,
                        198. + 36. * row as f32,
                        32.,
                        36.,
                    ),
                    vec4(u, v, u + 0.25, v + 0.25),
                )
                .with_color(if i < 24 {
                    UnormColor::from_rgba(160, 20, 60, 255)
                } else {
                    UnormColor::from_rgba(60, 22, 176, 255)
                })
                .render_sprite(
                    pass,
                    builder,
                    self.textures.line.as_source(),
                    matrix,
                );
            }
            for index in 0..17 {
                let rect = crate::layer::user::murderer::model::purple_person_rect(index).unwrap();
                quad(rect, full).render_sprite(
                    pass,
                    builder,
                    self.textures.non.as_source(),
                    matrix,
                );
                quad(rect, full)
                    .with_color(
                        if crate::layer::user::murderer::model::purple_person_is_red(index) {
                            UnormColor::from_rgba(255, 15, 15, 255)
                        } else {
                            UnormColor::WHITE
                        },
                    )
                    .render_sprite(
                        pass,
                        builder,
                        self.textures.suspects()[index].as_source(),
                        matrix,
                    );
                let (focus, flash) = model.purple_person_feedback(index);
                for amount in [focus * 0.5, flash] {
                    if amount > 0. {
                        quad(rect, full)
                            .with_color(UnormColor::from_rgba(255, 255, 255, (amount * 255.) as u8))
                            .fill(pass, builder, matrix);
                    }
                }
            }
        }
        let decoration = builder.depth_stencil(stencil);
        let xs = [690., 706., 1774., 1806.];
        let ys = [164., 180., 900., 932.];
        let uv = [0., 0.25, 0.5, 1.];
        for y in 0..3 {
            for x in 0..3 {
                quad(
                    vec4(xs[x], ys[y], xs[x + 1] - xs[x], ys[y + 1] - ys[y]),
                    vec4(uv[x], uv[y], uv[x + 1], uv[y + 1]),
                )
                .render_sprite(
                    pass,
                    decoration,
                    self.textures._right2.as_source(),
                    matrix,
                );
            }
        }
        if chapter {
            quad(vec4(1160., 272., 760., 880.), full)
                .with_color(FloatColor4::from_rgba(0.58, 0.36, 0., 0.25).into_unorm())
                .render_sprite(pass, decoration, self.textures._hana.as_source(), matrix);
        } else if let Some(atlas) = &self.silhouettes[model.purple_silhouette] {
            quad(vec4(1088., 72., 960., 1080.), full)
                .with_color(UnormColor::from_rgba(255, 255, 255, 63))
                .render_sprite(pass, decoration, atlas.first_texture().as_source(), matrix);
        }
        let height = self.purple_text_height(model);
        if height > 720. {
            quad(
                vec4(1614., 920., 176., 48.),
                vec4(0., 3. / 7., 0.55, 3. / 7. + 1. / 7.),
            )
            .render_sprite(
                pass,
                decoration,
                self.textures._help_chars.as_source(),
                matrix,
            );
            quad(vec4(1568., 920., 48., 48.), vec4(0.125, 0.5, 0.25, 1.)).render_sprite(
                pass,
                decoration,
                self.textures._key.as_source(),
                matrix,
            );
        }
        if let Some(index) = model.purple_original_index() {
            let page = if chapter {
                &self.pages.chapters[index]
            } else {
                &self.pages.persons[index]
            };
            page.render(
                pass,
                builder,
                matrix,
                model.purple_text_scroll,
                FloatColor4::WHITE,
            );
        }
        self.render_purple_scrollbar(
            pass,
            decoration,
            matrix,
            1762.,
            height,
            model.purple_text_scroll,
        );
        button(
            pass,
            &self.textures._back,
            BACK_RECT,
            full,
            0.,
            0.,
            model.back_flash(),
            decoration,
        );
    }

    fn render_purple_scrollbar(
        &self,
        pass: &mut RenderPass,
        builder: RenderRequestBuilder,
        matrix: Mat4,
        x: f32,
        height: f32,
        current: f32,
    ) {
        let height = height.ceil();
        let current = current.trunc();
        if height <= 720. || current < 0. || current + 720. > height {
            return;
        }
        let local = multiply_transforms(matrix, Mat4::from_translation(glam::vec3(x, 180., 0.)));
        let matrix = multiply_transforms(local, Mat4::from_translation(glam::vec3(0., 8., 0.)));
        let start = 704. * (current / height);
        let end = 704. * ((current + 720.) / height);
        for (u, ys) in [
            (0., [-8., 0., 704., 712.]),
            (0.5, [start - 8., start, end, end + 8.]),
        ] {
            let vertices: [PosColTexVertex; 8] = std::array::from_fn(|i| PosColTexVertex {
                position: glam::vec3(if i % 2 == 0 { 0. } else { 16. }, ys[i / 2], 1.),
                color: UnormColor::WHITE,
                texture_position: Vec2::new(
                    u + if i % 2 == 0 { 0. } else { 0.5 },
                    (i / 2) as f32 * 0.25,
                ),
            });
            pass.run(builder.build(
                RenderProgramWithArguments::Sprite {
                    vertices: VertexSource::VertexData {
                        vertices: &vertices,
                    },
                    sprite: self.adv_textures.get("scroll").as_source(),
                    transform: matrix,
                },
                DrawPrimitive::TrianglesStrip,
            ));
        }
    }
}
