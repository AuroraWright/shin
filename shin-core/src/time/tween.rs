use std::f32::consts::PI;

use crate::time::Ticks;

/// Curves the motion of a [`Tween`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Easing {
    /// Maintains a constant speed for the duration of the [`Tween`].
    Linear,
    /// Causes the [`Tween`] to start slow and speed up.
    SineIn,
    /// Causes the [`Tween`] to start fast and slow down.
    SineOut,
    /// Causes the [`Tween`] to start slow, speed up, and then slow back down.
    SineInOut,

    /// Causes the [`Tween`] to hold its value for the duration
    ///   of the [`Tween`] and then jump to the end value.
    Jump,

    /// Positive exponents accelerate; negative exponents decelerate.
    /// Zero selects the linear curve.
    Power(i32),
}

const HALF_PI: f32 = PI / 2.0;

impl Easing {
    fn apply(&self, x: f32) -> f32 {
        match *self {
            Easing::Linear => x,
            Easing::SineIn => 1.0 - (x * HALF_PI).cos(),
            Easing::SineOut => (x * HALF_PI).sin(),
            Easing::SineInOut => (1.0 - (PI * x).cos()) / 2.0,
            Easing::Jump => {
                if x < 1.0 {
                    0.0
                } else {
                    1.0
                }
            }
            Easing::Power(power) => {
                if power > 0 {
                    x.powf(power as f32)
                } else if power != 0 {
                    1.0 - (1.0 - x).powf(power.wrapping_neg() as f32)
                } else {
                    x
                }
            }
        }
    }
}

/// Describes a smooth transition between values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tween {
    /// The duration of the motion.
    pub duration: Ticks,
    /// The curve of the motion.
    pub easing: Easing,
}

impl Tween {
    pub const IMMEDIATE: Self = Self {
        duration: Ticks::ZERO,
        easing: Easing::Linear,
    };

    pub const MS_15: Self = Self {
        // I would use the function from_millis, but it can't be const yet (see https://github.com/rust-lang/rust/issues/57241)
        duration: Ticks::from_f32(Ticks::TICKS_PER_SECOND / 1000.0 * 15.0),
        easing: Easing::Linear,
    };

    pub fn linear(duration: Ticks) -> Self {
        Self {
            duration,
            easing: Easing::Linear,
        }
    }

    pub fn value(&self, time: Ticks) -> f32 {
        if self.duration <= Ticks::ZERO {
            return 1.0;
        }
        let x = time / self.duration;
        self.easing.apply(x)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_curve_samples_and_endpoints() {
        let curves = [
            (Easing::Linear, 0.25),
            (Easing::SineIn, 0.07612047),
            (Easing::SineOut, 0.38268343),
            (Easing::SineInOut, 0.14644662),
            (Easing::Jump, 0.0),
            (Easing::Power(2), 0.0625),
            (Easing::Power(-2), 0.4375),
            (Easing::Power(0), 0.25),
        ];
        for (easing, expected) in curves {
            let tween = Tween {
                duration: Ticks::from_f32(20.0),
                easing,
            };
            assert!((tween.value(Ticks::ZERO)).abs() < 0.00001);
            assert!((tween.value(Ticks::from_f32(5.0)) - expected).abs() < 0.00001);
            assert!((tween.value(Ticks::from_f32(20.0)) - 1.0).abs() < 0.00001);
        }
    }

    #[test]
    fn immediate_and_extreme_power_inputs_follow_native_float_operations() {
        for easing in [
            Easing::Linear,
            Easing::Power(2),
            Easing::Power(-2),
            Easing::Jump,
        ] {
            for duration in [Ticks::ZERO, Ticks::from_f32(-1.0)] {
                let tween = Tween { duration, easing };
                assert_eq!(tween.value(Ticks::ZERO), 1.0);
            }
        }
        // Native NEG w8 wraps before SCVTF; the minimum integer remains negative.
        let tween = Tween {
            duration: Ticks::from_f32(1.0),
            easing: Easing::Power(i32::MIN),
        };
        assert_eq!(tween.value(Ticks::from_f32(0.5)), f32::NEG_INFINITY);
        let tween = Tween {
            easing: Easing::Power(i32::MAX),
            ..tween
        };
        assert_eq!(tween.value(Ticks::from_f32(0.5)), 0.0);
    }

    #[test]
    #[ignore = "requires the supplied decrypted native executable (SHIN_REFERENCE_EXE)"]
    fn reference_easing_table_matches_native_handlers_and_constants() {
        use std::path::PathBuf;
        let path = std::env::var_os("SHIN_REFERENCE_EXE")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../umineko_switch_files/uncompressed_executable")
            });
        let data = std::fs::read(path).unwrap();
        let u32_at = |address: usize| {
            u32::from_le_bytes(data[address + 256..address + 260].try_into().unwrap())
        };
        let relocations: std::collections::HashMap<u64, u64> = (0x11e040usize..0x152eb8usize)
            .step_by(24)
            .map(|address| {
                let word = |offset: usize| {
                    u64::from_le_bytes(
                        data[address + 256 + offset..address + 264 + offset]
                            .try_into()
                            .unwrap(),
                    )
                };
                (word(0), word(16))
            })
            .collect();
        let table = relocations[&0x225118];
        for (index, handler) in [0xbb1c0, 0xbb1d0, 0xbb200, 0xbb220, 0xbb260, 0xbb280]
            .into_iter()
            .enumerate()
        {
            assert_eq!(relocations[&(table + index as u64 * 8)], handler);
        }
        assert_eq!(
            f32::from_bits(u32_at(relocations[&0x225100] as usize)),
            HALF_PI
        );
        assert_eq!(f32::from_bits(u32_at(relocations[&0x225108] as usize)), PI);
        assert_eq!(u32_at(0xbb29c), 0x4b0003e8); // NEG w8,w0
        assert_eq!(u32_at(0xbb288), 0x1e220001); // SCVTF s1,w0
    }
}
