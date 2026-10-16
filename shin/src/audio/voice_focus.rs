use shin_core::time::Ticks;

/// Native BGM gain words at offsets 0x28/0x2c, updated by 0x33ec0.
pub struct VoiceFocus {
    pub gain: f32,
}

impl Default for VoiceFocus {
    fn default() -> Self {
        Self { gain: 1.0 }
    }
}

impl VoiceFocus {
    pub(crate) fn update_gerokasu2_track(
        &mut self,
        track: &mut kira::track::TrackHandle,
        delta: Ticks,
        target: f32,
    ) {
        if self.update_gerokasu2(delta, target) {
            track.set_volume(
                shin_audio::amplitude_to_decibels(self.gain),
                kira::Tween {
                    duration: std::time::Duration::ZERO,
                    ..Default::default()
                },
            );
        }
    }
    pub(crate) fn update_gerokasu2(&mut self, delta: Ticks, target: f32) -> bool {
        let previous = self.gain;
        self.gain = if previous < target {
            let next = delta.as_f32().mul_add(0.04, previous);
            if next > target { target } else { next }
        } else if previous > target {
            let next = delta.as_f32().mul_add(-0.04, previous);
            if next < target { target } else { next }
        } else {
            previous
        };
        self.gain != previous
    }
    pub fn update_track(
        &mut self,
        track: &mut kira::track::TrackHandle,
        delta: Ticks,
        enabled: bool,
        voice_active: bool,
    ) {
        if self.update(delta, enabled, voice_active) {
            track.set_volume(
                shin_audio::amplitude_to_decibels(self.gain),
                kira::Tween {
                    duration: std::time::Duration::ZERO,
                    ..Default::default()
                },
            );
        }
    }

    pub fn update(&mut self, delta: Ticks, enabled: bool, voice_active: bool) -> bool {
        let target = if enabled && voice_active { 0.5 } else { 1.0 };
        let previous = self.gain;
        let step = delta.as_f32() * 0.04;
        self.gain = if previous < target {
            (previous + step).min(target)
        } else {
            (previous - step).max(target)
        };
        self.gain != previous
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[test]
    #[ignore = "original Ger38720 focus arithmetic; CPU only"]
    fn ger_focus_matches_original_fused_steps_and_conditional_clamps() {
        use std::{
            io::Write,
            process::{Command, Stdio},
        };
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let folder = root.join("gerokasu2/.local/adv-title-transition");
        std::fs::create_dir_all(&folder).unwrap();
        let binary = folder.join("native-focus-root-test");
        assert!(
            Command::new("clang")
                .args(["-O2", "-std=c11"])
                .arg(root.join("gerokasu2/native-adv-title-focus.c"))
                .arg("-o")
                .arg(&binary)
                .status()
                .unwrap()
                .success()
        );
        let mut input = String::new();
        let mut expected = Vec::new();
        let mut fused_discriminators = 0;
        let mut seed = 1u32;
        for i in 0..1024 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let current = match i {
                0 => -0.0,
                1 => 0.,
                2 => 0.5,
                3 => 1.,
                4 => f32::from_bits(0x3effffff),
                5 => f32::from_bits(0x3f000001),
                _ => (seed >> 8) as f32 / 16777216.,
            };
            for target in [0., 0.5, 1.] {
                for delta in [0., 0.25, 1., 2.5, 12.5, 1000., 1. / 60.] {
                    let mut focus = VoiceFocus { gain: current };
                    focus.update_gerokasu2(Ticks::from_f32(delta), target);
                    let scalar = if current < target {
                        (current + delta * 0.04).min(target)
                    } else {
                        (current - delta * 0.04).max(target)
                    };
                    fused_discriminators += usize::from(scalar.to_bits() != focus.gain.to_bits());
                    input.push_str(&format!(
                        "{:08x} {:08x} {:08x}\n",
                        current.to_bits(),
                        target.to_bits(),
                        delta.to_bits()
                    ));
                    expected.push([
                        current.to_bits(),
                        target.to_bits(),
                        delta.to_bits(),
                        focus.gain.to_bits(),
                    ]);
                }
            }
        }
        assert!(fused_discriminators > 0);
        let corpus = folder.join("focus-rust-profiles.txt");
        std::fs::write(&corpus, &input).unwrap();
        let output = Command::new(binary)
            .args([
                root.join("gerokasu2/uncompressed_executable"),
                root.join("gerokasu2/.local/newrodin-medium.fnt"),
                root.join("gerokasu2/.local/system.fnt"),
            ])
            .stdin(std::fs::File::open(corpus).unwrap())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .output()
            .unwrap();
        assert!(output.status.success());
        let rows: Vec<[u32; 4]> = String::from_utf8(output.stdout.clone())
            .unwrap()
            .lines()
            .map(|line| {
                line.split_whitespace()
                    .map(|x| u32::from_str_radix(x, 16).unwrap())
                    .collect::<Vec<_>>()
                    .try_into()
                    .unwrap()
            })
            .collect();
        assert_eq!(rows, expected);
        std::fs::File::create(folder.join("focus-rust-verified.output"))
            .unwrap()
            .write_all(&output.stdout)
            .unwrap();
        eprintln!(
            "GEROKASU2_FOCUS_NATIVE cases={} fused_discriminators={fused_discriminators} exact_float_bits_signed_zero_and_clamps=verified",
            rows.len()
        );
    }

    #[test]
    fn focus_ramps_by_native_ticks_and_clamps_both_targets() {
        let mut focus = VoiceFocus::default();
        assert!(!focus.update(Ticks::from_f32(1.0), false, true));
        focus.update(Ticks::from_f32(0.5), true, true);
        assert!((focus.gain - 0.98).abs() < 0.00001);
        for _ in 0..11 {
            focus.update(Ticks::from_f32(1.0), true, true);
        }
        assert!((focus.gain - 0.54).abs() < 0.00001);
        focus.update(Ticks::from_f32(1.0), true, true);
        assert!((focus.gain - 0.5).abs() < 0.00001);
        assert!(!focus.update(Ticks::from_f32(60.0), true, true));
        focus.update(Ticks::from_f32(1.0), true, false);
        assert!((focus.gain - 0.54).abs() < 0.00001);
        focus.update(Ticks::from_f32(60.0), true, false);
        assert_eq!(focus.gain, 1.0);
    }

    #[test]
    fn disabling_focus_recovers_gain_and_zero_time_does_not_advance() {
        let mut focus = VoiceFocus::default();
        assert!(!focus.update(Ticks::ZERO, true, true));
        focus.update(Ticks::from_f32(60.0), true, true);
        assert_eq!(focus.gain, 0.5);
        focus.update(Ticks::from_f32(1.0), false, true);
        assert!((focus.gain - 0.54).abs() < 0.00001);
        focus.update(Ticks::from_f32(60.0), false, true);
        assert_eq!(focus.gain, 1.0);
    }

    #[test]
    fn mixer_focus_multiplies_saved_gain_and_script_volume_then_recovers() {
        use kira::{
            AudioManager, AudioManagerSettings, Frame,
            sound::static_sound::{StaticSoundData, StaticSoundSettings},
            track::TrackBuilder,
        };
        use shin_audio::test_support::{MockBackendSettings, SampleBackend as MockBackend};
        let mut manager = AudioManager::<MockBackend>::new(AudioManagerSettings {
            backend_settings: MockBackendSettings { sample_rate: 1000 },
            internal_buffer_size: 1,
            ..Default::default()
        })
        .unwrap();
        let mut category = manager.add_sub_track(TrackBuilder::new()).unwrap();
        let mut track = category.add_sub_track(TrackBuilder::new()).unwrap();
        let mut settings = crate::audio::SettingsGain::default();
        settings.apply(&mut category, 70);
        let _sound = track
            .play(StaticSoundData {
                sample_rate: 1000,
                frames: vec![Frame::new(0.25, 0.25); 2000].into(),
                settings: StaticSoundSettings::new().volume(shin_audio::amplitude_to_decibels(0.5)),
                slice: None,
            })
            .unwrap();
        let advance = |manager: &mut AudioManager<MockBackend>| {
            let backend = manager.backend_mut();
            let mut frame = Frame::ZERO;
            for _ in 0..20 {
                backend.on_start_processing();
                frame = backend.process();
            }
            frame.left
        };
        let reference = advance(&mut manager);
        assert!(reference > 0.0);
        let mut focus = VoiceFocus::default();
        focus.update_track(&mut track, Ticks::from_f32(1.0), true, true);
        assert!((advance(&mut manager) / reference - 0.96).abs() < 0.0001);
        focus.update_track(&mut track, Ticks::from_f32(60.0), true, true);
        assert!((advance(&mut manager) / reference - 0.5).abs() < 0.0001);
        settings.apply(&mut category, 100);
        assert!((advance(&mut manager) / reference - 0.5 / 0.7).abs() < 0.0001);
        focus.update_track(&mut track, Ticks::from_f32(60.0), true, false);
        assert!((advance(&mut manager) / reference - 1.0 / 0.7).abs() < 0.0001);
    }
}
