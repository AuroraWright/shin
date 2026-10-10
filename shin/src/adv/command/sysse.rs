use super::prelude::*;

impl StartableCommand for command::runtime::SYSSE {
    type StateInfo = ();

    fn apply_state(&self, _state: &mut VmState) {}

    fn start(
        self,
        context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &VmState,
        _state_info: (),
        adv_state: &mut AdvState,
    ) -> CommandStartResult {
        // Original command at 0x77d0 only plays ID 0 (the horror sound).
        if self.sys_se_id == 0 {
            adv_state.play_system_sound(context.asset_server, "horror", self.volume);
        }
        self.token.finish().into()
    }
}

#[cfg(test)]
mod tests {
    use kira::manager::{
        AudioManager, AudioManagerSettings,
        backend::mock::{MockBackend, MockBackendSettings},
    };
    use shin_audio::{AudioData, AudioSettings};
    use shin_core::{
        format::{audio::AudioSource, rom::RomReader},
        primitives::stateless_reader::StatelessFile,
        vm::command::types::{AudioWaitStatus, Volume},
    };
    use shin_core::{time::Tween, vm::command::types::Pan};
    use std::{fs::File, io::Read};

    #[test]
    #[ignore = "requires the supplied Umineko ROM (SHIN_REFERENCE_ROM)"]
    fn reference_horror_sound_decodes_and_finishes() {
        check_reference_sound("horror", 312623);
    }
    #[test]
    #[ignore = "requires supplied ROM (SHIN_REFERENCE_ROM) and native executable; verifies UI sounds"]
    fn reference_ui_sounds_match_native_names_decode_and_finish() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let exe = std::fs::read(root.join("umineko_switch_files/uncompressed_executable")).unwrap();
        let relocations: std::collections::HashMap<_, _> = exe[0x11e040 + 256..0x152eb8 + 256]
            .chunks_exact(24)
            .map(|r| {
                (
                    u64::from_le_bytes(r[..8].try_into().unwrap()),
                    u64::from_le_bytes(r[16..24].try_into().unwrap()),
                )
            })
            .collect();
        for (got, name, count) in [
            (0x224780, "move", 3876),
            (0x2246e0, "decide", 4114),
            (0x224778, "cancel", 7290),
            (0x224ae8, "execute", 31355),
            (0x224f00, "attack", 110291),
            (0x224ef8, "ahaha", 43996),
        ] {
            let address = relocations[&relocations[&got]] as usize + 256;
            assert_eq!(&exe[address..address + name.len()], name.as_bytes());
            assert_eq!(exe[address + name.len()], 0);
            check_reference_sound(name, count);
        }
    }
    #[test]
    #[ignore = "requires supplied ROM and native executable; no GPU required"]
    fn reference_native_sysse_global_player_gain_capture_and_tail() {
        let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&path)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let word = |address: usize| {
            u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap())
        };
        for (address, opcode) in [
            (0x272b4, 0xf9440508),
            (0x272bc, 0xc8dffd00),
            (0x272c0, 0x14035ea8),
            (0xfeef4, 0x1e202108),
            (0xfeefc, 0x1e205900),
            (0xfef00, 0x3d42cd09),
            (0xfef04, 0x1e218c08),
            (0xfef30, 0x7e21d920),
            (0xfef38, 0xbd494901),
            (0xfef3c, 0x1e210803),
            (0xfef40, 0x1e230900),
            (0xfef50, 0x97fffce4),
            (0xfef7c, 0xf9000136),
            (0x1aa20, 0x94006378),
            (0x1aa2c, 0x94006481),
            (0x1aa38, 0x94035c82),
        ] {
            assert_eq!(word(address), opcode, "native system cue {address:x}");
        }
        let mut rom =
            RomReader::new(StatelessFile::new(File::open(path).unwrap()).unwrap()).unwrap();
        let mut bytes = Vec::new();
        rom.open_file(rom.find_file("/sysse.bin").unwrap())
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let bank = shin_core::format::sysse::read_sys_se(&bytes).unwrap();
        let sound = &bank.sounds["horror"];
        let new_manager = || {
            AudioManager::<MockBackend>::new(AudioManagerSettings {
                backend_settings: MockBackendSettings {
                    sample_rate: sound.sample_rate(),
                },
                ..Default::default()
            })
            .unwrap()
        };
        let mut reference = new_manager();
        let mut actual = new_manager();
        let track = actual
            .add_sub_track(kira::track::TrackBuilder::new())
            .unwrap();
        let data = |volume, track| AudioData {
            source: sound.decode(),
            settings: AudioSettings {
                paused: false,
                playback_rate: 1.0,
                track,
                fade_in: Tween::IMMEDIATE,
                loop_start: None,
                loop_count: 0,
                volume,
                pan: Pan::default(),
            },
        };
        let _reference = reference
            .play(data(Volume(1.0), kira::track::TrackId::Main))
            .unwrap();
        let cue = actual
            .play(data(
                crate::audio::system_cue_volume(Volume(0.5), 70),
                track.id(),
            ))
            .unwrap();
        let tail = crate::audio::SystemSoundTail {
            _track: track,
            sounds: vec![cue],
        };
        let mut energy = 0.0f64;
        for _ in 0..10000 {
            reference.backend_mut().on_start_processing();
            actual.backend_mut().on_start_processing();
            let expected = reference.backend_mut().process();
            let value = actual.backend_mut().process();
            assert!((value.left - expected.left * 0.35).abs() < 0.00001);
            assert!((value.right - expected.right * 0.35).abs() < 0.00001);
            energy += f64::from(value.left.abs() + value.right.abs());
        }
        assert!(energy > 0.01, "real reference PCM must be audible");
        let before = tail.sounds[0].position_millis();
        // A subsequent cue captures the new muted setting. Existing PCM must
        // retain its captured gain while the retired bus remains owned.
        let _muted = actual
            .play(data(
                crate::audio::system_cue_volume(Volume(1.0), 0),
                kira::track::TrackId::Main,
            ))
            .unwrap();
        for _ in 0..1000 {
            reference.backend_mut().on_start_processing();
            actual.backend_mut().on_start_processing();
            let expected = reference.backend_mut().process();
            let value = actual.backend_mut().process();
            assert!((value.left - expected.left * 0.35).abs() < 0.00001);
            assert!((value.right - expected.right * 0.35).abs() < 0.00001);
        }
        assert!(tail.sounds[0].position_millis() > before);
        drop(tail);
        actual.backend_mut().on_start_processing();
        for _ in 0..32 {
            actual.backend_mut().on_start_processing();
            assert_eq!(actual.backend_mut().process(), kira::Frame::ZERO);
        }
        assert_eq!(
            crate::audio::system_cue_volume(Volume(2.0), 70),
            Volume(0.7)
        );
        assert_eq!(
            crate::audio::system_cue_volume(Volume(-1.0), 70),
            Volume(0.0)
        );
        assert_eq!(
            crate::audio::system_cue_volume(Volume(f32::NAN), 70),
            Volume(0.0)
        );
    }

    fn check_reference_sound(name: &str, expected_samples: u32) {
        let path = std::env::var("SHIN_REFERENCE_ROM").expect("Set SHIN_REFERENCE_ROM");
        let mut rom =
            RomReader::new(StatelessFile::new(File::open(path).unwrap()).unwrap()).unwrap();
        let entry = rom.find_file("/sysse.bin").unwrap();
        let mut bytes = Vec::new();
        rom.open_file(entry)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let bank = shin_core::format::sysse::read_sys_se(&bytes).unwrap();
        let sound = &bank.sounds[name];
        assert_eq!(sound.sample_count(), expected_samples);
        let mut source = AudioSource::new(sound.decode());
        let mut count = 0;
        while let Some((left, right)) = source.read_sample() {
            assert!(left.is_finite() && right.is_finite());
            count += 1;
        }
        assert_eq!(count, sound.sample_count());
        let mut manager = AudioManager::<MockBackend>::new(AudioManagerSettings {
            backend_settings: MockBackendSettings {
                sample_rate: sound.sample_rate(),
            },
            ..Default::default()
        })
        .unwrap();
        let handle = manager
            .play(AudioData {
                source: sound.decode(),
                settings: AudioSettings {
                    paused: false,
                    playback_rate: 1.0,
                    track: kira::track::TrackId::Main,
                    fade_in: Tween::IMMEDIATE,
                    loop_start: None,
                    loop_count: 0,
                    volume: Volume::default(),
                    pan: Pan::default(),
                },
            })
            .unwrap();
        assert!(handle.get_wait_status().contains(AudioWaitStatus::PLAYING));
        let backend = manager.backend_mut();
        for _ in 0..count + 32 {
            backend.on_start_processing();
            let frame = backend.process();
            assert!(frame.left.is_finite() && frame.right.is_finite());
        }
        assert!(handle.get_wait_status().is_empty());
    }
}
