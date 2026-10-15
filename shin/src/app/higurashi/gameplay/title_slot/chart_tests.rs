//! NEW unregistered actual Title/ADV Chart-wrapper acceptance candidates.
//! Root owns compilation and all GPU/exact-Loopback execution. Capture checks
//! here prove retained composition transport, not independent fragment math.
use super::{Pending, TitleSlot};
use crate::{
    adv::{
        higurashi::{
            audio::HigurashiAudio,
            autosave::{Execution, ProgressEntry, SaveProgress},
            input::Buttons,
            music_commands::MusicCommands,
            reference_gpu_assets, reference_render_to_texture,
        },
        input_area::PointerFrame,
        play_clock::PlayClock,
    },
    app::higurashi::{
        chart::{catalog::Catalog, owner::Owner},
        first_boot::CommonAssets,
        gameplay::{
            Gameplay,
            tests::{audio, fresh, policy},
        },
        input_bridge::Analog,
        load_game::{Source, Storage},
        runtime_game_data::RuntimeGameData,
        runtime_saves::Slots,
        scene_host::{Globals, TitleTransition},
        title_demo::TitleRequest,
    },
    asset::system::AssetServer,
    audio::SystemSoundPlayer,
};
use anyhow::Result;
use parking_lot::Mutex;
use shin_core::{
    format::save::higurashi::{SaveData, SaveDate},
    primitives::color::UnormColor,
    time::Ticks,
    vm::higurashi::command::HigurashiRuntimeCommand as Cmd,
};
use shin_render::{
    render_texture::RenderTexture,
    shaders::types::texture::{TextureTarget, TextureTargetKind},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

const EXTENTS: [(u32, u32); 2] = [(1280, 720), (1920, 1080)];
const SEED: u32 = 0x2468_acf1;
#[derive(Default)]
struct MemoryStorage {
    bytes: Mutex<Vec<u8>>,
    writes: AtomicUsize,
}
impl Storage for MemoryStorage {
    fn trial_token(&self) -> bool {
        false
    }
    fn read(&self, _: Source) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }
    fn trial_exists(&self) -> Result<bool> {
        Ok(false)
    }
    fn write(&self, bytes: &[u8]) -> Result<()> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        *self.bytes.lock() = bytes.to_vec();
        Ok(())
    }
}
struct Fixture {
    device: wgpu::Device,
    queue: wgpu::Queue,
    server: Arc<AssetServer>,
    common: Arc<CommonAssets>,
}
impl Fixture {
    fn new() -> Self {
        let (device, queue, server) = reference_gpu_assets();
        let server = Arc::new(server);
        let common = Arc::new(CommonAssets {
            scenario: server.load_sync("/main.snr").unwrap(),
            font: server.load_sync("/newrodin.fnt").unwrap(),
            system: server.load_sync("/systex.txa").unwrap(),
            message: server.load_sync("/msgtex.txa").unwrap(),
            effects: server.load_sync("/effects.txa").unwrap(),
            sysse: server.load_sync("/sysse.bin").unwrap(),
        });
        Self {
            device,
            queue,
            server,
            common,
        }
    }
    fn globals(&self, common: Arc<CommonAssets>) -> (Globals, Arc<MemoryStorage>, Vec<u8>) {
        let manager = Arc::new(shin_audio::AudioManager::new_for_tests()); // EXACT Loopback Audio.
        let catalog = Catalog::from_scenario(&common.scenario).unwrap();
        assert_eq!(catalog.records.len(), 1449);
        let max = catalog
            .records
            .iter()
            .filter(|r| r.kind == 0)
            .map(|r| usize::from(r.value))
            .max()
            .unwrap();
        let mut saved = SaveData::default();
        saved.persist_data.0 = vec![31; 192];
        saved.save_vectors.extra_words_1 = vec![u32::MAX; max / 32 + 1];
        saved.save_vectors.seen_messages = vec![0xa5, 0x8765_4321];
        saved.save_vectors.select_seen_choices = vec![0xdead_beef];
        saved.save_vectors.select_last_choices = vec![7, 15, 3];
        saved.manual_save_slots[2] = Some(fresh());
        let bytes = saved.encode().unwrap();
        let storage = Arc::new(MemoryStorage {
            bytes: Mutex::new(bytes.clone()),
            writes: AtomicUsize::new(0),
        });
        let slots = Slots::loaded(&saved);
        let mut sounds = SystemSoundPlayer::new(manager.clone());
        sounds.bank = Some(common.sysse.clone());
        (
            Globals {
                common: common.clone(),
                data: saved,
                storage: Some(storage.clone()),
                runtime_saves: Some(slots),
                clock: PlayClock::from_saved_seconds(17),
                platform_flag: true,
                last_title_mode: 0,
                audio: HigurashiAudio::new(
                    manager.clone(),
                    self.server.clone(),
                    common.scenario.clone(),
                ),
                server: self.server.clone(),
                manager,
                sounds: Arc::new(Mutex::new(sounds)),
            },
            storage,
            bytes,
        )
    }
    fn title(
        &self,
        extent: (u32, u32),
        seed: Arc<dyn Fn() -> u32 + Send + Sync>,
    ) -> (TitleSlot, Arc<MemoryStorage>, Vec<u8>) {
        let (globals, storage, bytes) = self.globals(self.common.clone());
        let mut slot = TitleSlot::new(
            TitleTransition {
                request: TitleRequest {
                    selected: 3,
                    mode: 0,
                },
                globals,
            },
            None,
        );
        let title = slot.title.as_mut().unwrap();
        title.resize_menu_input((extent.0 as u16, extent.1 as u16));
        title.reference_chart_seed(seed);
        (slot, storage, bytes)
    }
    fn game(
        &self,
        extent: (u32, u32),
        seed: Arc<dyn Fn() -> u32 + Send + Sync>,
    ) -> (TitleSlot, Arc<MemoryStorage>, Vec<u8>, Arc<AtomicUsize>) {
        // Real SAVEINFO/CHART/MSGSET commands through the actual decoder/Runner.
        // The rest of the authored scenario (including Chart3000/3100) remains.
        let mut code = self.common.scenario.raw()[0x2545e..0x25465].to_vec();
        // Same independently audited authored BGM command used by the existing
        // real Gameplay replacement fixture, followed by controlled owners.
        code.extend([
            0xa0, 0, 2, b'A', 0, 0xa0, 1, 2, b'B', 0, 0xd3, 0, 4, 0, 0, 32, 7, 0x86, 1, 0, 0, 1,
            0x7f, 4, 0, b'@', b'r', b'A', 0, 0x83, 0, 0x83, 0xe8, 0, 0, 0,
        ]);
        let (_, common) =
            crate::app::higurashi::gameplay::tests::character::common(&self.server, &code);
        let (globals, storage, bytes) = self.globals(common);
        let mut data = RuntimeGameData::from(fresh());
        data.date = SaveDate {
            year: 2025,
            month: 11,
            day: 23,
            hour: 13,
            minute: 14,
            second: 15,
        };
        data.progress.extra_word = 2;
        data.progress.entries[0].random_seed = 42;
        data.progress.entries[0].response_data = vec![0x23, 0xee, 0x75, 0xff];
        data.progress.entries[1] = ProgressEntry {
            scenario_id: 2084,
            random_seed: 0x8765_4321,
            save_position: 23,
            replay_target: 29,
            response_data: vec![0xa5, 0xf1, 0x23],
        };
        data.trailing_values = vec![0xffff, 0x1234, 71];
        data.byte_variables = vec![255, 7, 128, 37];
        let mut game = None;
        reference_render_to_texture(&self.device, &self.queue, extent, |ctx, _| {
            let mut initial = ctx.new_render_texture("Chart actual ADV initial".into());
            {
                let mut pass = ctx.begin_pass(
                    initial.as_texture_target(),
                    Some(ctx.depth_stencil),
                    "Chart initial",
                );
                pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.));
            }
            game = Some(Gameplay::new(globals, data, 0, initial, || [1, 2, 3, 4]).unwrap());
        });
        let mut game = game.unwrap();
        game.resize_input((extent.0 as u16, extent.1 as u16))
            .unwrap();
        game.chart_seed = seed;
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        game.set_clock_source(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(SaveDate {
                year: 2026,
                month: 10,
                day: 14,
                hour: 12,
                minute: 34,
                second: 56,
            })
        });
        (TitleSlot::from_game(game), storage, bytes, calls)
    }
    fn capture(&self, slot: &mut TitleSlot, extent: (u32, u32)) -> (RenderTexture, Vec<u8>) {
        let mut composed = None;
        let pixels =
            reference_render_to_texture(&self.device, &self.queue, extent, |ctx, target| {
                slot.prepare(ctx).unwrap();
                let mut frame =
                    ctx.new_render_texture("Chart full retained source composition".into());
                for output in [
                    frame.as_texture_target(),
                    TextureTarget {
                        view: target,
                        kind: TextureTargetKind::RenderTexture,
                    },
                ] {
                    let mut pass = ctx.begin_pass(
                        output,
                        Some(ctx.depth_stencil),
                        "Chart wrapper composition",
                    );
                    pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.));
                    slot.render(&mut pass).unwrap();
                }
                composed = Some(frame);
            });
        (composed.unwrap(), pixels)
    }
    fn copied(&self, snapshot: &RenderTexture, extent: (u32, u32)) -> Vec<u8> {
        reference_render_to_texture(&self.device, &self.queue, extent, |ctx, target| {
            let overlay = crate::adv::higurashi::runner::reconstruction_mode::Overlay {
                snapshot: snapshot.copy_snapshot(&mut ctx.render_clone_ctx()),
                mask: None,
                clock: crate::adv::higurashi::reconstruction_mode::OverlayClock::captured(true),
            };
            let mut pass = ctx.begin_pass(
                TextureTarget {
                    view: target,
                    kind: TextureTargetKind::RenderTexture,
                },
                Some(ctx.depth_stencil),
                "Chart pure retained capture",
            );
            pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.));
            overlay.render(&mut pass);
        })
    }
    fn dispatch(
        &self,
        slot: &mut TitleSlot,
        composed: &RenderTexture,
        extent: (u32, u32),
    ) -> Result<bool> {
        let mut result = None;
        reference_render_to_texture(&self.device, &self.queue, extent, |ctx, _| {
            result = Some(slot.dispatch_chart_adv(ctx, composed));
        });
        result.unwrap()
    }
}
#[derive(Default)]
struct Frames {
    number: u32,
}
impl Frames {
    fn step(&mut self, slot: &mut TitleSlot, delta: f32, pressed: u32, repeat: u32) {
        self.number += 1;
        let pointer = PointerFrame {
            number: self.number,
            time_seconds: self.number as f64 / 60.,
            events: &[],
        };
        let buttons = Buttons {
            pressed,
            repeat,
            ..Default::default()
        };
        if slot.title.is_some() {
            slot.update_title(Ticks::from_f32(delta), true, buttons, &pointer)
                .unwrap();
        } else {
            shin_tasks::block_on(slot.update_game(
                Ticks::from_f32(delta),
                true,
                buttons,
                &pointer,
                policy(),
                audio(),
            ))
            .unwrap();
        }
    }
    fn until(&mut self, slot: &mut TitleSlot, predicate: impl Fn(&TitleSlot) -> bool) {
        let end = Instant::now() + Duration::from_secs(20);
        while !predicate(slot) {
            self.step(slot, 3., 0, 0);
            assert!(
                Instant::now() < end,
                "Chart wrapper timed out: {:?},Question {:?}",
                chart(slot).map(Owner::reference_state),
                chart(slot).and_then(Owner::reference_question)
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn title_chart(&mut self, slot: &mut TitleSlot) {
        self.until(slot, |s| {
            s.title.as_ref().unwrap().reference_menu_state() == Some(("top", 2))
        });
        assert_eq!(
            slot.title.as_ref().unwrap().reference_top_selection(),
            Some(3)
        );
        self.step(slot, 0., 1 << 24, 0);
        self.until(slot, |s| chart(s).is_some());
        assert_eq!(chart(slot).unwrap().reference_state().3, 0f32.to_bits());
        self.until(slot, |s| chart(s).unwrap().reference_state().2);
        assert_eq!(chart(slot).unwrap().reference_state().4, 0);
    }
    fn game_chart(&mut self, slot: &mut TitleSlot, fixture: &Fixture, extent: (u32, u32)) {
        self.until(slot, |s| {
            let g = s.game.as_ref().unwrap();
            g.input_permission
                && !g.host().unwrap().has_modes()
                && g.host()
                    .unwrap()
                    .runner
                    .native_read_state()
                    .is_some_and(|v| v.flags & 0x8000_0000 != 0)
        });
        assert_eq!(
            slot.game.as_ref().unwrap().bindings.memory.extra_words,
            [0, 0, 32, 7]
        );
        // Real mapped Y input requests SystemMenu. prepare performs the actual
        // production constructor; no ChildRequest/completion is synthesized.
        let end = Instant::now() + Duration::from_secs(20);
        while slot
            .game
            .as_ref()
            .unwrap()
            .host()
            .unwrap()
            .reference_menu_control()
            .is_none()
        {
            self.step(slot, 3., 1 << 15, 0);
            fixture.capture(slot, extent);
            self.step(slot, 3., 0, 0);
            assert!(
                Instant::now() < end,
                "Actual Y input did not open SystemMenu"
            );
        }
        self.until(slot, |s| {
            s.game
                .as_ref()
                .unwrap()
                .host()
                .unwrap()
                .reference_menu_control()
                .is_some_and(|(_, ready)| ready)
        });
        while slot
            .game
            .as_ref()
            .unwrap()
            .host()
            .unwrap()
            .reference_menu_control()
            .unwrap()
            .0
            != 4
        {
            self.step(slot, 0., 0, 1 << 6);
            self.step(slot, 0., 0, 0);
            assert!(
                Instant::now() < end,
                "Actual SystemMenu navigation did not reach Item4"
            );
        }
        self.step(slot, 0., 1 << 24, 0);
        self.until(slot, |s| chart(s).is_some());
        assert_eq!(chart(slot).unwrap().reference_state().3, 0f32.to_bits());
        self.until(slot, |s| chart(s).unwrap().reference_state().2);
        assert_eq!(chart(slot).unwrap().reference_state().4, 2);
    }
    fn yes(&mut self, slot: &mut TitleSlot, count: &AtomicUsize) -> RuntimeGameData {
        let state = chart(slot).unwrap().reference_state();
        let index = state.5;
        assert!(index >= 0);
        let scenario = scenario_for_node(slot, index);
        self.step(slot, 0., 1 << 24, 0);
        self.until(slot, |s| {
            chart(s)
                .unwrap()
                .reference_question()
                .is_some_and(|(_, p, _)| p == 2)
        });
        assert_eq!(
            chart(slot).unwrap().reference_question(),
            Some((index, 2, 1))
        );
        self.step(slot, 0., 0, 1 << 7);
        assert_eq!(
            chart(slot).unwrap().reference_question(),
            Some((index, 2, 0))
        );
        self.step(slot, 0., 1 << 24, 0);
        self.until(slot, |s| {
            chart(s).unwrap().reference_question() == Some((index, 4, 0))
        });
        assert!(
            !slot.pending_chart_adv(),
            "Actual Question stage0 must NOT select Chart"
        );
        assert_eq!(count.load(Ordering::SeqCst), 0);
        assert_eq!(chart(slot).unwrap().reference_state().3, 1f32.to_bits());
        self.until(slot, |s| {
            chart(s).unwrap().reference_question() == Some((index, -1, 0))
        });
        assert!(
            !slot.pending_chart_adv(),
            "New private Exit awaits its next snapshot"
        );
        assert_eq!(count.load(Ordering::SeqCst), 0);
        self.until(slot, TitleSlot::pending_chart_adv);
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert!(!chart(slot).unwrap().complete());
        expected(state.4, scenario)
    }
}
fn chart(slot: &TitleSlot) -> Option<&Owner> {
    if let Some(title) = &slot.title {
        title.reference_chart_owner()
    } else {
        slot.game.as_ref()?.chart_menu.as_ref().map(|m| &m.owner)
    }
}
fn seed_source() -> (Arc<dyn Fn() -> u32 + Send + Sync>, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    (
        Arc::new(move || SEED.wrapping_add(counter.fetch_add(1, Ordering::SeqCst) as u32)),
        calls,
    )
}
fn scenario_for_node(slot: &TitleSlot, index: i32) -> i32 {
    let common = if let Some(t) = &slot.title {
        &t.globals.common
    } else {
        &slot.game.as_ref().unwrap().globals.common
    };
    let catalog = Catalog::from_scenario(&common.scenario).unwrap();
    // All kind0 unlocks are set in this fixture; native node order is the
    // authored1..4 order. This reads the selected scenario, never writes it.
    let record = catalog
        .records
        .iter()
        .filter(|r| (1..=4).contains(&r.kind))
        .nth(index as usize)
        .unwrap();
    assert_eq!(record.kind, 1);
    let scenario = i32::from(record.value as i16);
    assert!(scenario >= 0);
    scenario
}
fn expected(group: i32, scenario: i32) -> RuntimeGameData {
    let empty = ProgressEntry {
        scenario_id: 0,
        random_seed: 0,
        save_position: 0,
        replay_target: 0,
        response_data: vec![],
    };
    RuntimeGameData::from_progress(
        SaveProgress::new(
            0,
            group as u32,
            [
                ProgressEntry {
                    scenario_id: scenario,
                    random_seed: SEED,
                    ..empty.clone()
                },
                empty,
            ],
        )
        .unwrap(),
        SaveDate {
            year: 0,
            month: 0,
            day: 0,
            hour: 0,
            minute: 0,
            second: 0,
        },
        vec![],
        vec![],
    )
}
fn no_write(storage: &MemoryStorage, before: &[u8]) {
    assert_eq!(
        storage.writes.load(Ordering::SeqCst),
        0,
        "Chart must not create SaveGame"
    );
    assert_eq!(&*storage.bytes.lock(), before);
}
fn finish_factory(
    fixture: &Fixture,
    slot: &mut TitleSlot,
    extent: (u32, u32),
    data: &RuntimeGameData,
    count: &AtomicUsize,
) {
    let from_game = slot.game.is_some();
    let sounds = slot.globals().unwrap().sounds.clone();
    let clock = slot.globals().unwrap().clock.ticks();
    let (composed, pixels) = fixture.capture(slot, extent);
    let atlas: Arc<crate::adv::character_view::CharacterTextures> =
        fixture.server.load_sync("/chart.txa").unwrap();
    let weak_atlas = Arc::downgrade(&atlas);
    drop(atlas);
    assert!(weak_atlas.upgrade().is_some());
    if let Some(Pending::ChartAdv(transfer)) = &slot.pending {
        assert_eq!(&transfer.data, data);
        assert_eq!(transfer.flags, 0);
        assert!(transfer.globals.is_none() && transfer.snapshot.is_none());
    } else {
        panic!("Actual Chart callback must retain its full factory");
    }
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert_eq!(
        slot.globals().unwrap().clock.ticks(),
        clock,
        "Prepare/render is not an update"
    );
    slot.reference_fail_next_constructor();
    let error = fixture.dispatch(slot, &composed, extent).unwrap_err();
    assert!(format!("{error:#}").contains("Injected retained scene constructor fault"));
    assert!(
        slot.title.is_none() && slot.game.is_none(),
        "Source death precedes actual constructor"
    );
    assert!(
        weak_atlas.upgrade().is_none(),
        "Source Chart worker/atlas retired before constructor retry"
    );
    assert_eq!(count.load(Ordering::SeqCst), 1);
    let Some(Pending::ChartAdv(transfer)) = &slot.pending else {
        panic!("Fault lost retained Chart payload")
    };
    assert_eq!(&transfer.data, data);
    let bindings = transfer.bindings.as_ref().unwrap();
    assert_eq!(&bindings.runtime_game_data().unwrap(), data);
    assert!(bindings.memory.entries().is_empty() && bindings.memory.extra_words.is_empty());
    assert!(
        bindings.progress.pending_persistent().is_empty()
            && bindings.progress.pending_bytes().is_empty()
    );
    assert_eq!(bindings.progress.responses().1, 0);
    assert!(Arc::ptr_eq(
        &transfer.globals.as_ref().unwrap().sounds,
        &sounds
    ));
    assert_eq!(
        fixture.copied(transfer.snapshot.as_ref().unwrap(), extent),
        pixels
    );
    assert!(fixture.dispatch(slot, &composed, extent).unwrap());
    assert!(!slot.pending_chart_adv());
    let game = slot.game.as_ref().unwrap();
    assert_eq!(
        &game.runtime_game_data().unwrap(),
        data,
        "Fresh complete two-record/four-vector payload"
    );
    assert!(game.chart_menu.is_none());
    assert!(
        game.bindings.memory.entries().is_empty() && game.bindings.memory.extra_words.is_empty()
    );
    assert!(Arc::ptr_eq(&game.globals.sounds, &sounds));
    assert_eq!(game.globals.clock.ticks(), clock);
    assert_eq!(
        fixture.copied(&game.snapshot.as_ref().unwrap().snapshot, extent),
        pixels
    );
    assert_eq!(
        count.load(Ordering::SeqCst),
        1,
        "Retry never resamples Chart's selected seed"
    );
    assert_eq!(
        slot.reference_scene_events
            .iter()
            .filter(|&&v| v == 1)
            .count(),
        1
    );
    assert_eq!(
        slot.reference_scene_events
            .iter()
            .filter(|&&v| v == 2)
            .count(),
        1
    );
    assert!(!from_game || game.bindings.history.bits().words().len() > 0);
}

#[test]
#[ignore = "actual Top3 Chart modal/Return0+1/real Question stage1/deferred full ADV/capture/fault retry720p1080p;offscreen GPU/exact Loopback Audio;no user saves/window"]
fn higurashi_chart_title_wrapper_retains_source_and_full_shortcut_factory() {
    let fixture = Fixture::new();
    for extent in EXTENTS {
        let (seed, count) = seed_source();
        let (mut slot, storage, before) = fixture.title(extent, seed);
        let mut frames = Frames::default();
        frames.title_chart(&mut slot);
        assert_eq!(count.load(Ordering::SeqCst), 0);
        let bgm = slot.title.as_ref().unwrap().globals.audio.bgm_clock();
        assert_ne!(slot.title.as_ref().unwrap().globals.audio.bgm_status(), 0);
        let clock = slot.globals().unwrap().clock.ticks();
        frames.step(&mut slot, 10., 0, 0);
        assert_eq!(
            slot.globals().unwrap().clock.ticks(),
            clock + 10.,
            "Full play clock is not clamped to Chart delta3"
        );
        fixture.capture(&mut slot, extent);
        frames.step(&mut slot, 0., 1 << 25, 0);
        frames.until(&mut slot, |s| !chart(s).unwrap().reference_state().2);
        assert!(!chart(&slot).unwrap().complete());
        assert_eq!(
            slot.title.as_ref().unwrap().reference_top_selection(),
            None,
            "Return0 cannot recreate Top"
        );
        assert_eq!(chart(&slot).unwrap().reference_state().3, 1f32.to_bits());
        frames.until(&mut slot, |s| chart(s).unwrap().complete());
        assert_eq!(
            slot.title.as_ref().unwrap().reference_top_selection(),
            Some(3)
        );
        assert_eq!(
            slot.title.as_ref().unwrap().reference_menu_state(),
            Some(("chart", 0)),
            "Old complete child remains in this draw snapshot"
        );
        frames.step(&mut slot, 0., 0, 0);
        assert!(chart(&slot).is_none());
        frames.until(&mut slot, |s| {
            s.title.as_ref().unwrap().reference_menu_state() == Some(("top", 2))
        });
        assert!(slot.title.as_ref().unwrap().globals.audio.bgm_clock() >= bgm);
        assert_ne!(slot.title.as_ref().unwrap().globals.audio.bgm_status(), 0);
        no_write(&storage, &before);
        frames.title_chart(&mut slot);
        // Actual runtime analog bridge: aliases must not add another held-pan.
        for _ in 0..7 {
            slot.set_analog_input(Analog {
                axes: [127, 0, 0, 0],
                dpad_held: 0,
            });
            frames.step(&mut slot, 0., 0, 0);
        }
        assert_eq!(chart(&slot).unwrap().reference_state().5, -1);
        for _ in 0..7 {
            slot.set_analog_input(Analog {
                axes: [-127, 0, 0, 0],
                dpad_held: 0,
            });
            frames.step(&mut slot, 0., 0, 0);
        }
        slot.set_analog_input(Analog::default());
        assert_eq!(chart(&slot).unwrap().reference_state().5, 0);
        let expected = frames.yes(&mut slot, &count);
        assert_eq!(expected.progress.entries[0].scenario_id, 3000);
        assert!(slot.title.is_some());
        assert_eq!(slot.title.as_ref().unwrap().reference_top_selection(), None);
        no_write(&storage, &before);
        finish_factory(&fixture, &mut slot, extent, &expected, &count);
        no_write(&storage, &before);
    }
    eprintln!(
        "Chart Title:2 actual Top3/back0+1/native snapshots,4 modal constructions,2 real stage1 shortcut Questions,one full retained seed each,source death/capture/retry at720p1080p;no SaveGame"
    );
}

#[test]
#[ignore = "actual mapped Y/SystemMenu4/Chart source resume,paused VM and pending queues,Question stage1/full fresh flags0 ADV/fault retry720p1080p;offscreen GPU/exact Loopback Audio;no user saves/window"]
fn higurashi_chart_system_menu_wrapper_preserves_paused_source_and_shortcut_payload() {
    let fixture = Fixture::new();
    for extent in EXTENTS {
        let (seed, count) = seed_source();
        let (mut slot, storage, before, dates) = fixture.game(extent, seed);
        let mut frames = Frames::default();
        frames.game_chart(&mut slot, &fixture, extent);
        assert_eq!(count.load(Ordering::SeqCst), 0);
        // Complete the native hide animation. The menu's phase/readiness
        // getter alone cannot distinguish fully hidden rows from visible ones.
        for _ in 0..10 {
            frames.step(&mut slot, 3., 0, 0);
        }
        assert!(
            slot.game
                .as_ref()
                .unwrap()
                .host()
                .unwrap()
                .reference_system_menu_rows()
                .unwrap()
                .1
                .iter()
                .all(|(_, slide)| *slide == 0.)
        );
        let game = slot.game.as_mut().unwrap();
        game.bindings
            .progress
            .dispatch_variables(&Cmd::SSET { args: [2, 999] }, Execution::Live, 0)
            .unwrap();
        game.bindings
            .progress
            .dispatch_variables(&Cmd::OpcodeDB { args: [0, 255] }, Execution::Live, 0)
            .unwrap();
        let source = game.runtime_game_data().unwrap();
        let commands = game.host().unwrap().runner.completed_commands();
        let visual = game.host().unwrap().runner.frame().visual_epoch();
        let date_calls = dates.load(Ordering::SeqCst);
        let clock = game.globals.clock.ticks();
        let music_clock = game.host().unwrap().audio.bgm_clock();
        assert_ne!(game.host().unwrap().audio.bgm_status(), 0);
        let cue = game.host().unwrap().modal_pause_depth();
        assert_eq!(cue, 1);
        frames.step(&mut slot, 10., 0, 0);
        let game = slot.game.as_ref().unwrap();
        assert_eq!(game.globals.clock.ticks(), clock + 10.);
        assert_eq!(game.host().unwrap().runner.completed_commands(), commands);
        assert_eq!(game.host().unwrap().runner.frame().visual_epoch(), visual);
        assert_eq!(game.runtime_game_data().unwrap(), source);
        fixture.capture(&mut slot, extent);
        frames.step(&mut slot, 0., 1 << 25, 0);
        frames.until(&mut slot, |s| !chart(s).unwrap().reference_state().2);
        assert!(!chart(&slot).unwrap().complete());
        assert!(
            slot.game
                .as_ref()
                .unwrap()
                .host()
                .unwrap()
                .reference_system_menu_rows()
                .unwrap()
                .1
                .iter()
                .all(|(_, slide)| *slide == 0.),
            "Return0 cannot reopen the hidden SystemMenu rows"
        );
        frames.until(&mut slot, |s| chart(s).unwrap().complete());
        assert!(
            slot.game
                .as_ref()
                .unwrap()
                .host()
                .unwrap()
                .reference_system_menu_rows()
                .unwrap()
                .1
                .iter()
                .any(|(_, slide)| *slide > 0.),
            "Return1 must run actual child_returned/1ba34 before lower menu clocks"
        );
        assert!(
            slot.game.as_ref().unwrap().chart_menu.is_some(),
            "Completion-frame child remains owned until prune"
        );
        frames.step(&mut slot, 0., 0, 0);
        assert!(chart(&slot).is_none());
        frames.until(&mut slot, |s| {
            s.game
                .as_ref()
                .unwrap()
                .host()
                .unwrap()
                .reference_menu_control()
                == Some((4, true))
        });
        let game = slot.game.as_ref().unwrap();
        assert_eq!(game.host().unwrap().modal_pause_depth(), 1);
        assert_eq!(game.host().unwrap().runner.completed_commands(), commands);
        assert_eq!(game.host().unwrap().runner.frame().visual_epoch(), visual);
        assert_eq!(game.runtime_game_data().unwrap(), source);
        assert_eq!(game.bindings.memory.extra_words, [0, 0, 32, 7]);
        assert_eq!(game.bindings.progress.pending_persistent().len(), 1);
        assert_eq!(game.bindings.progress.pending_bytes().len(), 1);
        assert_eq!(dates.load(Ordering::SeqCst), date_calls);
        assert_ne!(game.host().unwrap().audio.bgm_status(), 0);
        assert!(game.host().unwrap().audio.bgm_clock() >= music_clock);
        no_write(&storage, &before);
        // Same actual selected Item4 reopens through the host's input callback.
        frames.step(&mut slot, 0., 1 << 24, 0);
        frames.until(&mut slot, |s| chart(s).is_some());
        frames.until(&mut slot, |s| chart(s).unwrap().reference_state().2);
        assert_eq!(chart(&slot).unwrap().reference_state().4, 2);
        let expected = frames.yes(&mut slot, &count);
        assert_eq!(expected.progress.entries[0].scenario_id, 3100);
        let game = slot.game.as_ref().unwrap();
        assert_eq!(game.runtime_game_data().unwrap(), source);
        assert_eq!(game.host().unwrap().runner.completed_commands(), commands);
        assert_eq!(game.host().unwrap().runner.frame().visual_epoch(), visual);
        assert_eq!(game.host().unwrap().modal_pause_depth(), 1);
        assert_eq!(dates.load(Ordering::SeqCst), date_calls);
        no_write(&storage, &before);
        finish_factory(&fixture, &mut slot, extent, &expected, &count);
        assert_eq!(dates.load(Ordering::SeqCst), date_calls);
        no_write(&storage, &before);
    }
    eprintln!(
        "Chart ADV:2 actual Y/SystemMenu4/back0+1/native prune/resume,4 modal constructions,real CHART route/full source with pending writes and visual/VM freeze,2 actual stage1 Questions/fresh complete ADV payloads,one retained seed/source retirement/capture/retry at720p1080p;zero storage writes"
    );
}
