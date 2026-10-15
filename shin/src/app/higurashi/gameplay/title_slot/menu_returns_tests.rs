//! Actual SystemMenu5/7 dialogs, staged source retention and next-frame scenes.
use super::*;
use crate::{
    adv::{
        higurashi::{
            audio::HigurashiAudio, autosave::Execution, host::system_menu::MenuReturn,
            reference_gpu_assets, reference_render_to_texture,
        },
        play_clock::PlayClock,
    },
    app::higurashi::{
        gameplay::tests::{audio, fresh, policy},
        load_game::{Paths, Source, Storage},
        runtime_game_data::RuntimeGameData,
        runtime_saves::Slots,
    },
    audio::SystemSoundPlayer,
};
use shin_core::{
    primitives::color::UnormColor, vm::higurashi::command::HigurashiRuntimeCommand as Cmd,
};
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

struct Store {
    path: Paths,
    allowed: bool,
    writes: AtomicUsize,
    bytes: Mutex<Vec<u8>>,
    gate: (std::sync::Mutex<bool>, std::sync::Condvar),
}
impl Store {
    fn new(allowed: bool) -> Arc<Self> {
        let name = format!(
            "higurashi-system-menu-return-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        Arc::new(Self {
            path: Paths {
                current: Some(std::env::temp_dir().join(name).join("progress.dat")),
                trial: None,
            },
            allowed,
            writes: AtomicUsize::new(0),
            bytes: Mutex::new(Vec::new()),
            gate: (std::sync::Mutex::new(!allowed), std::sync::Condvar::new()),
        })
    }
    fn release(&self) {
        *self.gate.0.lock().unwrap() = true;
        self.gate.1.notify_all();
    }
}
impl Storage for Store {
    fn trial_token(&self) -> bool {
        false
    }
    fn trial_exists(&self) -> Result<bool> {
        Ok(false)
    }
    fn read(&self, source: Source) -> Result<Option<Vec<u8>>> {
        self.path.read(source)
    }
    fn write(&self, bytes: &[u8]) -> Result<()> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        ensure!(
            self.allowed,
            "Menu rejection/Kakera must not write progress"
        );
        *self.bytes.lock() = bytes.to_vec();
        let mut open = self.gate.0.lock().unwrap();
        while !*open {
            open = self.gate.1.wait(open).unwrap();
        }
        drop(open);
        self.path.write(bytes)
    }
}
struct Release(Arc<Store>);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.release();
    }
}

struct Fixture {
    device: wgpu::Device,
    queue: wgpu::Queue,
    server: Arc<AssetServer>,
    common: Arc<crate::app::higurashi::first_boot::CommonAssets>,
}
impl Fixture {
    fn new() -> Self {
        let (device, queue, server) = reference_gpu_assets();
        let server = Arc::new(server);
        let (_, common) = crate::app::higurashi::gameplay::tests::character::common(
            &server,
            &[
                // Y opens SystemMenu only while the native message owns input.
                // Decode a real MSGSET before the controlled WAIT.
                0x86, 1, 0, 0, 1, 0x7f, 4, 0, b'@', b'r', b'A', 0, 0x83, 1, 0x83, 0xe8, 0, 0, 0,
                0x83, 0, 1,
            ],
        );
        Self {
            device,
            queue,
            server,
            common,
        }
    }
    fn game(
        &self,
        size: (u32, u32),
        scenario: i32,
        active: usize,
        store: Arc<Store>,
    ) -> (TitleSlot, Frames, Arc<AtomicUsize>, Arc<AtomicUsize>) {
        let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
        let mut data = SaveData::default();
        data.persist_data.0 = vec![3; 192];
        data.settings.common.v3_sysvol = 37;
        data.save_vectors.seen_messages = vec![0xa5; 4];
        data.save_vectors.select_seen_choices = vec![0x1234; 3];
        data.save_vectors.select_last_choices = vec![0xf1, 0xe2, 3, 4];
        data.save_vectors.extra_words_0 = vec![0x55555555; 20];
        let mut slots = Slots::loaded(&data);
        for (i, row) in std::iter::once(&mut slots.auto)
            .chain(slots.manual.iter_mut())
            .enumerate()
        {
            row.valid = true;
            row.data = RuntimeGameData::from(fresh());
            row.data.progress.extra_word = 0xdead0000 | i as u32;
            row.data.progress.entries[0].response_data = vec![i as u8, 0xee];
            row.data.progress.entries[1] = row.data.progress.entries[0].clone();
            row.data.progress.entries[1].scenario_id = 2084;
            row.data.progress.entries[1].response_data = vec![0xf1, i as u8, 0x55];
            row.data.trailing_values = vec![0xffff, 0x1234, i as u16];
            row.data.byte_variables = vec![2, i as u8, 0xff];
        }
        let globals = Globals {
            common: self.common.clone(),
            data,
            storage: Some(store),
            runtime_saves: Some(slots),
            clock: PlayClock::from_saved_seconds(17),
            platform_flag: true,
            last_title_mode: 1,
            audio: HigurashiAudio::new(
                manager.clone(),
                self.server.clone(),
                self.common.scenario.clone(),
            ),
            server: self.server.clone(),
            manager: manager.clone(),
            sounds: Arc::new(Mutex::new(SystemSoundPlayer::new(manager))),
        };
        globals.sounds.lock().bank = Some(self.common.sysse.clone());
        let mut source = RuntimeGameData::from(fresh());
        source.progress.extra_word = 0xdeadbeef;
        source.progress.entries[1] = source.progress.entries[0].clone();
        source.progress.entries[0].scenario_id = 100;
        source.progress.entries[0].response_data = vec![0x23, 0xee];
        source.progress.entries[1].scenario_id = 2084;
        source.progress.entries[1].response_data = vec![0xf1, 0x55];
        source.progress.set_active_index(active).unwrap();
        source.progress.entries[active].scenario_id = scenario;
        source.trailing_values = vec![0xffff, 0x1234];
        source.byte_variables = vec![2; 64]; // Includes52: item7 must still return Kakera49.
        let seeds = Arc::new(AtomicUsize::new(0));
        let seed_provider = seeds.clone();
        let dates = Arc::new(AtomicUsize::new(0));
        let date_provider = dates.clone();
        let date = source.date;
        let mut game = None;
        reference_render_to_texture(&self.device, &self.queue, size, |ctx, _| {
            game = Some(
                Gameplay::new(
                    globals,
                    source,
                    0,
                    ctx.new_render_texture("Menu return source".into()),
                    move || {
                        seed_provider.fetch_add(1, Ordering::SeqCst);
                        [1, 2, 3, 4]
                    },
                )
                .unwrap(),
            );
        });
        game.as_mut().unwrap().set_clock_source(move || {
            date_provider.fetch_add(1, Ordering::SeqCst);
            Ok(date)
        });
        let mut slot = TitleSlot::from_game(game.unwrap());
        let mut frames = Frames {
            number: 0,
            size,
            composed: None,
        };
        frames.step(&self.device, &self.queue, &mut slot, 0., 0);
        let end = Instant::now() + Duration::from_secs(20);
        while !{
            let game = slot.game.as_ref().unwrap();
            game.input_permission
                && game
                    .host()
                    .unwrap()
                    .runner
                    .frame()
                    .message
                    .playback
                    .interested_in_input()
        } {
            frames.step(&self.device, &self.queue, &mut slot, 3., 0);
            assert!(Instant::now() < end, "Source Gameplay did not become ready");
            std::thread::sleep(Duration::from_millis(1));
        }
        let game = slot.game.as_mut().unwrap();
        assert!(matches!(
            game.host().unwrap().runner.pending_command(),
            Some(Cmd::MSGSET { auto_wait: 1, .. })
        ));
        game.bindings
            .progress
            .dispatch_variables(&Cmd::SSET { args: [2, 999] }, Execution::Live, 0)
            .unwrap();
        game.bindings
            .progress
            .dispatch_variables(&Cmd::OpcodeDB { args: [0, 255] }, Execution::Live, 0)
            .unwrap();
        (slot, frames, seeds, dates)
    }
    fn open(&self, frames: &mut Frames, slot: &mut TitleSlot, destination: MenuReturn) {
        let item = match destination {
            MenuReturn::Title => 5,
            MenuReturn::Kakera => 7,
        };
        if slot
            .game
            .as_ref()
            .unwrap()
            .host()
            .unwrap()
            .reference_menu_control()
            .is_none()
        {
            frames.step(&self.device, &self.queue, slot, 0., 1 << 15); // Actual Y request.
            assert!(
                slot.game
                    .as_ref()
                    .unwrap()
                    .host()
                    .unwrap()
                    .reference_menu_control()
                    .is_some(),
                "Source MSGSET must accept actual Y before waiting for the menu"
            );
        }
        let end = Instant::now() + Duration::from_secs(20);
        loop {
            if slot
                .game
                .as_ref()
                .unwrap()
                .host()
                .unwrap()
                .reference_menu_control()
                .is_some_and(|(_, ready)| ready)
            {
                break;
            }
            frames.step(&self.device, &self.queue, slot, 3., 0);
            assert!(Instant::now() < end, "Actual SystemMenu did not open");
        }
        while slot
            .game
            .as_ref()
            .unwrap()
            .host()
            .unwrap()
            .reference_menu_control()
            .unwrap()
            .0
            != item
        {
            frames.buttons(
                &self.device,
                &self.queue,
                slot,
                0.,
                Buttons {
                    repeat: 1 << 6,
                    ..Default::default()
                },
            );
            assert!(Instant::now() < end, "Native menu destination missing");
        }
        frames.step(&self.device, &self.queue, slot, 0., 1 << 24);
        loop {
            if slot
                .game
                .as_ref()
                .unwrap()
                .host()
                .unwrap()
                .reference_menu_return_question()
                == Some((destination, 2, 1))
            {
                break;
            }
            frames.step(&self.device, &self.queue, slot, 3., 0);
            assert!(
                Instant::now() < end,
                "Actual return Question did not load/open"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        // Parent rows stagger their hiding clocks beyond the Question opening.
        // Reach the actual hidden endpoint before testing callback reopening.
        while slot
            .game
            .as_ref()
            .unwrap()
            .host()
            .unwrap()
            .reference_system_menu_rows()
            .unwrap()
            .1
            .iter()
            .any(|(_, slide)| *slide != 0.)
        {
            frames.step(&self.device, &self.queue, slot, 3., 0);
            assert!(
                Instant::now() < end,
                "SystemMenu rows did not finish hiding"
            );
        }
        assert!(
            slot.game
                .as_mut()
                .unwrap()
                .host_mut()
                .unwrap()
                .take_menu_child_requests()
                .is_empty()
        );
    }
    fn yes(&self, frames: &mut Frames, slot: &mut TitleSlot, destination: MenuReturn) {
        frames.buttons(
            &self.device,
            &self.queue,
            slot,
            0.,
            Buttons {
                repeat: 1 << 4,
                ..Default::default()
            },
        );
        assert_eq!(
            slot.game
                .as_ref()
                .unwrap()
                .host()
                .unwrap()
                .reference_menu_return_question(),
            Some((destination, 2, 0))
        );
        frames.step(&self.device, &self.queue, slot, 0., 1 << 24);
        assert!(
            slot.pending.is_none(),
            "Question stage0 must not dispatch a destination"
        );
        let end = Instant::now() + Duration::from_secs(20);
        while slot
            .game
            .as_ref()
            .unwrap()
            .host()
            .unwrap()
            .reference_menu_return_question()
            .is_some_and(|(_, phase, _)| phase >= 0)
        {
            frames.step(&self.device, &self.queue, slot, 1., 0);
            assert!(
                Instant::now() < end,
                "Yes Question failed to finish closing"
            );
        }
    }
}
fn same_rows(current: &Slots, expected: &Slots) {
    for (a, b) in std::iter::once(&current.auto)
        .chain(current.manual.iter())
        .zip(std::iter::once(&expected.auto).chain(expected.manual.iter()))
    {
        assert_eq!(a.valid, b.valid);
        assert_eq!(a.data, b.data, "Full101-row state was projected");
    }
}
fn unchanged_vm(slot: &TitleSlot, data: &RuntimeGameData, commands: u64) {
    let game = slot.game.as_ref().unwrap();
    assert_eq!(&game.runtime_game_data().unwrap(), data);
    assert_eq!(game.host().unwrap().runner.completed_commands(), commands);
    assert!(!game.host().unwrap().runner.exit_closing());
    assert_eq!(game.bindings.progress.pending_persistent(), [(2, 999)]);
    assert_eq!(game.bindings.progress.pending_bytes(), [(0, 255)]);
}

#[test]
#[ignore = "actual SystemMenu5/7 default-No/A and B rejection,early reopening/no VM commits/save writes at720p1080p;offscreen Metal/exact Loopback,no window/user saves"]
fn higurashi_system_menu_title_and_fragment_rejections_reopen_before_question_prune() {
    let fixture = Fixture::new();
    let mut rejections = 0;
    for size in [(1280, 720), (1920, 1080)] {
        for (destination, scenario) in [(MenuReturn::Title, 100), (MenuReturn::Kakera, 2001)] {
            let store = Store::new(false);
            let (mut slot, mut frames, seeds, dates) =
                fixture.game(size, scenario, 0, store.clone());
            let data = slot.game.as_ref().unwrap().runtime_game_data().unwrap();
            let commands = slot
                .game
                .as_ref()
                .unwrap()
                .host()
                .unwrap()
                .runner
                .completed_commands();
            let seed_count = seeds.load(Ordering::SeqCst);
            let date_count = dates.load(Ordering::SeqCst);
            for pressed in [1 << 24, 1 << 25] {
                fixture.open(&mut frames, &mut slot, destination);
                frames.step(&fixture.device, &fixture.queue, &mut slot, 0., pressed);
                let end = Instant::now() + Duration::from_secs(20);
                // A-No first waits for its native button flash. Observe the
                // actual stage0 row reopening, rather than assuming one tick.
                loop {
                    frames.step(&fixture.device, &fixture.queue, &mut slot, 1., 0);
                    let host = slot.game.as_ref().unwrap().host().unwrap();
                    assert!(
                        host.reference_menu_return_question()
                            .is_some_and(|(_, phase, _)| phase >= 0)
                    );
                    if host
                        .reference_system_menu_rows()
                        .unwrap()
                        .1
                        .iter()
                        .any(|(_, slide)| *slide > 0. && *slide < 1.)
                    {
                        assert!(
                            host.reference_menu_return_question()
                                .is_some_and(|(_, phase, _)| phase == 4)
                        );
                        break;
                    }
                    assert!(
                        Instant::now() < end,
                        "Rejected Question did not reopen rows"
                    );
                }
                assert!(slot.pending.is_none());
                let host = slot.game.as_ref().unwrap().host().unwrap();
                assert!(
                    host.reference_menu_return_question()
                        .is_some_and(|(_, phase, _)| phase >= 0)
                );
                assert_eq!(host.reference_system_menu_rows().unwrap().0, 1);
                assert!(
                    host.reference_system_menu_rows()
                        .unwrap()
                        .1
                        .iter()
                        .any(|(_, slide)| *slide < 1.)
                );
                while slot
                    .game
                    .as_ref()
                    .unwrap()
                    .host()
                    .unwrap()
                    .reference_menu_return_question()
                    .is_some()
                {
                    frames.step(&fixture.device, &fixture.queue, &mut slot, 3., 0);
                    assert!(Instant::now() < end);
                }
                unchanged_vm(&slot, &data, commands);
                assert!(slot.pending.is_none());
                assert!(!slot.game.as_ref().unwrap().has_save_task());
                assert_eq!(store.writes.load(Ordering::SeqCst), 0);
                rejections += 1;
            }
            assert_eq!(seeds.load(Ordering::SeqCst), seed_count);
            assert_eq!(dates.load(Ordering::SeqCst), date_count);
        }
    }
    eprintln!(
        "SystemMenu5/7:4 actual owners/8 A-No+B cancellations,defaultNo1,stage0 parent reopening while Question still closes,next-snapshot prune,frozen full VM/pending queues,no save/seed/calendar changes at720p1080p"
    );
    assert_eq!(rejections, 8);
}

#[test]
#[ignore = "actual SystemMenu7 Yes copies complete active0/1 data,12049 stays Kakera49,32700 restore,next-frame source death/capture/all101 rows at720p1080p;offscreen Metal/exact Loopback,no window/user saves"]
fn higurashi_system_menu_fragment_yes_retains_full_scene_and_ignores_exit_special_routes() {
    let fixture = Fixture::new();
    let mut captures = 0;
    for size in [(1280, 720), (1920, 1080)] {
        for (scenario, active, selected) in [(2001, 0, 1), (12049, 1, 49)] {
            let store = Store::new(false);
            let (mut slot, mut frames, seeds, dates) =
                fixture.game(size, scenario, active, store.clone());
            let data = slot.game.as_ref().unwrap().runtime_game_data().unwrap();
            let rows = slot
                .game
                .as_ref()
                .unwrap()
                .globals
                .runtime_saves
                .as_ref()
                .unwrap()
                .clone();
            let commands = slot
                .game
                .as_ref()
                .unwrap()
                .host()
                .unwrap()
                .runner
                .completed_commands();
            let seed_count = seeds.load(Ordering::SeqCst);
            let date_count = dates.load(Ordering::SeqCst);
            fixture.open(&mut frames, &mut slot, MenuReturn::Kakera);
            let transport = &mut slot.game.as_mut().unwrap().host_mut().unwrap().audio;
            transport.browser_music_multiplier(0.5);
            transport.update_global_music(Ticks::from_f32(60.));
            assert_eq!(transport.reference_music_focus(), 0.5);
            fixture.yes(&mut frames, &mut slot, MenuReturn::Kakera);
            unchanged_vm(&slot, &data, commands);
            let Some(Pending::Kakera(request)) = &slot.pending else {
                panic!("Native item7 must queue Kakera")
            };
            assert_eq!(request.mode, selected);
            assert_eq!(request.data, data);
            assert!(slot.game.is_some() && slot.kakera.is_none());
            assert!(slot.game.as_ref().unwrap().pending_exit_request().is_none());
            let image = captured(
                &fixture.device,
                &fixture.queue,
                size,
                frames.composed.as_ref().unwrap(),
            );
            let n = slot.reference_scene_events.len();
            reference_render_to_texture(&fixture.device, &fixture.queue, size, |ctx, _| {
                assert!(
                    slot.dispatch_kakera(
                        ctx,
                        frames.composed.as_ref().unwrap(),
                        (size.0 as u16, size.1 as u16)
                    )
                    .unwrap()
                );
            });
            assert_eq!(&slot.reference_scene_events[n..], [1, 2]);
            assert!(slot.game.is_none());
            assert_eq!(
                slot.kakera
                    .as_ref()
                    .unwrap()
                    .motion
                    .model
                    .selected_fragment(),
                selected as usize
            );
            assert_eq!(
                slot.kakera
                    .as_ref()
                    .unwrap()
                    .globals
                    .runtime_saves
                    .as_ref()
                    .unwrap()
                    .auto
                    .data,
                rows.auto.data
            );
            same_rows(
                slot.kakera
                    .as_ref()
                    .unwrap()
                    .globals
                    .runtime_saves
                    .as_ref()
                    .unwrap(),
                &rows,
            );
            assert_eq!(
                render_only(&fixture.device, &fixture.queue, size, &mut slot),
                image
            );
            captures += 1;
            for _ in 0..20 {
                frames.step(&fixture.device, &fixture.queue, &mut slot, 3., 0);
            }
            assert_eq!(
                slot.kakera
                    .as_ref()
                    .unwrap()
                    .globals
                    .audio
                    .reference_music_focus(),
                1.
            );
            assert_eq!(store.writes.load(Ordering::SeqCst), 0);
            assert_eq!(seeds.load(Ordering::SeqCst), seed_count);
            assert_eq!(dates.load(Ordering::SeqCst), date_count);
        }
    }
    assert_eq!(captures, 4);
    eprintln!(
        "SystemMenu7:4 actual normal/alternate active0/1 Yes callbacks,no EXIT/commit/save,12049 byte52>=2 still Kakera49,restored music1,full two records/four vectors/all101 rows and provider owners,4 exact720p1080p composed captures/source death before factory"
    );
}

#[test]
#[ignore = "actual SystemMenu5 Yes/global flag1 SaveGame/held atomic temporary writer/minimum one second/notice/freshTitle(-1,-1),all101 full rows+source capture720p1080p;exact Loopback,no window/user saves"]
fn higurashi_system_menu_title_yes_saves_full_globals_before_retiring_adv() {
    let fixture = Fixture::new();
    for size in [(1280, 720), (1920, 1080)] {
        let store = Store::new(true);
        let (mut slot, mut frames, seeds, dates) = fixture.game(size, 100, 0, store.clone());
        let release = Release(store.clone()); // Before any input can schedule a held save.
        // Native wire trailing values belong only to a secondary-active row.
        // Other rows still retain their dormant records/tails in memory.
        slot.game
            .as_mut()
            .unwrap()
            .globals
            .runtime_saves
            .as_mut()
            .unwrap()
            .manual[37]
            .data
            .progress
            .set_active_index(1)
            .unwrap();
        let data = slot.game.as_ref().unwrap().runtime_game_data().unwrap();
        let rows = slot
            .game
            .as_ref()
            .unwrap()
            .globals
            .runtime_saves
            .as_ref()
            .unwrap()
            .clone();
        let commands = slot
            .game
            .as_ref()
            .unwrap()
            .host()
            .unwrap()
            .runner
            .completed_commands();
        let seed_count = seeds.load(Ordering::SeqCst);
        let date_count = dates.load(Ordering::SeqCst);
        fixture.open(&mut frames, &mut slot, MenuReturn::Title);
        fixture.yes(&mut frames, &mut slot, MenuReturn::Title);
        let save_wait_began = Instant::now();
        assert!(slot.pending.is_none() && slot.game.as_ref().unwrap().has_save_task());
        unchanged_vm(&slot, &data, commands);
        let end = Instant::now() + Duration::from_secs(20);
        while store.bytes.lock().is_empty() {
            frames.step(&fixture.device, &fixture.queue, &mut slot, 3., 0);
            assert!(slot.pending.is_none());
            assert!(Instant::now() < end, "Actual Title save did not begin");
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(store.writes.load(Ordering::SeqCst), 1);
        assert!(!store.path.current.as_ref().unwrap().exists());
        let bytes = store.bytes.lock().clone();
        let frozen = SaveData::decode(&bytes).unwrap();
        assert_eq!(
            frozen
                .manual_save_slots
                .iter()
                .filter(|r| r.is_some())
                .count(),
            100
        );
        assert!(frozen.auto_save_slot.is_some());
        assert_eq!(
            frozen.persist_data.get(2),
            3,
            "Menu Title must not commit pending999"
        );
        assert_eq!(
            frozen.manual_save_slots[37]
                .as_ref()
                .unwrap()
                .trailing_values,
            vec![1023, 0x234, 38]
        );
        assert_eq!(frozen.manual_save_slots[37].as_ref().unwrap().extra_word, 6);
        assert!(
            frozen.manual_save_slots[38]
                .as_ref()
                .unwrap()
                .trailing_values
                .is_empty()
        );
        // The producer already owns the complete frozen snapshot; later source
        // settings changes must neither alter it nor queue Title while held.
        slot.game.as_mut().unwrap().globals.data.extra_word = 0x4321;
        for _ in 0..5 {
            frames.step(
                &fixture.device,
                &fixture.queue,
                &mut slot,
                3.,
                (1 << 24) | (1 << 25),
            );
            assert!(slot.pending.is_none());
            unchanged_vm(&slot, &data, commands);
        }
        assert_eq!(*store.bytes.lock(), bytes);
        store.release();
        let end = Instant::now() + Duration::from_secs(20);
        loop {
            frames.step(&fixture.device, &fixture.queue, &mut slot, 3., 0);
            if slot.pending_title_return() {
                break;
            }
            assert!(
                Instant::now() < end,
                "Native SaveGame notice did not queue Title"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(
            save_wait_began.elapsed() >= Duration::from_secs(1),
            "SaveGame minimum wall-clock wait was skipped"
        );
        let Some(Pending::Title(request)) = &slot.pending else {
            unreachable!()
        };
        assert_eq!((request.selected, request.mode), (-1, u32::MAX));
        unchanged_vm(&slot, &data, commands);
        assert_eq!(
            std::fs::read(store.path.current.as_ref().unwrap()).unwrap(),
            bytes
        );
        same_rows(
            slot.game
                .as_ref()
                .unwrap()
                .globals
                .runtime_saves
                .as_ref()
                .unwrap(),
            &rows,
        );
        let image = captured(
            &fixture.device,
            &fixture.queue,
            size,
            frames.composed.as_ref().unwrap(),
        );
        let n = slot.reference_scene_events.len();
        reference_render_to_texture(&fixture.device, &fixture.queue, size, |ctx, _| {
            assert!(
                slot.dispatch_title_return(ctx, frames.composed.as_ref().unwrap())
                    .unwrap()
            );
        });
        assert_eq!(&slot.reference_scene_events[n..], [1, 2]);
        assert!(slot.game.is_none());
        assert_eq!(
            slot.title.as_ref().unwrap().reference_initial_selection(),
            (-1, 1)
        );
        same_rows(slot.globals().unwrap().runtime_saves.unwrap(), &rows);
        assert_eq!(
            render_only(&fixture.device, &fixture.queue, size, &mut slot),
            image
        );
        assert_eq!(seeds.load(Ordering::SeqCst), seed_count);
        assert_eq!(dates.load(Ordering::SeqCst), date_count);
        assert_eq!(store.writes.load(Ordering::SeqCst), 1);
        drop(release);
    }
    eprintln!(
        "SystemMenu5:2 actual stage1 Yes/global flag1 saves,held frozen101-row atomic temp writers/uncommitted VM and no early Title,one-second notification/final Title(-1,-1),2 exact720p1080p capture/source-before-factory transfers,full runtime rows and original seed/calendar owners"
    );
}

struct Frames {
    number: u32,
    size: (u32, u32),
    composed: Option<RenderTexture>,
}
impl Frames {
    fn step(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        slot: &mut TitleSlot,
        elapsed: f32,
        pressed: u32,
    ) -> Vec<u8> {
        self.buttons(
            device,
            queue,
            slot,
            elapsed,
            Buttons {
                pressed,
                ..Buttons::default()
            },
        )
    }
    fn buttons(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        slot: &mut TitleSlot,
        elapsed: f32,
        buttons: Buttons,
    ) -> Vec<u8> {
        self.number += 1;
        let mut composed = None;
        let image = reference_render_to_texture(device, queue, self.size, |context, target| {
            if let Some(previous) = &self.composed {
                shin_tasks::block_on(slot.update_frame(
                    context,
                    previous,
                    (self.size.0 as u16, self.size.1 as u16),
                    Ticks::from_f32(elapsed),
                    true,
                    buttons,
                    &PointerFrame {
                        number: self.number,
                        time_seconds: self.number as f64 / 60.0,
                        events: &[],
                    },
                    policy(),
                    audio(),
                ))
                .unwrap();
            }
            slot.prepare(context).unwrap();
            let mut frame = context.new_render_texture("SystemMenu composed source".into());
            for output in [
                frame.as_texture_target(),
                shin_render::shaders::types::texture::TextureTarget {
                    view: target,
                    kind: shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
                },
            ] {
                let mut pass = context.begin_pass(
                    output,
                    Some(context.depth_stencil),
                    "SystemMenu scene frame",
                );
                pass.clear(
                    Some(UnormColor::from_rgba(31, 63, 127, 255)),
                    Some(0),
                    Some(1.0),
                );
                slot.render(&mut pass).unwrap();
            }
            composed = Some(frame);
        });
        self.composed = composed;
        image
    }
}
fn captured(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    size: (u32, u32),
    source: &RenderTexture,
) -> Vec<u8> {
    reference_render_to_texture(device, queue, size, |context, target| {
        let overlay = crate::adv::higurashi::runner::reconstruction_mode::Overlay {
            snapshot: source.copy_snapshot(&mut context.render_clone_ctx()),
            mask: None,
            clock: crate::adv::higurashi::reconstruction_mode::OverlayClock::captured(true),
        };
        let mut pass = context.begin_pass(
            shin_render::shaders::types::texture::TextureTarget {
                view: target,
                kind: shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
            },
            Some(context.depth_stencil),
            "SystemMenu source copy",
        );
        pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
        overlay.render(&mut pass);
    })
}

fn render_only(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    size: (u32, u32),
    slot: &mut TitleSlot,
) -> Vec<u8> {
    reference_render_to_texture(device, queue, size, |ctx, target| {
        slot.prepare(ctx).unwrap();
        let mut pass = ctx.begin_pass(
            shin_render::shaders::types::texture::TextureTarget {
                view: target,
                kind: shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
            },
            Some(ctx.depth_stencil),
            "Fresh return factory capture",
        );
        pass.clear(
            Some(UnormColor::from_rgba(31, 63, 127, 255)),
            Some(0),
            Some(1.),
        );
        slot.render(&mut pass).unwrap();
    })
}
