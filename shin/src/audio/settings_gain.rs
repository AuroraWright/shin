use kira::{Volume, track::TrackHandle, tween::Tween};

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
            Volume::Amplitude(f64::from(f32::from(percent) * 0.01)),
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
    use kira::{
        Frame,
        manager::{
            AudioManager, AudioManagerSettings,
            backend::mock::{MockBackend, MockBackendSettings},
        },
        sound::static_sound::{StaticSoundData, StaticSoundSettings},
        track::TrackBuilder,
    };

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
            ..Default::default()
        })
        .unwrap();
        let mut track = manager.add_sub_track(TrackBuilder::new()).unwrap();
        let mut gain = SettingsGain::default();
        gain.apply(&mut track, 100);
        let data = StaticSoundData {
            sample_rate: 1000,
            frames: vec![Frame::new(0.25, 0.25); 2000].into(),
            settings: StaticSoundSettings::new()
                .output_destination(track.id())
                .volume(Volume::Amplitude(0.5)),
            slice: None,
        };
        let mut sound = manager.play(data).unwrap();
        let reference = advance(&mut manager, 20);
        assert!(reference.left > 0.0);
        gain.apply(&mut track, 70);
        let scaled = advance(&mut manager, 20);
        assert!((scaled.left / reference.left - 0.7).abs() < 0.0001);
        sound.set_volume(
            Volume::Amplitude(0.25),
            Tween {
                duration: std::time::Duration::from_millis(100),
                ..Default::default()
            },
        );
        let halfway = advance(&mut manager, 50);
        assert!((halfway.left / reference.left - 0.525).abs() < 0.02);
        let scaled = advance(&mut manager, 70);
        assert!((scaled.left / reference.left - 0.35).abs() < 0.0001);
        gain.apply(&mut track, 0);
        let silence = advance(&mut manager, 20);
        assert_eq!(silence, Frame::ZERO);
        assert!(sound.position() > 0.05);
        gain.apply(&mut track, 100);
        let resumed = advance(&mut manager, 20);
        assert!((resumed.left / reference.left - 0.5).abs() < 0.0001);
    }

    #[test]
    fn independent_category_tracks_do_not_change_each_others_gain() {
        let mut manager = AudioManager::<MockBackend>::new(AudioManagerSettings {
            backend_settings: MockBackendSettings { sample_rate: 1000 },
            ..Default::default()
        })
        .unwrap();
        let mut first = manager.add_sub_track(TrackBuilder::new()).unwrap();
        let mut second = manager.add_sub_track(TrackBuilder::new()).unwrap();
        let mut first_gain = SettingsGain::default();
        let mut second_gain = SettingsGain::default();
        first_gain.apply(&mut first, 70);
        second_gain.apply(&mut second, 55);
        let data = |track, frame| StaticSoundData {
            sample_rate: 1000,
            frames: vec![frame; 2000].into(),
            settings: StaticSoundSettings::new().output_destination(track),
            slice: None,
        };
        let _first_sound = manager
            .play(data(first.id(), Frame::new(0.25, 0.0)))
            .unwrap();
        let _second_sound = manager
            .play(data(second.id(), Frame::new(0.0, 0.25)))
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
