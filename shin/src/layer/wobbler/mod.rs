mod prng;

use std::f32::consts::PI;

use num_derive::FromPrimitive;
use num_traits::FromPrimitive;
use shin_core::time::Ticks;
use tracing::warn;

#[derive(FromPrimitive, Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum WobbleMode {
    Disabled = 0,
    /// Jumps to a random position every `period` ticks.
    Random = 1,
    /// Goes from 0 to 1 at 0 to .25, then from 1 to -1 at .25 to .75, then from -1 to 0 at .75 to 1.
    /// Results in kind-of choppy sine, as it's basically a triangular wave.
    /// https://www.desmos.com/calculator/j2wohvz0jt
    Triangular = 2,
    /// Jumps between -1 at 0 to .5 and 1 at .5 to 1.
    /// https://www.desmos.com/calculator/ntmtsrmh5j
    Square = 3,
    /// Plain old `sin(2pi * t)` wave.
    Sine = 4,
    /// Plain old `cos(2pi * t)` wave.
    Cosine = 5,
    /// `abs(sin(2pi * t))` wave.
    AbsSine = 6,
    /// Sawtooth: just goes from 0 to 1 each period, then jumps back.
    Sawtooth = 7,
    /// Sawtooth, but goes from 1 to 0.
    InvSawtooth = 8,
}

#[derive(Debug, Clone)]
pub struct Wobbler {
    mode: WobbleMode,
    seed: i32,
    /// Time, measured in periods
    time: f32,
}
impl Wobbler {
    #[cfg(test)]
    pub(super) fn reference_copy_words(&self) -> [u32; 3] {
        [self.seed as u32, self.mode as u32, self.time.to_bits()]
    }
    #[cfg(test)]
    pub(super) fn reference_with_state(seed: u32, mode: i32, time: f32) -> Self {
        Self {
            seed: seed as i32,
            mode: WobbleMode::from_i32(mode).unwrap(),
            time,
        }
    }

    #[cfg(test)]
    pub(super) fn reference_state(&self) -> (i32, f32, f32) {
        (self.mode as i32, self.time, self.value())
    }

    pub fn new() -> Self {
        Self::with_seed(crate::engine_random::next_seed())
    }

    fn with_seed(seed: u32) -> Self {
        Self {
            mode: WobbleMode::Disabled,
            seed: seed as i32,
            time: 0.0,
        }
    }

    pub fn value(&self) -> f32 {
        if !self.active() {
            return 0.0;
        }

        let t = self.time % 1.0;
        match self.mode {
            WobbleMode::Disabled => 0.0,
            WobbleMode::Random => prng::prng(t, (self.time - t) as i32, self.seed),
            WobbleMode::Triangular => {
                if t < 0.25 {
                    t * 4.0
                } else if t < 0.75 {
                    2.0 - t * 4.0
                } else {
                    t * 4.0 - 4.0
                }
            }
            WobbleMode::Square => {
                if t < 0.5 {
                    -1.0
                } else {
                    1.0
                }
            }
            WobbleMode::Sine => (t * 2.0 * PI).sin(),
            WobbleMode::Cosine => (t * 2.0 * PI).cos(),
            WobbleMode::AbsSine => (t * 2.0 * PI).sin().abs(),
            WobbleMode::Sawtooth => t,
            WobbleMode::InvSawtooth => 1.0 - t,
        }
    }

    pub fn active(&self) -> bool {
        self.mode != WobbleMode::Disabled
    }

    pub fn value_opt(&self) -> Option<f32> {
        if self.active() {
            Some(self.value())
        } else {
            None
        }
    }

    pub fn update(&mut self, delta_time: Ticks, mode: f32, period: Ticks) {
        let mode = mode as i32;
        let mode = WobbleMode::from_i32(mode).unwrap_or_else(|| {
            // TODO: this should be printed once per invalid mode
            warn!("Invalid wobble mode: {}", mode);
            WobbleMode::Disabled
        });

        // Native c7330 resets only when the normalized mode changes.
        // A zero period freezes an active oscillator; negative periods reverse it.
        if mode != self.mode {
            self.mode = mode;
            self.time = 0.0;
        }
        if period == Ticks::ZERO || !self.active() {
            return;
        }

        // c734c uses reciprocal then multiplication, followed by two modff calls.
        // Preserve that rounding and truncation, including signed time and wrap.
        let time = self.time + (1.0 / period.as_f32()) * delta_time.as_f32();
        let fraction = time.fract();
        let fraction = if fraction < 0.0 {
            fraction + 1.0
        } else {
            fraction
        };
        let cycles = (time.trunc() * 0.001).fract();
        let cycles = if cycles < 0.0 { cycles + 1.0 } else { cycles };
        self.time = fraction + cycles * 1000.0;
    }
}

#[cfg(test)]
mod tests;
