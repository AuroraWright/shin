//! Real Runtime/title/script/quiz/score-replay routing. The retained native
//! component oracles establish question, packet and shader behavior separately.
use super::*;
use crate::app::higurashi::{
    gameplay::Gameplay, runtime_game_data::RuntimeGameData, runtime_saves::Slots,
};
use shin_core::{
    format::save::higurashi::{GameData, GameDataEntry, SaveDate},
    vm::higurashi::command::HigurashiRuntimeCommand as Cmd,
};
use shin_input::inputs::GamepadButton;

fn initial() -> SaveData {
    let row = |index: u32| GameData {
        date: SaveDate {
            year: 2025,
            month: 10,
            day: 1 + (index % 28) as u8,
            hour: (index % 24) as u8,
            minute: (index % 60) as u8,
            second: ((index * 7) % 60) as u8,
        },
        extra_word: (index % 8) as u8,
        primary_entry: Some(GameDataEntry {
            scenario_id: index as i32,
            random_seed: 0x1234_0000 + index,
            save_position: index * 3,
            replay_target: index * 3,
            response_data: vec![index as u8, 0xf1, 0x55],
        }),
        entry: GameDataEntry {
            scenario_id: 1000 + index as i32,
            random_seed: 0x8765_0000 + index,
            save_position: index * 5,
            replay_target: index * 5,
            response_data: vec![0x23, index as u8, 0xee, 0x80],
        },
        trailing_values: vec![1023, index as u16, 0x123],
        byte_variables: vec![9, index as u8, 255],
    };
    let mut saved = SaveData::default();
    saved.play_seconds = 17;
    saved.settings.common.v3_sysvol = 37;
    saved.auto_save_slot = Some(row(0));
    saved.manual_save_slots = std::array::from_fn(|i| Some(row(i as u32 + 1)));
    // Controlled initial disk progress, not injected runtime/command state.
    // Normal BGM acquisitions cannot obscure unrelated bank preservation.
    saved.save_vectors.unlocked_movies = vec![u32::MAX; 256];
    saved.save_vectors.unlocked_bgms = vec![u32::MAX; 256];
    saved.save_vectors.unlocked_cgs = vec![u32::MAX; 256];
    saved.save_vectors.extra_words_0 = vec![u32::MAX; 20];
    saved.save_vectors.seen_messages = vec![0xa5a5_a5a5, 0, 0x1234_5678];
    saved.save_vectors.select_seen_choices = vec![0x4321, 0];
    saved.save_vectors.select_last_choices = vec![1, 2, 15, 0];
    saved
}
fn game(runtime: &Runtime) -> &Gameplay {
    runtime.slot().unwrap().game.as_ref().unwrap()
}
fn rows(runtime: &Runtime) -> Slots {
    runtime
        .globals()
        .unwrap()
        .unwrap()
        .runtime_saves
        .unwrap()
        .clone()
}
fn same_manual(actual: &Slots, expected: &Slots) {
    for (i, (actual, expected)) in actual.manual.iter().zip(&expected.manual).enumerate() {
        assert_eq!(actual.valid, expected.valid, "manual row{i} validity");
        assert_eq!(actual.data, expected.data, "manual row{i} complete payload");
    }
}
fn same_rows(actual: &Slots, expected: &Slots) {
    assert_eq!(actual.auto.valid, expected.auto.valid);
    assert_eq!(actual.auto.data, expected.auto.data);
    same_manual(actual, expected);
}
fn globals_identity(runtime: &Runtime) -> [usize; 5] {
    let globals = runtime.globals().unwrap().unwrap();
    [
        Arc::as_ptr(globals.common) as usize,
        Arc::as_ptr(globals.server) as usize,
        Arc::as_ptr(globals.manager) as usize,
        Arc::as_ptr(globals.sounds) as usize,
        Arc::as_ptr(globals.storage.unwrap()) as *const () as usize,
    ]
}
fn top_selected(runtime: &Runtime) -> Option<i32> {
    runtime.slot()?.title.as_ref()?.reference_top_selection()
}
fn quiz_menu(runtime: &Runtime) -> Option<(i32, usize)> {
    runtime.slot()?.title.as_ref()?.reference_quiz_menu()
}
fn quiz_signature(runtime: &Runtime) -> Option<(i32, i32, i32, u32, u32, bool)> {
    let motion = &runtime.slot()?.quiz.as_ref()?.motion;
    Some((
        motion.model.current_question,
        motion.model.chosen_answer,
        motion.model.score,
        motion.question_elapsed.to_bits(),
        motion.countdown_elapsed.to_bits(),
        motion.model.closing,
    ))
}
fn pure_composition(journey: &mut Journey, visible: bool) {
    let frames = journey.runtime.frames;
    let clock = journey.runtime.globals().unwrap().unwrap().clock.ticks();
    let quiz = quiz_signature(&journey.runtime);
    let pending = journey.runtime.slot().unwrap().pending_scene_factory();
    for extent in [(1280, 720), (1920, 1080)] {
        let pixels = journey.gpu.pixels(&journey.runtime, extent, false);
        let copy = journey.gpu.pixels(&journey.runtime, extent, true);
        let repeated = journey.gpu.pixels(&journey.runtime, extent, false);
        assert_eq!(pixels.len(), copy.len());
        assert_eq!(
            pixels.iter().zip(&copy).filter(|(a, b)| a != b).count(),
            0,
            "pure whole-composed capture {extent:?}"
        );
        assert_eq!(
            pixels.iter().zip(&repeated).filter(|(a, b)| a != b).count(),
            0,
            "pure repeated presentation {extent:?}"
        );
        if visible {
            assert!(pixels.chunks_exact(4).any(|p| p[..3] != [0, 0, 0]));
        }
    }
    assert_eq!(journey.runtime.frames, frames);
    assert_eq!(
        journey.runtime.globals().unwrap().unwrap().clock.ticks(),
        clock
    );
    assert_eq!(quiz_signature(&journey.runtime), quiz);
    assert_eq!(
        journey.runtime.slot().unwrap().pending_scene_factory(),
        pending
    );
}

#[test]
#[ignore = "actual Runtime Top7/QuizMenu0/authored scenario5/ten real question outcomes/natural BGM+voice EOF/score replay;real assets/offscreen Metal/exact Loopback Audio;temporary progress only"]
fn higurashi_runtime_authored_quiz_returns_score_through_actual_scene_factories() {
    let saved = initial();
    let file = saved.encode().unwrap();
    let mut journey = Journey::new("authored-quiz", &saved);
    // Unlike the shorter component fixtures, this journey never stops a
    // player or alters its status to pass the native Finish audio gate.
    let deadline = Instant::now() + Duration::from_secs(240);
    let viewport = viewport((1920, 1080));
    let blank = RawInputState::new();
    journey.boot_title(viewport, deadline);
    let globals = globals_identity(&journey.runtime);
    let original_rows = rows(&journey.runtime);
    assert_eq!(top_selected(&journey.runtime), Some(1)); // Valid autosave.
    journey.tap(KeyCode::ArrowLeft, viewport);
    assert_eq!(top_selected(&journey.runtime), Some(0));
    for selected in [2, 5, 7] {
        journey.tap(KeyCode::ArrowDown, viewport);
        assert_eq!(top_selected(&journey.runtime), Some(selected));
    }
    journey.tap(KeyCode::Enter, viewport);
    journey.until(
        viewport,
        deadline,
        "actual title QuizMenu0 did not open",
        |r| quiz_menu(r) == Some((2, 0)),
    );
    pure_composition(&mut journey, true);
    same_rows(&rows(&journey.runtime), &original_rows);
    journey.no_writes(Some(&file));
    let title_atlas = Arc::downgrade(
        journey
            .runtime
            .slot()
            .unwrap()
            .title
            .as_ref()
            .unwrap()
            .atlas
            .as_ref()
            .unwrap(),
    );
    journey.tap(KeyCode::Enter, viewport);
    journey.until(
        viewport,
        deadline,
        "QuizMenu0 did not construct authored ADV5",
        |r| r.slot().is_some_and(|slot| slot.game.is_some()),
    );
    assert!(title_atlas.upgrade().is_none());
    assert_eq!(
        game(&journey.runtime)
            .runtime_game_data()
            .unwrap()
            .progress
            .active_entry()
            .scenario_id,
        5
    );
    assert_eq!(globals_identity(&journey.runtime), globals);

    // Only ordinary fresh A presses advance the actual authored introduction.
    // Check the queue after EACH callback, retaining its real one-frame source
    // boundary instead of allowing a three-callback tap to accept it unseen.
    let mut enter_held = false;
    while journey.runtime.slot().unwrap().pending_quiz().is_none() {
        assert!(
            Instant::now() < deadline,
            "authored ADV5 never reached QUIZ"
        );
        let confirm = game(&journey.runtime).host().is_some_and(|host| {
            matches!(
                host.runner.pending_command(),
                Some(Cmd::MSGWAIT { .. } | Cmd::MSGSET { auto_wait: 1, .. } | Cmd::SELECT { .. })
            )
        });
        let press = confirm && !enter_held;
        let raw = if press {
            key(KeyCode::Enter)
        } else {
            RawInputState::new()
        };
        assert!(matches!(journey.step(&raw, viewport), Frame::Advanced(_)));
        enter_held = press;
    }
    let (payload, args, dest, source_bindings, source_commands) = {
        let slot = journey.runtime.slot().unwrap();
        let request = slot.pending_quiz().unwrap();
        let game = slot.game.as_ref().unwrap();
        let host = game.host().unwrap();
        let Some(Cmd::QUIZ { dest, args }) = host.runner.pending_command() else {
            panic!("queued actual Quiz has no retained QUIZ command");
        };
        assert_eq!(request.args, *args);
        assert_eq!(request.data.progress.active_entry().scenario_id, 5);
        assert!(slot.quiz.is_none());
        (
            request.data.clone(),
            request.args,
            *dest,
            game.reference_bindings(),
            host.runner.completed_commands(),
        )
    };
    let quiz_rows = rows(&journey.runtime);
    assert_eq!(
        source_bindings.1, 0,
        "authored first Quiz has no prior response"
    );
    assert_eq!(
        journey
            .runtime
            .globals()
            .unwrap()
            .unwrap()
            .data
            .persist_data
            .get(59),
        0
    );
    same_manual(&quiz_rows, &original_rows);
    pure_composition(&mut journey, false); // Native source blackout may be black.
    assert_eq!(
        game(&journey.runtime)
            .host()
            .unwrap()
            .runner
            .completed_commands(),
        source_commands
    );
    assert!(journey.runtime.slot().unwrap().pending_quiz().is_some());
    journey.step(&blank, viewport); // Actual next-frame capture/retire/factory.
    {
        let slot = journey.runtime.slot().unwrap();
        assert!(slot.game.is_none());
        assert_eq!(slot.active_scene().unwrap(), ActiveScene::Quiz);
        let quiz = slot.quiz.as_ref().unwrap();
        assert_eq!(quiz.motion.model.data, payload);
        assert_eq!(quiz.motion.model.args, args);
        assert_eq!(quiz.motion.model.score, 0);
        assert!(quiz.reference_capture().is_some());
    }
    assert_eq!(globals_identity(&journey.runtime), globals);
    same_rows(&rows(&journey.runtime), &quiz_rows);

    let mut score = 0;
    let mut keyboard_answers = 0;
    let mut controller_answers = 0;
    let mut timeouts = 0;
    for index in 0..10 {
        journey.until(
            viewport,
            deadline,
            "natural quiz question did not become answerable",
            |r| {
                r.slot()
                    .and_then(|slot| slot.quiz.as_ref())
                    .is_some_and(|quiz| {
                        let model = &quiz.motion.model;
                        quiz.reference_input_ready()
                            && model.current_question == index
                            && model.chosen_answer < 0
                            && model.clocks[2].is_idle()
                    })
            },
        );
        if index == 0 || index == 8 {
            pure_composition(&mut journey, true);
        }
        if index == 4 || index == 9 {
            // Native cadence expires these questions; there is no timeout API
            // or private clock edit in this Runtime path.
            timeouts += 1;
            journey.until(
                viewport,
                deadline,
                "real unanswered question never expired",
                |r| {
                    r.slot()
                        .unwrap()
                        .quiz
                        .as_ref()
                        .unwrap()
                        .motion
                        .model
                        .current_question
                        != index
                },
            );
            let model = &journey
                .runtime
                .slot()
                .unwrap()
                .quiz
                .as_ref()
                .unwrap()
                .motion
                .model;
            assert_eq!(model.marks[index as usize], 0);
            assert_eq!(model.score, score);
        } else {
            let answer = journey
                .runtime
                .slot()
                .unwrap()
                .quiz
                .as_ref()
                .unwrap()
                .motion
                .model
                .correct_answer;
            let mut raw = RawInputState::new();
            if index % 2 == 0 {
                raw = key([
                    KeyCode::KeyB,
                    KeyCode::Enter,
                    KeyCode::Escape,
                    KeyCode::KeyM,
                ][answer as usize]);
                keyboard_answers += 1;
            } else {
                raw.gamepads.unified.buttons[[
                    GamepadButton::X,
                    GamepadButton::A,
                    GamepadButton::B,
                    GamepadButton::Y,
                ][answer as usize]] = true;
                controller_answers += 1;
            }
            journey.step(&raw, viewport);
            score += 1;
            let model = &journey
                .runtime
                .slot()
                .unwrap()
                .quiz
                .as_ref()
                .unwrap()
                .motion
                .model;
            assert_eq!(model.current_question, index);
            assert_eq!(model.chosen_answer, answer);
            assert_eq!(model.score, score);
            assert_eq!(model.marks[index as usize], 1);
            journey.step(&blank, viewport);
        }
        assert_eq!(globals_identity(&journey.runtime), globals);
        same_rows(&rows(&journey.runtime), &quiz_rows);
        assert_eq!(
            journey
                .runtime
                .slot()
                .unwrap()
                .quiz
                .as_ref()
                .unwrap()
                .motion
                .model
                .data,
            payload
        );
        journey.no_writes(Some(&file));
    }
    assert_eq!(
        (keyboard_answers, controller_answers, timeouts, score),
        (4, 4, 2, 8)
    );
    journey.until(
        viewport,
        deadline,
        "natural quiz BGM/voice EOF and score fade never returned",
        |r| r.slot().unwrap().pending_kakera_return(),
    );
    {
        let model = &journey
            .runtime
            .slot()
            .unwrap()
            .quiz
            .as_ref()
            .unwrap()
            .motion
            .model;
        assert!(model.closing);
        assert_eq!(model.score, 8);
        assert_eq!(model.current_question, -1);
        assert_eq!(model.marks[..10].iter().sum::<i32>(), 8);
        assert_eq!(model.data, payload);
    }
    same_rows(&rows(&journey.runtime), &quiz_rows);
    pure_composition(&mut journey, false);
    assert!(journey.runtime.slot().unwrap().quiz.is_some());
    journey.step(&blank, viewport);
    assert_eq!(
        journey.runtime.slot().unwrap().active_scene().unwrap(),
        ActiveScene::Gameplay
    );
    assert!(journey.runtime.slot().unwrap().quiz.is_none());
    assert_eq!(
        game(&journey.runtime)
            .host()
            .unwrap()
            .runner
            .native_constructor_word(),
        8
    );
    assert_eq!(globals_identity(&journey.runtime), globals);
    journey.until(
        viewport,
        deadline,
        "actual resumed QUIZ replay did not write score register",
        |r| {
            game(r).host().is_some_and(|host| {
                host.runner.reconstruction_phase().is_none()
                    && host.runner.reference_register(dest) == Some(8)
            })
        },
    );
    let game = game(&journey.runtime);
    let returned = game.runtime_game_data().unwrap();
    assert_eq!(returned.progress.active_entry().scenario_id, 5);
    let inactive = 1 - payload.progress.active_index();
    assert_eq!(
        returned.progress.entries[inactive],
        payload.progress.entries[inactive]
    );
    assert_eq!(returned.progress.extra_word, payload.progress.extra_word);
    let bindings = game.reference_bindings();
    // The unmodified scenario continues in this same callback: after QUIZ at
    //191687, SGET $v3,59 at191695 reads0 before PIC1098 blocks. Native score8
    // writes1000, then short SGET0 writes10: exactly six bits and byte0x88.
    assert_eq!(
        game.host().unwrap().runner.reference_register(
            shin_core::format::scenario::instruction_elements::Register::from_regular_register(3)
        ),
        Some(0)
    );
    assert_eq!(
        bindings.1,
        source_bindings.1 + 6,
        "QUIZ4 plus authored SGET0's two bits"
    );
    assert_eq!(
        returned.progress.active_entry().response_data.first(),
        Some(&0x88)
    );
    assert_eq!(bindings.2, source_bindings.2, "retained read-history owner");
    assert_eq!(
        bindings.3, source_bindings.3,
        "retained choice-history owner"
    );
    assert_eq!(bindings.4, source_bindings.4, "retained last-choice owner");
    same_manual(&rows(&journey.runtime), &original_rows);
    pure_composition(&mut journey, false);
    journey.no_writes(Some(&file));
    assert!(journey.runtime.failure().is_none());
    println!(
        "Runtime Top7→QuizMenu0→authored ADV5→natural QUIZ→4 keyboard+4 controller answers+2 timeouts→natural audio EOF→score8 ADV replay;full101 rows and shared owners retained;pure720p/1080p whole captures;no command/state patch,player stop,window or progress write"
    );
}
