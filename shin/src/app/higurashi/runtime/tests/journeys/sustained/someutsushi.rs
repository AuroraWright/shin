//! Actual accepted five-ending Tarai disk -> Day3 original0/1 -> real in-game
//! Chart26 -> original3/Natsumi -> fullSome/all-six epilogue EXIT5cb04 save/Title.
use super::*;
use crate::app::higurashi::{
    chart::owner::Owner as ChartOwner, runtime_game_data::RuntimeGameData, runtime_saves::Slots,
};
use std::{cell::RefCell, collections::BTreeSet};
const TARAI: &str = "app::higurashi::runtime::tests::journeys::sustained::taraimawashi::higurashi_runtime_sustained_earned_chart_reaches_full_taraimawashi_exit_and_title";
const PREVIEW: [u32; 8] = [
    0x150ddc, 0x150df4, 0x150e0c, 0x150e24, 0x150eb0, 0x150f04, 0x15109b, 0x1510b3,
];
fn fnv(b: &[u8]) -> u64 {
    b.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3)
    })
}
fn earned() -> (SaveData, Vec<u8>) {
    let p = std::env::var("SHIN_HIGURASHI_EARNED_TARAIMAWASHI")
        .expect("actual accepted cumulative Tarai output required");
    let r = std::env::var("SHIN_HIGURASHI_EARNED_TARAIMAWASHI_RECORD")
        .expect("exact original passing Tarai record required");
    let proof = std::fs::read(r).unwrap();
    // Parent additionally pins full SHA/build/binary/log/transitive earned chain.
    assert_eq!(proof.len(), 351627);
    assert_eq!(fnv(&proof), 0x1ec7_d7a4_c913_270b);
    let proof: serde_json::Value = serde_json::from_slice(&proof).unwrap();
    assert_eq!(proof["exit_code"].as_i64(), Some(0));
    assert_eq!(proof["command"][1].as_str(), Some(TARAI));
    assert!(
        proof["changed_pinned_sources"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(proof["acceptance_pending"].as_bool(), Some(false));
    let bytes = std::fs::read(p).unwrap();
    assert_eq!(bytes.len(), 15364);
    assert_eq!(fnv(&bytes), 0x2af6_690b_7e8f_b146);
    // SHA791651f9f1389ec6daae4857a240a6355df261360147a96abae2551481b52dcc.
    let saved = SaveData::decode(&bytes).unwrap();
    assert_eq!(saved.encode().unwrap(), bytes);
    for (slot, value) in [
        (0, 0),
        (1, 1),
        (2, 3),
        (3, 3),
        (4, 3),
        (5, 3),
        (6, 3),
        (7, 3),
        (8, 0),
        (24, 15),
        (41, 0),
        (42, 1),
        (43, 1),
        (46, 0),
    ] {
        assert_eq!(
            saved.persist_data.get(slot),
            value,
            "actual accepted input{slot}"
        );
    }
    for node in [5, 26] {
        assert!(
            saved
                .save_vectors
                .extra_words_1
                .get(node / 32)
                .is_some_and(|w| w & (1 << (node % 32)) != 0)
        );
    }
    for node in [59, 60, 133] {
        assert!(
            !saved
                .save_vectors
                .extra_words_1
                .get(node / 32)
                .is_some_and(|w| w & (1 << (node % 32)) != 0),
            "unearned shortcut{node}"
        );
    }
    assert!(saved.auto_save_slot.is_some());
    let sentinel = saved.manual_save_slots[99].as_ref().unwrap();
    assert!(sentinel.primary_entry.is_some());
    assert!(!sentinel.trailing_values.is_empty() && !sentinel.byte_variables.is_empty());
    assert!(!saved.settings.common.v11_disallowskipunread);
    assert_eq!(saved.settings.common.v9_msgspeed, 100);
    assert_eq!(saved.settings.common.v10_skipspeed, 100);
    for tip in [148, 149, 150, 151, 152, 153, 155] {
        assert!(!acquired(&saved, tip), "new Some TIP{tip}");
    }
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
fn story_metadata(runtime: &Runtime, label: &str) -> bool {
    assert!(
        game(runtime).is_some(),
        "left ADV before authored story label {label}: {}",
        Driver::describe(runtime)
    );
    metadata(runtime, label)
}
fn chart(runtime: &Runtime) -> Option<&ChartOwner> {
    game(runtime)
        .and_then(|g| g.reference_chart_owner())
        .or_else(|| runtime.slot()?.title.as_ref()?.reference_chart_owner())
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
fn steer(j: &mut Journey, viewport: InputViewport, deadline: Instant, axis: usize, center: f32) {
    let start = cursor(&j.runtime)[axis];
    let target = start + ((center - start) / 10.).round() * 10.;
    let direction = if target >= start { 1. } else { -1. };
    let code = match (axis, direction > 0.) {
        (0, true) => KeyCode::ArrowRight,
        (0, false) => KeyCode::ArrowLeft,
        (1, true) => KeyCode::ArrowDown,
        (1, false) => KeyCode::ArrowUp,
        _ => unreachable!(),
    };
    let mut callbacks = 0;
    while cursor(&j.runtime)[axis] != target {
        assert!(
            Instant::now() < deadline && callbacks < 1000,
            "actual Chart steering deadline"
        );
        assert!(chart_ready(&j.runtime));
        let before = cursor(&j.runtime);
        assert!(matches!(j.step(&key(code), viewport), Frame::Advanced(_)));
        let after = cursor(&j.runtime);
        if after[axis] != before[axis] {
            assert_eq!(after[axis], before[axis] + 10. * direction);
        }
        assert_eq!(after[1 - axis], before[1 - axis]);
        callbacks += 1;
    }
    assert!((cursor(&j.runtime)[axis] - center).abs() <= 5.);
    assert!(matches!(
        j.step(&RawInputState::new(), viewport),
        Frame::Advanced(_)
    ));
}
fn enter_chart_node(
    j: &mut Journey,
    viewport: InputViewport,
    deadline: Instant,
    id: u32,
    scenario: i32,
    center: [f32; 2],
) {
    assert!(chart_ready(&j.runtime));
    steer(j, viewport, deadline, 0, center[0]);
    steer(j, viewport, deadline, 1, center[1]);
    assert_eq!(
        chart(&j.runtime).unwrap().reference_selected_entry(),
        Some((id, scenario))
    );
    pure_frame(j);
    j.tap(KeyCode::Enter, viewport);
    j.until(
        viewport,
        deadline,
        "actual Chart default-No Question",
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
}
fn script_choice_pending(runtime: &Runtime, pc: u32, character: bool) -> bool {
    assert!(
        game(runtime).is_some(),
        "left ADV before required script choice{pc:08x}: {}",
        Driver::describe(runtime)
    );
    let Some(h) = game(runtime).unwrap().host() else {
        return false;
    };
    let actual = h.runner.position().0;
    // The shared Driver answers any ready script choice. Reject a different
    // prompt before it can advance a wrong route while awaiting this one.
    match h.runner.pending_command() {
        Some(Cmd::SELECT { .. }) => assert!(
            !character && actual == pc,
            "unexpected SELECT while awaiting required choice{pc:08x}: {}",
            Driver::describe(runtime)
        ),
        Some(Cmd::CHARSELECT { .. }) => assert!(
            character && actual == pc,
            "unexpected CHARSELECT while awaiting required choice{pc:08x}: {}",
            Driver::describe(runtime)
        ),
        _ => return false,
    }
    true
}
fn choice_ready(runtime: &Runtime, pc: u32) -> bool {
    script_choice_pending(runtime, pc, false)
        && game(runtime).is_some_and(|g| {
            g.input_permission
                && g.reference_story_choice()
                    .is_some_and(|(phase, _, _)| phase == 2)
        })
}
fn choose_original(
    j: &mut Journey,
    viewport: InputViewport,
    deadline: Instant,
    pc: u32,
    original: usize,
) {
    // The original script answer is distinct from its current compact visible
    // row. Native visibility remains owned by the real SELECT model.
    for _ in 0..8 {
        assert!(choice_ready(&j.runtime, pc));
        let (_, selected, rows) = game(&j.runtime).unwrap().reference_story_choice().unwrap();
        let wanted = rows
            .iter()
            .position(|(id, _)| *id == original)
            .unwrap_or_else(|| {
                panic!("authored SELECT{pc:08x} original{original} unavailable: {rows:?}")
            });
        if selected == wanted as i32 {
            j.tap(KeyCode::Enter, viewport);
            j.until(viewport, deadline, "actual SELECT close/callback", |r| {
                !pending(r, pc)
            });
            return;
        }
        j.tap(KeyCode::ArrowDown, viewport);
    }
    panic!("actual SELECT{pc:08x} original{original} was not selected");
}
fn choose_character(j: &mut Journey, viewport: InputViewport, deadline: Instant) {
    for _ in 0..6 {
        let g = game(&j.runtime).unwrap();
        assert!(script_choice_pending(&j.runtime, 0x2abb0a, true));
        let (phase, selected, originals) = g.reference_story_character().unwrap();
        assert_eq!(phase, 2);
        assert!(g.input_permission);
        let wanted = originals
            .iter()
            .position(|i| *i == 3)
            .expect("actual committed41/46 must expose original3/Natsumi");
        if selected == wanted as i32 {
            j.tap(KeyCode::Enter, viewport);
            j.until(
                viewport,
                deadline,
                "actual Natsumi Character callback",
                |r| !pending(r, 0x2abb0a),
            );
            return;
        }
        j.tap(KeyCode::ArrowDown, viewport);
    }
    panic!("original3/Natsumi was not selected");
}
fn tips_ready(runtime: &Runtime, pc: u32) -> bool {
    assert!(
        game(runtime).is_some(),
        "left ADV before required finalTIPS155"
    );
    pending(runtime, pc)
        && game(runtime)
            .unwrap()
            .reference_story_choice()
            .is_some_and(|(p, _, _)| p == 2)
}
fn continue_tip(j: &mut Journey, viewport: InputViewport, deadline: Instant, pc: u32) {
    for _ in 0..8 {
        assert!(tips_ready(&j.runtime, pc));
        let (_, selected, rows) = game(&j.runtime).unwrap().reference_story_choice().unwrap();
        let wanted = rows
            .iter()
            .position(|(i, _)| *i == 0)
            .expect("actual Continue original0");
        if selected == wanted as i32 {
            j.tap(KeyCode::Enter, viewport);
            j.until(viewport, deadline, "actual TIPS Continue callback", |r| {
                !pending(r, pc)
            });
            return;
        }
        j.tap(KeyCode::ArrowDown, viewport);
    }
    panic!("actual Continue original0 not selected");
}
fn no_script_choice(runtime: &Runtime) {
    let g =
        game(runtime).unwrap_or_else(|| panic!("left required ADV: {}", Driver::describe(runtime)));
    if let Some(h) = g.host() {
        assert!(
            !matches!(
                h.runner.pending_command(),
                Some(Cmd::SELECT { .. } | Cmd::CHARSELECT { .. })
            ),
            "unexpected route-critical choice: {}",
            Driver::describe(runtime)
        );
    }
}
fn acquired(saved: &SaveData, tip: usize) -> bool {
    let bit = 2 * tip;
    saved
        .save_vectors
        .extra_words_0
        .get(bit / 32)
        .is_some_and(|w| w & (1 << (bit % 32)) != 0)
}
fn pin(runtime: &Runtime, at: usize, hex: &str) {
    let bytes = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        &runtime.globals().unwrap().unwrap().common.scenario.raw()[at..at + bytes.len()],
        bytes.as_slice(),
        "authored bytes{at:08x}"
    );
}
struct ExitBoundary {
    slots: Slots,
    current: RuntimeGameData,
    runner_commands: u64,
}

#[test]
#[ignore = "actual accepted cumulative Tarai disk/normalRuntime/Chart5/Day3 originals0,1/committedDay4/menu4Chart26/CHARSELECT original3/Movie7naturalEOF/protectedpreview5/fullSome+all-sixepilogue/EXIT5cb04 save+Title;realassets/offscreenMetal/exactLoopbackAudio;temporaryPaths only;twohour observation allowance"]
fn higurashi_runtime_sustained_earned_day3_chart_natsumi_reaches_full_someutsushi_epilogue_and_title()
 {
    let (saved, initial_file) = earned();
    let started = Instant::now();
    let deadline = started + Duration::from_secs(2 * 60 * 60);
    let viewport = viewport((1920, 1080));
    let mut j = Journey::new("sustained-earned-someutsushi", &saved);
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
    j.until(viewport, deadline, "actual Title Chart", chart_ready);
    // Source5 anchor10,7 PLUS label0,1: center378/476, accepted held target378/478.
    enter_chart_node(&mut j, viewport, deadline, 5, 3203, [378., 476.]);
    let mut driver = Driver::new();
    driver.until(&mut j, viewport, deadline, "fresh Day3", |r| {
        metadata(r, "3日目")
    });
    assert_eq!(
        game(&j.runtime)
            .unwrap()
            .runtime_game_data()
            .unwrap()
            .progress
            .active_entry()
            .scenario_id,
        3203
    );
    for (at, hex) in [
        (
            0x0001e429,
            "41001700014600b0900bb8829905004600b0900bb913aa0500a000063193fa96da004600b0900c80692e2200a000063293fa96da004600b0900c81014e2200a000063393fa96da004600b0900c82fa0d23004600b0900c836cb12300",
        ),
        (
            0x0001e521,
            "a000063893fa96da004600b0900c90c63d29004600b0900c91e4cb29004600b0900c9294b72a00",
        ),
        (
            0x0023b16c,
            "a00112233220838c836982c682cc8b4182e893b900a1d3000105",
        ),
        (
            0x0023b6c2,
            "8d0a0005000000900fff0b838c836982cc975582a200322a30332e82a082a0814182a282a282ba002a3234352e82a0814182b582dc82c182bd82a891dc82cc97708e9682aa816300004600b0001bb723004600b001a6bf2300",
        ),
        (
            0x0023d4c3,
            "8d0c0006000000900fff0f838c836982cc95f3925482b5826800472a302e8e8091cc82c582e0917b82b582c482f182b682e182c882a282c582b782a98148002a332e95f3925482b582dd82bd82a282c582b782e68163816382a082ea82c582e000004600b000b6e623004600b001daf62300",
        ),
        (
            0x0023f6da,
            "41002200018229014100230000822a004100240000822b004100270001822e01",
        ),
        (
            0x00242c24,
            "a000063493fa96da00a0011023312095948a8882d682cc8e5189c100a1d3000106",
        ),
        (
            0x002ab794,
            "d300011a810000034606b002a9b72a0047c2bb2a00a0010f8370815b83678369815b914991f000a1",
        ),
        (
            0x002aba37,
            "4602c0220052ba2a0041002200004600c0170052ba2a008122002941001800c0224600c0180082ba2a004602c027007cba2a0041002700004600c017007cba2a008127002e41001800c0274600c0180190ba2a004795ba2a004108010008",
        ),
        (0x002abb00, "8a004600b100dbbb2a00d112000000b1"),
        (
            0x002abb8c,
            "c80d02c20d00a383003c4600b000b3bb2a004600b001b8bb2a004600b002bdbb2a004770bc2a004776d12a004729642d004744333200",
        ),
        (
            0x002ad14c,
            "810c000841080c00018208bc810c000241080c00028202bc41000000054884f31400830001473c0f5800",
        ),
        (0x0014f3ac, "4600b005200d1500"),
        (0x00150d20, "a20096071e90270000836b"),
        (0x00151243, "a3474912150049"),
        (
            0x005ed0c4,
            "810c000841080c00028208bcb00dd001809b83008078d301018093810200184606b28040ffd05e00810c001841080c0080408218bcd20683008078830001472ef80100",
        ),
        (
            0x0001f82e,
            "810200034686b2027bf80100810200044686b2027bf80100810200064686b2027bf80100810200054686b2027bf80100810200074686b2027bf80100810200084686b2027bf801004813aa05004797f90100",
        ),
        (
            0x0005aa13,
            "a000138bbb8b7b8c788e408f908e968c8f92b28f9100a001096570696c6f67756500a1d3000101",
        ),
        (
            0x0005cae3,
            "810c000141080c00108201bc810c000041080c00028200bc4601c0170107cb050000000149",
        ),
    ] {
        pin(&j.runtime, at, hex);
    }
    driver.until(
        &mut j,
        viewport,
        deadline,
        "actual Rena invitation SELECT",
        |r| choice_ready(r, 0x23b6c2),
    );
    choose_original(&mut j, viewport, deadline, 0x23b6c2, 0);
    driver.until(&mut j, viewport, deadline, "actual treasure SELECT", |r| {
        choice_ready(r, 0x23d4c3)
    });
    choose_original(&mut j, viewport, deadline, 0x23d4c3, 1);
    driver.until(
        &mut j,
        viewport,
        deadline,
        "actual Day4 AUTOSAVE commits prerequisites",
        |r| {
            no_script_choice(r);
            metadata(r, "4日目")
                && game(r).is_some_and(|g| {
                    let (p, q) = g.reference_story_variables();
                    p.get(41) == 1
                        && p.get(46) == 1
                        && !q.iter().any(|(s, _)| matches!(*s, 41 | 46))
                        && g.input_permission
                })
        },
    );
    let (committed, queued) = game(&j.runtime).unwrap().reference_story_variables();
    for (slot, value) in [(41, 1), (42, 0), (43, 0), (46, 1)] {
        assert_eq!(committed.get(slot), value);
        assert!(!queued.iter().any(|(s, _)| *s == slot as u32));
    }
    assert!(
        !driver.pending_pcs.contains(&0x23e657),
        "desired branch skips Rika shopping SELECT"
    );
    // Use actual J to leave skip before waiting for native message/menu input.
    if game(&j.runtime).unwrap().reference_play_modes() & 2 != 0 {
        j.tap(KeyCode::KeyJ, viewport);
    }
    assert_eq!(game(&j.runtime).unwrap().reference_play_modes() & 2, 0);
    j.until(
        viewport,
        deadline,
        "actual Day4 message/menu admission",
        |r| {
            no_script_choice(r);
            let g = game(r).unwrap();
            g.input_permission
                && g.host().is_some_and(|h| {
                    !h.has_modes()
                        && h.runner
                            .native_read_state()
                            .is_some_and(|s| s.flags & 0x8000_0000 != 0)
                        && h.runner.frame().message.playback.interested_in_input()
                })
        },
    );
    j.tap(KeyCode::KeyM, viewport);
    j.until(viewport, deadline, "actual SystemMenu0", |r| {
        game(r).unwrap().host().unwrap().reference_menu_control() == Some((0, true))
    });
    // Snapshot only after actual Menu admission. Opening callbacks may still
    // finish legitimate source work before the native modal takes ownership.
    let source = game(&j.runtime).unwrap().runtime_game_data().unwrap();
    let source_commands = game(&j.runtime)
        .unwrap()
        .host()
        .unwrap()
        .runner
        .completed_commands();
    // Real ordinary rows[0,1,2,4,3,5]; Up wraps0 ->5 ->3 ->4.
    for selected in [5, 3, 4] {
        j.tap(KeyCode::ArrowUp, viewport);
        assert_eq!(
            game(&j.runtime)
                .unwrap()
                .host()
                .unwrap()
                .reference_menu_control(),
            Some((selected, true))
        );
    }
    j.tap(KeyCode::Enter, viewport);
    j.until(
        viewport,
        deadline,
        "actual retained in-game Chart",
        chart_ready,
    );
    assert_eq!(
        game(&j.runtime).unwrap().runtime_game_data().unwrap(),
        source
    );
    assert_eq!(
        game(&j.runtime)
            .unwrap()
            .host()
            .unwrap()
            .runner
            .completed_commands(),
        source_commands
    );
    same_manual(&rows(&j.runtime), &before);
    j.no_writes(Some(&initial_file));
    pure_frame(&mut j);
    // Source26 anchor10,29 PLUS label0,1 -> center378/1708. Actual source
    // route may center another current node; steer only from observed cursor.
    enter_chart_node(&mut j, viewport, deadline, 26, 3218, [378., 1708.]);
    driver.until(
        &mut j,
        viewport,
        deadline,
        "fresh actual partner ADV3218",
        |r| {
            metadata(r, "8日目")
                && game(r).is_some_and(|g| {
                    g.runtime_game_data()
                        .unwrap()
                        .progress
                        .active_entry()
                        .scenario_id
                        == 3218
                })
        },
    );
    assert!(chart(&j.runtime).is_none());
    let initial_current = game(&j.runtime).unwrap().runtime_game_data().unwrap();
    assert_eq!(initial_current.progress.active_index(), 0);
    assert_eq!(identities(&j.runtime), shared);
    driver.until(
        &mut j,
        viewport,
        deadline,
        "actual original3/Natsumi Character",
        |r| {
            script_choice_pending(r, 0x2abb0a, true)
                && game(r).is_some_and(|g| {
                    g.input_permission
                        && g.reference_story_character()
                            .is_some_and(|(p, _, _)| p == 2)
                })
        },
    );
    pure_frame(&mut j);
    choose_character(&mut j, viewport, deadline);
    driver.until(
        &mut j,
        viewport,
        deadline,
        "actual cleared Movie7 wait",
        |r| {
            assert!(game(r).is_some());
            pending(r, 0x2abb8c)
        },
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
    let preview = RefCell::new(BTreeSet::<u32>::new());
    driver.until(
        &mut j,
        viewport,
        deadline,
        "naturalMovieEOF/Summer prefix/preview5/fullSome",
        |r| {
            no_script_choice(r);
            if let Some(h) = game(r).unwrap().host() {
                let pc = h.runner.position().0;
                if PREVIEW.contains(&pc)
                    && matches!(
                        h.runner.pending_command(),
                        Some(Cmd::VOICEWAIT { status: 2 })
                    )
                {
                    assert_eq!(h.runner.native_read_state().unwrap().flags & 0x8000_0000, 0);
                    preview.borrow_mut().insert(pc);
                }
            }
            story_metadata(r, "染伝し編")
        },
    );
    assert!(
        !preview.borrow().is_empty(),
        "actual protectedpreview5 VOICEWAIT"
    );
    assert!(driver.movie_waits.contains(&0x2abb8c));
    assert!(!driver.movie_waits.contains(&0x2abbcc));
    assert_eq!(
        game(&j.runtime)
            .unwrap()
            .reference_story_variables()
            .0
            .get(8),
        1
    );
    same_manual(&rows(&j.runtime), &before);
    j.no_writes(Some(&initial_file));
    pure_frame(&mut j);
    driver.until(
        &mut j,
        viewport,
        deadline,
        "actual protected rolling ending",
        |r| {
            no_script_choice(r);
            pending(r, 0x5ed040)
        },
    );
    assert_eq!(
        game(&j.runtime)
            .unwrap()
            .host()
            .unwrap()
            .runner
            .native_read_state()
            .unwrap()
            .flags
            & 0x8000_0000,
        0
    );
    driver.until(
        &mut j,
        viewport,
        deadline,
        "actual final TIPS155 Continue",
        |r| tips_ready(r, 0x5ed0d2),
    );
    let (p, q) = game(&j.runtime).unwrap().reference_story_variables();
    assert_eq!(p.get(8), 3);
    assert!(!q.iter().any(|(s, _)| *s == 8));
    assert_eq!(p.get(24), 15);
    assert!(!q.iter().any(|(s, _)| *s == 24));
    for tip in [148, 149, 150, 151, 152, 153, 155] {
        assert!(acquired(j.runtime.globals().unwrap().unwrap().data, tip));
    }
    same_manual(&rows(&j.runtime), &before);
    j.no_writes(Some(&initial_file));
    pure_frame(&mut j);
    continue_tip(&mut j, viewport, deadline, 0x5ed0d2);
    driver.until(
        &mut j,
        viewport,
        deadline,
        "actual OTSUGET6 queued award",
        |r| {
            no_script_choice(r);
            pending(r, 0x5ed0f9) && game(r).unwrap().reference_story_notification() == Some((13, 2))
        },
    );
    let (p, q) = game(&j.runtime).unwrap().reference_story_variables();
    assert_eq!(p.get(8), 3);
    assert_eq!(p.get(24), 15);
    assert_eq!(q.iter().find(|(s, _)| *s == 24).map(|(_, v)| *v), Some(79));
    assert!(driver.exits.is_empty());
    same_manual(&rows(&j.runtime), &before);
    j.no_writes(Some(&initial_file));
    driver.until(
        &mut j,
        viewport,
        deadline,
        "actual all-six epilogue AUTOSAVE",
        |r| {
            no_script_choice(r);
            metadata(r, "興宮警察署事件調書")
        },
    );
    let (p, q) = game(&j.runtime).unwrap().reference_story_variables();
    assert_eq!(p.get(24), 79);
    assert!(!q.iter().any(|(s, _)| *s == 24));
    assert_eq!(p.get(0), 0);
    assert_eq!(p.get(1), 1, "epilogue final unlocks are still later");
    assert!(driver.exits.is_empty());
    pure_frame(&mut j);
    j.no_writes(Some(&initial_file));
    let exit = RefCell::new(None::<ExitBoundary>);
    driver.until(
        &mut j,
        viewport,
        deadline,
        "natural final EXIT5cb04/save/readyTitle",
        |r| {
            if let Some(g) = game(r) {
                if let Some(h) = g.host() {
                    if h.runner.position().0 == 0x5cb04
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
                            runner_commands: h.runner.completed_commands(),
                        });
                    }
                }
            }
            ready_menu(r, "top")
        },
    );
    let exit = exit
        .into_inner()
        .expect("actual epilogue EXIT retained save boundary");
    assert!(game(&j.runtime).is_none());
    assert_eq!(
        j.runtime.slot().unwrap().active_scene().unwrap(),
        ActiveScene::Title
    );
    assert_eq!(
        driver.exits.iter().map(|(pc, _)| *pc).collect::<Vec<_>>(),
        [0x5cb04]
    );
    for pc in [
        0x23b6c2, 0x23d4c3, 0x2abb0a, 0x2abb8c, 0x5ed040, 0x5ed044, 0x5ed0d2, 0x5ed0f9,
    ] {
        assert!(
            driver.pending_pcs.contains(&pc),
            "actual owned wait{pc:08x}"
        );
    }
    assert!(!driver.pending_pcs.contains(&0x1f997));
    assert!(!driver.pending_pcs.contains(&0x23e657));
    let after = rows(&j.runtime);
    same_manual(&exit.slots, &before);
    same_manual(&after, &before);
    assert_eq!(after.auto.valid, exit.slots.auto.valid);
    assert_eq!(
        after.auto.data, exit.slots.auto.data,
        "full101 actualEXIT transfer"
    );
    assert_eq!(exit.current.progress.active_index(), 0);
    assert_eq!(exit.current.progress.active_entry().scenario_id, 3218);
    assert!(exit.current.progress.active_entry().save_position > 0);
    assert_eq!(
        exit.current.progress.entries[1], initial_current.progress.entries[1],
        "fresh secondChart dormant current retained"
    );
    assert_eq!(identities(&j.runtime), shared);
    let globals = j.runtime.globals().unwrap().unwrap();
    assert!(globals.clock.ticks() > clock);
    let expected = [
        (0, 2),
        (1, 17),
        (2, 3),
        (3, 3),
        (4, 3),
        (5, 3),
        (6, 3),
        (7, 3),
        (8, 3),
        (24, 79),
        (41, 1),
        (42, 0),
        (43, 0),
        (46, 1),
    ];
    for (slot, v) in expected {
        assert_eq!(
            globals.data.persist_data.get(slot),
            v,
            "final committed{slot}"
        );
    }
    for tip in [148, 149, 150, 151, 152, 153, 155] {
        assert!(acquired(globals.data, tip));
    }
    for node in [1, 5, 26, 59, 60, 133, 147] {
        assert!(
            globals
                .data
                .save_vectors
                .extra_words_1
                .get(node / 32)
                .is_some_and(|w| w & (1 << (node % 32)) != 0),
            "actualChart{node}"
        );
    }
    let writes = j.storage.writes.lock().unwrap().clone();
    assert_eq!(
        writes.len(),
        1,
        "one actual epilogue flag1 save;no shutdown save"
    );
    assert_eq!(
        j.storage.paths.read(Source::Current).unwrap().as_deref(),
        Some(writes[0].as_slice())
    );
    let disk = SaveData::decode(&writes[0]).unwrap();
    assert_eq!(disk.manual_save_slots, saved.manual_save_slots);
    let mut projection = saved.clone();
    projection.auto_save_slot = Some(exit.slots.auto.data.save_data());
    assert_eq!(
        disk.auto_save_slot,
        SaveData::decode(&projection.encode().unwrap())
            .unwrap()
            .auto_save_slot,
        "wire projection separate from full101"
    );
    for (slot, v) in expected {
        assert_eq!(disk.persist_data.get(slot), v, "saved final{slot}");
    }
    for tip in [148, 149, 150, 151, 152, 153, 155] {
        assert!(acquired(&disk, tip));
    }
    assert!(j.runtime.failure().is_none());
    pure_frame(&mut j);
    println!(
        "Runtime actualearnedTarai->Chart5/Day3 originals0,1->committedDay4/SystemMenu4/Chart26->original3/Movie7naturalEOF/prefix/preview5/fullSome/rollingending/TIP155/Otsu6/all-sixepilogue/EXIT0005cb04 one save+readyTitle: elapsed{:.3}s,{}frames/{}driven callbacks;ADV phase completed counts{}+{}={} (two fresh runners, excludes pendingEXIT);full100manuals/101EXIT/shared/dormant/pure720p1080p;preview{:x?}",
        started.elapsed().as_secs_f64(),
        j.runtime.frames,
        driver.callbacks,
        source_commands,
        exit.runner_commands,
        source_commands + exit.runner_commands,
        preview.into_inner()
    );
}
