use crate::{
    render::normalized_projection_matrix,
    update::{AdvUpdatable, AdvUpdateContext},
    wiper::timed::{TimedWiper, TimedWiperWrapper},
};
use glam::vec2;
use shin_core::{primitives::color::UnormColor, time::Ticks};
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    quad_vertices::build_quad_vertices,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, texture::TextureSource, vertices::PosColTexVertex},
};

#[derive(Debug, Clone)]
pub struct ZoomWiperImpl {
    incoming_scale: f32,
    outgoing_scale: f32,
}

// Native factory21b08 converts signed integer parameters to thousandths.
// Renderer11c580..11c5ac interpolates each scale independently without clamps.
pub(super) fn scales(incoming: f32, outgoing: f32, progress: f32) -> (f32, f32) {
    (
        1.0 + (1.0 - progress) * (incoming - 1.0),
        1.0 + progress * (outgoing - 1.0),
    )
}
impl AdvUpdatable for ZoomWiperImpl {
    fn update(&mut self, _: &AdvUpdateContext) {}
}
impl TimedWiper for ZoomWiperImpl {
    #[cfg(test)]
    fn reference_set_batch_words(&mut self, words: &[u32]) {
        self.incoming_scale = f32::from_bits(words[0]);
        self.outgoing_scale = f32::from_bits(words[1]);
    }

    fn batch_parameters_match(&self, other: &Self) -> bool {
        self.incoming_scale == other.incoming_scale && self.outgoing_scale == other.outgoing_scale
    }
    fn render(
        &self,
        pass: &mut RenderPass,
        request: RenderRequestBuilder,
        target: TextureSource,
        source: TextureSource,
        progress: f32,
    ) {
        let (incoming_scale, outgoing_scale) =
            scales(self.incoming_scale, self.outgoing_scale, progress);
        let draws = [
            (source, outgoing_scale, 255, ColorBlendType::Opaque),
            (
                target,
                incoming_scale,
                (progress * 255.0) as u8,
                ColorBlendType::Layer1,
            ),
        ];
        for (texture, scale, alpha, blend) in draws {
            let vertices = build_quad_vertices(|uv| PosColTexVertex {
                position: (vec2(0.5, 0.5) + (uv - vec2(0.5, 0.5)) * scale).extend(0.0),
                texture_position: uv,
                color: UnormColor::from_rgba(255, 255, 255, alpha),
            });
            pass.run(request.color_blend_type(blend).build(
                RenderProgramWithArguments::Sprite {
                    vertices: VertexSource::VertexData {
                        vertices: &vertices,
                    },
                    sprite: texture,
                    transform: normalized_projection_matrix(),
                },
                DrawPrimitive::TrianglesStrip,
            ));
        }
    }
}
pub type ZoomWiper = TimedWiperWrapper<ZoomWiperImpl>;
impl ZoomWiper {
    pub fn new(duration: Ticks, incoming_scale: i32, outgoing_scale: i32) -> Self {
        Self::from_inner(
            ZoomWiperImpl {
                incoming_scale: incoming_scale as f32 * 0.001,
                outgoing_scale: outgoing_scale as f32 * 0.001,
            },
            duration,
        )
    }
}
#[cfg(test)]
mod tests;
