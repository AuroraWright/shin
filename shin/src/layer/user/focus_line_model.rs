//! FocusLineLayer constructor66ba0, refill66ef0 and update67520.
use shin_core::time::{Easing, Ticks, Tween, Tweener};

#[derive(Debug, Clone)]
pub(crate) struct Ray {
    pub angle: f32,
    pub alpha: Tweener,
}

#[derive(Debug, Clone)]
pub(crate) struct FocusLineRays {
    seed: u32,
    pub rays: Vec<Ray>,
}

impl FocusLineRays {
    #[cfg(test)]
    pub(super) fn reference_owner_mutate(&mut self) {
        self.seed = self.seed.wrapping_add(1);
        self.rays[0].angle = 1.25;
        self.rays[0].alpha.fast_forward_to(-0.25);
    }
    #[cfg(test)]
    pub(crate) fn reference_seed(&self) -> u32 {
        self.seed
    }
    pub fn new(seed: u32) -> Self {
        let mut model = Self {
            seed: (seed % 2_147_483_647).max(1),
            rays: Vec::new(),
        };
        model.refill();
        // Native warms each initial tween by a separately drawn integer0..29.
        for i in 0..model.rays.len() {
            let ticks = (model.random() * 30.0).floor();
            model.rays[i].alpha.update(Ticks::from_f32(ticks));
        }
        model
    }

    fn random(&mut self) -> f32 {
        let quotient = self.seed / 44_488;
        let remainder = self.seed % 44_488;
        let value = 48_271i64 * i64::from(remainder) - 3_399i64 * i64::from(quotient);
        self.seed = if value < 0 {
            (value + 2_147_483_647) as u32
        } else {
            value as u32
        };
        (self.seed - 1) as f32 * 4.656612873077393e-10
    }

    fn refill(&mut self) {
        while self.rays.len() < 40 {
            let angle = self.random() * 6.283180236816406 - 3.141590118408203;
            let duration = self.random() * 20.0 + 10.0;
            let mut alpha = Tweener::new(1.0);
            alpha.enqueue(
                0.0,
                Tween {
                    duration: Ticks::from_f32(duration),
                    easing: Easing::SineOut,
                },
            );
            self.rays.push(Ray { angle, alpha });
        }
    }

    pub fn update(&mut self, delta: Ticks) {
        // Expiry is checked before the clock advance. Newly completed rays
        // remain with alpha0 until the following update replaces them.
        self.rays.retain(|ray| !ray.alpha.is_idle());
        self.refill();
        for ray in &mut self.rays {
            ray.alpha.update(delta);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_random_angles_warmup_and_expiry_replacement_order() {
        let mut model = FocusLineRays::new(1);
        assert_eq!(model.rays.len(), 40);
        let first =
            (48_270u32 as f32 * 4.656612873077393e-10) * 6.283180236816406 - 3.141590118408203;
        assert_eq!(model.rays[0].angle, first);
        assert!(
            model
                .rays
                .iter()
                .all(|ray| (-3.141591..3.141591).contains(&ray.angle))
        );
        assert!(model.rays.iter().any(|ray| ray.alpha.value() > 0.0));
        assert!(model.rays.iter().any(|ray| ray.alpha.is_idle()));
        let mut clone = model.clone();
        model.update(Ticks::from_f32(50.0));
        assert_eq!(model.rays.len(), 40);
        assert!(
            model
                .rays
                .iter()
                .all(|ray| ray.alpha.is_idle() && ray.alpha.value() == 0.0)
        );
        clone.update(Ticks::from_f32(50.0));
        assert_eq!(model.seed, clone.seed);
        assert_eq!(
            model.rays.iter().map(|r| r.angle).collect::<Vec<_>>(),
            clone.rays.iter().map(|r| r.angle).collect::<Vec<_>>()
        );
        let old_angles: Vec<_> = model.rays.iter().map(|r| r.angle).collect();
        model.update(Ticks::ZERO);
        assert_eq!(model.rays.len(), 40);
        assert!(
            model
                .rays
                .iter()
                .all(|ray| !ray.alpha.is_idle() && ray.alpha.value() == 1.0)
        );
        assert_ne!(
            old_angles,
            model.rays.iter().map(|r| r.angle).collect::<Vec<_>>()
        );
        clone.update(Ticks::ZERO);
        assert_eq!(model.seed, clone.seed);
        model.update(Ticks::from_f32(-1.0));
        // Native bba50 ignores nonpositive tween deltas; earlier queue
        // validation already established this contract.
        assert!(model.rays.iter().all(|ray| ray.alpha.value() == 1.0));
    }
}
