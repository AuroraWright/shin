use shin_core::{primitives::color::FloatColor4, time::Ticks};
use shin_render::{
    ColorBlendType, RenderRequestBuilder, quad_vertices::QuadVertices, render_pass::RenderPass,
    render_texture::RenderTexture,
};

/// Native null-mask loads overlay the frozen scene with RGBA(1-t/duration).
/// Blend5 is premultiplied source-over, so RGB and alpha fade together.
pub(super) struct SceneLoadFade {
    source: RenderTexture,
    elapsed: Ticks,
    duration: Ticks,
}

impl SceneLoadFade {
    pub fn new(source: RenderTexture, duration: Ticks) -> Self {
        Self {
            source,
            elapsed: Ticks::ZERO,
            duration,
        }
    }

    pub fn update(&mut self, delta: Ticks) -> bool {
        self.elapsed += delta;
        self.elapsed >= self.duration
    }

    pub fn render(&self, pass: &mut RenderPass) {
        let opacity = (1.0 - self.elapsed.as_f32() / self.duration.as_f32()).clamp(0.0, 1.0);
        QuadVertices::new()
            .with_box(0.0, 0.0, 1920.0, 1080.0)
            .with_tex_box(0.0, 0.0, 1.0, 1.0)
            .with_color(FloatColor4::from_rgba(opacity, opacity, opacity, opacity).into_unorm())
            .render_sprite(
                pass,
                RenderRequestBuilder::new().color_blend_type(ColorBlendType::LayerPremultiplied1),
                self.source.as_texture_source(),
                crate::render::top_left_projection_matrix(),
            );
    }
}
