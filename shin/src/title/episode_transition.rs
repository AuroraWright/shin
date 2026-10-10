//! Captured-screen episode transition e32f0, clocks e3a18..e3afc.
use shin_core::time::{Easing, Ticks, Tween, Tweener};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct EpisodeFrame {
    pub rotation_progress: f32,
    pub door_offset: f32,
    pub cover_alpha: f32,
    pub cover_luminance: f32,
}

pub(super) struct EpisodeMotion {
    episode: u8,
    rotation: Tweener,
    offset: Tweener,
    cover_alpha: Tweener,
    cover_luminance: Tweener,
    destination: Option<u8>,
    dispatched: bool,
}

fn enqueue(clock: &mut Tweener, value: f32, ticks: f32, easing: Easing) {
    clock.enqueue(
        value,
        Tween {
            duration: Ticks::from_f32(ticks),
            easing,
        },
    );
}

impl EpisodeMotion {
    pub fn new(episode: u8) -> Self {
        let mut rotation = Tweener::new(0.0);
        enqueue(&mut rotation, 0.0, 60.0, Easing::Linear);
        enqueue(&mut rotation, 1.0, 180.0, Easing::SineIn);
        let mut offset = Tweener::new(0.0);
        enqueue(&mut offset, 0.0, 90.0, Easing::Linear);
        enqueue(&mut offset, 500.0, 150.0, Easing::SineIn);
        let mut cover_alpha = Tweener::new(0.0);
        enqueue(&mut cover_alpha, 0.0, 60.0, Easing::Linear);
        enqueue(&mut cover_alpha, 1.0, 180.0, Easing::SineIn);
        let mut cover_luminance = Tweener::new(1.0);
        enqueue(&mut cover_luminance, 1.0, 240.0, Easing::Linear);
        enqueue(&mut cover_luminance, 1.0, 0.0, Easing::Linear);
        enqueue(&mut cover_luminance, 0.0, 120.0, Easing::Linear);
        Self {
            episode,
            rotation,
            offset,
            cover_alpha,
            cover_luminance,
            destination: None,
            dispatched: false,
        }
    }

    pub fn update(&mut self, delta: Ticks) {
        // e3f34..e3f64 advances all four clocks before e4310 tests idle.
        // Native bba74..bba78 ignores nonpositive delta.
        if delta > Ticks::ZERO {
            for clock in [
                &mut self.rotation,
                &mut self.offset,
                &mut self.cover_alpha,
                &mut self.cover_luminance,
            ] {
                clock.update(delta);
            }
        }
        if !self.dispatched && self.cover_luminance.is_idle() {
            self.destination = Some(self.episode);
            self.dispatched = true;
        }
    }

    pub fn frame(&self) -> EpisodeFrame {
        EpisodeFrame {
            rotation_progress: self.rotation.value(),
            door_offset: self.offset.value(),
            cover_alpha: self.cover_alpha.value(),
            cover_luminance: self.cover_luminance.value(),
        }
    }

    pub fn take_episode(&mut self) -> Option<u8> {
        self.destination.take()
    }
}

/// Native constructor uses ceil(scale*480/270), then integer right shifts.
pub(super) fn capture_sizes(extent: (u16, u16)) -> [(u32, u32); 4] {
    let width = ((extent.0 as f32 / 1920.0) * 480.0).ceil() as u32;
    let height = ((extent.1 as f32 / 1080.0) * 270.0).ceil() as u32;
    std::array::from_fn(|level| (width >> level, height >> level))
}

/// Native e39f4..e3a14 stores the four blur tap weights verbatim.
pub(super) const BLUR_WEIGHTS: [f32; 4] = [
    f32::from_bits(0x3eceed6b),
    f32::from_bits(0x3ea127be),
    f32::from_bits(0x3e437dbe),
    f32::from_bits(0x3db8afdf),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn door_motion_samples_and_same_frame_completion_follow_native_clocks() {
        for elapsed in [
            0.0f32, 59.5, 60.0, 75.0, 90.0, 120.0, 180.0, 239.5, 240.0, 270.0, 359.5, 360.0, 720.0,
        ] {
            let mut motion = EpisodeMotion::new(47);
            motion.update(Ticks::from_f32(elapsed));
            let frame = motion.frame();
            // Independent formulas, rather than another queued tweener.
            let sine_in = |x: f32| 1.0 - (x.clamp(0.0, 1.0) * std::f32::consts::FRAC_PI_2).cos();
            let rotation = sine_in((elapsed - 60.0) / 180.0);
            assert!((frame.rotation_progress - rotation).abs() < 0.00001);
            assert!((frame.cover_alpha - rotation).abs() < 0.00001);
            assert!((frame.door_offset - 500.0 * sine_in((elapsed - 90.0) / 150.0)).abs() < 0.0001);
            assert!(
                (frame.cover_luminance - (1.0 - (elapsed - 240.0) / 120.0).clamp(0.0, 1.0)).abs()
                    < 0.00001
            );
            assert_eq!(motion.take_episode(), (elapsed >= 360.0).then_some(47));
            motion.update(Ticks::from_f32(10.0));
            assert_eq!(
                motion.take_episode(),
                (elapsed < 360.0 && elapsed + 10.0 >= 360.0).then_some(47)
            );
            motion.update(Ticks::from_f32(720.0));
            let _ = motion.take_episode();
            motion.update(Ticks::from_f32(720.0));
            assert_eq!(
                motion.take_episode(),
                None,
                "destination dispatches only once"
            );
        }
    }

    #[test]
    fn fractional_frame_partition_preserves_final_gate_and_nonpositive_delta() {
        let mut motion = EpisodeMotion::new(8);
        for _ in 0..719 {
            motion.update(Ticks::from_f32(0.5));
            assert_eq!(motion.take_episode(), None);
        }
        let frame = motion.frame();
        motion.update(Ticks::from_f32(-100.0));
        motion.update(Ticks::ZERO);
        assert_eq!(motion.frame(), frame);
        assert_eq!(motion.take_episode(), None);
        motion.update(Ticks::from_f32(0.5));
        assert_eq!(motion.take_episode(), Some(8));
        assert_eq!(motion.frame().cover_luminance, 0.0);
    }

    #[test]
    fn capture_pyramid_rounds_only_base_level_and_preserves_native_weights() {
        assert_eq!(
            capture_sizes((1920, 1080)),
            [(480, 270), (240, 135), (120, 67), (60, 33)]
        );
        assert_eq!(
            capture_sizes((960, 540)),
            [(240, 135), (120, 67), (60, 33), (30, 16)]
        );
        assert_eq!(capture_sizes((1919, 1079)), capture_sizes((1920, 1080)));
        assert_eq!(
            capture_sizes((1921, 1081)),
            [(481, 271), (240, 135), (120, 67), (60, 33)]
        );
        assert_eq!(
            BLUR_WEIGHTS.map(f32::to_bits),
            [0x3eceed6b, 0x3ea127be, 0x3e437dbe, 0x3db8afdf]
        );
    }

    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_episode_bridge_clocks_capture_and_fresh_adv_factory() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
        for (address, expected) in [
            (0xe3314, 0xb9000801),
            (0xe3384, 0xf90003f4),
            (0xe3398, 0x97fd0fa6),
            (0xe33a0, 0x97fd0fb0),
            (0xe33bc, 0x97ffb565),
            (0xe33d0, 0x1e290018),
            (0xe33e0, 0x1e290039),
            (0xe3504, 0x53017f1a),
            (0xe3508, 0x53017f3b),
            (0xe35e4, 0x53027f1a),
            (0xe35e8, 0x53027f3b),
            (0xe36c4, 0x53037f18),
            (0xe36c8, 0x53037f39),
            (0xe39f4, 0xd29dad68),
            (0xe39f8, 0xf2a7d9c8),
            (0xe39fc, 0xd28fb7c9),
            (0xe3a00, 0xf2a7c869),
            (0xe3a04, 0xf2c4f7c8),
            (0xe3a08, 0xf2e7d428),
            (0xe3a0c, 0xf2d5fbe9),
            (0xe3a10, 0xf2e7b709),
            (0xe3a14, 0xa91ba6c8),
            (0xe3a28, 0x321903e1),
            (0xe3a44, 0x320003e1),
            (0xe3a64, 0x321903e1),
            (0xe3a80, 0x320003e1),
            (0xe3a98, 0x321903e1),
            (0xe3aac, 0x320003e1),
            (0xe3acc, 0x2a1f03e2),
            (0xe3adc, 0x2a1f03e1),
            (0xe3af4, 0x2a1f03e1),
            (0xe3f3c, 0x97ff5ec5),
            (0xe3f48, 0x97ff5ec2),
            (0xe3f54, 0x97ff5ebf),
            (0xe3f64, 0x97ff5ebb),
            (0xe430c, 0xf94007e0),
            (0xe4310, 0x97ff5db8),
            (0xe4314, 0x370004a0),
            (0xe432c, 0x390043e8),
            (0xe4330, 0xb9400a68),
            (0xe4334, 0x2903a3ff),
            (0xe4338, 0x94005de2),
            (0xe434c, 0x2a1f03e1),
            (0xe4350, 0x97fcc830),
            (0xe435c, 0x97fd0c05),
        ] {
            assert_eq!(word(address), expected, "native episode bridge {address:x}");
        }
        for (address, expected) in [
            (0x159798, 60.0f32),
            (0x159814, 180.0),
            (0x1599e0, 90.0),
            (0x1597e4, 500.0),
            (0x159a20, 150.0),
            (0x159660, 240.0),
            (0x159a18, 120.0),
            (0x1597ec, 480.0),
            (0x1594b4, 270.0),
        ] {
            assert_eq!(
                word(address),
                expected.to_bits(),
                "native episode constant {address:x}"
            );
        }
        let relocation = |target: u64| {
            (0x11e040..0x152eb8)
                .step_by(24)
                .find_map(|a| {
                    let slot = u64::from_le_bytes(exe[a + 256..a + 264].try_into().unwrap());
                    (slot == target)
                        .then(|| u64::from_le_bytes(exe[a + 272..a + 280].try_into().unwrap()))
                })
                .unwrap()
        };
        assert_eq!(relocation(0x225518), 0x21fa20);
        assert_eq!(relocation(0x21fa40), 0xe3f00);
        assert_eq!(relocation(0x21fa48), 0xe49a0);
        let name = b"N4shin20ScenarioSelectBridgeE";
        assert_eq!(&exe[0x173d57 + 256..0x173d57 + 256 + name.len()], name);
        // Low six flag bits index the existing native easing table.
        let easing_table = relocation(0x225118);
        assert_eq!(relocation(easing_table + 8), 0xbb1d0); // SineIn
    }
}
