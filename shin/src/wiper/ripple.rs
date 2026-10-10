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
pub struct RippleWiperImpl {
    center: Vec2,
    aspect: f32,
    width: f32,
    amplitude: f32,
    phase_rate: f32,
    phase: f32,
}
impl RippleWiperImpl {
    fn parameters(&self, progress: f32) -> (Vec4, Vec4) {
        // Native d1138..d115c uses its original approximate PI constant.
        let ramp = if progress < 0.5 {
            progress
        } else {
            1.0 - progress
        };
        let envelope = 1.0 - (ramp * f32::from_bits(0x40490fd0)).cos();
        let inverse_width = if self.width != 0.0 {
            1.0 / self.width
        } else {
            self.width
        };
        (
            vec4(self.center.x, self.center.y, 1.0, self.aspect),
            vec4(
                inverse_width,
                self.amplitude * envelope,
                self.phase,
                progress,
            ),
        )
    }
}
impl AdvUpdatable for RippleWiperImpl {
    fn update(&mut self, context: &AdvUpdateContext) {
        if context.are_animations_allowed {
            self.phase += self.phase_rate * context.delta_ticks.as_f32();
        }
    }
}
impl TimedWiper for RippleWiperImpl {
    #[cfg(test)]
    fn reference_set_batch_words(&mut self, words: &[u32]) {
        self.center = glam::vec2(f32::from_bits(words[0]), f32::from_bits(words[1]));
        self.aspect = f32::from_bits(words[2]);
        self.width = f32::from_bits(words[3]);
        self.amplitude = f32::from_bits(words[4]);
        self.phase_rate = f32::from_bits(words[5]);
        self.phase = f32::from_bits(words[6]);
    }

    fn batch_parameters_match(&self, other: &Self) -> bool {
        use super::timed::batch_float_matches as matches;
        matches(self.center.x, other.center.x)
            && matches(self.center.y, other.center.y)
            && matches(self.aspect, other.aspect)
            && matches(self.width, other.width)
            && matches(self.amplitude, other.amplitude)
            && matches(self.phase_rate, other.phase_rate)
            && matches(self.phase, other.phase)
    }
    fn render(
        &self,
        pass: &mut RenderPass,
        request: RenderRequestBuilder,
        target: TextureSource,
        source: TextureSource,
        progress: f32,
    ) {
        let (center_scale, wave_params) = self.parameters(progress);
        let vertices = build_quad_vertices(|uv| PosTexVertex {
            position: uv,
            texture_position: uv,
        });
        pass.run(request.color_blend_type(ColorBlendType::Opaque).build(
            RenderProgramWithArguments::WiperRipple {
                vertices: VertexSource::VertexData {
                    vertices: &vertices,
                },
                texture_source: source,
                texture_target: target,
                transform: normalized_projection_matrix(),
                center_scale,
                wave_params,
            },
            DrawPrimitive::TrianglesStrip,
        ));
    }
}
pub type RippleWiper = TimedWiperWrapper<RippleWiperImpl>;
impl RippleWiper {
    pub fn new(duration: Ticks) -> Self {
        // Native21b8c fixes these values; command parameters are ignored.
        Self::from_inner(
            RippleWiperImpl {
                center: vec2(0.5, 0.5),
                aspect: 1.0,
                width: 0.25,
                amplitude: 0.2,
                phase_rate: 1.0 / 60.0,
                phase: 0.0,
            },
            duration,
        )
    }
}
#[cfg(test)]
mod tests;
