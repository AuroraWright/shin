use std::{fmt::Debug, sync::Arc};

use glam::vec2;
use shin_core::{
    time::Ticks,
    vm::command::types::{MaskFlags, MaskParam},
};
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, texture::TextureSource, vertices::MaskVertex},
};

use crate::{
    asset::mask::MaskTexture,
    render::{VIRTUAL_CANVAS_SIZE_VEC, top_left_projection_matrix},
    update::{AdvUpdatable, AdvUpdateContext},
    wiper::timed::{TimedWiper, TimedWiperWrapper},
};

#[derive(Clone)]
pub struct MaskWiperImpl {
    mask: Arc<MaskTexture>,
    param2: MaskParam,
    flags: MaskFlags,
}

impl Debug for MaskWiperImpl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MaskWiperImpl")
            .field("mask", &self.mask.label)
            .field("param2", &self.param2)
            .field("flags", &self.flags)
            .finish()
    }
}

impl AdvUpdatable for MaskWiperImpl {
    fn update(&mut self, _context: &AdvUpdateContext) {}
}

// Native a1854..a1890 computes old-scene weights at mask texels0 and1.
// Inversion changes the direction of the wipe while preserving its endpoints.
fn mask_weights(param2: MaskParam, flags: MaskFlags, progress: f32) -> glam::Vec2 {
    let inverse = 1.0 / param2.0;
    let span = inverse + 1.0;
    if flags.contains(MaskFlags::INVERT) {
        let first = span * (1.0 - progress);
        vec2(first, first - inverse)
    } else {
        let first = 1.0 - progress * span;
        vec2(first, first + inverse)
    }
}

impl TimedWiper for MaskWiperImpl {
    #[cfg(test)]
    fn reference_set_batch_words(&mut self, words: &[u32]) {
        self.param2 = MaskParam(f32::from_bits(words[3]));
        self.flags = MaskFlags::from_bits_retain(words[4] as i32);
    }

    fn batch_parameters_match(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.mask, &other.mask)
            && self.param2.0 == other.param2.0
            && self.flags.bits() == other.flags.bits()
    }
    fn render(
        &self,
        pass: &mut RenderPass,
        render_request_builder: RenderRequestBuilder,
        texture_target: TextureSource,
        texture_source: TextureSource,
        progress: f32,
    ) {
        let minmax = mask_weights(self.param2, self.flags, progress);
        let mask_size = self.mask.texture.size_vec();
        let transform = top_left_projection_matrix();
        let [mut x1, mut y1] = [0.0, 0.0];
        // Native a18dc tests bit3 here; MASKLOAD independently uses bit4.
        let [mut x2, mut y2] = if self.flags.contains(MaskFlags::WIPE_NATIVE_SIZE) {
            (VIRTUAL_CANVAS_SIZE_VEC / mask_size).to_array()
        } else {
            [1.0, 1.0]
        };
        if self.flags.contains(MaskFlags::FLIP_X) {
            std::mem::swap(&mut x1, &mut x2);
        }
        if self.flags.contains(MaskFlags::FLIP_Y) {
            std::mem::swap(&mut y1, &mut y2);
        }

        let vertices = &[
            MaskVertex {
                position: vec2(0.0, 0.0),
                texture_position: vec2(0.0, 0.0),
                mask_position: vec2(x1, y1),
            },
            MaskVertex {
                position: vec2(VIRTUAL_CANVAS_SIZE_VEC.x, 0.0),
                texture_position: vec2(1.0, 0.0),
                mask_position: vec2(x2, y1),
            },
            MaskVertex {
                position: vec2(0.0, VIRTUAL_CANVAS_SIZE_VEC.y),
                texture_position: vec2(0.0, 1.0),
                mask_position: vec2(x1, y2),
            },
            MaskVertex {
                position: vec2(VIRTUAL_CANVAS_SIZE_VEC.x, VIRTUAL_CANVAS_SIZE_VEC.y),
                texture_position: vec2(1.0, 1.0),
                mask_position: vec2(x2, y2),
            },
        ];

        pass.run(
            render_request_builder
                .color_blend_type(ColorBlendType::Opaque)
                .build(
                    RenderProgramWithArguments::WiperMask {
                        vertices: VertexSource::VertexData { vertices },
                        texture_source,
                        texture_target,
                        texture_mask: self.mask.texture.as_source(),
                        transform,
                        minmax,
                    },
                    DrawPrimitive::TrianglesStrip,
                ),
        );
    }
}

pub type MaskWiper = TimedWiperWrapper<MaskWiperImpl>;

impl MaskWiper {
    pub fn new(
        duration: Ticks,
        mask: Arc<MaskTexture>,
        param2: MaskParam,
        flags: MaskFlags,
    ) -> Self {
        Self::from_inner(
            MaskWiperImpl {
                mask,
                param2,
                flags,
            },
            duration,
        )
    }
}

#[cfg(test)]
mod tests;
