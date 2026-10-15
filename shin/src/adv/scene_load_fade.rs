use std::sync::Arc;

use crate::asset::mask::MaskTexture;
use glam::{Vec2, vec2};
use shin_core::{primitives::color::FloatColor4, time::Ticks};
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    quad_vertices::QuadVertices,
    render_pass::RenderPass,
    render_texture::RenderTexture,
    shaders::types::{buffer::VertexSource, vertices::MaskVertex},
};

/// Present a retained outgoing frame while the destination's workers run.
pub(crate) fn render_snapshot(source: &RenderTexture, pass: &mut RenderPass) {
    QuadVertices::new()
        .with_box(0.0, 0.0, 1920.0, 1080.0)
        .with_tex_box(0.0, 0.0, 1.0, 1.0)
        .render_sprite(
            pass,
            RenderRequestBuilder::new().color_blend_type(ColorBlendType::LayerPremultiplied1),
            source.as_texture_source(),
            crate::render::top_left_projection_matrix(),
        );
}

/// Native frozen-scene capture, with the prepared ADV's default load mask.
/// Null-mask quick return retains premultiplied RGBA(1-t/duration).
pub(super) struct SceneLoadFade {
    source: RenderTexture,
    elapsed: Ticks,
    duration: Ticks,
    skip_preparation_delta: bool,
    mask: Option<(Arc<MaskTexture>, f32)>,
}

impl SceneLoadFade {
    #[cfg(test)]
    pub fn is_masked(&self) -> bool {
        self.mask.is_some()
    }
    #[cfg(test)]
    pub fn reference_elapsed(&self) -> f32 {
        self.elapsed.as_f32()
    }
    pub fn new(source: RenderTexture, duration: Ticks) -> Self {
        Self {
            source,
            elapsed: Ticks::ZERO,
            duration,
            skip_preparation_delta: false,
            mask: None,
        }
    }

    pub fn with_mask(mut self, mask: Arc<MaskTexture>, softness: f32) -> Self {
        self.mask = Some((mask, softness));
        self
    }

    /// The first following delta includes the publication frame's resource and
    /// lazy GPU-pipeline preparation. Start the visible clock after that frame.
    pub fn start_after_publication(mut self) -> Self {
        self.skip_preparation_delta = true;
        self
    }

    pub fn update(&mut self, delta: Ticks) -> bool {
        if std::mem::take(&mut self.skip_preparation_delta) {
            tracing::trace!(
                delta = delta.as_f32(),
                "Saved-scene wipe preparation delta excluded"
            );
            return false;
        }
        self.elapsed += delta;
        tracing::trace!(
            delta = delta.as_f32(),
            elapsed = self.elapsed.as_f32(),
            duration = self.duration.as_f32(),
            "Saved-scene wipe advanced"
        );
        self.elapsed >= self.duration
    }

    pub fn render(&self, pass: &mut RenderPass) {
        if let Some((mask, softness)) = &self.mask {
            // Native 63048..63094, flags0: old-scene weights at mask texels0/1.
            // Capture program32 clamps the mask weight before premultiplying
            // the source (FFMA.SAT), unlike the ordinary layer-mask shader.
            // The target is already drawn; use the shared capture curtain.
            let inverse = 1.0 / softness;
            let first = 1.0 - self.elapsed.as_f32() / self.duration.as_f32() * (inverse + 1.0);
            let vertices = [Vec2::ZERO, Vec2::X, Vec2::Y, Vec2::ONE].map(|uv| MaskVertex {
                position: uv * vec2(1920.0, 1080.0),
                texture_position: uv,
                mask_position: uv,
            });
            pass.run(
                RenderRequestBuilder::new()
                    .color_blend_type(ColorBlendType::LayerPremultiplied1)
                    .build(
                        RenderProgramWithArguments::ScreenCurtain {
                            vertices: VertexSource::VertexData {
                                vertices: &vertices,
                            },
                            texture: self.source.as_texture_source(),
                            mask: mask.texture.as_source(),
                            transform: crate::render::top_left_projection_matrix(),
                            minmax: vec2(first, first + inverse),
                        },
                        DrawPrimitive::TrianglesStrip,
                    ),
            );
            return;
        }
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
