//! Gerokasu2 Rain A4698/A4A10/A4C70 and FocusLine 5D7C0/5DA90/5DE20.
//! Shared renderers and particle/ray records stay reusable; only numeric policy
//! changes. Nintendo SDK trigonometric coefficients are absent from the supplied
//! main image, so rotation uses the existing host geometry contract.

use crate::layer::{
    render_params::multiply_transforms,
    user::{FocusRay, RainParticle},
};
use glam::{Mat4, Vec2, Vec3};
use shin_core::time::{Easing, Ticks, Tween, Tweener};

fn random(seed: &mut u32) -> f32 {
    let quotient = *seed / 44_488;
    let remainder = *seed % 44_488;
    let value = i64::from(remainder) * 48_271 - i64::from(quotient) * 3_399;
    *seed = if value < 0 {
        (value + 2_147_483_647) as u32
    } else {
        value as u32
    };
    (*seed - 1) as f32 * f32::from_bits(0x30000000)
}

#[derive(Debug, Clone)]
pub(crate) struct GerRainParticles {
    pub mode: usize,
    pub min_distance: i32,
    pub max_distance: i32,
    seed: u32,
    emission: f32,
    pub particles: Vec<RainParticle>,
    pub expired: u32,
    pub needs_warm_up: bool,
}
impl GerRainParticles {
    pub(crate) fn new(mode: i32, first: i32, second: i32, seed: u32) -> Self {
        Self {
            mode: mode.clamp(0, 3) as usize,
            min_distance: first.min(second).clamp(100, 10_000),
            max_distance: first.max(second).clamp(100, 10_000),
            seed: (seed % 2_147_483_647).max(1),
            emission: 1.0,
            particles: Vec::new(),
            expired: 0,
            needs_warm_up: true,
        }
    }
    #[cfg(test)]
    pub(crate) fn reference_seed(&self) -> u32 {
        self.seed
    }
    #[cfg(test)]
    pub(crate) fn reference_emission(&self) -> f32 {
        self.emission
    }
    pub(crate) fn warm_up(&mut self, intensity: f32, direction: f32, speed: f32) {
        if !self.needs_warm_up {
            return;
        }
        if self.expired == 0 {
            // Zero intensity can finish: a fresh owner has one pending emission,
            // and existing particles retain the velocity captured at birth.
            // Leave genuinely nonterminating controls pending for a later call.
            let can_spawn =
                speed > 0.0 && (intensity > 0.0 || (intensity == 0.0 && self.emission >= 1.0));
            let can_expire_existing = self.particles.iter().any(|p| p.velocity.y > 0.0);
            if !can_spawn && !can_expire_existing {
                return;
            }
        }
        while self.expired == 0 {
            self.update(1.0, intensity, direction, speed);
        }
        self.needs_warm_up = false;
    }
    pub(crate) fn update(&mut self, delta: f32, intensity: f32, direction: f32, speed: f32) {
        let rate = (intensity * 0.001) * (self.max_distance as f32 / 1000.0);
        self.emission = rate.mul_add(delta, self.emission);
        if self.emission >= 1.0 {
            let attempts = self.emission.trunc() as i32;
            self.emission = self.emission.fract();
            for _ in 0..attempts {
                // A4E48 skips the decrement at A4DB4 on rejection: emission
                // counts accepted particles, rather than attempted samples.
                while !self.spawn(direction, speed) {}
            }
        }
        // Native consumes delta only for emission, then moves particles once.
        self.particles.retain_mut(|particle| {
            particle.position.y += particle.velocity.y;
            if particle.position.y >= 1080.0 + particle.radius {
                self.expired = self.expired.wrapping_add(1);
                return false;
            }
            let mut x = particle.position.x + particle.velocity.x;
            while x < 0.0 {
                x += 1920.0;
            }
            while x >= 1920.0 {
                x -= 1920.0;
            }
            particle.position.x = x;
            true
        });
    }
    fn spawn(&mut self, direction: f32, speed: f32) -> bool {
        let depth = random(&mut self.seed);
        let distance = depth.mul_add(
            (self.max_distance - self.min_distance) as f32,
            self.min_distance as f32,
        );
        let horizontal = random(&mut self.seed) * self.max_distance as f32;
        if horizontal > distance {
            return false;
        }
        let projection = 1000.0 / distance;
        let direction = (direction * 0.001).clamp(-1.0, 1.0) * f32::from_bits(0x3f860a8a);
        let jitter = random(&mut self.seed).mul_add(2.0, -1.0) * 0.1;
        let direction = direction.mul_add(jitter, direction);
        let speed = speed * 0.001;
        let scale = speed * projection;
        let mut shape = multiply_transforms(
            Mat4::from_rotation_z(direction),
            Mat4::from_scale(Vec3::new(scale, scale, 1.0)),
        );
        let radius = (f32::from_bits(match self.mode {
            0 => 0x43160da7,
            1 => 0x4229b4a4,
            _ => 0x41a9b4a4,
        }) * projection)
            * speed;
        let fall = match self.mode {
            0 => 400.0,
            1 => 8.0,
            _ => 5.0,
        };
        // Native 7F144: one multiply, two ordered FMAs, then translation add.
        let cols = shape.to_cols_array();
        let movement = |row: usize| {
            let value = cols[row] * 0.0;
            let value = cols[4 + row].mul_add(fall, value);
            cols[12 + row] + cols[8 + row].mul_add(0.0, value)
        };
        let x = movement(0);
        let y = movement(1);
        let velocity = Vec2::new(x * projection, y * projection);
        let position = Vec2::new(
            (-radius).mul_add(x / y, (horizontal / distance) * 1920.0),
            0.0 - radius,
        );
        if self.mode == 1 {
            let turn = random(&mut self.seed).mul_add(2.0, -1.0) * f32::from_bits(0x40490fd0);
            shape = multiply_transforms(shape, Mat4::from_rotation_z(turn));
        }
        self.particles.push(RainParticle {
            shape,
            position,
            depth,
            velocity,
            radius,
        });
        true
    }
}

#[derive(Debug, Clone)]
pub(crate) struct GerFocusRays {
    seed: u32,
    pub rays: Vec<FocusRay>,
}
impl GerFocusRays {
    pub(crate) fn new(seed: u32) -> Self {
        let mut result = Self {
            seed: (seed % 2_147_483_647).max(1),
            rays: Vec::new(),
        };
        result.refill();
        for ray in &mut result.rays {
            let ticks = random(&mut result.seed).mul_add(30.0, 0.0).floor();
            ray.alpha.update(Ticks::from_f32(ticks));
        }
        result
    }
    #[cfg(test)]
    pub(crate) fn reference_seed(&self) -> u32 {
        self.seed
    }
    fn refill(&mut self) {
        while self.rays.len() < 40 {
            let angle = random(&mut self.seed)
                .mul_add(f32::from_bits(0x40c90fd0), -f32::from_bits(0x40490fd0));
            let duration = random(&mut self.seed).mul_add(20.0, 10.0);
            let mut alpha = Tweener::new_gerokasu2(1.0);
            alpha.enqueue(
                0.0,
                Tween {
                    duration: Ticks::from_f32(duration),
                    easing: Easing::SineOut,
                },
            );
            self.rays.push(FocusRay { angle, alpha });
        }
    }
    pub(crate) fn update(&mut self, delta: Ticks) {
        self.rays.retain(|ray| !ray.alpha.is_idle());
        self.refill();
        for ray in &mut self.rays {
            ray.alpha.update(delta);
        }
    }
}

#[cfg(test)]
mod tests;
