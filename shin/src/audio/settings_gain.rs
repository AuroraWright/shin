use kira::{Tween, track::TrackHandle};

/// Native category gain multiplies the script-controlled sound volume.
/// See 0xc278c, 0xfef30 and 0x11a2fc; the shared constant is 0.01f.
#[derive(Default)]
pub struct SettingsGain {
    percent: Option<u8>,
}

impl SettingsGain {
    pub fn apply(&mut self, track: &mut TrackHandle, percent: u8) {
        if self.percent == Some(percent) {
            return;
        }
        track.set_volume(
            shin_audio::amplitude_to_decibels(f32::from(percent) * 0.01),
            Tween {
                duration: std::time::Duration::ZERO,
                ..Default::default()
            },
        );
        self.percent = Some(percent);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kira::{AudioManager, AudioManagerSettings, Frame, track::TrackBuilder};
    use shin_audio::test_support::{MockBackendSettings, SampleBackend as MockBackend};

    use shin_core::{
        format::audio::{AudioBuffer, AudioFrameSource},
        vm::command::types::{Pan, Volume},
    };

    struct ConstantPcm {
        frame: Frame,
        position: u32,
    }
    impl AudioFrameSource for ConstantPcm {
        fn max_frame_size(&self) -> usize {
            64
        }
        fn sample_rate(&self) -> u32 {
            1000
        }
        fn pre_skip(&self) -> u32 {
            0
        }
        fn pre_roll(&self) -> u32 {
            0
        }
        fn current_sample_position(&self) -> u32 {
            self.position
        }
        fn samples_seek(&mut self, position: u32) -> anyhow::Result<u32> {
            self.position = position;
            Ok(0)
        }
        fn read_frame(&mut self, buffer: &mut AudioBuffer) -> bool {
            let count = 64.min(2000 - self.position);
            for _ in 0..count {
                buffer.push((self.frame.left, self.frame.right));
            }
            self.position += count;
            count != 0
        }
    }
    fn data(frame: Frame, volume: Volume) -> shin_audio::AudioData<ConstantPcm> {
        shin_audio::AudioData {
            source: ConstantPcm { frame, position: 0 },
            settings: shin_audio::AudioSettings {
                paused: false,
                fade_in: shin_core::time::Tween::IMMEDIATE,
                loop_start: None,
                loop_count: 0,
                volume,
                pan: Pan::default(),
                playback_rate: 1.0,
            },
        }
    }

    fn advance(manager: &mut AudioManager<MockBackend>, samples: usize) -> Frame {
        let backend = manager.backend_mut();
        let mut frame = Frame::ZERO;
        for _ in 0..samples {
            backend.on_start_processing();
            frame = backend.process();
        }
        frame
    }

    #[test]
    fn category_gain_multiplies_sound_volume_and_keeps_muted_sounds_running() {
        let mut manager = AudioManager::<MockBackend>::new(AudioManagerSettings {
            backend_settings: MockBackendSettings { sample_rate: 1000 },
            internal_buffer_size: 1,
            ..Default::default()
        })
        .unwrap();
        let mut track = manager.add_sub_track(TrackBuilder::new()).unwrap();
        let mut gain = SettingsGain::default();
        gain.apply(&mut track, 100);
        let mut sound = track
            .play(data(Frame::new(0.25, 0.25), Volume(0.5))).unwrap();
        let reference = advance(&mut manager, 20);
        assert!(reference.left > 0.0);
        gain.apply(&mut track, 70);
        let scaled = advance(&mut manager, 20);
        assert!((scaled.left / reference.left - 0.7).abs() < 0.0001);
        sound.set_volume(
            Volume(0.25),
                shin_core::time::Tween::linear(shin_core::time::Ticks::from_millis(100.0)),
            )
            .unwrap();
        let halfway = advance(&mut manager, 50);
        assert!((halfway.left / reference.left - 0.525).abs() < 0.02);
        let scaled = advance(&mut manager, 70);
        assert!((scaled.left / reference.left - 0.35).abs() < 0.0001);
        gain.apply(&mut track, 0);
        let silence = advance(&mut manager, 20);
        assert_eq!(silence, Frame::ZERO);
        assert!(sound.position_millis() > 50);
        gain.apply(&mut track, 100);
        let resumed = advance(&mut manager, 20);
        assert!((resumed.left / reference.left - 0.5).abs() < 0.0001);
    }

    #[test]
    fn independent_category_tracks_do_not_change_each_others_gain() {
        let mut manager = AudioManager::<MockBackend>::new(AudioManagerSettings {
            backend_settings: MockBackendSettings { sample_rate: 1000 },
            internal_buffer_size: 1,
            ..Default::default()
        })
        .unwrap();
        let mut first = manager.add_sub_track(TrackBuilder::new()).unwrap();
        let mut second = manager.add_sub_track(TrackBuilder::new()).unwrap();
        let mut first_gain = SettingsGain::default();
        let mut second_gain = SettingsGain::default();
        first_gain.apply(&mut first, 70);
        second_gain.apply(&mut second, 55);
        let _first_sound = first
            .play(data(Frame::new(0.25, 0.0), Volume(1.0)))
            .unwrap();
        let _second_sound = second
            .play(data(Frame::new(0.0, 0.25), Volume(1.0)))
            .unwrap();
        let reference = advance(&mut manager, 20);
        first_gain.apply(&mut first, 0);
        let muted = advance(&mut manager, 20);
        assert_eq!(muted.left, 0.0);
        assert!((muted.right - reference.right).abs() < 0.0001);
        second_gain.apply(&mut second, 100);
        let changed = advance(&mut manager, 20);
        assert!((reference.right / changed.right - 0.55).abs() < 0.0001);
    }
}
