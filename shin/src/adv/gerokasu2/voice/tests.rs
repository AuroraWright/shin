use super::*;
use crate::audio::{
    character_voice_enabled, native_voice_clock, voice_pattern_matches, voice_speakers,
    voice_stop_volume,
};
use shin_core::{
    format::{
        audio::read_audio,
        rom::{IndexEntry, RomReader},
        scenario::types::U8SmallList,
    },
    primitives::stateless_reader::StatelessFile,
    vm::{
        IntoRuntimeForm, VmCtx,
        command::{CompiletimeCommand, compiletime},
        gerokasu2::Gerokasu2RuntimeCommand as G,
    },
};
use std::{
    io::{Read, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    time::{Duration, Instant},
};

fn rom() -> RomReader<StatelessFile> {
    RomReader::new(
        StatelessFile::new(
            std::fs::File::open(std::env::var_os("SHIN_GEROKASU2_ROM").unwrap()).unwrap(),
        )
        .unwrap(),
    )
    .unwrap()
}
fn scenario() -> Gerokasu2Scenario {
    let mut rom = rom();
    let mut bytes = Vec::new();
    let entry = rom.find_file("/main.snr").unwrap();
    rom.open_file(entry)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    Gerokasu2Scenario::new(bytes.into()).unwrap()
}
fn voice_names() -> Vec<String> {
    rom()
        .traverse()
        .filter_map(|(path, entry)| matches!(entry, IndexEntry::File(_)).then_some(path))
        .filter(|p| p.starts_with("/voice/") && p.ends_with(".nxa"))
        .map(|p| p[7..p.len() - 4].to_owned())
        .collect()
}
struct Native {
    child: Child,
    input: Option<ChildStdin>,
    output: ChildStdout,
    rows: usize,
    words: usize,
}
impl Native {
    fn new() -> Self {
        let mut child = Command::new(std::env::var_os("SHIN_GEROKASU2_VOICE_ORACLE").unwrap())
            .args([
                std::env::var_os("SHIN_GEROKASU2_EXECUTABLE").unwrap(),
                std::env::var_os("SHIN_GEROKASU2_NORMAL_FONT").unwrap(),
                std::env::var_os("SHIN_GEROKASU2_BOLD_FONT").unwrap(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        Self {
            input: child.stdin.take(),
            output: child.stdout.take().unwrap(),
            child,
            rows: 0,
            words: 0,
        }
    }
    fn input(&mut self, row: [u64; 8], tail: &[u8]) {
        let input = self.input.as_mut().unwrap();
        for n in row {
            input.write_all(&n.to_le_bytes()).unwrap();
        }
        input.write_all(tail).unwrap();
        input.flush().unwrap();
    }
    fn compare(&mut self, expected: &[u32]) {
        let actual = (0..expected.len())
            .map(|_| {
                let mut bytes = [0; 4];
                self.output.read_exact(&mut bytes).unwrap();
                u32::from_le_bytes(bytes)
            })
            .collect::<Vec<_>>();
        assert_eq!(actual, expected, "Native voice row{}", self.rows);
        self.rows += 1;
        self.words += expected.len();
    }
    fn finish(mut self, group: &str) {
        self.input.take();
        assert!(self.child.wait().unwrap().success());
        println!(
            "GEROKASU2_VOICE group={group} rows={} words={} original_mapping_clocks_status_MSGSYNC_command_polls=verified",
            self.rows, self.words
        );
    }
}
impl Drop for Native {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn supplied_nxa_headers_and_scenario_mapping_preserve_shared_format() {
    if std::env::var_os("SHIN_GEROKASU2_ROM").is_none() {
        return;
    }
    let scenario = scenario();
    let maps = mappings(&scenario);
    assert_eq!(maps.len(), 7);
    for (original, converted) in scenario.voice_mapping_info.iter().zip(&maps) {
        assert_eq!(
            original.name_pattern.as_str(),
            converted.name_pattern.as_str()
        );
        assert_eq!(
            original.lipsync_character_ids.0,
            converted.lipsync_character_ids.0
        );
    }
    let mut rom = rom();
    let files = rom
        .traverse()
        .filter_map(|(path, entry)| match entry {
            IndexEntry::File(file) if path.ends_with(".nxa") => Some((path, *file)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(files.len(), 4137);
    let mut voices = 0;
    let mut encoded_bytes = 0;
    for (path, file) in files {
        let mut data = Vec::new();
        rom.open_file(file).unwrap().read_to_end(&mut data).unwrap();
        assert_eq!(&data[..8], b"NXA1\x02\0\0\0");
        assert_eq!(
            u32::from_le_bytes(data[8..12].try_into().unwrap()) as usize,
            data.len()
        );
        let audio = read_audio(&data).unwrap();
        assert_eq!(audio.info().sample_rate, 48000);
        assert!(matches!(audio.info().channel_count, 1 | 2));
        encoded_bytes += data.len();
        voices += usize::from(path.starts_with("/voice/"));
    }
    assert_eq!(voices, 4068);
    println!(
        "GEROKASU2_NXA files=4137 voices=4068 mappings=7 encoded_bytes={encoded_bytes} unchanged_shared_NXA1_v2_reader=verified"
    );
}

#[test]
fn native_speaker_mappings_unicode_mask_and_sample_voice_clocks() {
    if std::env::var_os("SHIN_GEROKASU2_VOICE_ORACLE").is_none() {
        return;
    }
    let mut native = Native::new();
    let maps = mappings(&scenario());
    let names = voice_names();
    assert_eq!(names.len(), 4068);
    let mut mapping = |maps: &VoiceMappingInfo, name: &str, mask: u32, character: i32| {
        let mut tail = name.as_bytes().to_vec();
        for row in maps {
            tail.extend((row.name_pattern.0.len() as u32).to_le_bytes());
            tail.extend((row.lipsync_character_ids.0.len() as u32).to_le_bytes());
            tail.extend(row.name_pattern.as_str().as_bytes());
            tail.extend(row.lipsync_character_ids.0.iter());
        }
        native.input(
            [
                0,
                u64::from(mask),
                character as u32 as u64,
                name.len() as u64,
                maps.len() as u64,
                0,
                0,
                0,
            ],
            &tail,
        );
        let index = maps
            .iter()
            .position(|row| voice_pattern_matches(row.name_pattern.as_str(), name));
        let speakers = voice_speakers(maps, name, VoicePlayFlags::ENABLE_CHARACTER_LIPSYNC);
        assert_eq!(
            voice_speakers(maps, name, VoicePlayFlags::empty()),
            Vec::<u8>::new()
        );
        native.compare(&[
            index.map_or(u32::MAX, |i| i as u32),
            u32::from(character_voice_enabled(maps, name, mask)),
            u32::from(u8::try_from(character).is_ok_and(|id| speakers.contains(&id))),
        ]);
    };
    for name in &names {
        for mask in [0, 1, u32::MAX] {
            for character in [-1, 0, 1, 255, 256] {
                mapping(&maps, name, mask, character);
            }
        }
    }
    let synthetic: VoiceMappingInfo = [("é?", vec![2, 34]), ("*", vec![255]), ("空*", vec![])]
        .into_iter()
        .map(|(pattern, ids)| VoiceMappingInfoItem {
            name_pattern: U16String::new(pattern),
            lipsync_character_ids: U8SmallList::from_contents(ids),
        })
        .collect();
    for name in ["é水", "é🐈", "É水", "空白", "", "未知", "é"] {
        for mask in [0, 1 << 2, 1 << 31, u32::MAX] {
            for character in [-1, 2, 34, 255, 256] {
                mapping(&synthetic, name, mask, character);
            }
        }
    }
    let states = [
        vec![],
        vec![0],
        vec![1],
        vec![2],
        vec![2, 0, 1],
        vec![2, 2, 1],
        vec![1, 0],
    ];
    for rate in [1, 8000, 11025, 16000, 22050, 44100, 48000, 96000, u32::MAX] {
        for samples in [
            0,
            1,
            48,
            1000,
            rate as u64,
            u32::MAX as u64,
            (1u64 << 53) - 1,
            1u64 << 53,
            u64::MAX,
        ] {
            for origin in [0, 1000, u64::MAX - 1000, u64::MAX] {
                for states in &states {
                    let tail = states
                        .iter()
                        .flat_map(|n: &u32| n.to_le_bytes())
                        .collect::<Vec<_>>();
                    native.input(
                        [
                            1,
                            u64::from(rate),
                            samples,
                            origin,
                            states.len() as u64,
                            0,
                            0,
                            0,
                        ],
                        &tail,
                    );
                    let active = states.iter().find(|state| **state <= 1).copied();
                    let status = if states.contains(&0) {
                        3
                    } else if states.contains(&1) {
                        2
                    } else {
                        0
                    };
                    let sample = native_voice_clock(samples, rate);
                    let position = active.map_or(0, |state| {
                        origin.wrapping_add(if state == 0 { 0 } else { sample })
                    });
                    let voice = VoiceTimeline {
                        playing: status & 2 != 0,
                        position,
                    };
                    let queries = [-1, 0, 1, 1000, i32::MAX].map(|target| {
                        u32::from(voice.playing && voice.position < (target as i64) as u64)
                    });
                    let mut expected = vec![
                        status,
                        position as u32,
                        (position >> 32) as u32,
                        sample as u32,
                        (sample >> 32) as u32,
                        if let Some(state) = active {
                            1 + 6 * (if state == 0 { 1 } else { 2 })
                        } else {
                            1
                        },
                    ];
                    expected.extend(queries);
                    native.compare(&expected);
                }
            }
        }
    }
    native.finish("mappings-clocks");
}

#[derive(Default)]
struct Notifications {
    voices: Vec<String>,
    completed: Vec<u32>,
}
impl MessageNotifications for Notifications {
    fn observed_voice(&mut self, name: &str) {
        self.voices.push(name.to_owned());
    }
    fn completed(&mut self, id: u32) {
        self.completed.push(id);
    }
}

#[test]
fn native_explicit_voice_commands_skip_volume_flags_status_and_pending_ownership() {
    use super::super::message_commands::MessageCommandStatus;
    use super::super::voice_commands::{VoiceCommandState, VoiceCommands};
    if std::env::var_os("SHIN_GEROKASU2_VOICE_ORACLE").is_none() {
        return;
    }
    #[derive(Default)]
    struct Voice {
        status: i32,
        play: Vec<(String, f32, i32)>,
        stops: u32,
    }
    impl VoiceCommands for Voice {
        fn play_command(&mut self, n: &str, v: f32, f: i32) {
            self.play.push((n.into(), v, f));
        }
        fn stop(&mut self) {
            self.stops += 1;
        }
        fn status_bits(&self) -> i32 {
            self.status
        }
    }
    let mut native = Native::new();
    for name in [
        "hanp00010",
        "声/🐈",
        "long-native-voice-name-to-exercise-owned-string",
    ] {
        for volume in [i32::MIN, -1, 0, 1, 699, 700, 1000, 1001, i32::MAX] {
            for flags in [i32::MIN, -1, 0, 1, 2, 3, i32::MAX] {
                for skip in [false, true] {
                    let mut voice = Voice::default();
                    let mut notifications = Notifications::default();
                    let mut state = VoiceCommandState::new();
                    let status = state
                        .dispatch(
                            &G::VoicePlay {
                                name: name.into(),
                                volume,
                                flags,
                            },
                            skip,
                            &mut voice,
                            &mut notifications,
                        )
                        .unwrap()
                        .unwrap();
                    native.input(
                        [
                            2,
                            volume as u32 as u64,
                            flags as u32 as u64,
                            u64::from(skip),
                            name.len() as u64,
                            0,
                            0,
                            0,
                        ],
                        name.as_bytes(),
                    );
                    let request = voice.play.first();
                    native.compare(&[
                        u32::from(status == MessageCommandStatus::Complete),
                        notifications.voices.len() as u32,
                        voice.play.len() as u32,
                        voice.stops,
                        request.map_or(0, |r| r.1.to_bits()),
                        request.map_or(0, |r| r.2 as u32),
                        request.map_or(0, |r| r.0.len() as u32),
                        0,
                    ]);
                }
            }
        }
    }
    for mask in [i32::MIN, -1, 0, 1, 2, 3, 4, i32::MAX] {
        for status in [0, 2, 3] {
            let mut voice = Voice {
                status,
                ..Default::default()
            };
            let mut notifications = Notifications::default();
            let mut state = VoiceCommandState::new();
            let result = state
                .dispatch(
                    &G::VoiceWait { statuses: mask },
                    false,
                    &mut voice,
                    &mut notifications,
                )
                .unwrap()
                .unwrap();
            native.input([4, mask as u32 as u64, status as u64, 0, 0, 0, 0, 0], &[]);
            native.compare(&[
                u32::from(result == MessageCommandStatus::Complete),
                0,
                0,
                0,
                0,
                0,
                0,
                0,
            ]);
            if result == MessageCommandStatus::Pending {
                assert!(
                    state
                        .dispatch(
                            &G::VoiceWait { statuses: 0 },
                            false,
                            &mut voice,
                            &mut notifications
                        )
                        .is_err()
                );
                voice.status = 0;
                assert_eq!(state.poll(&voice), MessageCommandStatus::Complete);
            }
        }
    }
    let mut voice = Voice::default();
    let mut notifications = Notifications::default();
    let mut state = VoiceCommandState::new();
    let stop = G::Shared(
        CompiletimeCommand::VOICESTOP(compiletime::VOICESTOP {})
            .into_runtime_form(&VmCtx::new(0, 42)),
    );
    assert_eq!(
        state
            .dispatch(&stop, false, &mut voice, &mut notifications)
            .unwrap(),
        Some(MessageCommandStatus::Complete)
    );
    native.input([3, 0, 0, 0, 0, 0, 0, 0], &[]);
    native.compare(&[1, 0, 0, voice.stops, 0, 0, 0, 0]);
    native.finish("explicit-commands");
}

#[test]
fn native_message_microsecond_request_preserves_full_millisecond_domain() {
    for n in [0, 1, 1000, u32::MAX] {
        let request = VoiceRequest {
            filename: String::new(),
            volume: 1.0,
            lipsync: false,
            segment_start: u64::from(n) * 1000,
            segment_duration: u64::from(n) * 1000,
        };
        assert_eq!(message_segment(&request), Some((n, n)));
        let invalid = VoiceRequest {
            segment_start: request.segment_start + 1,
            ..request.clone()
        };
        assert_eq!(message_segment(&invalid), None);
    }
}

#[test]
fn native_stopping_voice_uses_fused_decay_and_saved_category_gain() {
    if std::env::var_os("SHIN_GEROKASU2_VOICE_ORACLE").is_none() {
        return;
    }
    let mut native = Native::new();
    for volume in [
        0.0,
        -0.0,
        0.1,
        0.2,
        0.7,
        0.70000005,
        1.0,
        100.0,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
    ] {
        for ticks in [
            -1.0,
            0.0,
            0.1,
            0.5,
            1.0,
            3.5,
            10.0,
            f32::INFINITY,
            f32::NEG_INFINITY,
        ] {
            for percent in [0, 1, 50, 100, 255] {
                let decayed = voice_stop_volume(volume, ticks, true);
                let gain = (percent as f32) * f32::from_bits(0x3c23d70a);
                let scaled = gain * decayed;
                native.input(
                    [
                        5,
                        u64::from(volume.to_bits()),
                        u64::from(ticks.to_bits()),
                        percent,
                        0,
                        0,
                        0,
                        0,
                    ],
                    &[],
                );
                native.compare(&[
                    u32::from(decayed > 0.0),
                    1,
                    0,
                    u32::from(!(decayed > 0.0)),
                    scaled.to_bits(),
                    decayed.to_bits(),
                    0,
                    0,
                ]);
            }
        }
    }
    native.finish("stop-fade");
}

#[test]
#[ignore = "requires supplied ROM/executable, Metal GPU and exact Loopback Audio"]
fn supplied_voice_transport_drives_message_waits_and_releases_playback() {
    use super::super::{
        message_clock::{MessageClock, MessageParams},
        message_commands::{MessageCommandContext, MessageCommandStatus},
        message_owner::Gerokasu2MessageOwner,
        voice_commands::VoiceCommandState,
    };
    use crate::{
        asset::font::gerokasu2::draw::gpu_tests::Offscreen,
        asset::system::{AssetLoadContext, AssetServer, locate_assets_for_game},
        cli::Game,
    };
    let gpu = Offscreen::new();
    let path = std::path::PathBuf::from(std::env::var_os("SHIN_GEROKASU2_ROM").unwrap());
    let assets = Arc::new(AssetServer::new(
        locate_assets_for_game(Game::Gerokasu2, path.parent().unwrap())
            .unwrap()
            .into(),
        AssetLoadContext {
            game: crate::cli::Game::Gerokasu2,
            wgpu_device: gpu.device.clone(),
            wgpu_queue: gpu.queue.clone(),
            bustup_cache: crate::asset::system::cache::AssetCache::new(),
            higurashi_bustup_cache: Default::default(),
            picture_cache: Default::default(),
            font_cache: Default::default(),
        },
    ));
    let scenario = scenario();
    let maps = mappings(&scenario);
    let names = voice_names()
        .into_iter()
        .filter(|name| {
            !voice_speakers(&maps, name, VoicePlayFlags::ENABLE_CHARACTER_LIPSYNC).is_empty()
        })
        .take(2)
        .collect::<Vec<_>>();
    assert_eq!(names.len(), 2);
    let manager = Arc::new(AudioManager::new_for_tests());
    let mut voice = Gerokasu2VoicePlayer::new(manager, assets.clone(), &scenario);
    voice.set_preferences(100, u32::MAX, true);
    voice.set_paused(true);
    let request = |index: usize, start: u32, duration: u32| VoiceRequest {
        filename: names[index].clone(),
        volume: 0.8,
        lipsync: true,
        segment_start: u64::from(start) * 1000,
        segment_duration: u64::from(duration) * 1000,
    };
    let loaded = |voice: &mut Gerokasu2VoicePlayer| {
        let deadline = Instant::now() + Duration::from_secs(5);
        while voice.wait_status().contains(AudioWaitStatus::LOADING) {
            assert!(Instant::now() < deadline);
            voice.update(Ticks::ZERO);
            std::thread::sleep(Duration::from_millis(2));
        }
    };
    assert!(voice.play(request(0, 10, 120)));
    assert_eq!(
        voice.wait_status(),
        AudioWaitStatus::LOADING | AudioWaitStatus::PLAYING
    );
    assert_eq!(voice.timeline().position, 10000);
    loaded(&mut voice);
    assert_eq!(voice.timeline().position, 10000);
    let speakers = voice_speakers(&maps, &names[0], VoicePlayFlags::ENABLE_CHARACTER_LIPSYNC);
    assert_eq!(
        voice.player.borrow().reference_voice_metadata()[0].0,
        speakers
    );
    assert!(
        voice
            .character_lipsync(i32::from(speakers[0]), -0.5)
            .is_some()
    );
    let mut query = MessageClock::new(
        &[],
        &[],
        glam::Vec2::ZERO,
        MessageParams {
            flags: 0,
            mode: 0,
            message_id: 0,
            message_arg: 0,
            animate_open: false,
        },
    )
    .unwrap();
    query.total_voices = 1;
    query.voice_counter = 0;
    assert!(query.voice_sync_is_waiting(0, 10001, Some(voice.timeline())));
    voice.set_paused(false);
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut advanced = false;
    while voice.timeline().playing {
        assert!(Instant::now() < deadline, "voice EOF timeout");
        advanced |= voice.timeline().position > 10000;
        voice.update(Ticks::from_f32(1.0));
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(advanced);
    assert_eq!(voice.timeline().position, 0);
    assert!(!query.voice_sync_is_waiting(0, 10001, Some(voice.timeline())));
    voice.set_paused(true);
    assert!(voice.play(request(0, 0, 120)));
    assert!(voice.play(request(1, 20, 120)));
    loaded(&mut voice);
    assert_eq!(voice.timeline().position, 20000);
    voice.stop();
    assert!(!voice.timeline().playing);
    assert_eq!(voice.timeline().position, 0);
    voice.update(Ticks::from_f32(3.5));
    assert_eq!(
        voice.player.borrow().reference_stopping_volumes()[0].to_bits(),
        voice_stop_volume(0.8, 3.5, true).to_bits(),
        "Ger transport selects the native fused stop policy"
    );
    voice.update(Ticks::from_f32(10.0));
    voice.set_preferences(100, 0, true);
    assert!(!voice.play(request(0, 0, 0)));
    assert!(voice.wait_status().is_empty());
    voice.set_preferences(100, u32::MAX, true);
    assert!(voice.play(VoiceRequest {
        filename: "missing-file".to_owned(),
        ..request(0, 0, 0)
    }));
    loaded(&mut voice);
    assert!(voice.wait_status().is_empty());
    let mut owner =
        shin_tasks::block_on(Gerokasu2MessageOwner::load(&assets, 0, false, -1)).unwrap();
    let mut notifications = Notifications::default();
    let context = MessageCommandContext {
        was_read: false,
        event_mode: -1,
        skip: true,
        message_speed: 50,
    };
    let text = format!("@r@v{}.A@e", names[0]);
    owner
        .dispatch(
            &G::MessageSet {
                msg_id: shin_core::format::scenario::instruction_elements::MessageId(88),
                auto_wait: true,
                message_arg: 0,
                text,
            },
            &gpu.device,
            &gpu.queue,
            context,
            Some(voice.timeline()),
            &scenario.bustup_info,
        )
        .unwrap()
        .unwrap();
    let mut frame = shin_core::primitives::update::FrameId::default();
    frame.advance();
    owner
        .update(
            frame,
            Ticks::from_f32(1.0),
            true,
            &mut Gerokasu2MessageEffects {
                voice: &mut voice,
                notifications: &mut notifications,
            },
        )
        .unwrap();
    assert_eq!(notifications.voices, vec![names[0].clone()]);
    assert!(voice.timeline().playing);
    loaded(&mut voice);
    assert_eq!(
        owner.poll_command(Some(voice.timeline())),
        MessageCommandStatus::Pending
    );
    voice.stop();
    owner
        .fast_forward(&mut Gerokasu2MessageEffects {
            voice: &mut voice,
            notifications: &mut notifications,
        })
        .unwrap();
    assert_eq!(
        owner.poll_command(Some(voice.timeline())),
        MessageCommandStatus::Complete
    );
    assert_eq!(notifications.completed, vec![88]);
    let mut commands = VoiceCommandState::new();
    let before = notifications.voices.len();
    commands
        .dispatch(
            &G::VoicePlay {
                name: names[0].clone(),
                volume: 700,
                flags: 3,
            },
            true,
            &mut voice,
            &mut notifications,
        )
        .unwrap();
    assert_eq!(notifications.voices.len(), before);
    assert!(!voice.timeline().playing);
    commands
        .dispatch(
            &G::VoicePlay {
                name: names[0].clone(),
                volume: 700,
                flags: 3,
            },
            false,
            &mut voice,
            &mut notifications,
        )
        .unwrap();
    assert!(voice.timeline().playing);
    assert_eq!(
        commands
            .dispatch(
                &G::VoiceWait { statuses: 3 },
                false,
                &mut voice,
                &mut notifications
            )
            .unwrap(),
        Some(MessageCommandStatus::Pending)
    );
    loaded(&mut voice);
    voice.stop();
    assert_eq!(commands.poll(&voice), MessageCommandStatus::Complete);
    voice.update(Ticks::from_f32(10.0));
    println!(
        "GEROKASU2_VOICE_GPU authored_voices=2 audio=Loopback Audio Metal real_NXA_async_segment_clock_pause_lipsync_muting_cancellation_EOF_missing_load_owner_effects_explicit_commands=verified"
    );
}
