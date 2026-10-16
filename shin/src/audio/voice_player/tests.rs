use super::*;
use shin_core::format::audio::AudioBuffer;
struct ConstantSource {
    position: u32,
    length: u32,
}
impl AudioFrameSource for ConstantSource {
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
    fn read_frame(&mut self, destination: &mut AudioBuffer) -> bool {
        let end = (self.position + 64).min(self.length);
        let read = self.position < end;
        for _ in self.position..end {
            destination.push((0.25, 0.25));
        }
        self.position = end;
        read
    }
    fn samples_seek(&mut self, position: u32) -> anyhow::Result<u32> {
        self.position = position.min(self.length);
        Ok(0)
    }
    fn current_sample_position(&self) -> u32 {
        self.position
    }
}

#[test]
fn native_voice_patterns_use_ascii_case_and_full_string_wildcards() {
    for (pattern, name, expected) in [
        ("EP?/KIN*", "ep1/kin0001", true),
        ("a*b*c", "AXBYC", true),
        ("*", "", true),
        ("?", "", false),
        ("a?", "abc", false),
        ("*a", "ab", false),
        ("a**?c", "abc", true),
        ("é?", "é水", true),
        ("é?", "É水", false),
    ] {
        assert_eq!(
            voice_pattern_matches(pattern, name),
            expected,
            "{pattern}/{name}"
        );
    }
}
#[test]
fn character_voice_mask_uses_first_mapping_and_any_enabled_speaker() {
    use shin_core::format::{
        scenario::{info::VoiceMappingInfoItem, types::U8SmallList},
        text::U16String,
    };
    let mapping = |name, ids: Vec<u8>| VoiceMappingInfoItem {
        name_pattern: U16String::new(name),
        lipsync_character_ids: U8SmallList::from_contents(ids),
    };
    let mappings = vec![
        mapping("group/*", vec![2, 31]),
        mapping("group/special", vec![1]),
        mapping("empty", vec![]),
        mapping("wrapped", vec![34]),
    ];
    assert_eq!(
        voice_speakers(
            &mappings,
            "GROUP/special",
            VoicePlayFlags::ENABLE_CHARACTER_LIPSYNC
        ),
        [2, 31]
    );
    assert!(
        voice_speakers(
            &mappings,
            "group/other",
            VoicePlayFlags::ENABLE_CHARACTER_MUTING
        )
        .is_empty()
    );
    assert!(voice_speakers(&mappings, "unknown", VoicePlayFlags::all()).is_empty());
    assert!(speaker_matches(&[2, 255], 255));
    assert!(!speaker_matches(&[2, 255], -1));
    assert!(!speaker_matches(&[2, 255], 258));
    assert!(!character_voice_enabled(&mappings, "GROUP/special", 1 << 1));
    assert!(character_voice_enabled(&mappings, "group/special", 1 << 2));
    assert!(character_voice_enabled(&mappings, "group/other", 1 << 31));
    assert!(!character_voice_enabled(&mappings, "group/other", 0));
    assert!(character_voice_enabled(&mappings, "unknown", 0));
    assert!(!character_voice_enabled(&mappings, "empty", u32::MAX));
    assert!(character_voice_enabled(&mappings, "wrapped", 1 << 2));
}
#[test]
#[ignore = "requires supplied Umineko ROM and executable"]
fn reference_native_voice_character_mask() {
    use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
    use std::io::Read;
    let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let mut rom =
        RomReader::new(StatelessFile::new(std::fs::File::open(&path).unwrap()).unwrap())
            .unwrap();
    let entry = rom.find_file("/main.snr").unwrap();
    let mut bytes = Vec::new();
    rom.open_file(entry)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    let scenario = Scenario::new(bytes.into()).unwrap();
    let mappings = &scenario.info_tables().voice_mapping_info;
    let item = mappings
        .iter()
        .find(|m| voice_pattern_matches(m.name_pattern.as_str(), "19/11900001"))
        .unwrap();
    assert!(!item.lipsync_character_ids.0.is_empty());
    assert!(character_voice_enabled(mappings, "19/11900001", u32::MAX));
    assert!(!character_voice_enabled(mappings, "19/11900001", 0));
    println!(
        "Native voice mappings:{};19/11900001 speakers:{:?}",
        mappings.len(),
        item.lipsync_character_ids
    );
    let exe = std::fs::read(
        std::path::Path::new(&path)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    for (a, bits) in [
        (0x159974, 0x3e4ccccd),
        (0x159594, 0x3ee66666),
        (0x159ae8, 0x3f0ccccd),
        (0x159570, 0x3dcccccd),
        (0x15946c, 0x3df5c28f),
        (0x1597d4, 0x38000000),
    ] {
        assert_eq!(
            u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap()),
            bits,
            "native lipsync constant{a:x}"
        );
    }
    let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
    for (a, value) in [
        (0x119e20, 0x7100053f),
        (0x119e24, 0x540000c0),
        (0x119e3c, 0xb90012a8),
        (0x119e64, 0x97ff6aef),
        (0x119eb4, 0xb90012b8),
        (0x11a2cc, 0x35001348),
        (0x11a300, 0xbd497501),
        (0x11a304, 0x1e210809),
        (0x11a310, 0x7100093f),
        (0x11a31c, 0x1e293800),
        (0x11a324, 0xbd001500),
        (0x11a32c, 0xb900151f),
        (0x11a338, 0x1e200900),
        (0x11a33c, 0x97ff6a49),
        (0x11a40c, 0x54000160),
        (0x11a43c, 0x1e202008),
        (0x11a448, 0x97ff6976),
        (0x11a14c, 0x7100011f),
        (0x11a154, 0x320007e0),
        (0x11a160, 0xb9400be8),
        (0x11a16c, 0x531f7900),
        (0xf4ecc, 0xd364fd16),
        (0xf4f78, 0x1b1631ac),
        (0xf4fc4, 0x7c5fc227),
        (0xf4fdc, 0x1e2508e7),
        (0xf4fe0, 0x1e20c0e7),
        (0xf5088, 0x395062ac),
        (0xf50a0, 0x1e2420c0),
        (0xf50a4, 0x5400010c),
        (0xf50ac, 0x1e2220c0),
        (0xf50b0, 0x540000ed),
        (0xf50bc, 0x1e2020c0),
        (0xf50c0, 0x54000125),
        (0xf50cc, 0x1e2320c0),
        (0xf50d0, 0x540000a5),
        (0xf50ec, 0x7100117f),
        (0xf50fc, 0x54000081),
        (0xf5124, 0x390ba1ac),
        (0xf4de8, 0xd344fc08),
        (0xf4df4, 0x9bc97d08),
        (0xf4dfc, 0xd348fd08),
        (0xf4e1c, 0x394ba114),
        (0x1196c0, 0x7100053f),
        (0x1196d0, 0x97ff43dc),
        (0x119714, 0x17ff6da7),
        (0x119724, 0x12800000),
        (0x36adc, 0x52800361),
        (0x36ae4, 0x1e380008),
        (0x36ae8, 0x34000828),
        (0x36b50, 0x1b087e89),
        (0x36b5c, 0x9b2a7d29),
        (0x36b74, 0x1a8ab128),
        (0x36b80, 0xbd40f660),
        (0x36bec, 0xf900767f),
        (0x36ea0, 0xb980ea89),
        (0x36ea4, 0xf8697908),
        (0x119654, 0xf9424908),
        (0x119658, 0x3942d508),
        (0x119668, 0x1e200c28),
        (0x119688, 0x97ff43ee),
        (0x119698, 0x97ff6d8e),
        (0x119960, 0x7200011f),
        (0x119964, 0x9a8003f5),
        (0xea64c, 0x7103fc3f),
        (0xea650, 0x54000168),
        (0xea670, 0x6b01017f),
        (0x14ed0, 0x940086b0),
        (0x14edc, 0x940086b1),
        (0x14ef4, 0x140411cf),
        (0x369fc, 0x1e2e1001),
        (0x36a04, 0x1e215801),
        (0x36a08, 0x1e222000),
        (0x36a0c, 0x1e228c28),
        (0xf4cf8, 0xbd02a688),
        (0x119918, 0x37080de8),
        (0x119c2c, 0x97ff41f5),
        (0x119c34, 0xb4fff560),
        (0x119c40, 0xb940c501),
        (0x119c48, 0x97ff4292),
        (0x119c4c, 0x3707f4a0),
        (0xea414, 0xa951d413),
        (0xea444, 0x9400000f),
        (0xea448, 0x370000a0),
        (0xea4c0, 0x7100a81f),
        (0xea4cc, 0x7100fc1f),
        (0xea4e0, 0x510186a8),
        (0xea4ec, 0x1a953128),
        (0xea4fc, 0x1a803149),
        (0xea51c, 0xaa1403e0),
        (0xea530, 0x97ffffd4),
        (0xea534, 0x37000720),
        (0xea60c, 0x94002385),
        (0xea614, 0x1a9f17f5),
        (0xea690, 0xb40001a0),
        (0xea694, 0xf9400408),
        (0xea698, 0x38401509),
        (0xea6a0, 0x320003e0),
        (0xea6ac, 0x386a690b),
        (0xea6b4, 0x1acb200b),
        (0xea6b8, 0x6a01017f),
        (0xea6bc, 0x54ffff40),
        (0xea6c4, 0x320003e0),
        (0xea6cc, 0x2a1f03e0),
    ] {
        assert_eq!(word(a), value, "Native voice mask{a:x}");
    }
}

#[test]
fn matched_voice_panning_changes_pcm_and_setting_centers_only_matches() {
    use kira::{AudioManager as KiraManager, AudioManagerSettings};
    use shin_audio::test_support::{MockBackendSettings, SampleBackend as MockBackend};
    let mut manager = KiraManager::<MockBackend>::new(AudioManagerSettings {
        backend_settings: MockBackendSettings { sample_rate: 1000 },
        internal_buffer_size: 1,
        ..Default::default()
    })
    .unwrap();
    let handle = manager
        .play(AudioData {
            source: ConstantSource {
                position: 0,
                length: 1000,
            },
            settings: AudioSettings {
                paused: false,
                playback_rate: 1.0,
                fade_in: Tween::IMMEDIATE,
                loop_start: None,
                loop_count: 0,
                volume: Volume::default(),
                pan: Pan::default(),
            },
        })
        .unwrap();
    let mut voice = PlayingVoice {
        handle,
        speakers: vec![19, 42],
        lipsync: vec![],
        volume: 1.0,
        stopping: false,
        pan: Pan::default(),
    };
    let mut sample = |voice: &mut PlayingVoice, id, pan, enabled| {
        voice.pan_for_character(id, pan, enabled);
        let backend = manager.backend_mut();
        let mut frame = kira::Frame::ZERO;
        for _ in 0..16 {
            backend.on_start_processing();
            frame = backend.process();
        }
        (frame.left, frame.right)
    };
    let hard = 0.25 * std::f32::consts::SQRT_2;
    for (id, pan, enabled, left, right) in [
        (19, -1.0, true, hard, 0.0),
        (7, 1.0, true, hard, 0.0),
        (275, 1.0, true, hard, 0.0),
        (42, 1.0, true, 0.0, hard),
        (7, 0.0, false, 0.0, hard),
        (19, 1.0, false, 0.25, 0.25),
    ] {
        let (l, r) = sample(&mut voice, id, pan, enabled);
        assert!(
            (l - left).abs() < 0.00001 && (r - right).abs() < 0.00001,
            "id={id}, enabled={enabled}, PCM={l},{r}"
        );
    }
}

#[test]
fn lipsync_windows_take_both_channels_and_ignore_incomplete_tail() {
    struct Peaks {
        position: u32,
    }
    impl AudioFrameSource for Peaks {
        fn max_frame_size(&self) -> usize {
            37
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
        fn read_frame(&mut self, buffer: &mut AudioBuffer) -> bool {
            let peaks = [0.13, 0.6, 0.5, 0.44, 0.11, 0.11, 0.11, 0.11, 0.13, 0.0, 1.0];
            let end = (self.position + 37).min(525);
            let read = self.position < end;
            while self.position < end {
                buffer.push((0.0, peaks[(self.position / 50) as usize]));
                self.position += 1;
            }
            read
        }
        fn samples_seek(&mut self, position: u32) -> anyhow::Result<u32> {
            self.position = position;
            Ok(0)
        }
        fn current_sample_position(&self) -> u32 {
            self.position
        }
    }
    assert_eq!(
        lipsync_windows(Peaks { position: 0 }),
        [1, 2, 2, 1, 1, 1, 1, 0, 1, 0]
    );
}

#[test]
fn stopping_voice_fades_absolute_request_volume_while_new_voice_remains_active() {
    use kira::{AudioManager as KiraManager, AudioManagerSettings};
    use shin_audio::test_support::{MockBackendSettings, SampleBackend as MockBackend};
    let mut manager = KiraManager::<MockBackend>::new(AudioManagerSettings {
        backend_settings: MockBackendSettings { sample_rate: 1000 },
        internal_buffer_size: 1,
        ..Default::default()
    })
    .unwrap();
    let mut voices = Vec::new();
    for (volume, stopping, level) in [(0.6, true, 1), (0.5, false, 2)] {
        let handle = manager
            .play(AudioData {
                source: ConstantSource {
                    position: 0,
                    length: 1000,
                },
                settings: AudioSettings {
                    paused: false,
                    playback_rate: 1.0,
                    fade_in: Tween::IMMEDIATE,
                    loop_start: None,
                    loop_count: 0,
                    volume: Volume(volume),
                    pan: Pan::default(),
                },
            })
            .unwrap();
        voices.push(PlayingVoice {
            handle,
            volume,
            stopping,
            speakers: vec![19],
            lipsync: vec![level],
            pan: Pan::default(),
        });
    }
    assert_eq!(
        character_lipsync(&voices, 19),
        Some(2),
        "The audible stopping tail must not drive mouth animation"
    );
    for (ticks, expected, finished) in [
        (0.0, 0.6, false),
        (1.0, 0.4, false),
        (0.5, 0.3, false),
        (2.0, 0.0, true),
    ] {
        assert_eq!(
            voices[0].update_stop(Ticks::from_f32(ticks), false),
            finished
        );
        assert!((voices[0].volume - expected).abs() < 0.000001);
        assert!(!voices[1].update_stop(Ticks::from_f32(ticks), false));
        assert_eq!(voices[1].volume, 0.5);
        let backend = manager.backend_mut();
        let mut frame = kira::Frame::ZERO;
        for _ in 0..16 {
            backend.on_start_processing();
            frame = backend.process();
        }
        let expected_pcm = (expected + 0.5) * 0.25;
        assert!(
            (frame.left - expected_pcm).abs() < 0.00001
                && (frame.right - expected_pcm).abs() < 0.00001,
            "tick{ticks}:PCM{frame:?}"
        );
    }
    assert!(voices[0].handle.get_wait_status().is_empty());
    assert!(
        voices[1]
            .handle
            .get_wait_status()
            .contains(AudioWaitStatus::PLAYING)
    );
}

#[test]
fn native_lipsync_thresholds_are_strict_and_half_open_times_out() {
    let mut detector = VoiceLipSyncDetector::default();
    for (peak, expected) in [
        (0.12, 0),
        (0.13, 1),
        (0.55, 1),
        (0.56, 2),
        (0.45, 2),
        (0.44, 1),
        (0.1, 1),
        (0.11, 1),
        (0.11, 1),
        (0.11, 0),
        (0.13, 1),
        (0.09, 0),
    ] {
        assert_eq!(detector.window(peak), expected, "peak{peak}");
    }
    assert_eq!(detector.window(1.0), 1); // even a loud onset first enters state1
    assert_eq!(detector.window(1.0), 2);
    for _ in 0..20 {
        assert_eq!(detector.window(1.0), 2);
    }
}

#[test]
fn lipsync_uses_first_matching_playing_voice_and_integer_window_positions() {
    use kira::{AudioManager as KiraManager, AudioManagerSettings};
    use shin_audio::test_support::{MockBackendSettings, SampleBackend as MockBackend};
    let mut manager = KiraManager::<MockBackend>::new(AudioManagerSettings {
        backend_settings: MockBackendSettings { sample_rate: 1000 },
        internal_buffer_size: 1,
        ..Default::default()
    })
    .unwrap();
    let mut voices = Vec::new();
    for (speakers, length, lipsync) in [
        (vec![], 1000, vec![2]),
        (vec![19, 42], 200, vec![0, 1, 2]),
        (vec![19], 1000, vec![2]),
    ] {
        let handle = manager
            .play(AudioData {
                source: ConstantSource {
                    position: 0,
                    length,
                },
                settings: AudioSettings {
                    paused: false,
                    playback_rate: 1.0,
                    fade_in: Tween::IMMEDIATE,
                    loop_start: None,
                    loop_count: 0,
                    volume: Volume::default(),
                    pan: Pan::default(),
                },
            })
            .unwrap();
        voices.push(PlayingVoice {
            handle,
            speakers,
            lipsync,
            volume: 1.0,
            stopping: false,
            pan: Pan::default(),
        });
    }
    assert_eq!(character_lipsync(&voices, 7), None);
    assert_eq!(character_lipsync(&voices, 19), Some(0));
    let backend = manager.backend_mut();
    for (samples, expected) in [(49, 0), (1, 1), (50, 2), (60, 2)] {
        for _ in 0..samples {
            backend.on_start_processing();
            let _ = backend.process();
        }
        backend.on_start_processing();
        assert_eq!(character_lipsync(&voices, 19), Some(expected));
        assert_eq!(character_lipsync(&voices, 42), Some(expected));
    }
    for _ in 0..100 {
        backend.on_start_processing();
        let _ = backend.process();
    }
    backend.on_start_processing();
    assert_eq!(character_lipsync(&voices, 42), None);
    assert_eq!(character_lipsync(&voices, 19), Some(2));
}

#[test]
fn resolves_simultaneous_voice_files() {
    assert_eq!(
        voice_paths("10/10100255|13/10400165"),
        ["/voice/10/10100255.nxa", "/voice/13/10400165.nxa"]
    );
    assert_eq!(voice_paths("41/RG4_00010"), ["/voice/41/rg4_00010.nxa"]);
    assert!(voice_paths("").is_empty());
}

#[test]
#[ignore = "requires the supplied Umineko ROM (SHIN_REFERENCE_ROM)"]
fn reference_nxa_playback_factories_use_native_pcm16() {
    use shin_core::{
        format::{audio::read_audio, rom::RomReader},
        primitives::stateless_reader::StatelessFile,
    };
    use std::io::Read;
    let mut rom = RomReader::new(
        StatelessFile::new(
            std::fs::File::open(std::env::var_os("SHIN_REFERENCE_ROM").unwrap()).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let mut compared = 0usize;
    let mut float_differences = 0usize;
    for path in [
        "/bgm/umib_055.nxa",
        "/se/umise_028.nxa",
        "/voice/19/11900001.nxa",
    ] {
        let mut bytes = Vec::new();
        rom.open_file(rom.find_file(path).unwrap())
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let file = Arc::new(read_audio(&bytes).unwrap());
        let settings = AudioSettings {
            paused: false,
            playback_rate: 1.0,

            fade_in: Tween::IMMEDIATE,
            loop_start: None,
            loop_count: 0,
            volume: Volume::default(),
            pan: Pan::default(),
        };
        // BGM and SE share this public playback factory. Voice segments
        // use a separate factory, so compare both against the signed API.
        let mut playback = AudioSource::new(
            AudioData::from_audio_file(file.clone(), settings)
                .unwrap()
                .source,
        );
        let mut voice = AudioSource::new(voice_segment(file.clone(), 0, 0).unwrap());
        let mut expected = AudioSource::new(AudioDecoder::new_pcm16(file.clone()).unwrap());
        let mut floating = AudioSource::new(AudioDecoder::new(file.clone()).unwrap());
        let mut differences = 0usize;
        for position in [
            0,
            123,
            u32::from(file.info().frame_samples) * 3,
            4590,
            4799,
            4800,
        ] {
            playback.samples_seek(position).unwrap();
            voice.samples_seek(position).unwrap();
            expected.samples_seek(position).unwrap();
            floating.samples_seek(position).unwrap();
            for _ in 0..4096 {
                let pcm = expected.read_sample();
                assert_eq!(
                    playback.read_sample(),
                    pcm,
                    "BGM/SE factory {path} at {position}"
                );
                assert_eq!(
                    voice.read_sample(),
                    pcm,
                    "Voice factory {path} at {position}"
                );
                differences += usize::from(floating.read_sample() != pcm);
                if let Some((left, right)) = pcm {
                    for sample in [left, right] {
                        assert_eq!((sample * 32768.0).trunc(), sample * 32768.0);
                        assert!((-1.0..1.0).contains(&sample));
                    }
                    compared += 1;
                }
            }
        }
        assert!(
            differences > 0,
            "Fixture must distinguish PCM16 from float decode: {path}"
        );
        float_differences += differences;
        println!(
            "Native PCM16 playback factory: {path}, channels{}, float differences{differences}",
            file.info().channel_count
        );
    }
    println!(
        "Playback factory comparison: {compared} frames, {float_differences} float differences; no audio device opened"
    );
}

#[test]
#[ignore = "requires the supplied Umineko ROM (SHIN_REFERENCE_ROM)"]
fn reference_voice_segments_decode_and_finish_playback() {
    use kira::{AudioManager as KiraManager, AudioManagerSettings};
    use shin_audio::test_support::{MockBackendSettings, SampleBackend as MockBackend};
    use shin_core::{
        format::{
            audio::{AudioSource, read_audio},
            rom::RomReader,
        },
        primitives::stateless_reader::StatelessFile,
    };
    use std::{fs::File, io::Read};

    let path = std::env::var("SHIN_REFERENCE_ROM").expect("Set SHIN_REFERENCE_ROM");
    let mut rom =
        RomReader::new(StatelessFile::new(File::open(path).unwrap()).unwrap()).unwrap();
    let entry = rom.find_file("/voice/19/11900001.nxa").unwrap();
    let mut bytes = Vec::new();
    rom.open_file(entry)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    let file = Arc::new(read_audio(&bytes).unwrap());
    let begin = std::time::Instant::now();
    let levels = voice_lipsync(file.clone(), 0, 0).unwrap();
    println!("Reference lipsync timeline load cost {:?}", begin.elapsed());
    assert!(!levels.is_empty());
    assert!(levels.iter().all(|&level| level <= 2));
    println!(
        "Native PCM16 voice lipsync:{} windows, counts{:?}, first{:?}",
        levels.len(),
        [0, 1, 2].map(|level| levels.iter().filter(|&&value| value == level).count()),
        &levels[..levels.len().min(30)]
    );
    assert!(voice_lipsync(file.clone(), 0, 49).unwrap().is_empty());
    assert_eq!(voice_lipsync(file.clone(), 10, 175).unwrap().len(), 3);
    let rate = file.info().sample_rate;
    let length = rate / 50; // 20 milliseconds

    let mut expected = AudioSource::new(AudioDecoder::new_pcm16(file.clone()).unwrap());
    expected.samples_seek(rate / 100).unwrap(); // 10 milliseconds
    let mut segment = AudioSource::new(voice_segment(file.clone(), 10, 20).unwrap());
    for _ in 0..length {
        assert_eq!(segment.read_sample(), expected.read_sample());
    }
    assert_eq!(segment.read_sample(), None);

    // Exercise the same AudioData/AudioSound path used by VoicePlayer,
    // without opening an audio device or constructing a GPU asset server.
    let mut manager = KiraManager::<MockBackend>::new(AudioManagerSettings {
        backend_settings: MockBackendSettings { sample_rate: rate },
        internal_buffer_size: 1,
        ..Default::default()
    })
    .unwrap();
    let handle = manager
        .play(AudioData {
            source: voice_segment(file, 10, 20).unwrap(),
            settings: AudioSettings {
                paused: false,
                playback_rate: 1.0,
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
    for _ in 0..length + 32 {
        backend.on_start_processing();
        let frame = backend.process();
        assert!(frame.left.is_finite() && frame.right.is_finite());
    }
    assert!(handle.get_wait_status().is_empty());
}
