//! Fresh New Game through the complete authored first book and natural EXIT.
//! No bytecode, VM, response, playback policy or media-clock mutation.
use super::*;
use crate::{
    adv::higurashi::read_state::native_message_id,
    app::higurashi::{runtime_game_data::RuntimeGameData, runtime_saves::Slots},
};
use shin_core::format::save::higurashi::{GameData, GameDataEntry, SaveDate};
use std::cell::RefCell;

fn rows(runtime: &Runtime) -> Slots {
    runtime
        .globals()
        .unwrap()
        .unwrap()
        .runtime_saves
        .unwrap()
        .clone()
}
fn identities(runtime: &Runtime) -> [usize; 5] {
    let globals = runtime.globals().unwrap().unwrap();
    [
        Arc::as_ptr(globals.common) as usize,
        Arc::as_ptr(globals.server) as usize,
        Arc::as_ptr(globals.manager) as usize,
        Arc::as_ptr(globals.sounds) as usize,
        Arc::as_ptr(globals.storage.unwrap()) as *const () as usize,
    ]
}
fn same_manual(actual: &Slots, expected: &Slots) {
    for (index, (actual, expected)) in actual.manual.iter().zip(&expected.manual).enumerate() {
        assert_eq!(actual.valid, expected.valid, "manual{index} validity");
        assert_eq!(
            actual.data, expected.data,
            "manual{index} full two-record payload"
        );
    }
}
fn pending(runtime: &Runtime, pc: u32) -> bool {
    game(runtime).is_some_and(|game| {
        game.host().is_some_and(|host| {
            host.runner.position().0 == pc && host.runner.pending_command().is_some()
        })
    })
}
fn metadata(runtime: &Runtime, level: usize, label: &str) -> bool {
    game(runtime).is_some_and(|game| {
        game.host().is_some_and(|host| {
            host.runner.native_save_metadata().is_some_and(|info| {
                // Actual AUTOSAVE has captured this authored label; do not
                // accept a command-budget stop between SAVEINFO and AUTOSAVE.
                info.current()[level].as_ref() == label && info.saved()[level].as_ref() == label
            })
        })
    })
}
fn acquired(saved: &SaveData, tip: usize) -> bool {
    let bit = tip * 2;
    saved
        .save_vectors
        .extra_words_0
        .get(bit / 32)
        .is_some_and(|word| word & (1 << (bit % 32)) != 0)
}
fn pin(runtime: &Runtime, start: usize, bytes: &[u8]) {
    let globals = runtime.globals().unwrap().unwrap();
    assert_eq!(
        &globals.common.scenario.raw()[start..start + bytes.len()],
        bytes,
        "updated authored bytes at{start:08x}"
    );
}
struct ExitBoundary {
    slots: Slots,
    current: RuntimeGameData,
}

#[test]
#[ignore = "complete fresh authored first book via Runtime/raw input/Movie0+7 natural EOF/event voices/TIPS+OTSUGET/natural EXIT1f997 real SaveThenTitle;real assets/offscreen Metal/the default audio output;temporary progress only;up to two hours"]
fn higurashi_runtime_sustained_fresh_first_book_reaches_natural_exit_and_title() {
    let mut saved = initial();
    // Unrelated disk data detects losing either record or any vector. This
    // never supplies playable story state or modifies pristine route flags.
    saved.manual_save_slots[99] = Some(GameData {
        date: SaveDate {
            year: 2026,
            month: 10,
            day: 13,
            hour: 18,
            minute: 19,
            second: 20,
        },
        extra_word: 5,
        primary_entry: Some(GameDataEntry {
            scenario_id: 0,
            random_seed: 0x1234_5678,
            save_position: 0,
            replay_target: 0,
            response_data: vec![0x55, 0x80, 0xee],
        }),
        entry: GameDataEntry {
            scenario_id: 2000,
            random_seed: 0x8765_4321,
            save_position: 0,
            replay_target: 0,
            response_data: vec![0x91, 0x23, 0xfe, 0x80],
        },
        trailing_values: vec![7, 1023, 0x123],
        byte_variables: vec![9, 0, 255, 1, 0x80],
    });
    for slot in [1, 2, 3, 4, 5, 6, 7, 8, 24, 34, 35, 55] {
        assert_eq!(saved.persist_data.get(slot), 0, "pristine slot{slot}");
    }
    let initial_file = saved.encode().unwrap();
    let initial_disk = SaveData::decode(&initial_file).unwrap();
    let started = Instant::now();
    // Bonus3284 messages already needs roughly15 minutes plus221 real seconds
    // of credits; this route has over11000 static messages and two real movies.
    // Two hours is an observation bound, never a media completion substitute.
    let deadline = started + Duration::from_secs(2 * 60 * 60);
    let mut journey = Journey::new("sustained-fresh-first-book", &saved);
    let viewport = viewport((1920, 1080));
    journey.boot_title(viewport, deadline);
    let before = rows(&journey.runtime);
    assert!(before.manual[99].valid);
    let shared = identities(&journey.runtime);
    let clock = journey.runtime.globals().unwrap().unwrap().clock.ticks();
    journey.no_writes(Some(&initial_file));
    journey.start_story(viewport, deadline);
    let initial_current = game(&journey.runtime).unwrap().runtime_game_data().unwrap();
    assert_eq!(initial_current.progress.active_index(), 0);
    assert_eq!(initial_current.progress.active_entry().scenario_id, 0);
    assert_eq!(
        game(&journey.runtime)
            .unwrap()
            .host()
            .unwrap()
            .runner
            .frame()
            .message
            .playback
            .message_id,
        native_message_id(1579)
    );
    pin(
        &journey.runtime,
        0x222e6b,
        &[0xc1, 0x0d, 0x07, 0, 2, 0x82, 0xee],
    );
    pin(
        &journey.runtime,
        0x2abbc4,
        &[0xc1, 0x0d, 0x07, 0, 3, 7, 0x82, 0xee],
    );
    pin(&journey.runtime, 0x2abbcc, &[0xc8, 0x0d, 2]);
    pin(&journey.runtime, 0x3e4f3a, &[0xd0, 2, 0x3f, 0x80, 0x40]);
    pin(&journey.runtime, 0x3e4f60, &[0xd2, 0]);
    pin(&journey.runtime, 0x3e4f69, &[0x47, 0x2e, 0xf8, 1, 0]);
    pin(&journey.runtime, 0x1f997, &[0, 0, 1]);
    pure_frame(&mut journey);

    let mut driver = Driver::new();
    driver.until(
        &mut journey,
        viewport,
        deadline,
        "actual opening Movie0 wait",
        |r| pending(r, 0x222e72),
    );
    assert!(matches!(
        game(&journey.runtime)
            .unwrap()
            .host()
            .unwrap()
            .runner
            .pending_command(),
        Some(Cmd::MOVIEWAIT { args: [13, 2] })
    ));
    driver.until(
        &mut journey,
        viewport,
        deadline,
        "Movie0 natural EOF and Day1",
        |r| metadata(r, 0, "1日目"),
    );
    assert!(driver.movie_waits.contains(&0x222e72));

    for day in [
        "2日目", "3日目", "4日目", "5日目", "6日目", "7日目", "8日目",
    ] {
        driver.until(&mut journey, viewport, deadline, day, |r| {
            metadata(r, 0, day)
        });
        let globals = journey.runtime.globals().unwrap().unwrap();
        assert_eq!(
            globals.data.persist_data.get(3) & 2,
            0,
            "clear flag before{day}"
        );
        assert_eq!(identities(&journey.runtime), shared);
        same_manual(&rows(&journey.runtime), &before);
        journey.no_writes(Some(&initial_file));
    }
    assert!(
        driver.pending_pcs.contains(&0x230ded),
        "actual Days1/2 TIPS[0,1,2]"
    );
    pure_frame(&mut journey);

    driver.until(
        &mut journey,
        viewport,
        deadline,
        "pristine authored Movie7 wait",
        |r| pending(r, 0x2abbcc),
    );
    assert!(matches!(
        game(&journey.runtime)
            .unwrap()
            .host()
            .unwrap()
            .runner
            .pending_command(),
        Some(Cmd::MOVIEWAIT { args: [13, 2] })
    ));
    pure_frame(&mut journey);
    driver.until(
        &mut journey,
        viewport,
        deadline,
        "Movie7 natural EOF and fresh Rena route",
        |r| metadata(r, 0, "route レナ"),
    );
    assert!(driver.movie_waits.contains(&0x2abbcc));
    assert!(
        !driver.pending_pcs.contains(&0x2abb0a),
        "pristine route bypasses CHARSELECT"
    );
    same_manual(&rows(&journey.runtime), &before);
    journey.no_writes(Some(&initial_file));

    driver.until(
        &mut journey,
        viewport,
        deadline,
        "authored festival preview and Onikakushi",
        |r| metadata(r, 0, "鬼隠し編"),
    );
    {
        let globals = journey.runtime.globals().unwrap().unwrap();
        assert_eq!(globals.data.persist_data.get(2), 3);
        assert_eq!(
            globals.data.persist_data.get(3),
            1,
            "entered book;not yet cleared"
        );
    }
    // Original14f384's protected preview starts and ends with real voice waits.
    // The shared Driver does not set statuses, stop audio or advance its clock.
    assert!(
        driver.pending_pcs.contains(&0x14f425),
        "preview first VOICEWAIT2"
    );
    assert!(
        driver.pending_pcs.contains(&0x14f70d),
        "preview last VOICEWAIT2"
    );
    pure_frame(&mut journey);
    driver.until(
        &mut journey,
        viewport,
        deadline,
        "authored END metadata before final awards",
        |r| metadata(r, 0, "鬼隠し編") && metadata(r, 1, "END"),
    );
    assert!(driver.exits.is_empty(), "END metadata is not natural EXIT");
    same_manual(&rows(&journey.runtime), &before);
    journey.no_writes(Some(&initial_file));

    driver.until(
        &mut journey,
        viewport,
        deadline,
        "final authored TIPS63/64 Choice",
        |r| {
            pending(r, 0x3e4f3a)
                && game(r)
                    .unwrap()
                    .reference_story_choice()
                    .is_some_and(|(phase, _, _)| phase == 2)
        },
    );
    assert_eq!(
        game(&journey.runtime)
            .unwrap()
            .reference_story_choice()
            .unwrap()
            .2,
        [(0, "本編を続ける".into()), (1, "TIPSを見る".into())]
    );
    pure_frame(&mut journey);
    driver.until(
        &mut journey,
        viewport,
        deadline,
        "fresh OTSUGET0 actual confirmation",
        |r| {
            pending(r, 0x3e4f60) && game(r).unwrap().reference_story_notification() == Some((13, 2))
        },
    );
    assert!(driver.exits.is_empty());

    let exit = RefCell::new(None::<ExitBoundary>);
    driver.until(
        &mut journey,
        viewport,
        deadline,
        "natural EXIT1f997/save/ready Title",
        |r| {
            if let Some(game) = game(r) {
                let host = game.host().unwrap();
                if host.runner.position().0 == 0x1f997
                    && matches!(
                        host.runner.pending_command(),
                        Some(Cmd::EXIT { arg1: 0, arg2: 1 })
                    )
                    && game.has_save_task()
                    && exit.borrow().is_none()
                {
                    // Freeze after native EXIT commits and owns its real save;
                    // legitimate story autosaves differ from the boot snapshot.
                    *exit.borrow_mut() = Some(ExitBoundary {
                        slots: rows(r),
                        current: game.runtime_game_data().unwrap(),
                    });
                }
            }
            ready_menu(r, "top")
        },
    );
    let exit = exit
        .into_inner()
        .expect("natural retained EXIT save boundary not observed");
    assert!(
        game(&journey.runtime).is_none(),
        "Title must retire the real ADV source"
    );
    assert_eq!(
        journey.runtime.slot().unwrap().active_scene().unwrap(),
        ActiveScene::Title
    );
    assert_eq!(
        driver.exits.iter().map(|(pc, _)| *pc).collect::<Vec<_>>(),
        [0x1f997]
    );
    assert!(
        driver.max_commands > 20_000,
        "complete route exceeds old bounded prefix"
    );
    for pc in [0x38a132, 0x394d4d, 0x39f333, 0x3c9edc, 0x3e4f3a, 0x3e4f60] {
        assert!(
            driver.pending_pcs.contains(&pc),
            "authored notification at{pc:08x}"
        );
    }
    let after = rows(&journey.runtime);
    same_manual(&exit.slots, &before);
    same_manual(&after, &before);
    assert_eq!(after.auto.valid, exit.slots.auto.valid);
    assert_eq!(
        after.auto.data, exit.slots.auto.data,
        "full101 source→Title handoff"
    );
    assert!(after.auto.valid);
    assert_eq!(exit.current.progress.active_index(), 0);
    assert_eq!(exit.current.progress.active_entry().scenario_id, 0);
    assert!(exit.current.progress.active_entry().save_position > 0);
    assert!(
        !exit
            .current
            .progress
            .active_entry()
            .response_data
            .is_empty()
    );
    assert_eq!(
        exit.current.progress.entries[1], initial_current.progress.entries[1],
        "dormant current record is retained through fresh-main commands"
    );
    assert_eq!(identities(&journey.runtime), shared);
    let globals = journey.runtime.globals().unwrap().unwrap();
    assert!(globals.clock.ticks() > clock);
    for (slot, expected) in [(1, 1), (2, 3), (3, 3), (34, 1), (35, 1)] {
        assert_eq!(
            globals.data.persist_data.get(slot),
            expected,
            "final slot{slot}"
        );
    }
    assert_eq!(globals.data.persist_data.get(24) & 1, 1);
    assert!(acquired(globals.data, 63) && acquired(globals.data, 64));
    let writes = journey.storage.writes.lock().unwrap().clone();
    assert_eq!(
        writes.len(),
        1,
        "one natural flag1 EXIT save;no extra shutdown"
    );
    assert_eq!(
        journey
            .storage
            .paths
            .read(Source::Current)
            .unwrap()
            .as_deref(),
        Some(writes[0].as_slice())
    );
    let disk = SaveData::decode(&writes[0]).unwrap();
    assert_eq!(disk.manual_save_slots, initial_disk.manual_save_slots);
    // Compare autosave through its documented wire projection separately from
    // the complete101 runtime equality above (inactive record/counters/tails).
    let mut projection = initial_disk.clone();
    projection.auto_save_slot = Some(exit.slots.auto.data.save_data());
    let expected_auto = SaveData::decode(&projection.encode().unwrap())
        .unwrap()
        .auto_save_slot;
    assert_eq!(disk.auto_save_slot, expected_auto);
    for (slot, expected) in [(1, 1), (2, 3), (3, 3), (34, 1), (35, 1)] {
        assert_eq!(
            disk.persist_data.get(slot),
            expected,
            "saved final slot{slot}"
        );
    }
    assert_eq!(disk.persist_data.get(24) & 1, 1);
    assert!(acquired(&disk, 63) && acquired(&disk, 64));
    assert!(journey.runtime.failure().is_none());
    pure_frame(&mut journey);
    println!(
        "Runtime full fresh first book: elapsed{:.3}s/{}callbacks/{}commands;Movie0+7 naturalEOF,real event voices,TIPS/OTSUGET,authored EXIT0001f997→one real save→ready Title;full101 payloads/100manuals/shared owners retained;pure720p/1080p;unmodified script/raw controls only",
        started.elapsed().as_secs_f64(),
        driver.callbacks,
        driver.max_commands
    );
}
