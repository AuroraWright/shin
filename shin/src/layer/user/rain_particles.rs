//! CPU particle behavior recovered from the supplied executable's RainLayer:
//! constructor 0xc9a40, update 0xc9e90, spawn 0xca150, warm-up 0xca704.

use glam::{Mat4, Vec2, Vec3};

#[derive(Debug, Clone)]
pub struct Particle {
    pub shape: Mat4,
    pub position: Vec2,
    pub depth: f32,
    pub velocity: Vec2,
    pub radius: f32,
}

#[cfg(test)]
impl RainParticles {
    pub(super) fn reference_owner_prepare(&mut self, count: usize, identity: usize) {
        self.emission = 0.5;
        self.expired = 7 + identity as u32;
        self.needs_warm_up = identity % 2 != 0;
        self.particles = Vec::with_capacity(count + 3);
        for i in 0..count {
            let columns = std::array::from_fn(|j| ((i % 7) * 16 + j + 1) as f32 / 16.0);
            self.particles.push(Particle {
                shape: Mat4::from_cols_array(&columns),
                position: Vec2::new(
                    if i % 3 == 0 { -20.0 } else { 1918.0 },
                    if i % 5 == 0 { 1080.0 } else { 100.0 + i as f32 },
                ),
                depth: 0.25 + (i % 3) as f32 * 0.25,
                velocity: Vec2::new(
                    if i % 3 == 0 { -3.0 } else { 7.0 },
                    if i % 5 == 0 { 40.0 } else { 2.0 },
                ),
                radius: 5.0 + (i % 4) as f32,
            });
        }
    }
    pub(crate) fn reference_owner_words(&self) -> (Option<usize>, Vec<String>) {
        let mut hash = 0xcbf29ce484222325u64;
        for p in &self.particles {
            for v in p.shape.to_cols_array().into_iter().chain([
                p.position.x,
                p.position.y,
                p.depth,
                p.velocity.x,
                p.velocity.y,
                p.radius,
            ]) {
                for byte in v.to_bits().to_le_bytes() {
                    hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
                }
            }
        }
        let radius = match self.mode {
            0 => 150.05333f32,
            1 => 42.426407,
            _ => 21.213203,
        };
        (
            if self.particles.is_empty() {
                None
            } else {
                Some(self.particles.as_ptr() as usize)
            },
            vec![
                format!("{:08x}", self.seed),
                self.mode.to_string(),
                self.min_distance.to_string(),
                self.max_distance.to_string(),
                format!("{:08x}", radius.to_bits()),
                self.expired.to_string(),
                usize::from(self.needs_warm_up).to_string(),
                format!("{:08x}", self.emission.to_bits()),
                self.particles.len().to_string(),
                format!("{hash:016x}"),
            ],
        )
    }
    pub(super) fn reference_owner_mutate(&mut self) {
        self.seed = self.seed.wrapping_add(1);
        self.expired = 123;
        self.needs_warm_up = false;
        self.emission = 0.25;
        if let Some(p) = self.particles.first_mut() {
            p.position.x = 999.0;
        }
    }
}

#[derive(Debug, Clone)]
pub struct RainParticles {
    pub mode: usize,
    pub min_distance: i32,
    pub max_distance: i32,
    seed: u32,
    emission: f32,
    rain_fall: f32,
    retry_rejected: bool,
    pub particles: Vec<Particle>,
    pub expired: u32,
    pub needs_warm_up: bool,
}

impl RainParticles {
    #[cfg(test)]
    pub(crate) fn reference_seed(&self) -> u32 {
        self.seed
    }
    pub fn new(mode: i32, first_distance: i32, second_distance: i32, seed: u32) -> Self {
        let min_distance = first_distance.min(second_distance).clamp(100, 10_000);
        let max_distance = first_distance.max(second_distance).clamp(100, 10_000);
        Self {
            mode: mode.clamp(0, 3) as usize,
            min_distance,
            max_distance,
            seed: (seed % 2_147_483_647).max(1),
            emission: 1.0,
            rain_fall: 400.0,
            retry_rejected: false,
            particles: Vec::new(),
            expired: 0,
            needs_warm_up: true,
        }
    }

    /// The older Higurashi a1848 emitter counts accepted particles rather than
    /// attempts, and mode0 uses a1f14's 300.0 falling distance. Keep the later
    /// Umineko constructor and its attempt-counting behavior unchanged.
    pub(crate) fn new_higurashi(
        mode: i32,
        first_distance: i32,
        second_distance: i32,
        seed: u32,
    ) -> Self {
        Self {
            rain_fall: 300.0,
            retry_rejected: true,
            ..Self::new(mode, first_distance, second_distance, seed)
        }
    }

    fn random(&mut self) -> f32 {
        // Native Schrage form: q44488,r3399,m2147483647,a48271.
        let quotient = self.seed / 44_488;
        let remainder = self.seed % 44_488;
        let value = i64::from(remainder) * 48_271 - i64::from(quotient) * 3_399;
        self.seed = if value < 0 {
            (value + 2_147_483_647) as u32
        } else {
            value as u32
        };
        (self.seed - 1) as f32 * f32::from_bits(0x30000000)
    }

    pub fn warm_up(&mut self, intensity: f32, direction: f32, speed: f32) {
        if !self.needs_warm_up {
            return;
        }
        // Native waits for the first expired particle. Nonpositive controls
        // cannot guarantee that event; leave simulation runnable in that case.
        if intensity > 0.0 && speed > 0.0 {
            while self.expired == 0 {
                self.update(1.0, intensity, direction, speed);
            }
        }
        self.needs_warm_up = false;
    }

    pub fn update(&mut self, delta: f32, intensity: f32, direction: f32, speed: f32) {
        self.emission += ((intensity * 0.001) * (self.max_distance as f32 / 1000.0)) * delta;
        if self.emission >= 1.0 {
            let attempts = self.emission.trunc() as i32;
            self.emission = self.emission.fract();
            for _ in 0..attempts {
                while !self.spawn(direction, speed) && self.retry_rejected {}
            }
        }
        // The native update uses delta for emission, but advances each existing
        // particle once by its stored velocity, including a zero-delta update.
        self.particles.retain_mut(|particle| {
            particle.position.y += particle.velocity.y;
            if particle.position.y >= 1080.0 + particle.radius {
                self.expired = self.expired.wrapping_add(1);
                return false;
            }
            particle.position.x = (particle.position.x + particle.velocity.x).rem_euclid(1920.0);
            true
        });
    }

    fn spawn(&mut self, direction: f32, speed: f32) -> bool {
        let depth = self.random();
        let distance =
            depth * (self.max_distance - self.min_distance) as f32 + self.min_distance as f32;
        let horizontal = self.random() * self.max_distance as f32;
        if horizontal > distance {
            return false;
        }
        let projection = 1000.0 / distance;
        let direction = ((direction * 0.001).clamp(-1.0, 1.0) * 1.0471966)
            * (1.0 + (self.random() * 2.0 - 1.0) * 0.1);
        let speed = speed * 0.001;
        let scale = speed * projection;
        let rotation = Mat4::from_rotation_z(direction);
        let mut shape = rotation * Mat4::from_scale(Vec3::new(scale, scale, 1.0));
        let radius = (match self.mode {
            0 => 150.05333,
            1 => 42.426407,
            _ => 21.213203,
        } * projection)
            * speed;
        let fall = match self.mode {
            0 => self.rain_fall,
            1 => 8.0,
            _ => 5.0,
        };
        let movement = shape.transform_vector3(Vec3::new(0.0, fall, 0.0));
        let velocity = Vec2::new(movement.x * projection, movement.y * projection);
        let position = Vec2::new(
            (horizontal / distance) * 1920.0 - (velocity.x / velocity.y) * radius,
            -radius,
        );
        if self.mode == 1 {
            let turn = (self.random() * 2.0 - 1.0) * 3.14159;
            shape *= Mat4::from_rotation_z(turn);
        }
        self.particles.push(Particle {
            shape,
            position,
            depth,
            velocity,
            radius,
        });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_random_sequence_and_rejected_spawns_preserve_rng_order() {
        let mut rain = RainParticles::new(0, 100, 1500, 1);
        for expected in [
            48_271,
            182_605_794,
            1_291_394_886,
            1_914_720_637,
            2_078_669_041,
        ] {
            rain.random();
            assert_eq!(rain.seed, expected);
        }
        let mut rain = RainParticles::new(0, 100, 1500, 1);
        rain.spawn(100.0, 1000.0);
        assert!(rain.particles.is_empty());
        assert_eq!(rain.seed, 182_605_794);
        rain.spawn(100.0, 1000.0);
        assert!(rain.particles.is_empty());
        assert_eq!(rain.seed, 1_914_720_637);
        rain.spawn(100.0, 1000.0);
        assert_eq!(rain.seed, 1_105_902_161);
        let particle = &rain.particles[0];
        assert!((particle.depth - 0.9679557).abs() < 0.000001);
        assert!((particle.position.x - 386.30383).abs() < 0.001);
        assert!((particle.radius - 103.11967).abs() < 0.001);
        assert!((particle.velocity.x + 19.80523).abs() < 0.001);
        assert!((particle.velocity.y - 187.86751).abs() < 0.001);
    }

    #[test]
    fn ranges_modes_warm_up_wrapping_and_emission_match_native_rules() {
        let rain = RainParticles::new(100, 2000, 1000, 0);
        assert_eq!(
            (rain.mode, rain.min_distance, rain.max_distance, rain.seed),
            (3, 1000, 2000, 1)
        );
        let rain = RainParticles::new(-1, 20, 20_000, 1);
        assert_eq!(
            (rain.mode, rain.min_distance, rain.max_distance),
            (0, 100, 10_000)
        );
        let mut rain = RainParticles::new(0, 100, 1500, 1);
        rain.warm_up(3000.0, 100.0, 1000.0);
        assert!(!rain.needs_warm_up);
        assert!(rain.expired > 0);
        assert!(!rain.particles.is_empty());
        for particle in &rain.particles {
            assert!((0.0..1920.0).contains(&particle.position.x));
        }
        let previous = rain.particles.clone();
        let seed = rain.seed;
        rain.update(0.0, 0.0, 0.0, 0.0);
        assert_eq!(rain.seed, seed);
        for particle in &rain.particles {
            let old = previous.iter().find(|p| p.depth == particle.depth).unwrap();
            assert!((particle.position.y - old.position.y - old.velocity.y).abs() < 0.001);
        }
        let mut clone = rain.clone();
        rain.update(1.0, 3000.0, -150.0, 1000.0);
        clone.update(1.0, 3000.0, -150.0, 1000.0);
        assert_eq!(rain.seed, clone.seed);
        assert_eq!(rain.particles.len(), clone.particles.len());
        for (a, b) in rain.particles.iter().zip(&clone.particles) {
            assert_eq!(a.position, b.position);
        }
    }
    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
    fn reference_rain_parameters_rng_and_wrapping_instructions() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let path = std::path::Path::new(&rom)
            .parent()
            .unwrap()
            .join("uncompressed_executable");
        let executable = std::fs::read(path).unwrap();
        for (address, expected) in [
            (0xc9ac4, 0x71000edfu32), // mode upper limit3
            (0xc9b00, 0x7101929f),    // distance lower bound100
            (0xc9b08, 0x5284e20e),    // distance upper bound10000
            (0xc9f18, 0x1e222020),    // y expiration comparison
            (0xca2a0, 0x72b791f6),    // RNG division multiplier high word
            (0xca2a4, 0x5295b917),    // RNG quotient divisor44488
            (0xca2ac, 0x5281a8f9),    // RNG remainder3399
            (0xca708, 0x34000148),    // first update warm-up gate
            (0xca74c, 0x1e380008),    // animation property integer conversion
            (0xcaa98, 0x54000104),    // left edge duplicated draw
            (0x1594c4, 0x30000000),   // RNG float multiplier2^-31
        ] {
            let actual =
                u32::from_le_bytes(executable[address + 256..address + 260].try_into().unwrap());
            assert_eq!(
                actual, expected,
                "Native instruction/constant at {address:x}"
            );
        }
    }
}
