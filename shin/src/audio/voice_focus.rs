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
                settings: StaticSoundSettings::new()
                    .volume(shin_audio::amplitude_to_decibels(0.5)),
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
