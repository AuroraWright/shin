//! Ger596B0: normalized composed snapshot, premultiplied blend5, program9/33.
use super::OverlayClock;
use crate::asset::mask::MaskTexture;
use glam::{Vec2, Vec4};
use shin_core::primitives::color::FloatColor4;
use shin_render::{
    ColorBlendType, DrawPrimitive, LayerFragmentShader, LayerShaderOutputKind,
    RenderProgramWithArguments, RenderRequestBuilder,
    quad_vertices::build_quad_vertices,
    render_pass::RenderPass,
    render_texture::RenderTexture,
    shaders::types::{
        buffer::VertexSource,
        vertices::{MaskVertex, PosTexVertex},
    },
};
use std::sync::Arc;

pub struct Overlay {
    pub clock: OverlayClock,
    pub snapshot: RenderTexture,
    pub mask: Option<Arc<MaskTexture>>,
}
pub enum DrawPacket {
    Plain {
        vertices: [PosTexVertex; 4],
        opacity: f32,
    },
    Masked {
        vertices: [MaskVertex; 4],
        minmax: Vec2,
    },
}
pub fn draw_packet(clock: &OverlayClock, dimensions: Option<(u32, u32)>) -> Option<DrawPacket> {
    if !clock.visible() {
        return None;
    }
    Some(if let Some((width, height)) = dimensions {
        let extent = if clock.flags & 8 == 0 {
            Vec2::ONE
        } else {
            glam::vec2(1920.0 / width as f32, 1080.0 / height as f32)
        };
        DrawPacket::Masked {
            vertices: build_quad_vertices(|uv| MaskVertex {
                position: uv,
                texture_position: uv,
                mask_position: glam::vec2(
                    if clock.flags & 2 != 0 {
                        1.0 - uv.x
                    } else {
                        uv.x
                    },
                    if clock.flags & 4 != 0 {
                        1.0 - uv.y
                    } else {
                        uv.y
                    },
                ) * extent,
            }),
            minmax: Vec2::from_array(fused_mask_weights(clock)),
        }
    } else {
        DrawPacket::Plain {
            vertices: build_quad_vertices(|uv| PosTexVertex {
                position: uv,
                texture_position: uv,
            }),
            opacity: 1.0 - clock.elapsed / clock.duration,
        }
    })
}

/// Native597A4 uses FMSUB: preserve its one-rounding endpoint arithmetic.
pub fn fused_mask_weights(clock: &OverlayClock) -> [f32; 2] {
    let inverse = 1.0 / clock.softness;
    let span = inverse + 1.0;
    let progress = clock.elapsed / clock.duration;
    if clock.flags & 1 == 0 {
        let first = (-span).mul_add(progress, 1.0);
        [first, inverse + first]
    } else {
        let first = span * (1.0 - progress);
        [first, first - inverse]
    }
}

impl Overlay {
    pub fn render(&self, pass: &mut RenderPass) {
        let dimensions = self.mask.as_ref().map(|m| {
            let s = m.texture.size();
            (s.width, s.height)
        });
        let Some(packet) = draw_packet(&self.clock, dimensions) else {
            return;
        };
        let builder =
            RenderRequestBuilder::new().color_blend_type(ColorBlendType::LayerPremultiplied1);
        let transform = crate::render::normalized_projection_matrix();
        if let DrawPacket::Masked { vertices, minmax } = packet {
            pass.run(builder.build(
                RenderProgramWithArguments::ScreenCurtain {
                    vertices: VertexSource::VertexData {
                        vertices: &vertices,
                    },
                    texture: self.snapshot.as_texture_source(),
                    mask: self.mask.as_ref().unwrap().texture.as_source(),
                    transform,
                    minmax,
                },
                DrawPrimitive::TrianglesStrip,
            ));
        } else if let DrawPacket::Plain { vertices, opacity } = packet {
            pass.run(builder.build(
                RenderProgramWithArguments::Layer {
                    output_kind: LayerShaderOutputKind::Layer,
                    fragment_shader: LayerFragmentShader::Default,
                    vertices: VertexSource::VertexData {
                        vertices: &vertices,
                    },
                    texture: self.snapshot.as_texture_source(),
                    transform,
                    color_multiplier: FloatColor4::from_rgba(opacity, opacity, opacity, opacity),
                    fragment_shader_param: Vec4::ZERO,
                },
                DrawPrimitive::TrianglesStrip,
            ));
        }
    }
}
