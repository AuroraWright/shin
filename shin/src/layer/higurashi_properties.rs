//! Older 65-property owner and four amplitude-driven oscillator clocks.
//! Seeds are supplied explicitly until the older construction seed service is
//! connected; this owner does not consume Umineko's global random stream.
use super::{
    render_params::{
        DrawableParams,
        higurashi::{HigurashiTransform, PROPERTY_COUNT},
    },
    wobbler::prng::first_mt_sample,
};
use glam::{Vec4, vec4};
use shin_core::{primitives::color::FloatColor4, time::Ticks};
use shin_render::{LayerBlendType, LayerFragmentShader};

mod tween;
pub use tween::HigurashiTweener;

/// Updated native defaults at14f3bc, addressed by relocated global1cc788.
pub const INITIAL_VALUES: [i32; PROPERTY_COUNT] = [
    0, 0, 1000, 0, 0, 1000, 1000, 1000, 1000, 1000, 0, 0, 1000, 1000, 0, 0, 0, 0, 0, 1, 0, 0, 0,
    1000, 1000, 1000, 1000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1000, 0, 0, 0, 0, 0, 1280, 720,
    0, 0, 1, 1000, 0, 1000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

/// Native modff retains negative zero for negative integral inputs.
fn fractional(value: f32) -> f32 {
    value.fract().copysign(value)
}

#[derive(Debug, Clone, Copy)]
pub struct HigurashiOscillator {
    pub seed: u32,
    /// Native stores a wrapped float phase, with its integer part used by
    /// random and square modes. Waveform changes do not reset this clock.
    pub phase: f32,
}

impl HigurashiOscillator {
    pub fn advance(&mut self, amplitude: f32, period: f32, delta: f32) {
        if amplitude == 0.0 {
            self.phase = 0.0;
            return;
        }
        if period == 0.0 {
            return;
        }
        let phase = self.phase + (1.0 / period) * delta;
        let fraction = fractional(phase);
        let fraction = if fraction < 0.0 {
            fraction + 1.0
        } else {
            fraction
        };
        let cycles = fractional(phase.trunc() * 0.001);
        let cycles = if cycles < 0.0 { cycles + 1.0 } else { cycles };
        self.phase = fraction + cycles * 1000.0;
    }

    pub fn sample(&self, mode: f32) -> f32 {
        let t = fractional(self.phase);
        match mode as i32 {
            0 => {
                let cycle = self.phase as i32;
                let seed = if self.seed == 0 { 123456789 } else { self.seed };
                let sample = |cycle: i32| {
                    let cycle = cycle % 1000;
                    if cycle == 0 {
                        0.0
                    } else {
                        first_mt_sample(seed.wrapping_add(cycle as u32))
                    }
                };
                let from = sample(cycle);
                let to = sample(cycle.wrapping_add(1));
                from + t * (to - from)
            }
            1 => {
                if t < 0.25 {
                    t * 4.0
                } else if t < 0.75 {
                    (0.5 - t) * 4.0
                } else {
                    (1.0 - t) * -4.0
                }
            }
            2 => {
                let angle = t * std::f32::consts::PI;
                (angle + angle).sin()
            }
            3 => (((self.phase as i32 & 1) * 2) - 1) as f32,
            4 => (t * std::f32::consts::PI).sin(),
            5 => t,
            _ => 0.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct HigurashiProperties {
    values: [HigurashiTweener; PROPERTY_COUNT],
    /// X, Y, rotation, alpha, in older native order.
    pub oscillators: [HigurashiOscillator; 4],
}

impl HigurashiProperties {
    pub fn from_seeds(seeds: [u32; 4]) -> Self {
        Self {
            values: std::array::from_fn(|i| HigurashiTweener::new(INITIAL_VALUES[i] as f32)),
            oscillators: seeds.map(|seed| HigurashiOscillator { seed, phase: 0.0 }),
        }
    }
    pub fn values(&self) -> [f32; PROPERTY_COUNT] {
        std::array::from_fn(|i| self.values[i].value())
    }
    pub fn tweener(&self, index: usize) -> Option<&HigurashiTweener> {
        self.values.get(index)
    }
    pub fn tweener_mut(&mut self, index: usize) -> Option<&mut HigurashiTweener> {
        self.values.get_mut(index)
    }
    /// Older native8d89c flags: low6 easing, bit6 speed, bit7 relative,
    /// bit8 replace from current, bit9 protected from fast-forward.
    /// Relative replacement resolves against the old queue's final target
    /// before clearing it. Upper flags have no owner-level meaning here.
    pub fn control(
        &mut self,
        index: usize,
        value: f32,
        time: Ticks,
        flags: u32,
    ) -> anyhow::Result<()> {
        let tween = self
            .values
            .get_mut(index)
            .ok_or_else(|| anyhow::anyhow!("Higurashi property {index} outside0..65"))?;
        tween.control(value, time, flags)
    }
    pub fn fast_forward(&mut self) {
        for tween in &mut self.values {
            tween.fast_forward_unless_prohibited();
        }
    }
    pub fn reset(&mut self) {
        for (tween, initial) in self.values.iter_mut().zip(INITIAL_VALUES) {
            tween.fast_forward_to(initial as f32);
        }
        for oscillator in &mut self.oscillators {
            oscillator.phase = 0.0;
        }
    }
    pub fn advance(&mut self, delta: Ticks) {
        for value in &mut self.values {
            value.update(delta);
        }
        let p = self.values();
        for (i, oscillator) in self.oscillators.iter_mut().enumerate() {
            oscillator.advance(p[28 + i * 3], p[29 + i * 3], delta.as_f32());
        }
    }
    pub fn samples(&self) -> [f32; 4] {
        std::array::from_fn(|i| self.oscillators[i].sample(self.values[27 + i * 3].value()))
    }
    pub fn local_transform(&self) -> HigurashiTransform {
        let s = self.samples();
        HigurashiTransform::from_properties(&self.values(), [s[0], s[1], s[2]])
    }
    pub fn composed_transform(&self, parent: &HigurashiTransform) -> HigurashiTransform {
        let mut transform = self.local_transform();
        transform.compose_with(parent, HigurashiTransform::compose_flags(&self.values()));
        transform
    }
    pub fn drawable_params(&self) -> DrawableParams {
        let p = self.values();
        let color = vec4(
            p[6] * 0.0005,
            p[7] * 0.0005,
            p[8] * 0.0005,
            p[9] * 0.001 + self.oscillators[3].sample(p[36]) * (p[37] * 0.001),
        )
        .clamp(Vec4::ZERO, Vec4::ONE)
            * vec4(2.0, 2.0, 2.0, 1.0);
        DrawableParams {
            color_multiplier: FloatColor4::from_vec4(color),
            blend_type: match p[20] as i32 {
                1 => LayerBlendType::Type2,
                2 => LayerBlendType::Type3,
                _ => LayerBlendType::Type1,
            },
            fragment_shader: match p[21] as i32 {
                1 => LayerFragmentShader::Mono,
                2 => LayerFragmentShader::Fill,
                3 => LayerFragmentShader::Fill2,
                4 => LayerFragmentShader::Negative,
                5 => LayerFragmentShader::Gamma,
                _ => LayerFragmentShader::Default,
            },
            shader_param: vec4(p[23], p[24], p[25], p[26]) * 0.001,
        }
    }
    pub fn is_visible(&self) -> bool {
        let p = self.values();
        p[19] as i32 != 0 && p[12] as i32 != 0 && p[13] as i32 != 0 && p[9] as i32 > 0
    }
}

#[cfg(test)]
mod tests;
