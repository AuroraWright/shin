//! Original1382c: live root below a quantized half-brightness modal snapshot.
use glam::{Mat4, vec3};
use shin_core::primitives::color::UnormColor;
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    quad_vertices::build_quad_vertices,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, texture::TextureSource, vertices::PosColTexVertex},
};

pub struct Plan {
    pub render_root: bool,
    pub backdrop: Option<Packet>,
}
pub struct Packet {
    pub projection: Mat4,
    pub blend: ColorBlendType,
    pub vertices: [PosColTexVertex; 4],
}

pub fn plan(has_snapshot: bool, progress: f32) -> Plan {
    let fade = if has_snapshot { progress } else { 0.0 };
    Plan {
        render_root: fade < 1.0,
        backdrop: (fade > 0.0).then(|| {
            //4cc90 truncates/saturates each channel before Sprite submission.
            // Keep this byte quantization; a floating Layer multiplier differs.
            let grey = ((fade * 0.5).max(0.0) * 255.0) as u8;
            let alpha = (fade.max(0.0) * 255.0) as u8;
            let color = UnormColor::from_rgba(grey, grey, grey, alpha);
            Packet {
                projection: super::capture::packets()[0].projection,
                blend: if fade < 1.0 {
                    ColorBlendType::LayerPremultiplied1
                } else {
                    ColorBlendType::Opaque
                },
                vertices: build_quad_vertices(|uv| PosColTexVertex {
                    position: vec3(uv.x * 1920.0, uv.y * 1080.0, 0.0),
                    color,
                    texture_position: uv,
                }),
            }
        }),
    }
}
impl Packet {
    pub fn render(&self, pass: &mut RenderPass, snapshot: TextureSource) {
        let mut projection = self.projection;
        projection.z_axis.z *= 0.5;
        projection.w_axis.z = 0.5;
        pass.run(
            RenderRequestBuilder::new()
                .color_blend_type(self.blend)
                .build(
                    RenderProgramWithArguments::Sprite {
                        vertices: VertexSource::VertexData {
                            vertices: &self.vertices,
                        },
                        sprite: snapshot,
                        transform: projection,
                    },
                    DrawPrimitive::TrianglesStrip,
                ),
        );
    }
}

#[cfg(test)]
mod tests;
