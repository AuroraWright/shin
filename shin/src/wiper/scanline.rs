use crate::{
    update::{AdvUpdatable, AdvUpdateContext},
    wiper::timed::{TimedWiper, TimedWiperWrapper},
};
use shin_core::time::Ticks;
use shin_render::{
    ColorBlendType, RenderRequestBuilder, quad_vertices::QuadVertices, render_pass::RenderPass,
    shaders::types::texture::TextureSource, shin_orthographic_projection_matrix,
};

#[derive(Debug, Clone)]
pub struct ScanlineWiperImpl {
    exponent: f32,
}
fn shaped_progress(progress: f32, exponent: f32) -> f32 {
    if exponent < 0.0 {
        1.0 - (1.0 - progress).powf(-exponent)
    } else if exponent > 0.0 {
        progress.powf(exponent)
    } else {
        progress
    }
}
fn strip_offsets(progress: f32, exponent: f32) -> [f32; 2] {
    // Native dc8a8..dc8bc uses separate multiply/add/subtract instructions.
    let distance = shaped_progress(progress, exponent) * 1920.0;
    [distance + -1920.0, 1920.0 - distance]
}
fn strip_geometry(strip: u32, offset: f32) -> ([f32; 4], [f32; 4]) {
    let y = (strip * 2) as f32;
    let v = y / 1080.0;
    (
        [offset, y, offset + 1920.0, y + 2.0],
        // Native dc8f8 adds this literal to the top UV rather than dividing
        // the lower vertex's Y. Preserve the rounding difference.
        [0.0, v, 1.0, v + f32::from_bits(0x3af2b9d6)],
    )
}
impl AdvUpdatable for ScanlineWiperImpl {
    fn update(&mut self, _: &AdvUpdateContext) {}
}
impl TimedWiper for ScanlineWiperImpl {
    #[cfg(test)]
    fn reference_set_batch_words(&mut self, words: &[u32]) {
        self.exponent = f32::from_bits(words[0]);
    }

    fn batch_parameters_match(&self, other: &Self) -> bool {
        self.exponent == other.exponent
    }
    fn batch_uses_timer(&self) -> bool {
        // Native dc6b0 checks only the exponent, without calling11b0d0.
        false
    }
    fn render(
        &self,
        pass: &mut RenderPass,
        request: RenderRequestBuilder,
        target: TextureSource,
        source: TextureSource,
        progress: f32,
    ) {
        let transform = shin_orthographic_projection_matrix(0.0, 1920.0, 1080.0, 0.0, -1.0, 1.0);
        let request = request.color_blend_type(ColorBlendType::Opaque);
        QuadVertices::new()
            .with_box(0.0, 0.0, 1920.0, 1080.0)
            .render_sprite(pass, request, source, transform);
        let offsets = strip_offsets(progress, self.exponent);
        for strip in 0..540 {
            let (position, uv) = strip_geometry(strip, offsets[strip as usize & 1]);
            QuadVertices::new()
                .with_box(position[0], position[1], position[2], position[3])
                .with_tex_box(uv[0], uv[1], uv[2], uv[3])
                .render_sprite(pass, request, target, transform);
        }
    }
}
pub type ScanlineWiper = TimedWiperWrapper<ScanlineWiperImpl>;
impl ScanlineWiper {
    pub fn new(duration: Ticks, exponent: i32) -> Self {
        Self::from_inner(
            ScanlineWiperImpl {
                exponent: exponent as f32,
            },
            duration,
        )
    }
}
#[cfg(test)]
mod tests;
