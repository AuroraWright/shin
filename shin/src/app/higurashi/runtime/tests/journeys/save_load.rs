//! Raw-input Runtime journey; native SaveMenu/Question/Load/browser algorithms
//! retain their unchanged component oracles. This adds their outer app wiring.
use super::*;
use crate::app::higurashi::{
    gameplay::Gameplay, runtime_game_data::RuntimeGameData, runtime_saves::Slots,
};
use shin_core::format::save::higurashi::{GameData, GameDataEntry, SaveDate};
use std::sync::atomic::AtomicUsize;

const TARGET: usize = 2;
const NOW: SaveDate = SaveDate {
    year: 2026,
    month: 10,
    day: 13,
    hour: 18,
    minute: 19,
    second: 20,
};
fn game(runtime: &Runtime) -> &Gameplay {
    runtime.slot().unwrap().game.as_ref().unwrap()
}
fn slots(runtime: &Runtime) -> Slots {
    runtime
        .globals()
        .unwrap()
        .unwrap()
        .runtime_saves
        .unwrap()
        .clone()
}
fn same_slots(actual: &Slots, expected: &Slots) {
    assert_eq!(actual.auto.valid, expected.auto.valid);
    assert_eq!(actual.auto.data, expected.auto.data);
    for (i, (actual, expected)) in actual.manual.iter().zip(&expected.manual).enumerate() {
        assert_eq!(actual.valid, expected.valid, "manual validity {i}");
        assert_eq!(actual.data, expected.data, "complete manual payload {i}");
    }
}
fn message_ready(runtime: &Runtime) -> bool {
    runtime
        .slot()
        .and_then(|s| s.game.as_ref())
        .and_then(Gameplay::host)
        .is_some_and(|host| {
            let message = &host.runner.frame().message.playback;
            !host.has_modes()
                && host.modal_snapshot().is_none()
                && host.runner.reconstruction_phase().is_none()
                && message.wait.is_some()
                && message.interested_in_input()
        })
}
fn save_ready(runtime: &Runtime) -> bool {
    game(runtime)
        .reference_save_menu()
        .is_some_and(|menu| menu.reference_input_ready())
}
fn menu_ready(runtime: &Runtime, item: u32) -> bool {
    game(runtime).host().unwrap().reference_menu_control() == Some((item, true))
}
fn message_id(runtime: &Runtime) -> u32 {
    game(runtime)
        .host()
        .unwrap()
        .runner
        .frame()
        .message
        .playback
        .message_id
}
fn commands(runtime: &Runtime) -> u64 {
    game(runtime).host().unwrap().runner.completed_commands() as u64
}
fn current(runtime: &Runtime) -> RuntimeGameData {
    game(runtime).runtime_game_data().unwrap()
}
fn initial() -> SaveData {
    let mut saved = SaveData::default();
    saved.play_seconds = 17;
    saved.settings.common.v3_sysvol = 37;
    saved.save_menu_position = 0;
    saved.save_vectors.seen_messages = vec![1, 0, 0x8000_0000];
    saved.save_vectors.select_seen_choices = vec![0x4000];
    saved.save_vectors.select_last_choices = vec![3, 9, 15];
    // Offscreen row99 is an independent, valid two-record disk fixture. The
    // actual target/current VM comes only from the ordinary authored story.
    saved.manual_save_slots[99] = Some(GameData {
        date: NOW,
        extra_word: 5,
        entry: GameDataEntry {
            scenario_id: 0,
            random_seed: 42,
            save_position: 0,
            replay_target: 0,
            response_data: vec![],
        },
        primary_entry: Some(GameDataEntry {
            scenario_id: 2084,
            random_seed: 0x8765_4321,
            save_position: 17,
            replay_target: 19,
            response_data: vec![0xf1, 0x55, 0x23, 0xee],
        }),
        trailing_values: vec![7, 1023],
        byte_variables: vec![2, 255],
    });
    saved
}
fn same_pixels(actual: &[u8], expected: &[u8], label: &str) {
    assert_eq!(actual.len(), expected.len());
    let differences = actual.iter().zip(expected).filter(|(a, b)| a != b).count();
    let maximum = actual
        .iter()
        .zip(expected)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap_or(0);
    assert_eq!(
        differences, 0,
        "{label}: {differences} channels differ, max {maximum}"
    );
}
fn pure_composition(journey: &mut Journey, size: (u32, u32), visible: bool) -> Vec<u8> {
    let ticks = journey.runtime.globals().unwrap().unwrap().clock.ticks();
    let callbacks = journey.runtime.frames;
    let actual = journey.gpu.pixels(&journey.runtime, size, false);
    let copy = journey.gpu.pixels(&journey.runtime, size, true);
    same_pixels(
        &actual,
        &copy,
        "Runtime capture is the completed composed frame",
    );
    if visible {
        assert!(actual.chunks_exact(4).any(|p| p[..3] != [0, 0, 0]));
    }
    assert_eq!(journey.runtime.frames, callbacks);
    assert_eq!(
        journey.runtime.globals().unwrap().unwrap().clock.ticks(),
        ticks
    );
    actual
}

#[test]
#[ignore = "real Runtime boot/raw keys+pointer/actual manual Save+Load/authored message rewind/backlog B return at720p/1080p;offscreen Metal/exact Loopback Audio;temporary Paths only"]
fn higurashi_runtime_save_load_and_backlog_return_preserve_complete_owners() {
    for size in [(1280, 720), (1920, 1080)] {
        let saved = initial();
        let initial_bytes = saved.encode().unwrap();
        let mut journey = Journey::new("save-load-backlog", &saved);
        let viewport = viewport(size);
        let deadline = Instant::now() + Duration::from_secs(120);
        journey.boot_title(viewport, deadline);
        journey.no_writes(Some(&initial_bytes));
        {
            // Fixture-only, unreachable from the wire codec: prove an untouched
            // complete runtime owner survives while only its write is projected.
            // No current/target payload, owner, scene or input is injected.
            let globals = journey
                .runtime
                .slot
                .as_mut()
                .unwrap()
                .active_globals_mut()
                .unwrap();
            let sentinel = &mut globals.runtime_saves.as_mut().unwrap().manual[99].data;
            sentinel.progress.extra_word = 0xabcd_0005;
            sentinel.trailing_values = vec![0xffff, 0x1234];
        }
        journey.start_story(viewport, deadline);
        journey.until(
            viewport,
            deadline,
            "first authored message input not ready",
            message_ready,
        );
        let shared = journey.runtime.globals().unwrap().unwrap();
        let common = shared.common.clone();
        let manager = shared.manager.clone();
        let sounds = shared.sounds.clone();
        let calendar = Arc::new(AtomicUsize::new(0));
        let calls = calendar.clone();
        journey
            .runtime
            .slot
            .as_mut()
            .unwrap()
            .game
            .as_mut()
            .unwrap()
            .set_clock_source(move || {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(NOW)
            });
        let source = current(&journey.runtime);
        let source_message = message_id(&journey.runtime);
        let source_commands = commands(&journey.runtime);
        let source_bindings = game(&journey.runtime).reference_bindings();
        let before = slots(&journey.runtime);
        assert!(!before.manual[TARGET].valid);

        // M is the user's Y binding. The actual source menu must be ready,
        // selected on item0; writing a logical request would bypass this path.
        journey.tap(KeyCode::KeyM, viewport);
        journey.until(
            viewport,
            deadline,
            "actual SystemMenu Save row not ready",
            |r| menu_ready(r, 0),
        );
        assert_eq!(commands(&journey.runtime), source_commands);
        journey.tap(KeyCode::Enter, viewport);
        journey.until(
            viewport,
            deadline,
            "actual SaveMenu Input not ready",
            save_ready,
        );
        assert_eq!(
            game(&journey.runtime)
                .reference_save_menu()
                .unwrap()
                .reference_current(),
            Some(&source)
        );
        assert_eq!(
            game(&journey.runtime).host().unwrap().modal_pause_depth(),
            1
        );
        if size.0 == 1280 {
            for expected in 1..=TARGET {
                journey.tap(KeyCode::ArrowDown, viewport);
                journey.until(
                    viewport,
                    deadline,
                    "Save row navigation remained busy",
                    save_ready,
                );
                assert_eq!(
                    game(&journey.runtime)
                        .reference_save_menu()
                        .unwrap()
                        .state()
                        .selected,
                    expected as i32
                );
            }
            journey.tap(KeyCode::Enter, viewport);
        } else {
            // Actual native row hit geometry; Runtime owns the single physical
            // viewport conversion and the menu owns canonical InputArea scaling.
            let scroll = game(&journey.runtime)
                .reference_save_menu()
                .unwrap()
                .state()
                .scroll;
            let native = Vec2::new(960., 162. + 206. * TARGET as f32 + 100. - scroll);
            let point = viewport.offset + native / Vec2::new(1920., 1080.) * viewport.size;
            journey.step(&mouse(point, true), viewport);
            journey.step(&mouse(point, false), viewport);
        }
        journey.until(
            viewport,
            deadline,
            "Save default-No question not ready",
            |r| game(r).reference_save_menu().unwrap().reference_question() == Some((2, 1)),
        );
        assert_eq!(
            game(&journey.runtime)
                .reference_save_menu()
                .unwrap()
                .state()
                .selected,
            TARGET as i32
        );
        pure_composition(&mut journey, size, true);
        journey.tap(KeyCode::Enter, viewport); // Actual default No.
        journey.until(
            viewport,
            deadline,
            "Save No did not return to retained Input",
            save_ready,
        );
        journey.no_writes(Some(&initial_bytes));
        assert_eq!(calendar.load(Ordering::SeqCst), 0);
        same_slots(&slots(&journey.runtime), &before);
        assert_eq!(current(&journey.runtime), source);

        journey.tap(KeyCode::Enter, viewport);
        journey.until(viewport, deadline, "second Save question not ready", |r| {
            game(r).reference_save_menu().unwrap().reference_question() == Some((2, 1))
        });
        journey.tap(KeyCode::ArrowLeft, viewport); // No1 -> Yes0.
        assert_eq!(
            game(&journey.runtime)
                .reference_save_menu()
                .unwrap()
                .reference_question(),
            Some((2, 0))
        );
        journey.tap(KeyCode::Enter, viewport);
        let waiting_clock = journey.runtime.globals().unwrap().unwrap().clock.ticks();
        while !save_ready(&journey.runtime) {
            assert!(
                Instant::now() < deadline,
                "real atomic manual Save did not finish"
            );
            journey.step(&RawInputState::new(), viewport);
            assert_eq!(commands(&journey.runtime), source_commands);
            assert_eq!(current(&journey.runtime), source);
        }
        assert!(journey.runtime.globals().unwrap().unwrap().clock.ticks() > waiting_clock);
        assert_eq!(
            calendar.load(Ordering::SeqCst),
            1,
            "Save refreshes only its owned date once"
        );
        let mut stored = source.clone();
        stored.date = NOW;
        let mut expected = before.clone();
        expected.manual[TARGET].valid = true;
        expected.manual[TARGET].data = stored.clone();
        same_slots(&slots(&journey.runtime), &expected);
        assert_eq!(
            journey
                .runtime
                .globals()
                .unwrap()
                .unwrap()
                .data
                .save_menu_position,
            TARGET as u8
        );
        assert_eq!(game(&journey.runtime).reference_bindings(), source_bindings);
        let writes = journey.storage.writes.lock().unwrap().clone();
        assert_eq!(writes.len(), 1);
        assert_eq!(
            std::fs::read(journey.storage.paths.current.as_ref().unwrap()).unwrap(),
            writes[0]
        );
        let disk = SaveData::decode(&writes[0]).unwrap();
        let mut projected = stored.save_data();
        projected.extra_word &= 7;
        projected.entry.replay_target = projected.entry.save_position;
        if let Some(primary) = &mut projected.primary_entry {
            primary.replay_target = primary.save_position;
            for value in &mut projected.trailing_values {
                *value &= 1023;
            }
        } else {
            // Native record0 wire rows omit both the dormant record and u16
            // bank. Live rows above remain complete; Load never uses this copy.
            projected.trailing_values.clear();
        }
        assert_eq!(disk.manual_save_slots[TARGET].as_ref().unwrap(), &projected);
        assert_eq!(disk.manual_save_slots[99].as_ref().unwrap().extra_word, 5);
        assert_eq!(
            disk.manual_save_slots[99].as_ref().unwrap().trailing_values,
            [1023, 0x234]
        );
        assert_eq!(
            slots(&journey.runtime).manual[99].data.progress.extra_word,
            0xabcd_0005
        );
        assert_eq!(
            slots(&journey.runtime).manual[99].data.trailing_values,
            [0xffff, 0x1234]
        );

        journey.tap(KeyCode::Escape, viewport);
        journey.until(
            viewport,
            deadline,
            "Save Back did not resume the same SystemMenu",
            |r| game(r).reference_save_menu().is_none() && menu_ready(r, 0),
        );
        assert_eq!(current(&journey.runtime), source);
        assert_eq!(game(&journey.runtime).reference_bindings(), source_bindings);
        assert_eq!(
            game(&journey.runtime).host().unwrap().modal_pause_depth(),
            1
        );
        journey.tap(KeyCode::Escape, viewport);
        journey.until(
            viewport,
            deadline,
            "SystemMenu close did not resume ADV",
            message_ready,
        );

        // Actually leave the saved authored message. A Load which merely closes
        // its UI now cannot satisfy the subsequent VM/message rewind assertions.
        for _ in 0..16 {
            if message_id(&journey.runtime) != source_message {
                break;
            }
            journey.until(
                viewport,
                deadline,
                "authored message not accepting A",
                message_ready,
            );
            journey.tap(KeyCode::Enter, viewport);
        }
        journey.until(
            viewport,
            deadline,
            "next authored message did not become ready",
            |r| message_ready(r) && message_id(r) != source_message,
        );
        assert!(
            current(&journey.runtime)
                .progress
                .active_entry()
                .save_position
                > stored.progress.active_entry().save_position
        );
        let advanced = current(&journey.runtime);
        let preserved = slots(&journey.runtime);
        let global_before_load = game(&journey.runtime).freeze_progress().unwrap().data;
        journey.tap(KeyCode::KeyM, viewport);
        journey.until(viewport, deadline, "second SystemMenu not ready", |r| {
            menu_ready(r, 0)
        });
        journey.tap(KeyCode::ArrowDown, viewport);
        assert!(
            menu_ready(&journey.runtime, 1),
            "real Down must select Load item1"
        );
        journey.tap(KeyCode::Enter, viewport);
        journey.until(viewport, deadline, "LoadMenu Input not ready", save_ready);
        assert!(
            game(&journey.runtime)
                .reference_save_menu()
                .unwrap()
                .reference_current()
                .is_none()
        );
        assert_eq!(
            game(&journey.runtime)
                .reference_save_menu()
                .unwrap()
                .state()
                .selected,
            TARGET as i32
        );
        journey.tap(KeyCode::Enter, viewport);
        journey.until(
            viewport,
            deadline,
            "Load default-No question not ready",
            |r| game(r).reference_save_menu().unwrap().reference_question() == Some((2, 1)),
        );
        journey.tap(KeyCode::ArrowLeft, viewport);
        journey.tap(KeyCode::Enter, viewport);
        while !journey.runtime.slot().unwrap().pending_manual_load() {
            assert!(
                Instant::now() < deadline,
                "final Load callback did not queue flags0 ADV"
            );
            journey.step(&RawInputState::new(), viewport);
            assert_eq!(current(&journey.runtime), advanced);
        }
        same_slots(&slots(&journey.runtime), &preserved);
        // Native final Load queues the factory only after Exit slides all menu
        // artwork offscreen. This early authored scene has a black backdrop;
        // preserve its actual completed frame, including a legitimate black.
        let closed_menu = game(&journey.runtime).reference_save_menu().unwrap();
        assert_eq!(closed_menu.state().progress, 0.);
        assert!(closed_menu.model.loaded && closed_menu.model.closing);
        let prior_frame = pure_composition(&mut journey, size, false);
        let calendar_before_load = calendar.load(Ordering::SeqCst);
        // Enter is now held across the real factory boundary. The Runtime must
        // quarantine it rather than advancing the first restored message.
        journey.step(&key(KeyCode::Enter), viewport);
        assert!(!journey.runtime.slot().unwrap().pending_manual_load());
        assert_eq!(current(&journey.runtime), stored);
        assert_eq!(game(&journey.runtime).reference_bindings().0, 0);
        assert_eq!(game(&journey.runtime).reference_bindings().1, 0);
        assert_eq!(commands(&journey.runtime), 0);
        assert_eq!(calendar.load(Ordering::SeqCst), calendar_before_load);
        same_slots(&slots(&journey.runtime), &preserved);
        let new_frame = journey.gpu.pixels(&journey.runtime, size, false);
        same_pixels(
            &new_frame,
            &prior_frame,
            "new First displays the actual prior whole composition",
        );
        journey.step(&key(KeyCode::Enter), viewport);
        assert_eq!(journey.runtime.input.buttons().pressed & (1 << 24), 0);
        journey.step(&RawInputState::new(), viewport);
        journey.until(
            viewport,
            deadline,
            "restored saved authored message not ready",
            |r| message_ready(r) && message_id(r) == source_message,
        );
        let globals = journey.runtime.globals().unwrap().unwrap();
        assert!(Arc::ptr_eq(globals.common, &common));
        assert!(Arc::ptr_eq(globals.manager, &manager));
        assert!(Arc::ptr_eq(globals.sounds, &sounds));
        assert_eq!(
            globals.data.persist_data.0,
            global_before_load.persist_data.0
        );
        assert_eq!(
            globals.data.save_vectors.select_seen_choices,
            global_before_load.save_vectors.select_seen_choices
        );
        assert_eq!(
            globals.data.save_vectors.select_last_choices,
            global_before_load.save_vectors.select_last_choices
        );
        assert_eq!(current(&journey.runtime).date, NOW);
        pure_composition(&mut journey, size, true);

        // B is keyboard X, opening backlog; Escape is controller B, closing it.
        // Plain close retains the freshly loaded bindings rather than replacing
        // the scene as the separately native-compared Yes/jump path would do.
        let loaded = current(&journey.runtime);
        let loaded_slots = slots(&journey.runtime);
        let loaded_bindings = game(&journey.runtime).reference_bindings();
        let loaded_commands = commands(&journey.runtime);
        let calendar_before_browser = calendar.load(Ordering::SeqCst);
        let browser_clock = journey.runtime.globals().unwrap().unwrap().clock.ticks();
        journey.tap(KeyCode::KeyB, viewport);
        journey.until(
            viewport,
            deadline,
            "actual Runtime backlog not ready",
            |r| game(r).host().unwrap().reference_browser_ready(),
        );
        assert_eq!(
            game(&journey.runtime).host().unwrap().modal_pause_depth(),
            1
        );
        assert_eq!(commands(&journey.runtime), loaded_commands);
        assert_eq!(current(&journey.runtime), loaded);
        let (killed, retirements) = game(&journey.runtime)
            .host()
            .unwrap()
            .reference_browser_lifetime()
            .unwrap();
        assert!(!killed.load(Ordering::SeqCst));
        pure_composition(&mut journey, size, true);
        journey.tap(KeyCode::Escape, viewport);
        journey.until(
            viewport,
            deadline,
            "backlog B return did not retire its mode and resume ADV",
            |r| {
                message_ready(r)
                    && game(r)
                        .host()
                        .unwrap()
                        .reference_browser_selected()
                        .is_none()
            },
        );
        assert!(killed.load(Ordering::SeqCst));
        assert!(
            retirements
                .iter()
                .all(shin_tasks::LoaderRetirement::is_finished)
        );
        assert_eq!(
            game(&journey.runtime).host().unwrap().modal_pause_depth(),
            0
        );
        assert_eq!(current(&journey.runtime), loaded);
        assert_eq!(commands(&journey.runtime), loaded_commands);
        assert_eq!(game(&journey.runtime).reference_bindings(), loaded_bindings);
        assert_eq!(calendar.load(Ordering::SeqCst), calendar_before_browser);
        same_slots(&slots(&journey.runtime), &loaded_slots);
        assert!(journey.runtime.globals().unwrap().unwrap().clock.ticks() > browser_clock);
        assert_eq!(
            *journey.storage.writes.lock().unwrap(),
            writes,
            "Load/backlog return must not write progress"
        );
    }
}
