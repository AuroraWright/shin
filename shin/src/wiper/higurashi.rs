//! Updated Higurashi default wipe. Its clock is distinct from WiperBase in
//! Umineko: unsigned constructor duration, no frame marker, and an update on
//! an already completed clock leaves it unchanged (native ec994/ec9e4).
use shin_core::time::Ticks;
#[cfg(test)]
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    quad_vertices::build_quad_vertices,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, texture::TextureSource, vertices::PosTexVertex},
};

pub(crate) mod derived;

#[derive(Debug, Clone)]
pub struct HigurashiDefaultWiper {
    current: f32,
    total: f32,
}

impl HigurashiDefaultWiper {
    pub fn new(duration: u32) -> Self {
        if duration == 0 {
            Self {
                current: 1.0,
                total: 1.0,
            }
        } else {
            Self {
                current: 0.0,
                total: duration as f32,
            }
        }
    }
    /// One invocation of native ec9e4. The scene scheduler controls whether
    /// this callback runs; the clock itself has no animation/frame gate.
    pub fn advance(&mut self, delta: Ticks) -> bool {
        if self.current < self.total {
            self.current += delta.as_f32();
            if self.current > self.total {
                self.current = self.total;
            }
        }
        self.is_running()
    }
    pub fn is_running(&self) -> bool {
        self.current < self.total
    }
    pub fn fast_forward(&mut self) {
        self.current = self.total;
    }
    pub fn progress(&self) -> f32 {
        self.current / self.total
    }
    #[cfg(test)]
    #[allow(dead_code, reason = "Retained standalone native default-wipe rendering seam for GPU comparisons.")]
    pub fn render(
        &self,
        pass: &mut RenderPass,
        builder: RenderRequestBuilder,
        target: TextureSource,
        source: TextureSource,
    ) {
        let vertices = build_quad_vertices(|v| PosTexVertex {
            position: v,
            texture_position: v,
        });
        pass.run(builder.color_blend_type(ColorBlendType::Opaque).build(
            RenderProgramWithArguments::WiperHigurashiDefault {
                vertices: VertexSource::VertexData {
                    vertices: &vertices,
                },
                texture_target: target,
                texture_source: source,
                transform: crate::render::normalized_projection_matrix(),
                alpha: self.progress(),
            },
            DrawPrimitive::TrianglesStrip,
        ));
    }
    #[cfg(test)]
    fn time_words(&self) -> [u32; 2] {
        [self.current.to_bits(), self.total.to_bits()]
    }
}

#[cfg(test)]
mod derived_integration_tests;
#[cfg(test)]
mod derived_native_tests;
#[cfg(test)]
mod tests;
