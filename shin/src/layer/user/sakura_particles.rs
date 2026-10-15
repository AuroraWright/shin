//! SakuraAnimationLayer ctor25550, update25760 and render25f10.
//! The supplied /effects.txa lacks the required `Sakura` texture. Keep the
//! recovered simulation and sprite descriptors independent of GPU assets.
use glam::{Mat4, Vec2, Vec3};

const PI: f32 = f32::from_bits(0x40490fd0);
const PERIODS: [[f32; 2]; 6] = [
    [128.0, 256.0],
    [128.0, 256.0],
    [128.0, 256.0],
    [128.0, 256.0],
    [150.0, 250.0],
    [150.0, 250.0],
];
// Scale, travel speed, travel direction in degrees, spin speed in degrees.
const PARAMETERS: [[f32; 8]; 6] = [
    [0.12, 0.17, 1.5, 1.875, 60.0, 80.0, -1.28, 1.28],
    [0.17, 0.23, 1.875, 2.25, 60.0, 80.0, -1.28, 1.28],
    [0.24, 0.34, 1.5, 1.875, 60.0, 80.0, -1.28, 1.28],
    [0.34, 0.46, 1.875, 2.25, 60.0, 80.0, -1.28, 1.28],
    [0.15, 0.25, 1.5, 1.9, 60.0, 80.0, -1.28, 1.28],
    [0.30, 0.45, 1.5, 1.9, 60.0, 80.0, -1.28, 1.28],
];
const DURATIONS: [[i32; 12]; 2] = [[3; 12], [5, 4, 3, 2, 3, 4, 5, 4, 3, 2, 3, 4]];

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Particle {
    pub alive: bool,
    pub variant: usize,
    pub origin: Vec2,
    pub travel_speed: f32,
    pub travel_angle: f32,
    pub radius: f32,
    pub scale: f32,
    pub spin_angle: f32,
    pub spin_speed: f32,
    pub age: f32,
}

impl Particle {
    pub fn transform(&self, projection: Mat4) -> Mat4 {
        projection
            * Mat4::from_translation(self.origin.extend(0.0))
            * Mat4::from_rotation_z(self.travel_angle)
            * Mat4::from_translation(Vec3::new(self.radius, 0.0, 0.0))
            * Mat4::from_rotation_z(self.spin_angle)
            * Mat4::from_scale(Vec3::new(self.scale, self.scale, 1.0))
    }

    pub fn sprite(&self) -> &'static SpriteFrame {
        // FCVTZS and signed remainder, not Euclidean wrapping. Negative ages
        // select the first frame until their signed remainder becomes positive.
        let durations = &DURATIONS[self.variant];
        let mut time = (self.age as i32) % durations.iter().sum::<i32>();
        let frame = durations
            .iter()
            .position(|&duration| {
                if time < duration {
                    true
                } else {
                    time -= duration;
                    false
                }
            })
            .unwrap();
        &SPRITES[self.variant * 12 + frame]
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SakuraParticles {
    pub mode: usize,
    pub elapsed: f32,
    seed: u32,
    emission: f32,
    pub particles: Vec<Particle>,
    arithmetic: SakuraArithmetic,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SakuraArithmetic {
    Legacy,
    Gerokasu2,
}

impl SakuraParticles {
    #[cfg(test)]
    pub(crate) fn reference_seed(&self) -> u32 {
        self.seed
    }
    #[cfg(test)]
    pub(crate) fn reference_owner_emission(&self) -> f32 {
        self.emission
    }
    #[cfg(test)]
    pub(super) fn reference_owner_mutate(&mut self) {
        self.seed += 1;
        self.elapsed = 123.0;
        self.emission = -1.0;
        self.particles[0].alive = false;
    }
    // Native unsigned mode<6 check returns no layer for all other values.
    pub fn new(mode: i32, seed: u32) -> Option<Self> {
        Self::new_with_arithmetic(mode, seed, SakuraArithmetic::Legacy)
    }
    pub(crate) fn new_with_arithmetic(
        mode: i32,
        seed: u32,
        arithmetic: SakuraArithmetic,
    ) -> Option<Self> {
        if !(0..6).contains(&mode) {
            return None;
        }
        let mut model = Self {
            mode: mode as usize,
            elapsed: 0.0,
            seed: (seed % 2_147_483_647).max(1),
            emission: 0.0,
            particles: Vec::new(),
            arithmetic,
        };
        model.emission = model.period();
        // Derived body is invoked directly500 times, with permissiontrue.
        // No render runs here: particles cannot expire during construction.
        for _ in 0..500 {
            model.update(1.0, true);
        }
        Some(model)
    }

    fn next_seed(&mut self) -> u32 {
        let value =
            48_271i64 * i64::from(self.seed % 44_488) - 3_399i64 * i64::from(self.seed / 44_488);
        self.seed = if value < 0 {
            (value + 2_147_483_647) as u32
        } else {
            value as u32
        };
        self.seed
    }

    fn random(&mut self) -> f32 {
        (self.next_seed() - 1) as f32 * f32::from_bits(0x30000000)
    }

    fn period(&mut self) -> f32 {
        let [low, high] = PERIODS[self.mode].map(|v| v * 0.6);
        let random = self.random();
        if self.arithmetic == SakuraArithmetic::Gerokasu2 {
            (high - low).mul_add(random, low)
        } else {
            low + (high - low) * random
        }
    }

    fn spawn(&mut self) {
        let p = PARAMETERS[self.mode];
        let variant = (self.next_seed() & 1) as usize;
        let random = self.random();
        let x = if self.arithmetic == SakuraArithmetic::Gerokasu2 {
            random.mul_add(1480.0, -200.0)
        } else {
            random * 1480.0 - 200.0
        };
        // Native consumes a random number even though the Y range is zero.
        let random = self.random();
        let y = if self.arithmetic == SakuraArithmetic::Gerokasu2 {
            random.mul_add(0.0, -10.0)
        } else {
            random * 0.0 - 10.0
        };
        let travel_speed = self.parameter(p[2], p[3]);
        let travel_angle = (self.parameter(p[4], p[5]) * PI) / 180.0;
        let scale = self.parameter(p[0], p[1]);
        let random = self.random();
        let spin_angle = if self.arithmetic == SakuraArithmetic::Gerokasu2 {
            random.mul_add(PI * 2.0, -PI)
        } else {
            random * (PI * 2.0) - PI
        };
        let spin_speed = (self.parameter(p[6], p[7]) * PI) / 180.0;
        self.particles.push(Particle {
            alive: true,
            variant,
            origin: Vec2::new(x, y),
            travel_speed,
            travel_angle,
            radius: 0.0,
            scale,
            spin_angle,
            spin_speed,
            age: 0.0,
        });
    }
    fn parameter(&mut self, low: f32, high: f32) -> f32 {
        let random = self.random();
        if self.arithmetic == SakuraArithmetic::Gerokasu2 {
            (high - low).mul_add(random, low)
        } else {
            low + (high - low) * random
        }
    }

    pub fn update(&mut self, delta: f32, animations_allowed: bool) {
        if animations_allowed {
            self.elapsed += delta;
        }
        self.emission -= delta;
        self.particles.retain(|particle| particle.alive);
        while self.emission <= 0.0 {
            self.spawn();
            self.emission += self.period();
        }
        // No age offset for births: all particles advance by the full delta.
        // The stored spawn origin stays fixed; +18 is radius, not Y position.
        for particle in &mut self.particles {
            if self.arithmetic == SakuraArithmetic::Gerokasu2 {
                particle.radius = particle.travel_speed.mul_add(delta, particle.radius);
                particle.spin_angle = particle.spin_speed.mul_add(delta, particle.spin_angle);
            } else {
                particle.radius += particle.travel_speed * delta;
                particle.spin_angle += particle.spin_speed * delta;
            }
            particle.age += delta;
        }
    }

    pub fn render_particles(&mut self, projection: Mat4, mut draw: impl FnMut(Mat4, &SpriteFrame)) {
        for particle in &mut self.particles {
            if !particle.alive {
                continue;
            }
            let transform = particle.transform(projection);
            if transform.project_point3(Vec3::ZERO).y < -1.1 {
                particle.alive = false;
                continue;
            }
            draw(transform, particle.sprite());
        }
    }
}

#[derive(Debug, PartialEq)]
pub(crate) struct SpriteFrame {
    pub uv: [f32; 4],
    pub bounds: [i32; 4],
}

#[cfg(test)]
mod tests;

// Native tile records1d2ec8..1d3287; preserve the literal UV rounding.
static SPRITES: [SpriteFrame; 24] = [
    SpriteFrame {
        uv: [
            f32::from_bits(0x3c2aac11),
            f32::from_bits(0x3ba0a0f5),
            f32::from_bits(0x3e18e3ac),
            f32::from_bits(0x3e61e1d2),
        ],
        bounds: [-21, -46, 19, 42],
    },
    SpriteFrame {
        uv: [
            f32::from_bits(0x3e438e2a),
            f32::from_bits(0x3bf0f16f),
            f32::from_bits(0x3e971c76),
            f32::from_bits(0x3e5cdcca),
        ],
        bounds: [-17, -45, 13, 40],
    },
    SpriteFrame {
        uv: [
            f32::from_bits(0x3ebc71d6),
            f32::from_bits(0x3ba0a0f5),
            f32::from_bits(0x3eec71d6),
            f32::from_bits(0x3e646456),
        ],
        bounds: [-14, -46, 13, 43],
    },
    SpriteFrame {
        uv: [
            f32::from_bits(0x3f08e38a),
            f32::from_bits(0x3ba0a0f5),
            f32::from_bits(0x3f1e38eb),
            f32::from_bits(0x3e57d7c3),
        ],
        bounds: [-14, -46, 10, 38],
    },
    SpriteFrame {
        uv: [
            f32::from_bits(0x3f355550),
            f32::from_bits(0x3ba0a0f5),
            f32::from_bits(0x3f4b8e3b),
            f32::from_bits(0x3e646456),
        ],
        bounds: [-12, -46, 13, 43],
    },
    SpriteFrame {
        uv: [
            f32::from_bits(0x3f5b8e3b),
            f32::from_bits(0x3b20a0f5),
            f32::from_bits(0x3f7638eb),
            f32::from_bits(0x3e5cdcca),
        ],
        bounds: [-17, -47, 13, 40],
    },
    SpriteFrame {
        uv: [
            f32::from_bits(0x3c2aac11),
            f32::from_bits(0x3e850508),
            f32::from_bits(0x3e15553f),
            f32::from_bits(0x3ef3736d),
        ],
        bounds: [-21, -46, 18, 42],
    },
    SpriteFrame {
        uv: [
            f32::from_bits(0x3e400000),
            f32::from_bits(0x3e86464a),
            f32::from_bits(0x3e955561),
            f32::from_bits(0x3ef0f0e9),
        ],
        bounds: [-18, -45, 12, 40],
    },
    SpriteFrame {
        uv: [
            f32::from_bits(0x3ebaaa9f),
            f32::from_bits(0x3e86464a),
            f32::from_bits(0x3ee71c76),
            f32::from_bits(0x3ef4b4af),
        ],
        bounds: [-15, -45, 10, 43],
    },
    SpriteFrame {
        uv: [
            f32::from_bits(0x3f0b8e3b),
            f32::from_bits(0x3e86464a),
            f32::from_bits(0x3f20e38a),
            f32::from_bits(0x3eefafa7),
        ],
        bounds: [-11, -45, 13, 39],
    },
    SpriteFrame {
        uv: [
            f32::from_bits(0x3f32aab0),
            f32::from_bits(0x3e850508),
            f32::from_bits(0x3f48e38a),
            f32::from_bits(0x3ef4b4af),
        ],
        bounds: [-15, -46, 10, 43],
    },
    SpriteFrame {
        uv: [
            f32::from_bits(0x3f5aaab0),
            f32::from_bits(0x3e86464a),
            f32::from_bits(0x3f755550),
            f32::from_bits(0x3ef0f0e9),
        ],
        bounds: [-18, -45, 12, 40],
    },
    SpriteFrame {
        uv: [
            f32::from_bits(0x00000000),
            f32::from_bits(0x3f03c3c6),
            f32::from_bits(0x3e0aaac1),
            f32::from_bits(0x3f3c3c3a),
        ],
        bounds: [-20, -46, 19, 44],
    },
    SpriteFrame {
        uv: [
            f32::from_bits(0x3e1c71d6),
            f32::from_bits(0x3f03c3c6),
            f32::from_bits(0x3e855561),
            f32::from_bits(0x3f3d7d7c),
        ],
        bounds: [-16, -46, 15, 46],
    },
    SpriteFrame {
        uv: [
            f32::from_bits(0x3e9aaa9f),
            f32::from_bits(0x3f046467),
            f32::from_bits(0x3ecaaa9f),
            f32::from_bits(0x3f3d7d7c),
        ],
        bounds: [-13, -45, 14, 46],
    },
    SpriteFrame {
        uv: [
            f32::from_bits(0x3edc71d6),
            f32::from_bits(0x3f050508),
            f32::from_bits(0x3f0b8e3b),
            f32::from_bits(0x3f3c3c3a),
        ],
        bounds: [-16, -44, 17, 44],
    },
    SpriteFrame {
        uv: [
            f32::from_bits(0x3f138e3b),
            f32::from_bits(0x3f046467),
            f32::from_bits(0x3f2b8e3b),
            f32::from_bits(0x3f3d7d7c),
        ],
        bounds: [-14, -45, 13, 46],
    },
    SpriteFrame {
        uv: [
            f32::from_bits(0x3f3638eb),
            f32::from_bits(0x3f03c3c6),
            f32::from_bits(0x3f51c715),
            f32::from_bits(0x3f3c3c3a),
        ],
        bounds: [-15, -46, 16, 44],
    },
    SpriteFrame {
        uv: [
            f32::from_bits(0x3b638a7e),
            f32::from_bits(0x3f450508),
            f32::from_bits(0x3e0e38eb),
            f32::from_bits(0x3f7d7d7c),
        ],
        bounds: [-19, -46, 20, 44],
    },
    SpriteFrame {
        uv: [
            f32::from_bits(0x3e200000),
            f32::from_bits(0x3f450508),
            f32::from_bits(0x3e871c76),
            f32::from_bits(0x3f7d7d7c),
        ],
        bounds: [-15, -46, 16, 44],
    },
    SpriteFrame {
        uv: [
            f32::from_bits(0x3e98e38a),
            f32::from_bits(0x3f45a5a9),
            f32::from_bits(0x3ec8e38a),
            f32::from_bits(0x3f7ebebe),
        ],
        bounds: [-14, -45, 13, 46],
    },
    SpriteFrame {
        uv: [
            f32::from_bits(0x3edc71d6),
            f32::from_bits(0x3f46464a),
            f32::from_bits(0x3f0b8e3b),
            f32::from_bits(0x3f7d7d7c),
        ],
        bounds: [-16, -44, 17, 44],
    },
    SpriteFrame {
        uv: [
            f32::from_bits(0x3f1471c5),
            f32::from_bits(0x3f45a5a9),
            f32::from_bits(0x3f2c71c5),
            f32::from_bits(0x3f7ebebe),
        ],
        bounds: [-13, -45, 14, 46],
    },
    SpriteFrame {
        uv: [
            f32::from_bits(0x3f355550),
            f32::from_bits(0x3f450508),
            f32::from_bits(0x3f50e38a),
            f32::from_bits(0x3f7d7d7c),
        ],
        bounds: [-16, -46, 15, 44],
    },
];
