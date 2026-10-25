use super::selection::Frames;
use super::*;
use crate::adv::higurashi::{
    audio::HigurashiAudio, reference_gpu_assets, reference_render_to_texture,
};
use shin_core::primitives::color::UnormColor;
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
#[ignore = "actual Gameplay QUIZ copy/owned three-argument factory request and replay-stop live skip with real startup/resources on offscreen Metal/the default audio output;quiz scene remains pending,no window/saves"]
fn higurashi_gameplay_quiz_queues_owned_current_game_data_and_restores_score_at_saved_boundary() {
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    let code = &[
        0x82, 7, 1, 0x86, 1, 0, 0, 0, 0x7f, 4, 0, b'@', b'r', b'A', 0, 0xd7, 31, 0, 2, 3, 4, 0x82,
        9, 2, 0x83, 0, 0x83, 0xe8, 0, 0, 0,
    ];
    let (authored, common) = super::character::common(&server, code);
    for (expanded, saved_boundary) in [(false, false), (true, false), (true, true)] {
        let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
        let mut saved = SaveData::default();
        let mut legacy = fresh();
        legacy.trailing_values = vec![0x1234, 0xffff];
        legacy.byte_variables = vec![9, 8];
        saved.auto_save_slot = Some(legacy.clone());
        saved.manual_save_slots[2] = Some(legacy.clone());
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
        let old_date = SaveDate {
            year: 2025,
            month: 1,
            day: 2,
            hour: 3,
            minute: 4,
            second: 5,
        };
        let now = SaveDate {
            year: 2026,
            month: 10,
            day: 12,
            hour: 20,
            minute: 21,
            second: 22,
        };
        let mut data = fresh();
        data.date = old_date;
        data.extra_word = 0x12;
        data.entry.random_seed = 42;
        data.entry.response_data = vec![0x23, 0xee];
        data.trailing_values = vec![7, 0xffff];
        data.byte_variables = vec![2, 255];
        if expanded {
            let mut first = data.entry.clone();
            first.scenario_id = 1000;
            first.random_seed = 17;
            first.save_position = 77;
            first.replay_target = 77;
            first.response_data = vec![0xaa];
            data.primary_entry = Some(first);
        }
        if saved_boundary {
            data.entry.save_position = 2;
            data.entry.replay_target = 2;
        }
        let mut runtime =
            crate::app::higurashi::runtime_game_data::RuntimeGameData::from(data.clone());
        if !expanded {
            runtime.progress.entries[1] = crate::adv::higurashi::autosave::ProgressEntry {
                scenario_id: 2084,
                random_seed: 0x87654321,
                save_position: 19,
                replay_target: 23,
                response_data: vec![0xf1, 0x55, 0x23, 0xee],
            };
            runtime.progress.extra_word = 0xdeadbe12;
        }
        let dormant = runtime.progress.entries[1].clone();
        let mut game = None;
        reference_render_to_texture(&device, &queue, (1280, 720), |context, _| {
            let mut snapshot = context.new_render_texture("Quiz source".into());
            {
                let mut pass = context.begin_pass(
                    snapshot.as_texture_target(),
                    Some(context.depth_stencil),
                    "Quiz source",
                );
                pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
            }
            game = Some(
                Gameplay::new(
                    globals,
                    runtime.clone(),
                    if saved_boundary { 13 } else { 7 },
                    snapshot,
                    || [1, 2, 3, 4],
                )
                .unwrap(),
            );
        });
        let mut game = game.unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        let counter = count.clone();
        game.set_clock_source(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(now)
        });
        let mut frames = Frames { number: 0 };
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            frames.idle(&mut game);
            if game.kakera_queued
                || game
                    .host()
                    .unwrap()
                    .runner
                    .pending_command()
                    .is_some_and(|c| {
                        matches!(
                            c,
                            shin_core::vm::higurashi::command::HigurashiRuntimeCommand::WAIT { .. }
                        )
                    })
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "QUIZ did not reach its scene handoff"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(game.selection.ui.is_none());
        assert!(game.selection.character.job.is_none());
        assert!(game.reference_overlay_cues.is_empty());
        assert_eq!(
            game.bindings.progress.responses().1,
            if saved_boundary { 4 } else { 0 }
        );
        assert_eq!(
            game.bindings.progress.responses().0,
            if saved_boundary {
                [0xd0, 0xee]
            } else {
                [0x23, 0xee]
            }
        );
        assert_eq!(game.globals.data.manual_save_slots[2], Some(legacy.clone()));
        if saved_boundary {
            assert!(!game.kakera_queued);
            assert!(game.take_quiz_scene_request().is_none());
            assert_eq!(game.host().unwrap().runner.reference_register(shin_core::format::scenario::instruction_elements::Register::from_regular_register(31)),Some(13));
            assert_eq!(count.load(Ordering::SeqCst), 0);
            assert_eq!(game.bindings.progress.pending_persistent(), [(9, 2)]);
            assert_eq!(game.globals.data.auto_save_slot, Some(legacy));
            continue;
        }
        assert!(game.kakera_queued);
        assert!(matches!(
            game.host().unwrap().runner.pending_command(),
            Some(shin_core::vm::higurashi::command::HigurashiRuntimeCommand::QUIZ { .. })
        ));
        assert_eq!(count.load(Ordering::SeqCst), 2);
        assert!(game.bindings.progress.pending_persistent().is_empty());
        assert_eq!(game.bindings.progress.persistent().get(7), 1);
        assert_eq!(
            game.host()
                .unwrap()
                .runner
                .native_message_timeline()
                .unwrap()
                .boundary,
            2
        );
        assert!(
            game.host()
                .unwrap()
                .runner
                .native_playback_descriptors()
                .unwrap()
                .message
                .text
                .is_some()
        );
        let request = game.take_quiz_scene_request().unwrap();
        assert_eq!(request.args, [2, 3, 4]);
        assert_eq!(request.data.date, old_date);
        assert_eq!(
            request.data.progress.extra_word,
            runtime.progress.extra_word
        );
        if !expanded {
            assert_eq!(request.data.progress.entries[1], dormant);
        }
        assert_eq!(request.data.progress.active_entry().scenario_id, 0);
        assert_eq!(request.data.progress.active_entry().random_seed, 42);
        assert_eq!(request.data.progress.active_entry().save_position, 2);
        assert_eq!(request.data.progress.active_entry().replay_target, 2);
        assert_eq!(
            request.data.progress.active_entry().response_data,
            [0x23, 0xee]
        );
        assert_eq!(request.data.save_data().primary_entry, data.primary_entry);
        assert_eq!(request.data.trailing_values, data.trailing_values);
        assert_eq!(request.data.byte_variables, data.byte_variables);
        let captured = game.globals.data.auto_save_slot.as_ref().unwrap();
        assert_eq!(captured.date, now);
        assert_eq!(captured.entry.save_position, 2);
        assert_eq!(captured.entry.response_data, [0x23, 0xee]);
        assert_eq!(captured.trailing_values, legacy.trailing_values);
        assert_eq!(captured.byte_variables, legacy.byte_variables);
        let before = (
            game.host().unwrap().runner.position(),
            game.host().unwrap().runner.next_position(),
            game.host().unwrap().runner.random_state(),
        );
        assert!(
            shin_tasks::block_on(game.update(
                Ticks::from_f32(3.0),
                true,
                Buttons::default(),
                &PointerFrame {
                    number: 999,
                    time_seconds: 999.0,
                    events: &[]
                },
                policy(),
                audio()
            ))
            .is_err()
        );
        assert_eq!(
            (
                game.host().unwrap().runner.position(),
                game.host().unwrap().runner.next_position(),
                game.host().unwrap().runner.random_state()
            ),
            before
        );
        assert_eq!(count.load(Ordering::SeqCst), 2);
        assert!(game.take_quiz_scene_request().is_none());
        assert_eq!(
            game.host().unwrap().runner.reference_register(
                shin_core::format::scenario::instruction_elements::Register::from_regular_register(
                    31
                )
            ),
            Some(0)
        );
        let active = game.bindings.autosave.progress.active_index();
        game.bindings.autosave.progress.entries[active].response_data[0] ^= 0x55;
        assert_eq!(
            request.data.progress.active_entry().response_data,
            [0x23, 0xee]
        );
    }
    eprintln!(
        "Gameplay Quiz:3 real single/expanded source owners,current11780 date versus preserved factory date,full source response/u16/byte vectors,three-argument owned request and source quiescence;retained replay boundary injects four-bit score/full register and skips copy/scene before resuming script pass;quiz scene still pending"
    );
}
