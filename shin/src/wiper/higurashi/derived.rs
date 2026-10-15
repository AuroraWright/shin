//! Older1..4 effects share rendering mathematics, but use ec994/ec9e4 clocks.
use super::HigurashiDefaultWiper;
use crate::asset::mask::MaskTexture;
mod packets;
use crate::layer::render_params::multiply_transforms;
use anyhow::{Context, Result};
use glam::{Mat4, Vec2, vec2, vec3};
pub(crate) use packets::Packet;
use packets::{orthographic, quad, turn_matrix};
use shin_core::{format::scenario::instruction_elements::UntypedNumberArray, time::Ticks};
use shin_render::{
    ColorBlendType, RenderRequestBuilder,
    quad_vertices::build_quad_vertices,
    render_pass::RenderPass,
    shaders::types::{texture::TextureSource, vertices::MaskVertex},
};
use std::sync::Arc;

enum Effect {
    Default,
    Mask {
        texture: Arc<MaskTexture>,
        strength: f32,
        flags: i32,
    },
    Scroll(i32, i32),
    Zoom(f32, f32),
    Turn(i32),
}

pub(crate) struct HigurashiWiper {
    clock: HigurashiDefaultWiper,
    effect: Effect,
}
impl HigurashiWiper {
    pub(crate) fn default(duration: u32) -> Self {
        Self {
            clock: HigurashiDefaultWiper::new(duration),
            effect: Effect::Default,
        }
    }
    pub(crate) fn new(
        kind: i32,
        duration: u32,
        params: UntypedNumberArray,
        mask: Option<Arc<MaskTexture>>,
    ) -> Result<Self> {
        let effect = match kind {
            1 => Effect::Mask {
                texture: mask.context("Higurashi Mask wipe requires its prepared asset")?,
                // Actual6a30 replaces integer0 by100;74870 takes fabs. There is
                // no later engine clamp to1000 or substitution of float1.
                strength: ((if params.1 == 0 { 100 } else { params.1 }) as f32 * (1.0 / 256.0))
                    .abs(),
                flags: params.2,
            },
            // Only scalar mathematics is reused; older geometry/projection
            // stages and the clock stay separate from Umineko's wrapper.
            2 => Effect::Scroll(params.0, params.1),
            3 => Effect::Zoom(params.0 as f32 * 0.001, params.1 as f32 * 0.001),
            4 => Effect::Turn(params.0),
            _ => Effect::Default,
        };
        Ok(Self {
            clock: HigurashiDefaultWiper::new(duration),
            effect,
        })
    }
    pub(crate) fn advance(&mut self, delta: Ticks) -> bool {
        self.clock.advance(delta)
    }
    pub(crate) fn fast_forward(&mut self) {
        self.clock.fast_forward();
    }
    pub(crate) fn is_running(&self) -> bool {
        self.clock.is_running()
    }
    pub(crate) fn progress(&self) -> f32 {
        self.clock.progress()
    }
    #[cfg(test)]
    pub(crate) fn reference_clock(&self) -> [u32; 2] {
        self.clock.time_words()
    }
    #[cfg(test)]
    pub(crate) fn reference_set_clock(&mut self, words: [u32; 2]) {
        self.clock.current = f32::from_bits(words[0]);
        self.clock.total = f32::from_bits(words[1]);
    }
    pub(crate) fn packets(&self) -> Vec<Packet> {
        let progress = self.progress();
        match &self.effect {
            Effect::Default => vec![Packet::Default {
                transform: orthographic(1., 1., false),
                vertices: build_quad_vertices(|uv| {
                    shin_render::shaders::types::vertices::PosTexVertex {
                        position: uv,
                        texture_position: uv,
                    }
                }),
                alpha: progress,
            }],
            Effect::Scroll(incoming, outgoing) => {
                let mut projection = orthographic(2., 2., true);
                // Raw outgoing74d34 signedzero X translation is normalized by
                // subsequent CPU matrix transport only in some classes.
                projection.w_axis.x = -0.;
                [
                    (
                        false,
                        super::super::scroll::outgoing_offset(*outgoing, progress),
                        255,
                        ColorBlendType::Opaque,
                    ),
                    (
                        true,
                        super::super::scroll::direction(*incoming) * (1. - progress),
                        (progress * 255.) as u8,
                        ColorBlendType::Layer1,
                    ),
                ]
                .into_iter()
                .map(|(incoming, offset, alpha, blend)| {
                    let mut vertices = quad(vec2(2., 2.), 1., alpha);
                    let offset = offset * 2.;
                    for v in &mut vertices {
                        v.position.x += offset.x;
                        v.position.y += offset.y;
                    }
                    Packet::Sprite {
                        transform: projection,
                        vertices,
                        incoming,
                        blend,
                    }
                })
                .collect()
            }
            Effect::Zoom(incoming, outgoing) => {
                let (incoming_scale, outgoing_scale) =
                    super::super::zoom::scales(*incoming, *outgoing, progress);
                let projection = orthographic(1920., 1080., true);
                [
                    (false, outgoing_scale, 255, ColorBlendType::Opaque),
                    (
                        true,
                        incoming_scale,
                        (progress * 255.) as u8,
                        ColorBlendType::Layer1,
                    ),
                ]
                .into_iter()
                .map(|(incoming, scale, alpha, blend)| {
                    // ecba4 multiplies native Scale then identity RotationZ
                    // into projection; preserve the ordered CPU FMA stages.
                    let local = multiply_transforms(
                        Mat4::IDENTITY,
                        Mat4::from_scale(vec3(scale, scale, 1.)),
                    );
                    let transform = multiply_transforms(projection, local);
                    Packet::Sprite {
                        transform,
                        vertices: quad(vec2(1920., 1080.), 1., alpha),
                        incoming,
                        blend,
                    }
                })
                .collect()
            }
            Effect::Turn(direction) => {
                let (angle, alpha) = super::super::turn::rotation_and_opacity(*direction, progress);
                vec![
                    Packet::Sprite {
                        transform: orthographic(1920., 1080., true),
                        vertices: quad(vec2(1920., 1080.), 0., 255),
                        incoming: *direction != 0,
                        blend: ColorBlendType::Opaque,
                    },
                    Packet::Sprite {
                        transform: turn_matrix(angle),
                        vertices: quad(vec2(1920., 1080.), 0., alpha),
                        incoming: *direction == 0,
                        blend: ColorBlendType::Layer1,
                    },
                ]
            }
            Effect::Mask {
                texture,
                strength,
                flags,
            } => {
                let inverse = 1. / strength;
                let span = inverse + 1.;
                // Older74b84/74c34 use invert/flipX/flipY bits0/1/2;
                // Umineko's shared MaskFlags puts inversion at bit2.
                let minmax = if flags & 1 != 0 {
                    let first = span * (1. - progress);
                    vec2(first, first - inverse)
                } else {
                    let first = 1. - progress * span;
                    vec2(first, first + inverse)
                };
                let mut start = Vec2::ZERO;
                let mut end = if flags & 8 != 0 {
                    vec2(1920., 1080.) / texture.texture.size_vec()
                } else {
                    Vec2::ONE
                };
                if flags & 2 != 0 {
                    std::mem::swap(&mut start.x, &mut end.x);
                }
                if flags & 4 != 0 {
                    std::mem::swap(&mut start.y, &mut end.y);
                }
                let vertices = build_quad_vertices(|uv| MaskVertex {
                    position: uv * vec2(1920., 1080.),
                    texture_position: uv,
                    mask_position: vec2(
                        if uv.x == 0. { start.x } else { end.x },
                        if uv.y == 0. { start.y } else { end.y },
                    ),
                });
                vec![Packet::Mask {
                    transform: orthographic(1920., 1080., false),
                    vertices,
                    minmax,
                }]
            }
        }
    }
    pub(crate) fn render(
        &self,
        pass: &mut RenderPass,
        builder: RenderRequestBuilder,
        incoming: TextureSource,
        outgoing: TextureSource,
    ) {
        let mask = match &self.effect {
            Effect::Mask { texture, .. } => Some(texture.texture.as_source()),
            _ => None,
        };
        for packet in self.packets() {
            packet.render(pass, builder, incoming, outgoing, mask);
        }
    }
}
