//! Updated dddD4 seed normalization,de384 field and de9c0 sway transform.
use crate::layer::render_params::multiply_transforms;
use glam::{Mat4, vec3};
use shin_core::time::Ticks;

#[derive(Clone, Debug)]
pub(super) struct Particle {
    pub scale: f32,
    pub velocity: f32,
    pub amplitude: f32,
    pub period: f32,
    pub x: f32,
    pub y: f32,
    pub elapsed: f32,
}
pub(super) struct Particles {
    pub seed: u32,
    pub spawn: f32,
    pub items: Vec<Particle>,
    pub warmup_steps: u32,
    pub retired: usize,
}
impl Particles {
    pub fn cold(seed: u32) -> Self {
        let seed = seed % 0x7fffffff;
        Self {
            seed: if seed == 0 { 1 } else { seed },
            spawn: 60.0,
            items: Vec::new(),
            warmup_steps: 0,
            retired: 0,
        }
    }
    pub fn warm(seed: u32) -> Self {
        let mut value = Self::cold(seed);
        loop {
            value.warmup_steps += 1;
            if value.advance(Ticks::from_f32(1.0)) != 0 {
                return value;
            }
        }
    }
    fn unit(&mut self) -> f32 {
        self.seed = ((u64::from(self.seed) * 48271) % 0x7fffffff) as u32;
        (self.seed - 1) as f32 * f32::from_bits(0x30000000)
    }
    pub fn advance(&mut self, delta: Ticks) -> usize {
        let mut remaining = delta.as_f32();
        while remaining > 0.0 {
            let step = remaining.min(1.0);
            self.spawn += step;
            if self.spawn >= 60.0 {
                let scale = self.unit() * f32::from_bits(0x3fa66666) + f32::from_bits(0x3e4ccccd);
                let velocity = scale * (self.unit() * 0.5 + 1.5);
                let amplitude = scale * (self.unit() * 8.0 + 8.0);
                let period = self.unit() * 90.0 + 90.0;
                let x = self.unit() * 2000.0 - 40.0;
                self.items.push(Particle {
                    scale,
                    velocity,
                    amplitude,
                    period,
                    x,
                    y: 1960.0,
                    elapsed: 0.0,
                });
                self.spawn -= 60.0;
            }
            for particle in &mut self.items {
                // Native intentionally does not multiply velocity by step.
                particle.y -= particle.velocity;
                particle.elapsed += step;
            }
            remaining -= step;
        }
        let count = self.items.len();
        self.items.retain(|particle| particle.y >= -40.0);
        self.retired = count - self.items.len();
        self.retired
    }
}
impl Particle {
    pub fn matrix(&self, projection: Mat4) -> Mat4 {
        let angle = (self.elapsed / self.period) * f32::from_bits(0x40c90fd0);
        let x = self.amplitude * angle.sin() + self.x;
        let translation = Mat4::from_translation(vec3(x, self.y + 0.0, 0.0));
        let scale = Mat4::from_scale(vec3(self.scale, self.scale, 1.0));
        multiply_transforms(projection, multiply_transforms(translation, scale))
    }
    #[cfg(test)]
    pub fn words(&self) -> [u32; 7] {
        [
            self.scale,
            self.velocity,
            self.amplitude,
            self.period,
            self.x,
            self.y,
            self.elapsed,
        ]
        .map(f32::to_bits)
    }
}
