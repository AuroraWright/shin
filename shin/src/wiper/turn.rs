use crate::{
    render::normalized_projection_matrix,
    update::{AdvUpdatable, AdvUpdateContext},
    wiper::timed::{TimedWiper, TimedWiperWrapper},
};
use glam::{Mat4, vec2, vec3};
use shin_core::{primitives::color::UnormColor, time::Ticks};
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    quad_vertices::build_quad_vertices,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, texture::TextureSource, vertices::PosColTexVertex},
};

#[derive(Debug, Clone)]
pub struct TurnWiperImpl {
    direction: i32,
}

// Native116e9c treats every nonzero parameter as the outgoing turn.
pub(super) fn rotation_and_opacity(direction: i32, progress: f32) -> (f32, u8) {
    let (rotation, opacity) = if direction == 0 {
        (progress - 1.0, progress)
    } else {
        (progress, 1.0 - progress)
    };
    (
        rotation * std::f32::consts::FRAC_PI_2,
        (opacity * 255.0) as u8,
    )
}
fn turn_transform(angle: f32) -> Mat4 {
    // Native a1990 composes its row matrices in reverse order in glam.
    glam::camera::rh::proj::opengl::perspective(0.99026656, 1920.0 / 1080.0, 1.0, 10000.0)
        * Mat4::from_translation(vec3(0.0, 0.0, -1000.0))
        * Mat4::from_scale(vec3(1.0, -1.0, 1.0))
        * Mat4::from_rotation_y(angle)
}
impl AdvUpdatable for TurnWiperImpl {
    fn update(&mut self, _: &AdvUpdateContext) {}
}
impl TimedWiper for TurnWiperImpl {
    #[cfg(test)]
    fn reference_set_batch_words(&mut self, words: &[u32]) {
        self.direction = words[0] as i32;
    }

    fn batch_parameters_match(&self, other: &Self) -> bool {
        self.direction == other.direction
    }
    fn render(
        &self,
        pass: &mut RenderPass,
        request: RenderRequestBuilder,
        target: TextureSource,
        source: TextureSource,
        progress: f32,
    ) {
        let (angle, alpha) = rotation_and_opacity(self.direction, progress);
        let (background, rotating) = if self.direction == 0 {
            (source, target)
        } else {
            (target, source)
        };
        for (texture, transform, perspective, alpha, blend) in [
            (
                background,
                normalized_projection_matrix(),
                false,
                255,
                ColorBlendType::Opaque,
            ),
            (
                rotating,
                turn_transform(angle),
                true,
                alpha,
                ColorBlendType::Layer1,
            ),
        ] {
            let vertices = build_quad_vertices(|uv| PosColTexVertex {
                position: if perspective {
                    ((uv - vec2(0.5, 0.5)) * vec2(1920.0, 1080.0)).extend(0.0)
                } else {
                    uv.extend(0.0)
                },
                texture_position: uv,
                color: UnormColor::from_rgba(255, 255, 255, alpha),
            });
            pass.run(request.color_blend_type(blend).build(
                RenderProgramWithArguments::Sprite {
                    vertices: VertexSource::VertexData {
                        vertices: &vertices,
                    },
                    sprite: texture,
                    transform,
                },
                DrawPrimitive::TrianglesStrip,
            ));
        }
    }
}

pub type TurnWiper = TimedWiperWrapper<TurnWiperImpl>;
impl TurnWiper {
    pub fn new(duration: Ticks, direction: i32) -> Self {
        Self::from_inner(TurnWiperImpl { direction }, duration)
    }
}
#[cfg(test)]
mod tests;
