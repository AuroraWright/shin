//! Native Hints panel, depth mask, atlas rows and scrollbar (original91b20).
use super::*;
use crate::layer::render_params::multiply_transforms;
use shin_render::shaders::types::{
    buffer::VertexSource,
    vertices::{PosColTexVertex, PosColVertex},
};
use shin_render::{
    DepthFunction, DepthState, DepthStencilState, DrawPrimitive, RenderProgramWithArguments,
};

impl MurdererAccusationImpl {
    pub(super) fn render_hints(
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
        let button = |pass: &mut RenderPass,
                      texture: &GpuTexture,
                      rect: Vec4,
                      uv: Vec4,
                      first: Vec4,
                      second: Vec4,
                      builder: RenderRequestBuilder| {
            let vertices = quad(rect, uv);
            // Nativec6f60/c6f10 select Sprite when both flash alphas are zero.
            // Preserve the program choice for fractional atlas sampling.
            if first.w > 0.0 || second.w > 0.0 {
                vertices.render_button(pass, builder, texture.as_source(), matrix, first, second);
            } else {
                vertices.render_sprite(pass, builder, texture.as_source(), matrix);
            }
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
        let xs = [264.0, 600.0, 1640.0, 1672.0];
        let ys = [164.0, 564.0, 900.0, 932.0];
        let us = [0.0, 0.875, 352.0 / 384.0, 1.0];
        let vs = [0.0, 400.0 / 448.0, 416.0 / 448.0, 1.0];
        for y in 0..3 {
            for x in 0..3 {
                quad(
                    vec4(xs[x], ys[y], xs[x + 1] - xs[x], ys[y + 1] - ys[y]),
                    vec4(us[x], vs[y], us[x + 1], vs[y + 1]),
                )
                .render_sprite(
                    pass,
                    builder,
                    self.textures.left.as_source(),
                    matrix,
                );
            }
        }
        // Native91f34 clears depth to0, writes the transformed clip rectangle
        // at z=-1 with Always/write, then tests all full rows using Less.
        // Keep the mask geometry; cropping rows on the CPU changes UV rounding
        // and does not implement the native clipping under 3D transforms.
        pass.clear(None, None, Some(0.0));
        let mask = [
            PosColVertex {
                position: glam::vec3(280.0, 180.0, -1.0),
                color: UnormColor::from_rgba(255, 255, 255, 128),
            },
            PosColVertex {
                position: glam::vec3(1640.0, 180.0, -1.0),
                color: UnormColor::from_rgba(255, 255, 255, 128),
            },
            PosColVertex {
                position: glam::vec3(280.0, 900.0, -1.0),
                color: UnormColor::from_rgba(255, 255, 255, 128),
            },
            PosColVertex {
                position: glam::vec3(1640.0, 900.0, -1.0),
                color: UnormColor::from_rgba(255, 255, 255, 128),
            },
        ];
        pass.run(
            builder
                .color_blend_type(ColorBlendType::NoColor)
                .depth_stencil(DepthStencilState {
                    depth: DepthState {
                        function: DepthFunction::Always,
                        write_enable: true,
                    },
                    ..stencil
                })
                .build(
                    RenderProgramWithArguments::Fill {
                        vertices: VertexSource::VertexData { vertices: &mask },
                        transform: matrix,
                    },
                    DrawPrimitive::TrianglesStrip,
                ),
        );
        // Native computes index*90 - (scroll -180), then adds1 for the plate
        // or13 for the text. Preserve those separately rounded operations.
        let scroll_origin = model.hint_scroll - 180.0;
        for index in 0..model.available_hints() {
            let y = (90 * index) as f32 - scroll_origin;
            let (focus, flash) = model.hint_feedback(index);
            let first = vec4(1.0, 1.0, 1.0, focus * 0.5);
            let second = vec4(1.0, 1.0, 1.0, flash);
            button(
                pass,
                &self.textures._hint_menu,
                vec4(320.0, y + 1.0, 1280.0, 88.0),
                full,
                first,
                second,
                builder,
            );
            let v = model.hint_texture_row(index).unwrap() as f32 / 22.0;
            button(
                pass,
                &self.textures._hint_text,
                vec4(352.0, y + 13.0, 1216.0, 64.0),
                vec4(0.0, v, 1.0, v + 1.0 / 22.0),
                first,
                second,
                builder,
            );
        }
        let decoration = builder.depth_stencil(stencil);
        let height = (model.available_hints() * 90) as f32;
        let current = model.hint_scroll.trunc();
        if height > 720.0 && current >= 0.0 && current + 720.0 <= height {
            // Nativeeee50 supplies two eight-vertex strips, preserving their
            // topology, local z=1 and ordered matrix composition.
            let local = multiply_transforms(
                matrix,
                Mat4::from_translation(glam::vec3(1628.0, 180.0, 0.0)),
            );
            let bar_matrix =
                multiply_transforms(local, Mat4::from_translation(glam::vec3(0.0, 8.0, 0.0)));
            let start = 704.0 * (current / height);
            let end = 704.0 * ((current + 720.0) / height);
            for (u, ys) in [
                (0.0, [-8.0, 0.0, 704.0, 712.0]),
                (0.5, [start - 8.0, start, end, end + 8.0]),
            ] {
                let vertices: [PosColTexVertex; 8] = std::array::from_fn(|i| PosColTexVertex {
                    position: glam::vec3(if i % 2 == 0 { 0.0 } else { 16.0 }, ys[i / 2], 1.0),
                    color: UnormColor::WHITE,
                    texture_position: Vec2::new(
                        u + if i % 2 == 0 { 0.0 } else { 0.5 },
                        (i / 2) as f32 * 0.25,
                    ),
                });
                pass.run(decoration.build(
                    RenderProgramWithArguments::Sprite {
                        vertices: VertexSource::VertexData {
                            vertices: &vertices,
                        },
                        sprite: self.adv_textures.get("scroll").as_source(),
                        transform: bar_matrix,
                    },
                    DrawPrimitive::TrianglesStrip,
                ));
            }
        }
        button(
            pass,
            &self.textures._back,
            BACK_RECT,
            full,
            Vec4::ZERO,
            vec4(1.0, 1.0, 1.0, model.back_flash()),
            decoration,
        );
    }
}
