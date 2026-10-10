use crate::{
    render::normalized_projection_matrix,
    update::{AdvUpdatable, AdvUpdateContext},
    wiper::timed::{TimedWiper, TimedWiperWrapper},
};
use glam::{Vec2, vec2};
use shin_core::{primitives::color::UnormColor, time::Ticks};
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    quad_vertices::build_quad_vertices,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, texture::TextureSource, vertices::PosColTexVertex},
};

#[derive(Debug, Clone)]
pub struct ScrollWiperImpl {
    incoming_direction: i32,
    outgoing_direction: i32,
}

// Native ef5c0 reads two independent eight-way selectors. Zero and values
// outside1..8 are stationary; do not fold the second direction into the first.
fn direction(value: i32) -> Vec2 {
    match value {
        1 => vec2(1.0, 0.0),
        2 => vec2(1.0, 1.0),
        3 => vec2(0.0, 1.0),
        4 => vec2(-1.0, 1.0),
        5 => vec2(-1.0, 0.0),
        6 => vec2(-1.0, -1.0),
        7 => vec2(0.0, -1.0),
        8 => vec2(1.0, -1.0),
        _ => Vec2::ZERO,
    }
}
fn outgoing_offset(direction: i32, progress: f32) -> Vec2 {
    // Native ef688..ef6d8 wraps each nonzero remaining displacement across
    // the opposite edge. At exactly completion the old image returns to0,
    // underneath the incoming image with vertex alpha1.
    let remaining = self::direction(direction) * (1.0 - progress);
    let wrap = |value: f32| {
        if value < 0.0 {
            -1.0 - value
        } else if value > 0.0 {
            1.0 - value
        } else {
            value
        }
    };
    vec2(wrap(remaining.x), wrap(remaining.y))
}
impl AdvUpdatable for ScrollWiperImpl {
    fn update(&mut self, _: &AdvUpdateContext) {}
}
impl TimedWiper for ScrollWiperImpl {
    #[cfg(test)]
    fn reference_set_batch_words(&mut self, words: &[u32]) {
        self.incoming_direction = words[0] as i32;
        self.outgoing_direction = words[1] as i32;
    }

    fn batch_parameters_match(&self, other: &Self) -> bool {
        self.incoming_direction == other.incoming_direction
            && self.outgoing_direction == other.outgoing_direction
    }
    fn render(
        &self,
        pass: &mut RenderPass,
        request: RenderRequestBuilder,
        target: TextureSource,
        source: TextureSource,
        progress: f32,
    ) {
        let draws = [
            (
                source,
                outgoing_offset(self.outgoing_direction, progress),
                255,
                ColorBlendType::Opaque,
            ),
            (
                target,
                direction(self.incoming_direction) * (1.0 - progress),
                (progress * 255.0) as u8,
                ColorBlendType::Layer1,
            ),
        ];
        for (texture, offset, alpha, blend) in draws {
            let vertices = build_quad_vertices(|uv| PosColTexVertex {
                position: (uv + offset).extend(0.0),
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
pub type ScrollWiper = TimedWiperWrapper<ScrollWiperImpl>;
impl ScrollWiper {
    pub fn new(duration: Ticks, incoming_direction: i32, outgoing_direction: i32) -> Self {
        Self::from_inner(
            ScrollWiperImpl {
                incoming_direction,
                outgoing_direction,
            },
            duration,
        )
    }
}
#[cfg(test)]
mod tests;
