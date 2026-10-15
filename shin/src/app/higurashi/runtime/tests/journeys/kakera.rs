//! Real Title Load enters the authored fragment script. All subsequent source
//! menus, Kakera cards and both ADV factories use actual Runtime raw input.
use super::*;
use crate::app::higurashi::{
    gameplay::Gameplay, runtime_game_data::RuntimeGameData, runtime_saves::Slots,
};
use shin_core::format::save::higurashi::{GameData, GameDataEntry, SaveDate};
use std::sync::atomic::AtomicUsize;

const DATE: SaveDate = SaveDate {
    year: 2026,
    month: 10,
    day: 13,
    hour: 18,
    minute: 19,
    second: 20,
};
fn fixture() -> SaveData {
    let mut saved = SaveData::default();
    saved.play_seconds = 17;
    saved.settings.common.v3_sysvol = 37;
    saved.persist_data.set(60, 2); // Native normal fragment0's authored status.
    saved.save_vectors.seen_messages = vec![0; 4096];
    saved.save_vectors.select_seen_choices = vec![0x4000];
    saved.save_vectors.select_last_choices = vec![3, 9, 15];
    let entry = |scenario_id, random_seed| GameDataEntry {
        scenario_id,
        random_seed,
        save_position: 0,
        replay_target: 0,
        response_data: vec![],
    };
    // Native65be8's card producer selects record1 and2000+fragment. This
    // controlled disk save describes that genuine entry; no VM/source is patched.
    let data = GameData {
        date: DATE,
        extra_word: 5,
        entry: entry(2000, 42),
        primary_entry: Some(entry(0, 17)),
        trailing_values: vec![7, 1023],
        byte_variables: vec![0; 64],
    };
    saved.manual_save_slots[0] = Some(data.clone());
    saved.manual_save_slots[99] = Some(data);
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
fn same_rows(actual: &Slots, expected: &Slots) {
    assert_eq!(actual.auto.valid, expected.auto.valid);
    assert_eq!(actual.auto.data, expected.auto.data);
    same_manual(actual, expected);
}
fn same_manual(actual: &Slots, expected: &Slots) {
    for (i, (a, e)) in actual.manual.iter().zip(&expected.manual).enumerate() {
        assert_eq!(a.valid, e.valid, "manual row{i} validity");
        assert_eq!(a.data, e.data, "manual row{i} complete payload");
    }
}
fn message_ready(runtime: &Runtime, scenario: i32) -> bool {
    runtime
        .slot()
        .and_then(|s| s.game.as_ref())
        .is_some_and(|g| {
            let h = g.host().unwrap();
            let m = &h.runner.frame().message.playback;
            g.runtime_game_data()
                .unwrap()
                .progress
                .active_entry()
                .scenario_id
                == scenario
                && !h.has_modes()
                && h.modal_snapshot().is_none()
                && h.runner.reconstruction_phase().is_none()
                && m.wait.is_some()
                && m.interested_in_input()
        })
}
fn pure_frame(journey: &mut Journey, size: (u32, u32), visible: bool) {
    let before = journey.runtime.frames;
    let ticks = journey.runtime.globals().unwrap().unwrap().clock.ticks();
    let frame = journey.gpu.pixels(&journey.runtime, size, false);
    let copy = journey.gpu.pixels(&journey.runtime, size, true);
    let changed = frame.iter().zip(&copy).filter(|(a, b)| a != b).count();
    assert_eq!(frame.len(), copy.len());
    assert_eq!(changed, 0, "pure completed Runtime composition differs");
    if visible {
        assert!(frame.chunks_exact(4).any(|p| p[..3] != [0, 0, 0]));
    }
    assert_eq!(journey.runtime.frames, before);
    assert_eq!(
        journey.runtime.globals().unwrap().unwrap().clock.ticks(),
        ticks
    );
}
fn menu7(journey: &mut Journey, viewport: InputViewport, deadline: Instant) {
    journey.tap(KeyCode::KeyM, viewport);
    journey.until(viewport, deadline, "fragment SystemMenu not ready", |r| {
        game(r).host().unwrap().reference_menu_control() == Some((0, true))
    });
    // Fragment2000 has actual rows[0,1,2,3,7,5]. Up wraps0→5→7.
    for selected in [5, 7] {
        journey.tap(KeyCode::ArrowUp, viewport);
        assert_eq!(
            game(&journey.runtime)
                .host()
                .unwrap()
                .reference_menu_control(),
            Some((selected, true))
        );
    }
    journey.tap(KeyCode::Enter, viewport);
    journey.until(
        viewport,
        deadline,
        "item7 default-No question not ready",
        |r| {
            game(r)
                .host()
                .unwrap()
                .reference_menu_return_question()
                .is_some_and(|(_, phase, selected)| phase == 2 && selected == 1)
        },
    );
}
fn queue_kakera(journey: &mut Journey, viewport: InputViewport, deadline: Instant) {
    journey.tap(KeyCode::ArrowLeft, viewport);
    assert!(
        game(&journey.runtime)
            .host()
            .unwrap()
            .reference_menu_return_question()
            .is_some_and(|(_, phase, selected)| phase == 2 && selected == 0)
    );
    journey.tap(KeyCode::Enter, viewport);
    journey.until(
        viewport,
        deadline,
        "item7 final Yes did not queue Kakera",
        |r| r.slot().unwrap().pending_kakera(),
    );
}
fn ready_kakera(runtime: &Runtime) -> bool {
    runtime
        .slot()
        .and_then(|s| s.kakera.as_ref())
        .is_some_and(|k| k.reference_input_ready())
}
fn input_scene(journey: &mut Journey, viewport: InputViewport, deadline: Instant) {
    journey.until(
        viewport,
        deadline,
        "Kakera First/Input did not become ready",
        ready_kakera,
    );
    let menu = journey.runtime.slot().unwrap().kakera.as_ref().unwrap();
    assert!(!menu.motion.model.alternate);
    assert_eq!(menu.motion.model.selected_fragment(), 0);
    assert!(
        menu.motion.model.statuses[0] > 0,
        "actual card0 remains unlocked"
    );
}

#[test]
#[ignore = "actual Runtime TitleLoad/authoredfragment2000/SystemMenu7/Kakera/card+Back Yes ADV factories at720p1080p;offscreen Metal/exact Loopback;temporary Paths only"]
fn higurashi_runtime_fragment_menu_returns_through_both_actual_kakera_adv_factories() {
    for size in [(1280, 720), (1920, 1080)] {
        let saved = fixture();
        let initial = saved.encode().unwrap();
        let mut journey = Journey::new("fragment-kakera", &saved);
        let viewport = viewport(size);
        let deadline = Instant::now() + Duration::from_secs(120);
        journey.boot_title(viewport, deadline);
        let globals = journey.runtime.globals().unwrap().unwrap();
        let common = globals.common.clone();
        let manager = globals.manager.clone();
        let sounds = globals.sounds.clone();
        let original_rows = rows(&journey.runtime);
        assert_eq!(
            journey
                .runtime
                .slot()
                .unwrap()
                .title
                .as_ref()
                .unwrap()
                .reference_top_selection(),
            Some(0)
        );
        journey.tap(KeyCode::ArrowRight, viewport);
        assert_eq!(
            journey
                .runtime
                .slot()
                .unwrap()
                .title
                .as_ref()
                .unwrap()
                .reference_top_selection(),
            Some(1)
        );
        journey.tap(KeyCode::Enter, viewport);
        journey.until(viewport, deadline, "Title Load Input not ready", |r| {
            r.slot()
                .unwrap()
                .title
                .as_ref()
                .unwrap()
                .reference_save_menu()
                .is_some_and(|menu| menu.reference_input_ready())
        });
        journey.tap(KeyCode::Enter, viewport);
        journey.until(viewport, deadline, "Title Load question not ready", |r| {
            r.slot()
                .unwrap()
                .title
                .as_ref()
                .unwrap()
                .reference_save_menu()
                .unwrap()
                .reference_question()
                == Some((2, 1))
        });
        journey.tap(KeyCode::ArrowLeft, viewport);
        journey.tap(KeyCode::Enter, viewport);
        journey.until(viewport, deadline, "Title Load factory not queued", |r| {
            r.slot().unwrap().pending_manual_load()
        });
        same_rows(&rows(&journey.runtime), &original_rows);
        journey.until(
            viewport,
            deadline,
            "unmodified authored fragment2000 did not reach message input",
            |r| message_ready(r, 2000),
        );
        assert_eq!(
            game(&journey.runtime)
                .runtime_game_data()
                .unwrap()
                .progress
                .active_index(),
            1
        );
        same_manual(&rows(&journey.runtime), &original_rows);
        journey.no_writes(Some(&initial));

        let dates = Arc::new(AtomicUsize::new(0));
        let observed = dates.clone();
        journey
            .runtime
            .slot
            .as_mut()
            .unwrap()
            .game
            .as_mut()
            .unwrap()
            .set_clock_source(move || {
                observed.fetch_add(1, Ordering::SeqCst);
                Ok(DATE)
            });
        let source = game(&journey.runtime).runtime_game_data().unwrap();
        let bindings = game(&journey.runtime).reference_bindings();
        let paused_rows = rows(&journey.runtime);
        let commands = game(&journey.runtime)
            .host()
            .unwrap()
            .runner
            .completed_commands();
        menu7(&mut journey, viewport, deadline);
        // The native default No must reopen the same retained source before a
        // second final Yes can own a deferred scene factory.
        journey.tap(KeyCode::Enter, viewport);
        journey.until(
            viewport,
            deadline,
            "item7 No did not reopen source row7",
            |r| {
                let h = game(r).host().unwrap();
                h.reference_menu_return_question().is_none()
                    && h.reference_menu_control() == Some((7, true))
            },
        );
        assert!(!journey.runtime.slot().unwrap().pending_kakera());
        assert_eq!(game(&journey.runtime).runtime_game_data().unwrap(), source);
        assert_eq!(
            game(&journey.runtime)
                .host()
                .unwrap()
                .runner
                .completed_commands(),
            commands
        );
        assert_eq!(dates.load(Ordering::SeqCst), 0);
        same_rows(&rows(&journey.runtime), &paused_rows);
        journey.tap(KeyCode::Enter, viewport);
        journey.until(viewport, deadline, "second item7 question not ready", |r| {
            game(r)
                .host()
                .unwrap()
                .reference_menu_return_question()
                .is_some_and(|(_, phase, selected)| phase == 2 && selected == 1)
        });
        queue_kakera(&mut journey, viewport, deadline);
        assert!(journey.runtime.slot().unwrap().game.is_some());
        assert_eq!(game(&journey.runtime).runtime_game_data().unwrap(), source);
        pure_frame(&mut journey, size, false);
        journey.step(&RawInputState::new(), viewport);
        assert!(journey.runtime.slot().unwrap().game.is_none());
        let menu = journey.runtime.slot().unwrap().kakera.as_ref().unwrap();
        assert_eq!(menu.motion.model.data, source);
        let retired = menu.reference_retirements();
        assert!(
            !retired.is_empty(),
            "the actual Kakera First owns its worker"
        );
        input_scene(&mut journey, viewport, deadline);
        assert!(
            retired
                .iter()
                .all(shin_tasks::LoaderRetirement::is_finished)
        );
        assert_eq!(
            dates.load(Ordering::SeqCst),
            0,
            "item7 copies full VM data without11780/calendar"
        );
        same_rows(&rows(&journey.runtime), &paused_rows);
        pure_frame(&mut journey, size, true);

        // Card0 is a new actual fragment ADV: record0 remains the main-story
        // continuation, record1 gets scenario2000/new seed/counters0.
        journey.tap(KeyCode::Enter, viewport);
        journey.until(viewport, deadline, "Kakera card did not queue ADV", |r| {
            r.slot().unwrap().pending_kakera_return()
        });
        let card = journey
            .runtime
            .slot()
            .unwrap()
            .kakera
            .as_ref()
            .unwrap()
            .motion
            .model
            .data
            .clone();
        assert_eq!(card.progress.active_index(), 1);
        assert_eq!(card.progress.entries[0], source.progress.entries[0]);
        assert_eq!(card.progress.entries[1].scenario_id, 2000);
        assert_eq!(card.progress.entries[1].save_position, 0);
        assert_eq!(card.progress.entries[1].replay_target, 0);
        assert_eq!(
            card.progress.entries[1].response_data,
            source.progress.entries[1].response_data
        );
        assert_eq!(card.date, source.date);
        assert_eq!(card.progress.extra_word, source.progress.extra_word);
        assert_eq!(card.trailing_values, source.trailing_values);
        assert_eq!(card.byte_variables, source.byte_variables);
        journey.step(&RawInputState::new(), viewport);
        assert!(journey.runtime.slot().unwrap().kakera.is_none());
        assert_eq!(game(&journey.runtime).runtime_game_data().unwrap(), card);
        let fresh = game(&journey.runtime).reference_bindings();
        assert_eq!((fresh.0, fresh.1), (0, 0));
        assert_eq!(
            (fresh.2, fresh.3, fresh.4),
            (bindings.2, bindings.3, bindings.4)
        );
        same_rows(&rows(&journey.runtime), &paused_rows);
        journey.until(
            viewport,
            deadline,
            "card-selected authored fragment did not resume",
            |r| message_ready(r, 2000),
        );
        pure_frame(&mut journey, size, true);
        journey.no_writes(Some(&initial));

        let second_source = game(&journey.runtime).runtime_game_data().unwrap();
        let second_rows = rows(&journey.runtime);
        menu7(&mut journey, viewport, deadline);
        queue_kakera(&mut journey, viewport, deadline);
        input_scene(&mut journey, viewport, deadline);
        same_rows(&rows(&journey.runtime), &second_rows);
        journey.tap(KeyCode::Escape, viewport);
        journey.until(viewport, deadline, "Kakera Back question not ready", |r| {
            r.slot()
                .unwrap()
                .kakera
                .as_ref()
                .unwrap()
                .reference_question()
                == Some((2, 1))
        });
        journey.tap(KeyCode::Enter, viewport); // Default No leaves the menu alive.
        input_scene(&mut journey, viewport, deadline);
        assert!(!journey.runtime.slot().unwrap().pending_kakera_return());
        journey.tap(KeyCode::Escape, viewport);
        journey.until(
            viewport,
            deadline,
            "second Kakera Back question not ready",
            |r| {
                r.slot()
                    .unwrap()
                    .kakera
                    .as_ref()
                    .unwrap()
                    .reference_question()
                    == Some((2, 1))
            },
        );
        journey.tap(KeyCode::ArrowLeft, viewport);
        journey.tap(KeyCode::Enter, viewport);
        journey.until(
            viewport,
            deadline,
            "Kakera Back Yes did not queue main ADV",
            |r| r.slot().unwrap().pending_kakera_return(),
        );
        let mut main = second_source.clone();
        main.progress.set_active_index(0).unwrap();
        assert_eq!(
            journey
                .runtime
                .slot()
                .unwrap()
                .kakera
                .as_ref()
                .unwrap()
                .motion
                .model
                .data,
            main
        );
        journey.step(&RawInputState::new(), viewport);
        assert_eq!(game(&journey.runtime).runtime_game_data().unwrap(), main);
        same_rows(&rows(&journey.runtime), &second_rows);
        journey.until(
            viewport,
            deadline,
            "original primary main-story ADV did not resume",
            |r| message_ready(r, 0),
        );
        let globals = journey.runtime.globals().unwrap().unwrap();
        assert!(Arc::ptr_eq(globals.common, &common));
        assert!(Arc::ptr_eq(globals.manager, &manager));
        assert!(Arc::ptr_eq(globals.sounds, &sounds));
        assert_eq!(
            game(&journey.runtime)
                .runtime_game_data()
                .unwrap()
                .progress
                .active_index(),
            0
        );
        assert_eq!(
            game(&journey.runtime)
                .runtime_game_data()
                .unwrap()
                .progress
                .entries[1],
            main.progress.entries[1]
        );
        same_manual(&rows(&journey.runtime), &original_rows);
        pure_frame(&mut journey, size, true);
        journey.no_writes(Some(&initial));
    }
}
