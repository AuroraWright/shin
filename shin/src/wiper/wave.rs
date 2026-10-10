use crate::{
    render::normalized_projection_matrix,
    update::{AdvUpdatable, AdvUpdateContext},
    wiper::timed::{TimedWiper, TimedWiperWrapper},
};
use glam::{Vec2, Vec4, vec2, vec4};
use shin_core::time::Ticks;
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    quad_vertices::build_quad_vertices,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, texture::TextureSource, vertices::PosTexVertex},
};

#[derive(Debug, Clone)]
pub struct WaveWiperImpl {
    center: Vec2,
    aspect: f32,
    width: f32,
}
impl WaveWiperImpl {
    fn parameters(&self, progress: f32) -> (Vec4, Vec4) {
        let metric = if self.aspect > 1.0 {
            vec2(1.0 / self.aspect, 1.0)
        } else {
            vec2(1.0, self.aspect)
        };
        let reach = |center: f32| {
            if center < 0.5 {
                (self.width + 1.0) - center
            } else {
                self.width + center
            }
        };
        let extent = vec2(reach(self.center.x), reach(self.center.y)) * metric;
        let radius = (extent.x * extent.x + extent.y * extent.y).sqrt();
        (
            vec4(self.center.x, self.center.y, metric.x, metric.y),
            vec4(progress * radius, self.width, progress, 0.0),
        )
    }
}
impl AdvUpdatable for WaveWiperImpl {
    fn update(&mut self, _: &AdvUpdateContext) {}
}
impl TimedWiper for WaveWiperImpl {
    #[cfg(test)]
    fn reference_set_batch_words(&mut self, words: &[u32]) {
        self.center = glam::vec2(f32::from_bits(words[0]), f32::from_bits(words[1]));
        self.aspect = f32::from_bits(words[2]);
        self.width = f32::from_bits(words[3]);
    }

    fn batch_parameters_match(&self, other: &Self) -> bool {
        use super::timed::batch_float_matches as matches;
        matches(self.center.x, other.center.x)
            && matches(self.center.y, other.center.y)
            && matches(self.aspect, other.aspect)
            && matches(self.width, other.width)
    }
    fn render(
        &self,
        pass: &mut RenderPass,
        request: RenderRequestBuilder,
        target: TextureSource,
        source: TextureSource,
        progress: f32,
    ) {
        let (center_scale, front_width) = self.parameters(progress);
        let vertices = build_quad_vertices(|uv| PosTexVertex {
            position: uv,
            texture_position: uv,
        });
        pass.run(request.color_blend_type(ColorBlendType::Opaque).build(
            RenderProgramWithArguments::WiperWave {
                vertices: VertexSource::VertexData {
                    vertices: &vertices,
                },
                texture_source: source,
                texture_target: target,
                transform: normalized_projection_matrix(),
                center_scale,
                front_width,
            },
            DrawPrimitive::TrianglesStrip,
        ));
    }
}
pub type WaveWiper = TimedWiperWrapper<WaveWiperImpl>;
impl WaveWiper {
    pub fn new(duration: Ticks) -> Self {
        // Native21c64 fixes all four values; command parameters are ignored.
        Self::from_inner(
            WaveWiperImpl {
                center: vec2(0.5, 0.5),
                aspect: 1.0,
                width: 0.25,
            },
            duration,
        )
    }
}
#[cfg(test)]
mod tests;
