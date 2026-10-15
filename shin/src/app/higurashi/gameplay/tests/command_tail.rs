//! Whole Gameplay transport for the updated command tail. Original function
//! comparisons and literal widget packet/glyph comparisons are separate tests.
//! This fixture splices explicitly bounded authored commands into a copy of
//! the supplied scenario; it is not a proof of a complete chapter route.
use super::{character::common, selection::Frames, *};
use crate::{
    adv::{
        higurashi::{
            audio::HigurashiAudio,
            autosave::{ProgressEntry, SaveProgress},
            descriptors::PlaybackDescriptors,
            fake_select::view::Shared,
            input::OverlayInput,
            read_state::MessageTimeline,
            reference_gpu_assets, reference_render_to_texture,
            touch::Overlay as InputOverlay,
            voice_commands::VoiceCommands,
        },
        input_area::{PointerEvent, PointerKind},
    },
    app::higurashi::{runtime_game_data::RuntimeGameData, runtime_saves::Slots},
};
use shin_core::{
    format::scenario::higurashi::HigurashiScenario, primitives::color::UnormColor,
    vm::higurashi::command::HigurashiRuntimeCommand as Cmd,
};
use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

const SOURCE_DATE: SaveDate = SaveDate {
    year: 2025,
    month: 1,
    day: 2,
    hour: 3,
    minute: 4,
    second: 5,
};
const NOW: SaveDate = SaveDate {
    year: 2026,
    month: 10,
    day: 13,
    hour: 12,
    minute: 34,
    second: 56,
};
const MESSAGE: &[u8] = &[0x86, 1, 0, 0, 0, 0x7f, 4, 0, b'@', b'r', b'A', 0];
const MARKER: &[u8] = &[0x82, 9, 23, 0x83, 0, 0x83, 0xe8, 0, 0, 0];

fn authored_slice(scenario: &HigurashiScenario, command: &str, index: usize) -> Vec<u8> {
    let evidence: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../reverse-engineering/higurashi/layerwait-authored-audit.json"
    )))
    .unwrap();
    let row = &evidence["first_occurrences"][command][index];
    let start = usize::from_str_radix(row["start"].as_str().unwrap(), 16).unwrap();
    let end = usize::from_str_radix(row["end"].as_str().unwrap(), 16).unwrap();
    let raw = row["raw"].as_str().unwrap();
    let expected: Vec<_> = (0..raw.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&raw[i..i + 2], 16).unwrap())
        .collect();
    assert_eq!(
        &scenario.raw()[start..end],
        expected,
        "unchanged authored {command}"
    );
    expected
}

fn script(authored: &HigurashiScenario) -> Vec<u8> {
    let mut code = MESSAGE.to_vec();
    code.extend([0x83, 0, 12]); // Observe the completed current message first.
    code.extend(authored_slice(authored, "LOGSET", 0));
    code.extend(authored_slice(authored, "DEBUGOUT", 0));
    code.extend([0x83, 0, 13]);
    // Unmodified TROPHY1 followed by the actual SGET1 / OR1 / SSET1
    // continuation. Its ambient persistent slot1 is supplied by this fixture.
    let trophy = authored_slice(authored, "TROPHY", 0);
    assert_eq!(trophy, [0xb0, 1]);
    assert_eq!(
        &authored.raw()[0x5a9ee..0x5a9fa],
        &[0x81, 12, 0, 1, 0x41, 8, 12, 0, 1, 0x82, 1, 0xbc]
    );
    code.extend_from_slice(&authored.raw()[0x5a9ec..0x5a9fa]);
    code.extend([0x83, 0, 14]);
    code.extend(authored_slice(authored, "UNLOCK", 1)); // Actual CG16-ID list.
    code.extend(authored_slice(authored, "UNLOCK", 0)); // Actual BGM80/103.
    // No authored kind2 exists: explicitly synthetic Movie bank coverage.
    code.extend([0xd9, 2, 3, 0, 6, 9]);
    code.extend([0x83, 0, 63]);
    let fake = authored_slice(authored, "FAKESELECT", 0);
    assert_eq!(fake, [0xd8]);
    code.extend(fake);
    code.extend(MARKER); // SSET9=23, then retained WAIT1000; no EXIT/save.
    code
}

struct Fixture {
    game: Gameplay,
    frames: Frames,
    os: Arc<AtomicU32>,
    dates: Arc<AtomicUsize>,
}
impl Fixture {
    fn until(&mut self, mut condition: impl FnMut(&Gameplay) -> bool, reason: &str) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !condition(&self.game) {
            self.frames.idle(&mut self.game);
            assert!(
                Instant::now() < deadline,
                "{reason}: {:?}",
                self.game.host().unwrap().runner.pending_command()
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn wait(&mut self, amount: i32) {
        self.until(
            |game| {
                matches!(game.host().unwrap().runner.pending_command(),
            Some(Cmd::WAIT { amount: actual, .. }) if *actual == amount)
            },
            "bounded WAIT not reached",
        );
    }
}
fn fixture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    server: &Arc<AssetServer>,
    code: &[u8],
    size: (u32, u32),
    active: usize,
) -> Fixture {
    let (authored, common) = common(server, code);
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let current = RuntimeGameData::from_progress(
        SaveProgress::new(
            active,
            0xfedc_ab98,
            std::array::from_fn(|i| ProgressEntry {
                scenario_id: if i == active { 0 } else { 12345 },
                random_seed: if i == active { 42 } else { 0xdead_beef },
                save_position: if i == active { 0 } else { 53 },
                replay_target: if i == active { 0 } else { 54 },
                response_data: vec![0x93, i as u8, 0xfe, 0x12, 0x87],
            }),
        )
        .unwrap(),
        SOURCE_DATE,
        vec![0xffff, 0x102, 0x7f00],
        vec![1, 0x9a, 0xff],
    );
    let mut saved = SaveData::default();
    saved.settings.common.v14_showroutenavi = false;
    saved.settings.common.v11_disallowskipunread = false;
    saved.persist_data.set(1, 0x40);
    saved.save_vectors.seen_messages = vec![1, 0x4000_0000];
    saved.save_vectors.select_seen_choices = vec![0x1234_5678, 0];
    saved.save_vectors.select_last_choices = vec![0xf5, 0x81, 0];
    saved.save_vectors.unlocked_cgs = vec![0; 256];
    saved.save_vectors.unlocked_bgms = vec![0; 256];
    saved.save_vectors.unlocked_movies = vec![0; 256];
    for words in [
        &mut saved.save_vectors.unlocked_cgs,
        &mut saved.save_vectors.unlocked_bgms,
        &mut saved.save_vectors.unlocked_movies,
    ] {
        words[0] = 0x8000_0000;
        words[255] = 0x0020_0000; // Physical trailing words survive unchanged.
    }
    let mut slots = Slots::loaded(&saved);
    slots.auto.valid = true;
    slots.auto.data = current.clone();
    slots.auto.data.trailing_values = vec![7, 0xffff, 0x4567];
    slots.auto.data.byte_variables = vec![0x53, 0xfe];
    for (i, slot) in slots.manual.iter_mut().enumerate() {
        slot.valid = i % 3 != 0;
        slot.data = current.clone();
        slot.data.progress.extra_word ^= i as u32;
        slot.data.progress.entries[1 - active].random_seed ^= i as u32;
        slot.data.trailing_values.push(0x8000 | i as u16);
        slot.data.byte_variables.push(i as u8);
    }
    // The full Slots owner is authoritative. Populate the legacy mirror
    // directly; do not truncate unrelated live settings/choice bytes by
    // passing this initialization through a disk writer.
    saved.auto_save_slot = Some(slots.auto.data.save_data());
    saved.manual_save_slots = std::array::from_fn(|i| {
        slots.manual[i]
            .valid
            .then(|| slots.manual[i].data.save_data())
    });
    let globals = Globals {
        storage: None,
        runtime_saves: Some(slots),
        common,
        data: saved,
        clock: PlayClock::from_saved_seconds(0),
        platform_flag: true,
        last_title_mode: 0,
        audio: HigurashiAudio::new(manager.clone(), server.clone(), authored),
        server: server.clone(),
        manager: manager.clone(),
        sounds: Arc::new(Mutex::new(SystemSoundPlayer::new(manager))),
    };
    let os = Arc::new(AtomicU32::new(0));
    let counter = os.clone();
    let mut game = None;
    reference_render_to_texture(device, queue, size, |context, _| {
        let mut initial = context.new_render_texture("Command tail source".into());
        {
            let mut pass = context.begin_pass(
                initial.as_texture_target(),
                Some(context.depth_stencil),
                "Command tail source",
            );
            pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.));
        }
        game = Some(
            Gameplay::new(globals, current, 0, initial, move || {
                std::array::from_fn(|_| 0xabc0_0000 | (counter.fetch_add(1, Ordering::SeqCst) + 1))
            })
            .unwrap(),
        );
    });
    let dates = Arc::new(AtomicUsize::new(0));
    let calls = dates.clone();
    let mut game = game.unwrap();
    game.set_clock_source(move || {
        calls.fetch_add(1, Ordering::SeqCst);
        Ok(NOW)
    });
    Fixture {
        game,
        frames: Frames { number: 0 },
        os,
        dates,
    }
}

#[derive(Debug, PartialEq)]
struct ReadSnapshot {
    timeline: MessageTimeline,
    descriptors: PlaybackDescriptors,
    flags: (u32, u8, u8),
    history: Vec<u32>,
    choices: (Vec<u32>, Vec<u8>),
    checkpoints: Vec<(u32, usize)>,
    response_cursor: usize,
}
fn read_snapshot(game: &Gameplay) -> ReadSnapshot {
    let runner = &game.host().unwrap().runner;
    let read = runner.native_read_state().unwrap();
    ReadSnapshot {
        timeline: runner.native_message_timeline().unwrap(),
        descriptors: runner.native_playback_descriptors().unwrap().clone(),
        flags: (read.flags, read.fast_forward_blocked, read.is_read),
        history: game.bindings.history.bits().words().to_vec(),
        choices: (
            game.bindings.choices.seen().words().to_vec(),
            game.bindings.choices.last().to_vec(),
        ),
        checkpoints: game
            .bindings
            .memory
            .entries()
            .iter()
            .map(|e| (e.boundary, Arc::as_ptr(&e.state) as usize))
            .collect(),
        response_cursor: game.bindings.progress.responses().1,
    }
}
fn manual_rows(game: &Gameplay) -> Vec<(bool, RuntimeGameData)> {
    game.globals
        .runtime_saves
        .as_ref()
        .unwrap()
        .manual
        .iter()
        .map(|s| (s.valid, s.data.clone()))
        .collect()
}
fn wire_pair(data: &Option<GameData>) -> Option<GameData> {
    let mut data = data.clone()?;
    // The native secondary-record bit also gates the u16 vector on wire.
    // Runtime's complete vector remains independently checked above/below.
    if data.primary_entry.is_none() {
        data.trailing_values.clear();
    }
    // Native reader reconstructs entry+c from entry+8; it is not on wire.
    data.entry.replay_target = data.entry.save_position;
    if let Some(entry) = &mut data.primary_entry {
        entry.replay_target = entry.save_position;
    }
    Some(data)
}
fn fake(game: &Gameplay) -> Option<Shared> {
    game.selection.fake.ui.clone()
}
fn model_words(ui: &Shared) -> Vec<u32> {
    let ui = ui.0.lock();
    let m = &ui.model;
    let mut words = vec![
        m.phase,
        m.selected as u32,
        m.countdown.to_bits(),
        m.remaining,
        m.seed,
    ];
    for r in &m.rows {
        words.extend([
            r.x.value().to_bits(),
            r.alpha.value().to_bits(),
            r.focus.value().to_bits(),
        ]);
    }
    words
}
fn pixels(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    size: (u32, u32),
    game: &mut Gameplay,
) -> Vec<u8> {
    reference_render_to_texture(device, queue, size, |context, target| {
        game.prepare(context).unwrap();
        let runner = &game.host.as_ref().unwrap().runner;
        let snapshot = game
            .screens
            .as_mut()
            .unwrap()
            .capture_frame(context, runner)
            .unwrap();
        assert_eq!(snapshot.dimensions(), (1920, 1080));
        let mut pass = context.begin_pass(
            TextureTarget {
                view: target,
                kind: TextureTargetKind::RenderTexture,
            },
            Some(context.depth_stencil),
            "Retained command tail",
        );
        pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.));
        game.render(&mut pass).unwrap();
    })
}
fn same_pixels(a: &[u8], b: &[u8]) {
    assert_eq!(a.len(), b.len());
    let count = a.iter().zip(b).filter(|(a, b)| a != b).count();
    let maximum = a
        .iter()
        .zip(b)
        .map(|(&a, &b)| a.abs_diff(b))
        .max()
        .unwrap_or(0);
    assert_eq!(
        count, 0,
        "Pure redraw/capture changed{count} channels,max{maximum}"
    );
}
struct Replacement;
impl InputOverlay for Replacement {
    fn identity(&self) -> usize {
        0x1234_5678
    }
    fn input_permissions(&self) -> OverlayInput {
        OverlayInput {
            menus: false,
            hide_window: false,
        }
    }
    fn probe(&mut self, _: glam::Vec2) -> bool {
        false
    }
    fn press(&mut self, _: glam::Vec2) {}
    fn release(&mut self, _: glam::Vec2) {}
    fn cancel(&mut self) {}
}

#[test]
#[ignore = "actual Gameplay five-command tail, supplied autonomous FakeSelect/fonts/two NXA EOF, offscreen Metal and exact Loopback Audio; no windows or saves"]
fn higurashi_gameplay_command_tail_preserves_read_state_and_autonomous_child_ownership() {
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    let authored: Arc<HigurashiScenario> = server.load_sync("/main.snr").unwrap();
    let code = script(&authored);
    for (active, size) in [(0, (1280, 720)), (1, (1920, 1080))] {
        crate::engine_random::with_reference_seed(5489, || {
            let mut f = fixture(&device, &queue, &server, &code, size, active);
            f.wait(12);
            let manual = manual_rows(&f.game);
            let current = f.game.bindings.runtime_game_data().unwrap();
            let before = read_snapshot(&f.game);
            let copied = f
                .game
                .globals
                .runtime_saves
                .as_ref()
                .unwrap()
                .auto
                .data
                .clone();
            let os = f.os.load(Ordering::SeqCst);
            let mt = crate::engine_random::seed_call_count();
            let completed = f.game.host().unwrap().runner.completed_commands();
            assert_eq!(f.dates.load(Ordering::SeqCst), 1);
            f.wait(13);
            assert_eq!(
                read_snapshot(&f.game),
                before,
                "LOGSET/DEBUGOUT altered live history/current message"
            );
            assert_eq!(f.game.bindings.runtime_game_data().unwrap(), current);
            assert_eq!(
                f.game.globals.runtime_saves.as_ref().unwrap().auto.data,
                copied
            );
            assert_eq!(f.dates.load(Ordering::SeqCst), 1);
            assert_eq!(
                f.game.host().unwrap().runner.completed_commands(),
                completed + 3
            );
            f.wait(14);
            assert_eq!(
                f.game.bindings.progress.pending_persistent(),
                [(1, 0x41)],
                "Original TROPHY continuation did not execute SGET/OR/SSET"
            );
            assert_eq!(f.game.bindings.progress.persistent().get(1), 0x40);
            assert_eq!(f.os.load(Ordering::SeqCst), os);
            assert_eq!(crate::engine_random::seed_call_count(), mt);
            f.wait(63);
            let vectors = &f.game.globals.data.save_vectors;
            for (words, ids) in [
                (
                    &vectors.unlocked_cgs,
                    &[
                        1028usize, 1029, 1030, 1031, 1032, 1033, 1034, 1035, 1036, 1037, 1038,
                        1039, 1040, 1041, 1159, 1160,
                    ][..],
                ),
                (&vectors.unlocked_bgms, &[80usize, 103][..]),
                (&vectors.unlocked_movies, &[0usize, 6, 9][..]),
            ] {
                let mut expected = vec![0u32; 256];
                expected[0] = 0x8000_0000;
                expected[255] = 0x0020_0000;
                for &id in ids {
                    expected[id >> 5] |= 1 << (id & 31);
                }
                assert_eq!(
                    *words, expected,
                    "UNLOCK bank or untouched physical words differ"
                );
            }
            assert_eq!(manual_rows(&f.game), manual);
            assert_eq!(f.os.load(Ordering::SeqCst), os);
            assert_eq!(crate::engine_random::seed_call_count(), mt);
            assert_eq!(f.dates.load(Ordering::SeqCst), 1);
            let before_fake = read_snapshot(&f.game);
            let before_data = f.game.bindings.runtime_game_data().unwrap();
            let auto_vectors = {
                let d = &f.game.globals.runtime_saves.as_ref().unwrap().auto.data;
                (d.trailing_values.clone(), d.byte_variables.clone())
            };
            f.until(|g| fake(g).is_some(), "FakeSelect child not attached");
            let ui = fake(&f.game).unwrap();
            let weak = Arc::downgrade(&ui.0);
            let identity = ui.identity();
            assert_eq!(
                f.game.host().unwrap().reference_touch_overlay_identity(),
                Some(identity)
            );
            assert_eq!(InputOverlay::owner_id(&ui), 0);
            assert_eq!(f.os.load(Ordering::SeqCst), os + 4);
            assert_eq!(crate::engine_random::seed_call_count(), mt + 1);
            assert_eq!(f.dates.load(Ordering::SeqCst), 2);
            assert_eq!(read_snapshot(&f.game), before_fake);
            assert_eq!(f.game.bindings.runtime_game_data().unwrap(), before_data);
            assert_eq!(
                ui.0.lock().properties.oscillators.map(|o| o.seed),
                std::array::from_fn(|i| 0xabc0_0000 | (os + i as u32 + 1))
            );
            let auto = &f.game.globals.runtime_saves.as_ref().unwrap().auto;
            assert!(auto.valid);
            assert_eq!(auto.data.progress, before_data.progress);
            assert_eq!(auto.data.date, NOW);
            assert_eq!(
                (&auto.data.trailing_values, &auto.data.byte_variables),
                (&auto_vectors.0, &auto_vectors.1)
            );
            assert_eq!(manual_rows(&f.game), manual);
            assert!(!f.game.notification_busy());
            f.until(
                |g| fake(g).is_some_and(|u| u.0.lock().model.phase == 2),
                "FakeSelect did not finish opening",
            );
            let deadline = Instant::now() + Duration::from_secs(20);
            let (mut loading, mut jitter_checked, mut denied_checked, mut ambient_started) =
                (false, false, false, false);
            let mut playing_segments = [false; 3];
            let mut voices = Vec::new();
            let mut previous_remaining = ui.0.lock().model.remaining;
            while !ui.0.lock().model.complete() {
                let before = {
                    let u = ui.0.lock();
                    (
                        u.model.phase,
                        u.model.countdown.to_bits(),
                        u.model.remaining,
                    )
                };
                if before.0 == 2 && !denied_checked {
                    let words = model_words(&ui);
                    let cues = f.game.reference_overlay_cues.clone();
                    for permission in [true, false] {
                        f.frames.number += 1;
                        shin_tasks::block_on(f.game.update(
                            Ticks::ZERO,
                            permission,
                            Buttons {
                                pressed: (1 << 24) | (1 << 25) | (1 << 15) | (1 << 12),
                                repeat: 0xf0,
                                ..Default::default()
                            },
                            &PointerFrame {
                                number: f.frames.number,
                                time_seconds: f.frames.number as f64 / 60.,
                                events: &[
                                    PointerEvent {
                                        kind: PointerKind::Down,
                                        id: 19,
                                        position: glam::ivec2(640, 360),
                                    },
                                    PointerEvent {
                                        kind: PointerKind::Up,
                                        id: 19,
                                        position: glam::ivec2(640, 360),
                                    },
                                ],
                            },
                            Policy {
                                paused: !permission,
                                ..policy()
                            },
                            audio(),
                        ))
                        .unwrap();
                        assert_eq!(model_words(&ui), words);
                        assert_eq!(f.game.reference_overlay_cues, cues);
                        assert!(!f.game.host().unwrap().has_modes());
                        assert!(f.game.pending_input_request().is_none());
                    }
                    denied_checked = true;
                }
                let jitter = {
                    let u = ui.0.lock();
                    u.model.remaining == 1 && (30. ..100.).contains(&u.model.countdown)
                };
                if jitter && !jitter_checked {
                    let words = model_words(&ui);
                    let cache = f
                        .game
                        .host()
                        .unwrap()
                        .runner
                        .frame()
                        .cache
                        .reference_snapshot();
                    let a = pixels(&device, &queue, size, &mut f.game);
                    let b = pixels(&device, &queue, size, &mut f.game);
                    same_pixels(&a, &b);
                    assert!(
                        a.chunks_exact(4)
                            .any(|p| p[0] != 0 || p[1] != 0 || p[2] != 0)
                    );
                    assert_eq!(model_words(&ui), words, "Getter/redraw resampled shake");
                    assert_eq!(
                        f.game
                            .host()
                            .unwrap()
                            .runner
                            .frame()
                            .cache
                            .reference_snapshot(),
                        cache
                    );
                    assert_eq!(InputOverlay::owner_id(&ui), 0);
                    jitter_checked = true;
                }
                // Real NXA loads and actual EOF release the busy gate. No test
                // stop, fake status, model edit, forced callback or fast-skip.
                if !ambient_started {
                    // Supply one pre-existing real ADV voice, as the native
                    // shared owner permits. Its immediately observable3 proves
                    // the loading gate without relying on worker scheduling.
                    f.game.host_mut().unwrap().audio.play_command(
                        crate::adv::higurashi::fake_select::VOICES[0],
                        1.,
                        0,
                    );
                    assert_eq!(f.game.host().unwrap().audio.status_bits(), 3);
                    ambient_started = true;
                }
                let status = f.game.host().unwrap().audio.status_bits();
                loading |= status & 1 != 0;
                let before = {
                    let u = ui.0.lock();
                    (
                        u.model.phase,
                        u.model.countdown.to_bits(),
                        u.model.remaining,
                    )
                };
                if status == 2 && before.0 == 2 && (1..=3).contains(&before.2) {
                    playing_segments[(3 - before.2) as usize] = true;
                }
                f.frames.idle(&mut f.game);
                let after = {
                    let u = ui.0.lock();
                    (
                        u.model.phase,
                        u.model.countdown.to_bits(),
                        u.model.remaining,
                    )
                };
                if status != 0 && before.0 == 2 {
                    assert_eq!(
                        (after.1, after.2),
                        (before.1, before.2),
                        "Busy shared voice did not gate countdown"
                    );
                }
                if after.2 != previous_remaining {
                    if previous_remaining == 3 || previous_remaining == 2 {
                        voices.push(after.2);
                    }
                    previous_remaining = after.2;
                }
                assert_eq!(read_snapshot(&f.game), before_fake);
                assert_eq!(f.game.bindings.runtime_game_data().unwrap(), before_data);
                assert_eq!(f.dates.load(Ordering::SeqCst), 2);
                assert!(
                    Instant::now() < deadline,
                    "Autonomous FakeSelect did not reach VOID callback"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(
                loading && playing_segments.iter().all(|v| *v) && jitter_checked && denied_checked,
                "Missing actual loading/ambient+two authored playing stages or retained jitter/input checks:{playing_segments:?}"
            );
            assert_eq!(voices, [2, 1]);
            assert_eq!(
                f.game.reference_overlay_cues,
                ["select", "move", "move", "move", "seldecide"]
            );
            assert_eq!(f.game.host().unwrap().audio.status_bits(), 0);
            assert!(matches!(
                f.game.host().unwrap().runner.pending_command(),
                Some(Cmd::FAKESELECT {})
            ));
            if active == 1 {
                f.game
                    .host_mut()
                    .unwrap()
                    .set_touch_overlay(Some(Box::new(Replacement)));
            }
            f.wait(1000);
            assert!(fake(&f.game).is_none());
            assert_eq!(
                f.game.host().unwrap().reference_touch_overlay_identity(),
                (active == 1).then_some(0x1234_5678)
            );
            assert_eq!(
                Arc::strong_count(&ui.0),
                1,
                "Retired command/root still retain old widget"
            );
            assert_eq!(
                f.game.bindings.progress.pending_persistent(),
                [(1, 0x41), (9, 23)]
            );
            assert_eq!(read_snapshot(&f.game), before_fake);
            assert_eq!(manual_rows(&f.game), manual);
            assert_eq!(f.os.load(Ordering::SeqCst), os + 4);
            assert_eq!(crate::engine_random::seed_call_count(), mt + 1);
            assert_eq!(f.dates.load(Ordering::SeqCst), 2);
            let slots = f.game.globals.runtime_saves.as_ref().unwrap();
            let disk = slots.disk(&f.game.globals.data);
            let decoded = SaveData::decode(&disk.encode().unwrap()).unwrap();
            assert_eq!(
                decoded.save_vectors.unlocked_cgs,
                disk.save_vectors.unlocked_cgs
            );
            assert_eq!(
                decoded.save_vectors.unlocked_bgms,
                disk.save_vectors.unlocked_bgms
            );
            assert_eq!(
                decoded.save_vectors.unlocked_movies,
                disk.save_vectors.unlocked_movies
            );
            assert_eq!(decoded.auto_save_slot, wire_pair(&disk.auto_save_slot));
            let expected_manual: [Option<GameData>; 100] =
                std::array::from_fn(|i| wire_pair(&disk.manual_save_slots[i]));
            assert_eq!(decoded.manual_save_slots, expected_manual);
            // Disk projection is explicitly allowed to truncate full extra/
            // high trailing values and omit dormant record when active0.
            assert_eq!(manual_rows(&f.game), manual);
            drop(ui);
            assert!(weak.upgrade().is_none());
            assert!(f.game.globals.storage.is_none());
        });
    }
    eprintln!(
        "Gameplay tail:2 authored-bounded720p/1080p profiles;TROPHY continuation,L/D read invariance,3 unlock banks,full101rows,one4OS+MT/copy,real two-voice EOF,autonomous VOID/identity retire and pure cached redraw; no window or save writes"
    );
}

#[test]
#[ignore = "actual Gameplay FakeSelect post-attachment clock failure retaining child/cache, offscreen Metal/exact Loopback Audio; no desktop recovery claim"]
fn higurashi_gameplay_fake_select_clock_fault_retains_published_child_without_reconstruction() {
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    let authored: Arc<HigurashiScenario> = server.load_sync("/main.snr").unwrap();
    let mut code = MESSAGE.to_vec();
    code.extend([0x83, 0, 63]);
    code.extend(authored_slice(&authored, "FAKESELECT", 0));
    code.extend(MARKER);
    let mut f = fixture(&device, &queue, &server, &code, (1280, 720), 1);
    f.wait(63);
    let os = f.os.load(Ordering::SeqCst);
    let mt = crate::engine_random::seed_call_count();
    let before_read = read_snapshot(&f.game);
    let before_auto = f
        .game
        .globals
        .runtime_saves
        .as_ref()
        .unwrap()
        .auto
        .data
        .clone();
    let dates = Arc::new(AtomicUsize::new(0));
    let calls = dates.clone();
    f.game.set_clock_source(move || {
        calls.fetch_add(1, Ordering::SeqCst);
        bail!("controlled FakeSelect clock unavailable")
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    let cache_at_fault = loop {
        f.frames.number += 1;
        let before_cache = f
            .game
            .host()
            .unwrap()
            .runner
            .frame()
            .cache
            .reference_snapshot();
        let result = shin_tasks::block_on(f.game.update(
            Ticks::from_f32(3.),
            true,
            Buttons::default(),
            &PointerFrame {
                number: f.frames.number,
                time_seconds: f.frames.number as f64 / 60.,
                events: &[],
            },
            policy(),
            audio(),
        ));
        if result.is_err() {
            break before_cache;
        }
        assert!(
            Instant::now() < deadline,
            "FakeSelect did not reach failing clock"
        );
    };
    let ui = fake(&f.game).expect("native attach must precede11780 clock");
    assert_eq!(
        f.game.host().unwrap().reference_touch_overlay_identity(),
        Some(ui.identity())
    );
    assert_eq!(
        ui.0.lock().model.phase,
        0,
        "Fault must stop before visual update"
    );
    assert_eq!(f.os.load(Ordering::SeqCst), os + 4);
    assert_eq!(crate::engine_random::seed_call_count(), mt + 1);
    assert_eq!(dates.load(Ordering::SeqCst), 1);
    assert_eq!(read_snapshot(&f.game), before_read);
    assert_eq!(
        f.game.globals.runtime_saves.as_ref().unwrap().auto.data,
        before_auto
    );
    // Waiting-frame prepares before the fault are valid; the failing call
    // itself must not prepare the newly attached widget or mutate its seed.
    assert_eq!(
        f.game
            .host()
            .unwrap()
            .runner
            .frame()
            .cache
            .reference_snapshot(),
        cache_at_fault
    );
    let cache = f
        .game
        .host()
        .unwrap()
        .runner
        .frame()
        .cache
        .reference_snapshot();
    let state = model_words(&ui);
    let a = pixels(&device, &queue, (1280, 720), &mut f.game);
    let b = pixels(&device, &queue, (1280, 720), &mut f.game);
    same_pixels(&a, &b);
    assert_eq!(model_words(&ui), state);
    assert_eq!(
        f.game
            .host()
            .unwrap()
            .runner
            .frame()
            .cache
            .reference_snapshot(),
        cache
    );
    f.frames.number += 1;
    assert!(
        shin_tasks::block_on(f.game.update(
            Ticks::ZERO,
            true,
            Buttons::default(),
            &PointerFrame {
                number: f.frames.number,
                time_seconds: 0.,
                events: &[]
            },
            policy(),
            audio()
        ))
        .is_err()
    );
    assert_eq!(fake(&f.game).unwrap().identity(), ui.identity());
    assert_eq!(f.os.load(Ordering::SeqCst), os + 4);
    assert_eq!(crate::engine_random::seed_call_count(), mt + 1);
    assert_eq!(dates.load(Ordering::SeqCst), 1);
    assert_eq!(model_words(&ui), state);
    assert!(f.game.reference_overlay_cues.is_empty());
    assert!(f.game.bindings.progress.pending_persistent().is_empty());
    assert!(f.game.globals.storage.is_none());
    // Pure command/service tests cover retry. Gameplay deliberately retains
    // its explicit desktop fault; this fixture neither clears it nor claims
    // a native exception rollback/recovery path.
}
