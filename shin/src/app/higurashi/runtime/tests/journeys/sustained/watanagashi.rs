//! Genuine earned first-book progress -> actual Chart Day7 -> actual Mion
//! selection -> complete authored Watanagashi -> retained EXIT save -> Title.
use super::*;
use crate::app::higurashi::{
    chart::owner::Owner as ChartOwner, runtime_game_data::RuntimeGameData, runtime_saves::Slots,
};
use std::cell::RefCell;

const FIRST_BOOK: &str = "app::higurashi::runtime::tests::journeys::sustained::first_book::higurashi_runtime_sustained_fresh_first_book_reaches_natural_exit_and_title";
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3)
    })
}
fn earned() -> (SaveData, Vec<u8>) {
    let path = std::env::var("SHIN_HIGURASHI_EARNED_FIRST_BOOK")
        .expect("require the actual accepted first-book progress file");
    let record = std::env::var("SHIN_HIGURASHI_EARNED_FIRST_BOOK_RECORD")
        .expect("require the original accepted first-book execution record");
    let proof = std::fs::read(record).unwrap();
    // Original PASS record SHA2564c8aad99d7e969f05bd2b401dc5c5aa0b3026dc99847aaba25e48909f9a8e3af.
    // The parent launcher independently verifies full SHA256 for both files.
    assert_eq!(proof.len(), 4155);
    assert_eq!(fnv(&proof), 0xe241_31da_5d5e_c8f2);
    let proof: serde_json::Value = serde_json::from_slice(&proof).unwrap();
    assert_eq!(proof["exit_code"].as_i64(), Some(0));
    assert_eq!(proof["command"][1].as_str(), Some(FIRST_BOOK));
    assert!(
        proof["changed_pinned_sources"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let bytes = std::fs::read(path).unwrap();
    // Exact earned output: SHA256295776258fcf0cc7edda633487613d1740120bab1ad21632d82dd641ed1718b8.
    assert_eq!(bytes.len(), 12292);
    assert_eq!(fnv(&bytes), 0x6938_2f94_8580_6574);
    let saved = SaveData::decode(&bytes).unwrap();
    // Journey::new performs ordinary disk setup through the encoder. Prove
    // that it will copy the earned bytes unchanged, not project new flags.
    assert_eq!(saved.encode().unwrap(), bytes);
    for (slot, value) in [(1, 1), (2, 3), (3, 3), (4, 0), (24, 1), (31, 1), (58, 1)] {
        assert_eq!(saved.persist_data.get(slot), value, "earned slot{slot}");
    }
    assert!(saved.auto_save_slot.is_some());
    let sentinel = saved.manual_save_slots[99].as_ref().unwrap();
    assert!(sentinel.primary_entry.is_some());
    assert!(!sentinel.trailing_values.is_empty());
    assert!(!sentinel.byte_variables.is_empty());
    assert!(!saved.settings.common.v11_disallowskipunread);
    assert_eq!(saved.settings.common.v9_msgspeed, 100);
    assert_eq!(saved.settings.common.v10_skipspeed, 100);
    (saved, bytes)
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
fn same_manual(actual: &Slots, before: &Slots) {
    for (i, (actual, expected)) in actual.manual.iter().zip(&before.manual).enumerate() {
        assert_eq!(actual.valid, expected.valid, "manual{i} validity");
        assert_eq!(
            actual.data, expected.data,
            "manual{i} full two-record payload"
        );
    }
}
fn identities(runtime: &Runtime) -> [usize; 5] {
    let g = runtime.globals().unwrap().unwrap();
    [
        Arc::as_ptr(g.common) as usize,
        Arc::as_ptr(g.server) as usize,
        Arc::as_ptr(g.manager) as usize,
        Arc::as_ptr(g.sounds) as usize,
        Arc::as_ptr(g.storage.unwrap()) as *const () as usize,
    ]
}
fn pending(runtime: &Runtime, pc: u32) -> bool {
    game(runtime).is_some_and(|g| {
        g.host()
            .is_some_and(|h| h.runner.position().0 == pc && h.runner.pending_command().is_some())
    })
}
fn metadata(runtime: &Runtime, label: &str) -> bool {
    game(runtime).is_some_and(|g| {
        g.host().is_some_and(|h| {
            h.runner
                .native_save_metadata()
                .is_some_and(|m| m.current()[0].as_ref() == label && m.saved()[0].as_ref() == label)
        })
    })
}
fn chart(runtime: &Runtime) -> Option<&ChartOwner> {
    runtime.slot()?.title.as_ref()?.reference_chart_owner()
}
fn chart_ready(runtime: &Runtime) -> bool {
    chart(runtime).is_some_and(|c| {
        let s = c.reference_state();
        s.0 && s.1 && s.2 && s.3 == 1f32.to_bits() && s.4 == 0 && c.reference_question().is_none()
    })
}
fn cursor(runtime: &Runtime) -> [f32; 2] {
    let (x, y, _) = chart(runtime).unwrap().reference_cursor();
    [f32::from_bits(x), f32::from_bits(y)]
}
fn steer(
    j: &mut Journey,
    viewport: InputViewport,
    deadline: Instant,
    axis: usize,
    target: f32,
    code: KeyCode,
) {
    let mut callbacks = 0;
    let mut admitted = 0;
    while cursor(&j.runtime)[axis] < target {
        assert!(
            Instant::now() < deadline && callbacks < 1000,
            "Chart steering deadline"
        );
        assert!(chart_ready(&j.runtime));
        let before = cursor(&j.runtime);
        assert!(matches!(j.step(&key(code), viewport), Frame::Advanced(_)));
        let after = cursor(&j.runtime);
        // A native contact/readiness gate may consume a callback without
        // admitting controller movement. Observe it; never advance the cursor.
        if after[axis] != before[axis] {
            assert_eq!(
                after[axis],
                before[axis] + 10.,
                "admitted held D-pad movement"
            );
            admitted += 1;
        }
        assert_eq!(after[1 - axis], before[1 - axis]);
        callbacks += 1;
    }
    assert_eq!(cursor(&j.runtime)[axis], target);
    assert!(
        admitted > 0,
        "target reached through actual controller movement"
    );
    assert!(matches!(
        j.step(&RawInputState::new(), viewport),
        Frame::Advanced(_)
    ));
}
fn choose_doll(j: &mut Journey, viewport: InputViewport, deadline: Instant) {
    // Driver has stopped before it can autoaccept this actual SELECT. The
    // original0/Rena option is authored always-visible after earned31!=0.
    for _ in 0..5 {
        let g = game(&j.runtime).unwrap();
        assert!(pending(&j.runtime, 0x28cf33));
        let (phase, selected, rows) = g.reference_story_choice().unwrap();
        assert_eq!(phase, 2);
        assert!(g.input_permission);
        let wanted = rows
            .iter()
            .position(|(original, _)| *original == 0)
            .unwrap();
        if selected == wanted as i32 {
            j.tap(KeyCode::Enter, viewport);
            j.until(viewport, deadline, "actual doll close/callback", |r| {
                !pending(r, 0x28cf33)
            });
            return;
        }
        j.tap(KeyCode::ArrowDown, viewport);
    }
    panic!("actual original0 doll row not selected");
}
fn choose_mion(j: &mut Journey, viewport: InputViewport, deadline: Instant) {
    // Native mask and original row IDs remain owned by the actual Character
    // command/view/wrapper. No model assignment, fake callback or B shortcut.
    for _ in 0..5 {
        let g = game(&j.runtime).unwrap();
        assert!(pending(&j.runtime, 0x2abb0a));
        let (phase, selected, originals) = g.reference_story_character().unwrap();
        assert_eq!(phase, 2);
        assert!(g.input_permission);
        let wanted = originals
            .iter()
            .position(|original| *original == 1)
            .expect("actual Day7 choice must make original1/Mion available");
        if selected == wanted as i32 {
            j.tap(KeyCode::Enter, viewport);
            j.until(viewport, deadline, "actual Character close/callback", |r| {
                !pending(r, 0x2abb0a)
            });
            return;
        }
        j.tap(KeyCode::ArrowDown, viewport);
    }
    panic!("actual original1/Mion row not selected");
}
fn acquired(saved: &SaveData, tip: usize) -> bool {
    let bit = 2 * tip;
    saved
        .save_vectors
        .extra_words_0
        .get(bit / 32)
        .is_some_and(|w| w & (1 << (bit % 32)) != 0)
}
fn pin(runtime: &Runtime, at: usize, bytes: &[u8]) {
    assert_eq!(
        &runtime.globals().unwrap().unwrap().common.scenario.raw()[at..at + bytes.len()],
        bytes
    );
}
struct ExitBoundary {
    slots: Slots,
    current: RuntimeGameData,
}

#[test]
#[ignore = "actual accepted-earned disk/normal Runtime/Chart23/Day7 doll0/CHARSELECT original1/Movie7 natural EOF/complete Watan/EXIT1f997 real save+Title;real assets/offscreen Metal/the default audio output;temporary writes only;up to two hours"]
fn higurashi_runtime_sustained_earned_chart_mion_reaches_full_watanagashi_exit_and_title() {
    let (saved, initial_file) = earned();
    let started = Instant::now();
    // Whole authored Mion/commonB/Watan has8949 static MSGSET before the
    // already-real Day7/8 prefix; this bound never substitutes for media EOF.
    let deadline = started + Duration::from_secs(2 * 60 * 60);
    let viewport = viewport((1920, 1080));
    let mut j = Journey::new("sustained-earned-watanagashi", &saved);
    j.boot_title(viewport, deadline);
    let before = rows(&j.runtime);
    let shared = identities(&j.runtime);
    let clock = j.runtime.globals().unwrap().unwrap().clock.ticks();
    assert!(before.manual[99].valid && before.auto.valid);
    assert_eq!(
        j.runtime
            .slot()
            .unwrap()
            .title
            .as_ref()
            .unwrap()
            .reference_top_selection(),
        Some(1)
    );
    j.no_writes(Some(&initial_file));
    j.tap(KeyCode::ArrowDown, viewport);
    assert_eq!(
        j.runtime
            .slot()
            .unwrap()
            .title
            .as_ref()
            .unwrap()
            .reference_top_selection(),
        Some(3)
    );
    j.tap(KeyCode::Enter, viewport);
    j.until(viewport, deadline, "real retained Title Chart", chart_ready);
    assert_eq!(
        chart(&j.runtime)
            .unwrap()
            .reference_selected_entry()
            .unwrap()
            .0,
        0
    );
    assert_eq!(cursor(&j.runtime), [18., 28.]);
    // Actual held keyboard D-pad moves10 per admitted poll. Native node23's
    // hit rectangle contains(378,1368); auto-scroll follows the real cursor.
    steer(&mut j, viewport, deadline, 0, 378., KeyCode::ArrowRight);
    steer(&mut j, viewport, deadline, 1, 1368., KeyCode::ArrowDown);
    assert_eq!(
        chart(&j.runtime).unwrap().reference_selected_entry(),
        Some((23, 3215))
    );
    pure_frame(&mut j);
    same_manual(&rows(&j.runtime), &before);
    j.no_writes(Some(&initial_file));
    j.tap(KeyCode::Enter, viewport);
    j.until(
        viewport,
        deadline,
        "Chart shortcut actual default-No Question",
        |r| {
            chart(r).is_some_and(|c| {
                c.reference_question()
                    .is_some_and(|(_, phase, selected)| phase == 2 && selected == 1)
            })
        },
    );
    j.tap(KeyCode::ArrowLeft, viewport);
    assert_eq!(
        chart(&j.runtime).unwrap().reference_question().unwrap().2,
        0
    );
    j.tap(KeyCode::Enter, viewport);
    let mut driver = Driver::new();
    driver.until(
        &mut j,
        viewport,
        deadline,
        "Chart actual next-frame ADV3215/Day7",
        |r| metadata(r, "7日目"),
    );
    assert!(j.runtime.slot().unwrap().title.is_none());
    let initial_current = game(&j.runtime).unwrap().runtime_game_data().unwrap();
    assert_eq!(initial_current.progress.active_index(), 0);
    assert_eq!(initial_current.progress.active_entry().scenario_id, 3215);
    assert_eq!(identities(&j.runtime), shared);
    for (at, bytes) in [
        (
            0x1e517,
            &[0x46, 0x00, 0xb0, 0x90, 0x0c, 0x8f, 0x1e, 0xdb, 0x27, 0x00][..],
        ),
        (0x27db3d, &[0xd3, 0x00, 0x01, 0x17][..]),
        (
            0x14f38c,
            &[0x46, 0x00, 0xb0, 0x01, 0xa3, 0xf8, 0x14, 0x00][..],
        ),
        (0x14f910, &[0x9e, 0x02][..]),
        (0x14fbe7, &[0x9e, 0x02][..]),
        (
            0x28cfac,
            &[0x41, 0x00, 0x33, 0x00, 0x00, 0x82, 0x3a, 0x00][..],
        ),
        (0x2abb0a, &[0xd1, 0x12, 0x00, 0x00, 0x00, 0xb1][..]),
        (0x2abb8c, &[0xc8, 0x0d, 0x02][..]),
        (0x32333f, &[0x47, 0x35, 0x82, 0x2c, 0x00][..]),
        (0x4659ad, &[0xd0, 0x01, 0x80, 0x4d][..]),
        (0x4659d2, &[0xd2, 0x03][..]),
        (0x4659db, &[0x47, 0x2e, 0xf8, 0x01, 0x00][..]),
        (0x1f997, &[0x00, 0x00, 0x01][..]),
    ] {
        pin(&j.runtime, at, bytes);
    }
    driver.until(
        &mut j,
        viewport,
        deadline,
        "actual Day7 doll SELECT ready before autoaccept",
        |r| {
            pending(r, 0x28cf33)
                && game(r).is_some_and(|g| {
                    g.input_permission
                        && g.reference_story_choice()
                            .is_some_and(|(phase, _, _)| phase == 2)
                })
        },
    );
    choose_doll(&mut j, viewport, deadline);
    driver.until(
        &mut j,
        viewport,
        deadline,
        "actual Day8 after doll original0",
        |r| metadata(r, "8日目"),
    );
    assert_eq!(
        j.runtime
            .globals()
            .unwrap()
            .unwrap()
            .data
            .persist_data
            .get(58),
        0
    );
    driver.until(
        &mut j,
        viewport,
        deadline,
        "actual CHARSELECT ready before result",
        |r| {
            pending(r, 0x2abb0a)
                && game(r).is_some_and(|g| {
                    g.input_permission
                        && g.reference_story_character()
                            .is_some_and(|(phase, _, _)| phase == 2)
                })
        },
    );
    pure_frame(&mut j);
    choose_mion(&mut j, viewport, deadline);
    driver.until(
        &mut j,
        viewport,
        deadline,
        "cleared-route actual Movie7 wait",
        |r| pending(r, 0x2abb8c),
    );
    assert!(matches!(
        game(&j.runtime)
            .unwrap()
            .host()
            .unwrap()
            .runner
            .pending_command(),
        Some(Cmd::MOVIEWAIT { args: [13, 2] })
    ));
    driver.until(
        &mut j,
        viewport,
        deadline,
        "Movie7 natural EOF/actual Mion1",
        |r| metadata(r, "route 魅音1"),
    );
    assert!(driver.movie_waits.contains(&0x2abb8c));
    assert!(!driver.movie_waits.contains(&0x2abbcc));
    for label in [
        "route 魅音2",
        "route 魅音3",
        "route 魅音4",
        "route 魅音5",
        "route 綿流しB",
        "綿流し編",
    ] {
        driver.until(&mut j, viewport, deadline, label, |r| metadata(r, label));
        assert_eq!(identities(&j.runtime), shared);
        same_manual(&rows(&j.runtime), &before);
        j.no_writes(Some(&initial_file));
    }
    assert_eq!(
        j.runtime
            .globals()
            .unwrap()
            .unwrap()
            .data
            .persist_data
            .get(4),
        1,
        "authored entered flag before clear"
    );
    assert!(driver.pending_pcs.contains(&0x14f910));
    assert!(driver.pending_pcs.contains(&0x14fbe7));
    pure_frame(&mut j);
    driver.until(
        &mut j,
        viewport,
        deadline,
        "actual final TIPS77 Continue child",
        |r| {
            pending(r, 0x4659ad)
                && game(r).is_some_and(|g| {
                    g.reference_story_choice()
                        .is_some_and(|(phase, _, _)| phase == 2)
                })
        },
    );
    assert_eq!(
        j.runtime
            .globals()
            .unwrap()
            .unwrap()
            .data
            .persist_data
            .get(4),
        3
    );
    driver.until(
        &mut j,
        viewport,
        deadline,
        "actual OTSUGET3 phase2 acknowledgement",
        |r| {
            pending(r, 0x4659d2) && game(r).unwrap().reference_story_notification() == Some((13, 2))
        },
    );
    assert!(driver.exits.is_empty());
    j.no_writes(Some(&initial_file));
    let exit = RefCell::new(None::<ExitBoundary>);
    driver.until(
        &mut j,
        viewport,
        deadline,
        "natural EXIT1f997/save/ready Title",
        |r| {
            if let Some(g) = game(r) {
                let h = g.host().unwrap();
                if h.runner.position().0 == 0x1f997
                    && matches!(
                        h.runner.pending_command(),
                        Some(Cmd::EXIT { arg1: 0, arg2: 1 })
                    )
                    && g.has_save_task()
                    && exit.borrow().is_none()
                {
                    *exit.borrow_mut() = Some(ExitBoundary {
                        slots: rows(r),
                        current: g.runtime_game_data().unwrap(),
                    });
                }
            }
            ready_menu(r, "top")
        },
    );
    let exit = exit
        .into_inner()
        .expect("actual retained EXIT save boundary");
    assert!(game(&j.runtime).is_none());
    assert_eq!(
        j.runtime.slot().unwrap().active_scene().unwrap(),
        ActiveScene::Title
    );
    assert_eq!(
        driver.exits.iter().map(|(pc, _)| *pc).collect::<Vec<_>>(),
        [0x1f997]
    );
    for pc in [
        0x28cf33, 0x2abb0a, 0x2d63f7, 0x3fe3dc, 0x408e11, 0x40e5a8, 0x417a54, 0x41df4e, 0x426a44,
        0x434019, 0x43e408, 0x4659ad, 0x4659d2,
    ] {
        assert!(
            driver.pending_pcs.contains(&pc),
            "authored owned wait{pc:08x}"
        );
    }
    let after = rows(&j.runtime);
    same_manual(&exit.slots, &before);
    same_manual(&after, &before);
    assert_eq!(after.auto.valid, exit.slots.auto.valid);
    assert_eq!(
        after.auto.data, exit.slots.auto.data,
        "actual full101 EXIT transfer"
    );
    assert_eq!(exit.current.progress.active_index(), 0);
    assert_eq!(exit.current.progress.active_entry().scenario_id, 3215);
    assert!(exit.current.progress.active_entry().save_position > 0);
    assert_eq!(
        exit.current.progress.entries[1], initial_current.progress.entries[1],
        "fresh Chart dormant current record retained"
    );
    assert_eq!(identities(&j.runtime), shared);
    let globals = j.runtime.globals().unwrap().unwrap();
    assert!(globals.clock.ticks() > clock);
    for (slot, value) in [(1, 1), (2, 3), (3, 3), (4, 3), (24, 9), (58, 0)] {
        assert_eq!(
            globals.data.persist_data.get(slot),
            value,
            "committed final slot{slot}"
        );
    }
    assert!(acquired(globals.data, 77));
    assert!(
        globals
            .data
            .save_vectors
            .extra_words_1
            .get(132 / 32)
            .is_some_and(|w| w & (1 << (132 % 32)) != 0)
    );
    let writes = j.storage.writes.lock().unwrap().clone();
    assert_eq!(
        writes.len(),
        1,
        "one real flag1 EXIT save;no additional shutdown"
    );
    assert_eq!(
        j.storage.paths.read(Source::Current).unwrap().as_deref(),
        Some(writes[0].as_slice())
    );
    let disk = SaveData::decode(&writes[0]).unwrap();
    assert_eq!(disk.manual_save_slots, saved.manual_save_slots);
    let mut projection = saved.clone();
    projection.auto_save_slot = Some(exit.slots.auto.data.save_data());
    let projected_auto = SaveData::decode(&projection.encode().unwrap())
        .unwrap()
        .auto_save_slot;
    assert_eq!(
        disk.auto_save_slot, projected_auto,
        "disk projection separate from complete101 runtime equality"
    );
    for (slot, value) in [(1, 1), (2, 3), (3, 3), (4, 3), (24, 9), (58, 0)] {
        assert_eq!(disk.persist_data.get(slot), value, "saved final slot{slot}");
    }
    assert!(acquired(&disk, 77));
    assert!(j.runtime.failure().is_none());
    pure_frame(&mut j);
    println!(
        "Runtime earned Chart23→doll0→CHARSELECT original1→complete Watan→EXIT0001f997/one save/Title: elapsed{:.3}s,{} Runtimeframes/{} driven callbacks/{}commands;real Movie7 EOF/preview/TIPS/OTSUGET;full100manuals+101 EXIT boundary,shared Globals,unmodified exactearned input,pure720p/1080p",
        started.elapsed().as_secs_f64(),
        j.runtime.frames,
        driver.callbacks,
        driver.max_commands
    );
}
