use super::*;
use crate::adv::higurashi::{
    audio::HigurashiAudio, reference_gpu_assets, reference_render_to_texture,
};
use crate::app::higurashi::gameplay::tests::{audio, fresh, policy};
use crate::app::higurashi::{
    load_game::{Paths, Source, Storage},
    runtime_game_data::RuntimeGameData,
    runtime_saves::Slots,
};
use shin_core::{
    format::save::higurashi::{GameDataEntry, SaveDate},
    primitives::color::UnormColor,
};
use std::{
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    time::{Duration, Instant},
};

struct Store {
    path: Paths,
    entered: AtomicBool,
    writes: AtomicUsize,
    gate: (std::sync::Mutex<bool>, std::sync::Condvar),
    bytes: std::sync::Mutex<Vec<u8>>,
}
struct Release(Arc<Store>);
impl Drop for Release {
    fn drop(&mut self) {
        *self.0.gate.0.lock().unwrap() = true;
        self.0.gate.1.notify_all();
    }
}
impl Storage for Store {
    fn trial_token(&self) -> bool {
        false
    }
    fn read(&self, source: Source) -> Result<Option<Vec<u8>>> {
        self.path.read(source)
    }
    fn trial_exists(&self) -> Result<bool> {
        Ok(false)
    }
    fn write(&self, bytes: &[u8]) -> Result<()> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        *self.bytes.lock().unwrap() = bytes.to_vec();
        self.entered.store(true, Ordering::SeqCst);
        let (gate, ready) = &self.gate;
        let mut open = gate.lock().unwrap();
        while !*open {
            open = ready.wait(open).unwrap();
        }
        drop(open);
        self.path.write(bytes)
    }
}
fn update(slot: &mut TitleSlot, number: &mut u32, buttons: Buttons) -> Status {
    *number += 1;
    shin_tasks::block_on(slot.update_game(
        Ticks::from_f32(3.0),
        true,
        buttons,
        &PointerFrame {
            number: *number,
            time_seconds: *number as f64 / 60.0,
            events: &[],
        },
        policy(),
        audio(),
    ))
    .unwrap()
}
#[test]
#[ignore = "actual EXIT/SaveGame no-answer dialog/held atomic writer/one-second producer/final title(-1,-1) callback and next-frame retirement on offscreen Metal/the default audio output;only temporary files"]
fn higurashi_exit_saves_frozen_full_slots_then_returns_to_real_title() {
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    let code = [0x83, 1, 0x83, 0xe8, 0x82, 18, 2, 0, 0, 1, 0x83, 0, 1];
    let (scenario, common) =
        crate::app::higurashi::gameplay::tests::character::common(&server, &code);
    let mut captures = 0;
    for extent in [(1280, 720), (1920, 1080)] {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "higurashi-save-title-{}-{unique}",
            std::process::id()
        ));
        let path = dir.join("progress.dat");
        let store = Arc::new(Store {
            path: Paths {
                current: Some(path.clone()),
                trial: None,
            },
            entered: AtomicBool::new(false),
            writes: AtomicUsize::new(0),
            gate: (std::sync::Mutex::new(false), std::sync::Condvar::new()),
            bytes: std::sync::Mutex::new(Vec::new()),
        });
        let mut saved = SaveData::default();
        saved.persist_data.0 = vec![1; 192];
        saved.save_menu_position = 7;
        let mut legacy = fresh();
        legacy.date = SaveDate {
            year: 2026,
            month: 10,
            day: 12,
            hour: 18,
            minute: 19,
            second: 20,
        };
        legacy.entry = GameDataEntry {
            scenario_id: 0,
            random_seed: 42,
            save_position: 7,
            replay_target: 7,
            response_data: vec![0x23, 0xee],
        };
        legacy.extra_word = 7;
        legacy.trailing_values = vec![7, 1023];
        legacy.byte_variables = vec![2, 255];
        saved.auto_save_slot = Some(legacy.clone());
        saved.manual_save_slots[2] = Some(legacy.clone());
        let mut slots = Slots::loaded(&saved);
        slots.auto.data.progress.extra_word = 0xdeadbeef;
        slots.auto.data.progress.entries[1] = crate::adv::higurashi::autosave::ProgressEntry {
            scenario_id: 2084,
            random_seed: 0x87654321,
            save_position: 17,
            replay_target: 19,
            response_data: vec![0xf1, 0x55],
        };
        slots.manual[2].data.progress.extra_word = 0xabcd0005;
        slots.manual[2].data.progress.entries[1] = slots.auto.data.progress.entries[1].clone();
        slots.manual[2].data.trailing_values = vec![0xffff, 0x1234];
        let full_auto = slots.auto.data.clone();
        let full_manual = slots.manual[2].data.clone();
        let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
        let globals = Globals {
            common: common.clone(),
            data: saved,
            storage: Some(store.clone()),
            runtime_saves: Some(slots),
            clock: PlayClock::from_saved_seconds(17),
            platform_flag: true,
            last_title_mode: 3,
            audio: HigurashiAudio::new(manager.clone(), server.clone(), scenario.clone()),
            server: server.clone(),
            manager: manager.clone(),
            sounds: Arc::new(Mutex::new(SystemSoundPlayer::new(manager))),
        };
        let mut game = None;
        reference_render_to_texture(&device, &queue, extent, |context, _| {
            let mut frame = context.new_render_texture("Save source".into());
            let mut pass = context.begin_pass(
                frame.as_texture_target(),
                Some(context.depth_stencil),
                "Save source",
            );
            pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
            drop(pass);
            game = Some(
                Gameplay::new(globals, RuntimeGameData::from(fresh()), 0, frame, || {
                    [1, 2, 3, 4]
                })
                .unwrap(),
            );
        });
        let mut slot = TitleSlot::from_game(game.unwrap());
        let _release = Release(store.clone());
        let mut number = 0;
        let deadline = Instant::now() + Duration::from_secs(10);
        while !matches!(
            slot.game
                .as_ref()
                .unwrap()
                .host()
                .unwrap()
                .runner
                .pending_command(),
            Some(
                shin_core::vm::higurashi::command::HigurashiRuntimeCommand::WAIT {
                    amount: 1000,
                    ..
                }
            )
        ) {
            update(&mut slot, &mut number, Buttons::default());
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        slot.game
            .as_mut()
            .unwrap()
            .bindings
            .history
            .record_completion(true, 7);
        let history = slot
            .game
            .as_ref()
            .unwrap()
            .bindings
            .history
            .bits()
            .words()
            .as_ptr();
        let auto = slot
            .game
            .as_ref()
            .unwrap()
            .bindings
            .autosave
            .progress
            .entries[0]
            .response_data
            .as_ptr();
        assert!(
            slot.game
                .as_mut()
                .unwrap()
                .host_mut()
                .unwrap()
                .runner
                .interrupt_wait()
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        while !store.entered.load(Ordering::SeqCst) {
            update(&mut slot, &mut number, Buttons::default());
            assert!(!slot.pending_title_return());
            reference_render_to_texture(&device, &queue, extent, |context, target| {
                slot.prepare(context).unwrap();
                let mut pass = context.begin_pass(
                    shin_render::shaders::types::texture::TextureTarget {
                        kind:
                            shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
                        view: target,
                    },
                    Some(context.depth_stencil),
                    "Saving dialog frame",
                );
                pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
                slot.render(&mut pass).unwrap();
            });
            assert!(Instant::now() < deadline, "save producer did not start");
            std::thread::sleep(Duration::from_millis(2));
        }
        let began = Instant::now();
        assert_eq!(store.writes.load(Ordering::SeqCst), 1);
        assert!(!path.exists());
        let frozen = SaveData::decode(&store.bytes.lock().unwrap()).unwrap();
        assert_eq!(frozen.persist_data.get(18), 2);
        assert_eq!(frozen.save_menu_position, 7);
        assert_ne!(frozen.save_vectors.seen_messages[0] & 128, 0);
        assert_eq!(frozen.auto_save_slot.as_ref().unwrap().extra_word, 7);
        assert_eq!(frozen.manual_save_slots[2].as_ref().unwrap().extra_word, 5);
        // A and B cannot answer a notification with no buttons. The snapshot
        // already exists;later memory/time changes must not alter its bytes.
        slot.game.as_mut().unwrap().globals.data.extra_word = 0x12345678;
        for _ in 0..10 {
            update(
                &mut slot,
                &mut number,
                Buttons {
                    pressed: (1 << 24) | (1 << 25),
                    ..Default::default()
                },
            );
            assert!(!slot.pending_title_return());
        }
        assert_eq!(
            slot.game
                .as_ref()
                .unwrap()
                .host()
                .unwrap()
                .modal_pause_depth(),
            0
        );
        assert!(slot.game.as_ref().unwrap().host().unwrap().has_modes());
        *store.gate.0.lock().unwrap() = true;
        store.gate.1.notify_all();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !slot.pending_title_return() {
            update(&mut slot, &mut number, Buttons::default());
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(
            began.elapsed() >= Duration::from_millis(900),
            "native minimum one-second producer was bypassed"
        );
        assert_eq!(std::fs::read(&path).unwrap(), *store.bytes.lock().unwrap());
        assert_ne!(frozen.extra_word, 0x12345678);
        let Some(Pending::Title(request)) = &slot.pending else {
            panic!("wrong save destination")
        };
        assert_eq!((request.selected, request.mode), (-1, u32::MAX));
        assert!(
            slot.game.as_ref().unwrap().host().unwrap().has_modes(),
            "completed save mode retires with source,next platform frame"
        );
        slot.reference_scene_events.clear();
        reference_render_to_texture(&device, &queue, extent, |context, _| {
            let mut frame = context.new_render_texture("Saved composed frame".into());
            let mut pass = context.begin_pass(
                frame.as_texture_target(),
                Some(context.depth_stencil),
                "Saved composed",
            );
            pass.clear(Some(UnormColor::WHITE), Some(0), Some(1.0));
            drop(pass);
            assert!(slot.dispatch_title_return(context, &frame).unwrap());
            captures += 1;
        });
        assert!(slot.game.is_none());
        assert!(slot.kakera.is_none());
        assert_eq!(slot.reference_scene_events, [1, 2]);
        let title = slot.title.as_ref().unwrap();
        assert_eq!(title.globals.last_title_mode, 3);
        assert_eq!(title.globals.data.extra_word, 0x12345678);
        let slots = title.globals.runtime_saves.as_ref().unwrap();
        assert_eq!(slots.auto.data, full_auto);
        assert_eq!(slots.manual[2].data, full_manual);
        let continuation = slot.continuation.as_ref().unwrap();
        assert_eq!(continuation.history.bits().words().as_ptr(), history);
        assert_eq!(
            continuation.autosave.progress.entries[0]
                .response_data
                .as_ptr(),
            auto
        );
        // The concrete title remains owned on rejected story preflight. A
        // following valid factory transfers the original global continuations.
        slot.pending = Some(Pending::Story(fresh(), 0));
        let original = slot.title.as_ref().unwrap().globals.common.clone();
        let mut wrong = shin_core::format::scenario::higurashi::HigurashiScenario::new(
            original.scenario.raw().to_vec().into(),
        )
        .unwrap();
        wrong.variant = shin_core::format::scenario::higurashi::HigurashiVariant::Original;
        slot.title.as_mut().unwrap().globals.common = Arc::new(super::super::CommonAssets {
            scenario: Arc::new(wrong),
            font: original.font.clone(),
            system: original.system.clone(),
            message: original.message.clone(),
            effects: original.effects.clone(),
            sysse: original.sysse.clone(),
        });
        reference_render_to_texture(&device, &queue, extent, |context, _| {
            let frame = context.new_render_texture("Rejected saved title story".into());
            assert!(
                slot.dispatch_story(context, &frame, || panic!("rejected source consumed seeds"))
                    .is_err()
            );
        });
        assert!(slot.title.is_some());
        assert!(slot.pending_story());
        assert!(slot.continuation.is_some());
        slot.title.as_mut().unwrap().globals.common = original;
        reference_render_to_texture(&device, &queue, extent, |context, _| {
            let mut frame = context.new_render_texture("Saved title story".into());
            let mut pass = context.begin_pass(
                frame.as_texture_target(),
                Some(context.depth_stencil),
                "Saved title story",
            );
            pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
            drop(pass);
            assert!(
                slot.dispatch_story(context, &frame, || panic!(
                    "saved seed provider was replaced"
                ))
                .unwrap()
            );
        });
        let game = slot.game.as_ref().unwrap();
        assert!(slot.title.is_none());
        assert!(slot.continuation.is_none());
        assert_eq!(game.bindings.history.bits().words().as_ptr(), history);
        assert_eq!(
            game.bindings.autosave.progress.entries[0]
                .response_data
                .as_ptr(),
            auto
        );
        assert_eq!(
            game.globals.runtime_saves.as_ref().unwrap().auto.data,
            full_auto
        );
        assert_eq!(
            game.globals.runtime_saves.as_ref().unwrap().manual[2].data,
            full_manual
        );
        drop(slot);
        std::fs::remove_dir_all(dir).unwrap();
    }
    println!(
        "EXIT SaveGame:2 real720p/1080p source-to-title captures;native no-answer dialog/held single producer/minimum one second/frozen committed settings/full runtime101 slots/3-bit-10-bit disk projection/atomic write/final callback(-1,-1)/source-before-title retirement/original history-autosave owners through fresh story/rejected preflight pass"
    );
}
