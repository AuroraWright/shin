use super::*;
use crate::adv::{
    higurashi::{audio::HigurashiAudio, reference_gpu_assets, reference_render_to_texture},
    input_area::{PointerEvent, PointerKind},
};
use shin_core::{format::scenario::higurashi::HigurashiScenario, primitives::color::UnormColor};
use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
use std::sync::atomic::{AtomicUsize, Ordering};

pub(super) struct Frames {
    pub(super) number: u32,
}
impl Frames {
    pub(super) fn step(&mut self, game: &mut Gameplay, buttons: Buttons, events: &[PointerEvent]) {
        self.number += 1;
        shin_tasks::block_on(game.update(
            Ticks::from_f32(3.0),
            true,
            buttons,
            &PointerFrame {
                number: self.number,
                time_seconds: self.number as f64 / 60.0,
                events,
            },
            policy(),
            audio(),
        ))
        .unwrap();
    }
    pub(super) fn idle(&mut self, game: &mut Gameplay) {
        self.step(game, Buttons::default(), &[]);
    }
}
#[test]
#[ignore = "actual Gameplay SELECT/filter/controller/contact/hide/autosave/script feedback on offscreen Metal/the default audio output"]
fn higurashi_gameplay_selection_returns_original_index_and_retires_overlay() {
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    let authored: Arc<HigurashiScenario> = server.load_sync("/main.snr").unwrap();
    // Immediate message, a filtered three-option choice, then script feedback
    // queued into persistent slot9 by the following SSET command.
    let code = [
        0x86, 1, 0, 0, 0, 0x7f, 4, 0, b'@', b'r', b'A', 0, 0x8d, 1, 0, 1, 0, 31, 0, 5, 1, 0, 7,
        b'A', 0, b'B', 0, b'C', 0, 0, 0x82, 9, 0xc0, 31, 0x83, 0, 0x83, 0xe8, 0, 0, 0,
    ];
    let mut raw = authored.raw().to_vec();
    let start = authored.header.code_offset as usize;
    raw[start..start + code.len()].copy_from_slice(&code);
    let scenario = Arc::new(HigurashiScenario::new(raw.into()).unwrap());
    let common = Arc::new(CommonAssets {
        scenario,
        font: server.load_sync("/newrodin.fnt").unwrap(),
        system: server.load_sync("/systex.txa").unwrap(),
        message: server.load_sync("/msgtex.txa").unwrap(),
        effects: server.load_sync("/effects.txa").unwrap(),
        sysse: server.load_sync("/sysse.bin").unwrap(),
    });
    for (pointer, size, reconstruct) in [
        (false, (1280, 720), false),
        (true, (1920, 1080), false),
        (false, (1280, 720), true),
    ] {
        let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
        let mut saved = SaveData::default();
        saved.settings.common.v14_showroutenavi = false;
        let mut slot = fresh();
        slot.trailing_values = vec![7, 0xffff];
        slot.byte_variables = vec![1, 255];
        saved.auto_save_slot = Some(slot.clone());
        saved.manual_save_slots[2] = Some(slot.clone());
        let globals = Globals {
            storage: None,
            runtime_saves: None,
            common: common.clone(),
            data: saved,
            clock: PlayClock::from_saved_seconds(0),
            platform_flag: true,
            last_title_mode: 0,
            audio: HigurashiAudio::new(manager.clone(), server.clone(), authored.clone()),
            server: server.clone(),
            manager: manager.clone(),
            sounds: Arc::new(Mutex::new(SystemSoundPlayer::new(manager))),
        };
        let mut game = None;
        reference_render_to_texture(&device, &queue, size, |context, _| {
            let mut snapshot = context.new_render_texture("Choice initial".into());
            {
                let mut pass = context.begin_pass(
                    snapshot.as_texture_target(),
                    Some(context.depth_stencil),
                    "Choice initial",
                );
                pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
            }
            let mut data = fresh();
            data.entry.random_seed = 42;
            game = Some(Gameplay::new(globals, data, 0, snapshot, || [1, 2, 3, 4]).unwrap());
        });
        let mut game = game.unwrap();
        let mut frames = Frames { number: 0 };
        let date = SaveDate {
            year: 2026,
            month: 10,
            day: 12,
            hour: 13,
            minute: 12,
            second: 34,
        };
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        game.set_clock_source(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(date)
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            frames.idle(&mut game);
            if game
                .selection
                .ui
                .as_ref()
                .is_some_and(|ui| ui.lock().model.phase == 2)
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "SELECT did not open: {:?}",
                game.host().unwrap().runner.pending_command()
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let ui = game.selection.ui.as_ref().unwrap().clone();
        {
            let ui = ui.lock();
            assert_eq!(
                ui.model
                    .rows
                    .iter()
                    .map(|r| r.spec.original)
                    .collect::<Vec<_>>(),
                [0, 2]
            );
            assert!(!ui.route_navigation);
            assert_eq!(ui.properties.values()[5], 1.0);
        }
        let initial = game.globals.data.auto_save_slot.clone().unwrap();
        assert_eq!(initial.entry.save_position, 1);
        assert_eq!(initial.entry.replay_target, 1);
        assert_eq!(initial.date, date);
        // B hides the windows without answering the pending SELECT. The return
        // frame consumes A, and the overlay resumes after its opacity recovers.
        frames.step(
            &mut game,
            Buttons {
                pressed: 1 << 25,
                ..Default::default()
            },
            &[],
        );
        assert!(game.host().unwrap().has_modes());
        assert!(ui.lock().model.hidden);
        assert_eq!(ui.lock().model.phase, 2);
        for _ in 0..4 {
            frames.idle(&mut game);
        }
        frames.step(
            &mut game,
            Buttons {
                pressed: 1 << 24,
                ..Default::default()
            },
            &[],
        );
        for _ in 0..5 {
            frames.idle(&mut game);
        }
        assert!(!ui.lock().model.hidden);
        assert_eq!(ui.lock().model.phase, 2);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let pixels = reference_render_to_texture(&device, &queue, size, |context, target| {
            game.prepare(context).unwrap();
            let mut pass = context.begin_pass(
                TextureTarget {
                    view: target,
                    kind: TextureTargetKind::RenderTexture,
                },
                Some(context.depth_stencil),
                "Choice gameplay",
            );
            game.render(&mut pass).unwrap();
        });
        assert!(
            pixels
                .chunks_exact(4)
                .any(|p| p[0] != 0 || p[1] != 0 || p[2] != 0)
        );
        if reconstruct {
            let fork = game
                .bindings
                .memory
                .fork_for_restore(0, 0, &game.bindings.progress)
                .unwrap();
            let mut replay =
                Some(ReplaySession::new(game.globals.common.scenario.clone(), fork).unwrap());
            let history = game.bindings.history.clone();
            let mut screens = game.screens.take().unwrap();
            reference_render_to_texture(&device, &queue, size, |context, _| {
                game.host_mut()
                    .unwrap()
                    .begin_scene_reconstruction(
                        &mut replay,
                        0,
                        context,
                        &mut screens,
                        history,
                        || [1, 2, 3, 4],
                    )
                    .unwrap();
            });
            game.screens = Some(screens);
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                frames.idle(&mut game);
                if !game.host().unwrap().runner.native_vm_attached() {
                    break;
                }
                assert!(Instant::now() < deadline, "Choice source was not retired");
                std::thread::sleep(Duration::from_millis(1));
            }
            assert!(game.selection.ui.is_none());
            assert_eq!(
                game.host().unwrap().reference_touch_overlay_identity(),
                None
            );
            assert_eq!(Arc::strong_count(&ui), 1);
            assert_eq!(ui.lock().model.result, None);
            assert_eq!(calls.load(Ordering::SeqCst), 2);
            continue;
        }
        if pointer {
            let row_y = {
                let ui = ui.lock();
                let row = &ui.model.rows[1];
                (row.y + row.spec.height * 0.5) / 1.5
            };
            let down = PointerEvent {
                kind: PointerKind::Down,
                id: 17,
                position: glam::ivec2(640, row_y as i32),
            };
            frames.step(&mut game, Buttons::default(), &[down]);
            // Native contact press fires after150ms; a tap can release sooner.
            assert_eq!(ui.lock().model.selected, -1);
            for _ in 0..10 {
                frames.idle(&mut game);
            }
            assert_eq!(ui.lock().model.selected, 1);
            let up = PointerEvent {
                kind: PointerKind::Up,
                id: 17,
                position: glam::ivec2(0, 0),
            };
            frames.step(&mut game, Buttons::default(), &[up]);
        } else {
            frames.step(
                &mut game,
                Buttons {
                    repeat: 1 << 6,
                    ..Default::default()
                },
                &[],
            );
            frames.step(
                &mut game,
                Buttons {
                    repeat: 1 << 6,
                    ..Default::default()
                },
                &[],
            );
            assert_eq!(ui.lock().model.selected, 1);
            frames.step(
                &mut game,
                Buttons {
                    pressed: 1 << 24,
                    ..Default::default()
                },
                &[],
            );
        }
        assert_eq!(ui.lock().model.phase, 3);
        for _ in 0..45 {
            frames.idle(&mut game);
        }
        assert_eq!(ui.lock().model.result, Some(2));
        assert!(game.selection.ui.is_none());
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert_eq!(game.bindings.progress.pending_persistent(), [(9, 2)]);
        assert_eq!(game.bindings.progress.responses().1, 4);
        assert_eq!(game.bindings.progress.responses().0[0] >> 4, 2);
        assert!(game.bindings.choices.seen().contains(2));
        assert_eq!(game.bindings.choices.last()[0], 3);
        assert_eq!(
            game.host()
                .unwrap()
                .runner
                .native_message_timeline()
                .unwrap()
                .boundary,
            2
        );
        let final_slot = game.globals.data.auto_save_slot.as_ref().unwrap();
        assert_eq!(final_slot.entry.save_position, 2);
        assert_eq!(final_slot.entry.replay_target, 2);
        assert_eq!(final_slot.entry.response_data[0] >> 4, 2);
        assert_eq!(final_slot.trailing_values, slot.trailing_values);
        assert_eq!(final_slot.byte_variables, slot.byte_variables);
        assert_eq!(game.globals.data.manual_save_slots[2], Some(slot));
        // The completed SELECT has released its actual root overlay slot.
        assert_eq!(
            game.host().unwrap().reference_touch_overlay_identity(),
            None
        );
    }
    eprintln!(
        "Gameplay choices:720p/controller and1080p/contact;filtered original index2,window hide/return,initial/final11780 copies,4-bit responses,seen/last and script feedback pass"
    );
}
