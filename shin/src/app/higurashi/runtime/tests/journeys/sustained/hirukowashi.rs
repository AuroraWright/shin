//! Genuine accepted Meakashi -> Category1/SNRSEL1 row3 -> full Hirukowashi.
//! Parent admission binds actual earned input/PASS; full live execution pending.
//! No VM, responses, media, policy, source data or clock mutation.
use super::*;
use crate::app::higurashi::{
    runtime_game_data::RuntimeGameData, runtime_saves::Slots,
    scenario_select::owner::Owner as PickerOwner,
};
use shin_core::format::scenario::instruction_elements::Register;
use std::cell::RefCell;

const MEAKASHI: &str = "app::higurashi::runtime::tests::journeys::sustained::meakashi::higurashi_runtime_sustained_earned_category_reaches_full_meakashi_exit_and_title";
struct EarnedPins {
    save_len: usize,
    save_fnv: u64,
    record_len: usize,
    record_fnv: u64,
}
// Exact actual accepted earned Meakashi output/PASS, independently recorded.
const ACCEPTED_MEAKASHI: Option<EarnedPins> = Some(EarnedPins {
    save_len: 17412,
    save_fnv: 0xedc0fd3733e0e905,
    record_len: 177454,
    record_fnv: 0xaa02bea9726b2c9c,
});

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3)
    })
}
fn earned() -> (SaveData, Vec<u8>) {
    let pins = ACCEPTED_MEAKASHI
        .as_ref()
        .expect("actual accepted Meakashi output/PASS has not been pinned; conditional draft only");
    let path = std::env::var("SHIN_HIGURASHI_EARNED_MEAKASHI")
        .expect("actual accepted cumulative Meakashi output required");
    let record = std::env::var("SHIN_HIGURASHI_EARNED_MEAKASHI_RECORD")
        .expect("exact original passing Meakashi record required");
    let proof = std::fs::read(record).unwrap();
    assert_eq!(proof.len(), pins.record_len);
    assert_eq!(fnv(&proof), pins.record_fnv);
    let proof: serde_json::Value = serde_json::from_slice(&proof).unwrap();
    assert_eq!(proof["exit_code"].as_i64(), Some(0));
    assert_eq!(proof["command"][1].as_str(), Some(MEAKASHI));
    assert!(
        proof["changed_pinned_sources"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(proof["acceptance_pending"].as_bool(), Some(false));
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(bytes.len(), pins.save_len);
    assert_eq!(fnv(&bytes), pins.save_fnv);
    let saved = SaveData::decode(&bytes).unwrap();
    assert_eq!(saved.encode().unwrap(), bytes);
    assert!(
        saved
            .save_vectors
            .extra_words_1
            .get(204 / 32)
            .is_none_or(|word| word & (1 << (204 % 32)) == 0),
        "actual earned input must not already have Hirukowashi good-ending Chart204"
    );
    for (slot, value) in [
        (0, 2),
        (1, 19),
        (2, 3),
        (3, 3),
        (4, 3),
        (5, 3),
        (6, 3),
        (7, 3),
        (8, 3),
        (9, 3),
        (10, 3),
        (11, 1),
        (12, 1),
        (24, 95),
    ] {
        assert_eq!(saved.persist_data.get(slot), value, "actual input{slot}");
    }
    assert_ne!(
        saved.persist_data.get(1) & 2,
        0,
        "already-earned prologue bypass"
    );
    assert!(saved.auto_save_slot.is_some());
    let sentinel = saved.manual_save_slots[99].as_ref().unwrap();
    assert!(sentinel.primary_entry.is_some());
    assert!(!sentinel.trailing_values.is_empty() && !sentinel.byte_variables.is_empty());
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
    for (i, (a, b)) in actual.manual.iter().zip(&before.manual).enumerate() {
        assert_eq!(a.valid, b.valid, "manual{i} validity");
        assert_eq!(a.data, b.data, "manual{i} full two-record payload");
    }
}
fn same_rows(actual: &Slots, before: &Slots) {
    same_manual(actual, before);
    assert_eq!(actual.auto.valid, before.auto.valid);
    assert_eq!(actual.auto.data, before.auto.data, "full101 owner transfer");
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
fn metadata(runtime: &Runtime, level: usize, label: &str) -> bool {
    game(runtime).is_some_and(|g| {
        g.host().is_some_and(|h| {
            h.runner.native_save_metadata().is_some_and(|m| {
                m.current()[level].as_ref() == label && m.saved()[level].as_ref() == label
            })
        })
    })
}
fn chart_acquired(runtime: &Runtime, node: usize) -> bool {
    runtime
        .globals()
        .unwrap()
        .unwrap()
        .data
        .save_vectors
        .extra_words_1
        .get(node / 32)
        .is_some_and(|word| word & (1 << (node % 32)) != 0)
}
fn script_choice_pending(runtime: &Runtime, pc: u32) -> bool {
    let g = game(runtime).unwrap_or_else(|| {
        panic!(
            "left ADV before required SELECT{pc:08x}: {}",
            Driver::describe(runtime)
        )
    });
    let Some(h) = g.host() else {
        return false;
    };
    match h.runner.pending_command() {
        Some(Cmd::SELECT { .. }) => assert_eq!(
            h.runner.position().0,
            pc,
            "unexpected SELECT before required SELECT{pc:08x}: {}",
            Driver::describe(runtime)
        ),
        Some(Cmd::CHARSELECT { .. }) => panic!(
            "unexpected CHARSELECT before required SELECT{pc:08x}: {}",
            Driver::describe(runtime)
        ),
        _ => return false,
    }
    true
}
fn choice_ready(runtime: &Runtime, pc: u32) -> bool {
    script_choice_pending(runtime, pc)
        && game(runtime).is_some_and(|g| {
            g.input_permission && g.reference_story_choice().is_some_and(|(p, _, _)| p == 2)
        })
}
fn no_script_choice(runtime: &Runtime) {
    let g =
        game(runtime).unwrap_or_else(|| panic!("left required ADV: {}", Driver::describe(runtime)));
    assert!(
        !metadata(runtime, 1, "BADEND (敗退)"),
        "wrong Hirukowashi ending: {}",
        Driver::describe(runtime)
    );
    if let Some(h) = g.host() {
        assert!(
            !matches!(
                h.runner.pending_command(),
                Some(Cmd::SELECT { .. } | Cmd::CHARSELECT { .. } | Cmd::OTSUGET { .. })
            ),
            "unexpected route-critical choice: {}",
            Driver::describe(runtime)
        );
    }
}
fn choose_original(
    j: &mut Journey,
    viewport: InputViewport,
    deadline: Instant,
    pc: u32,
    wanted_original: usize,
) {
    for _ in 0..8 {
        assert!(choice_ready(&j.runtime, pc));
        let (_, selected, choices) = game(&j.runtime).unwrap().reference_story_choice().unwrap();
        let wanted = choices
            .iter()
            .position(|(id, _)| *id == wanted_original)
            .expect("required native original ID must be visible");
        if selected == wanted as i32 {
            j.tap(KeyCode::Enter, viewport);
            j.until(viewport, deadline, "actual SELECT close/callback", |r| {
                !pending(r, pc)
            });
            return;
        }
        j.tap(KeyCode::ArrowDown, viewport);
    }
    panic!("SELECT{pc:08x} original{wanted_original} not selected");
}

fn tips_ready(runtime: &Runtime, pc: u32) -> bool {
    no_script_choice(runtime);
    pending(runtime, pc)
        && game(runtime)
            .unwrap()
            .reference_story_choice()
            .is_some_and(|(p, _, _)| p == 2)
}
fn continue_tip(j: &mut Journey, viewport: InputViewport, deadline: Instant, pc: u32) {
    for _ in 0..8 {
        assert!(tips_ready(&j.runtime, pc));
        let (_, selected, choices) = game(&j.runtime).unwrap().reference_story_choice().unwrap();
        let wanted = choices
            .iter()
            .position(|(id, _)| *id == 0)
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
fn picker(runtime: &Runtime) -> Option<&PickerOwner> {
    Some(runtime.slot()?.scenario_select.as_ref()?.reference_owner())
}
fn picker_ready(runtime: &Runtime) -> bool {
    picker(runtime).is_some_and(|p| {
        let s = p.reference_state();
        let m = p.reference_model();
        s.0 && s.2 && s.3.is_none() && m.ready && !m.closing
    })
}
fn register(runtime: &Runtime, index: u16) -> i32 {
    game(runtime)
        .unwrap()
        .host()
        .unwrap()
        .runner
        .reference_register(Register::from_regular_register(index))
        .unwrap()
}
fn acquired(saved: &SaveData, tip: usize) -> bool {
    let bit = tip * 2;
    saved
        .save_vectors
        .extra_words_0
        .get(bit / 32)
        .is_some_and(|w| w & (1 << (bit % 32)) != 0)
}
fn pin(runtime: &Runtime, at: usize, hex: &str) {
    let b = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        &runtime.globals().unwrap().unwrap().common.scenario.raw()[at..at + b.len()],
        b.as_slice(),
        "authored bytes{at:08x}"
    );
}
struct ExitBoundary {
    slots: Slots,
    current: RuntimeGameData,
    commands: u64,
}

#[test]
#[ignore = "actual accepted Meakashi save/PASS required;ordinary Category1/SNRSEL1original3/scenario203/Movie10/baseball originals0,0,1,0/protectedBGM97credits/TIP158/EXIT1f997 save+Title;real assets/offscreenMetal/exactLoopbackAudio;temporary Paths only;two-hour observation allowance"]
fn higurashi_runtime_sustained_earned_category_reaches_full_hirukowashi_exit_and_title() {
    let (saved, initial_file) = earned();
    let persistent32 = saved.persist_data.get(32);
    let started = Instant::now();
    let deadline = started + Duration::from_secs(2 * 60 * 60);
    let viewport = viewport((1920, 1080));
    let mut j = Journey::new("sustained-earned-hirukowashi", &saved);
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
    j.tap(KeyCode::ArrowLeft, viewport);
    assert_eq!(
        j.runtime
            .slot()
            .unwrap()
            .title
            .as_ref()
            .unwrap()
            .reference_top_selection(),
        Some(0)
    );
    j.tap(KeyCode::Enter, viewport);
    j.until(viewport, deadline, "actual story Category0", |r| {
        ready_menu(r, "cat")
    });
    assert_eq!(
        j.runtime
            .slot()
            .unwrap()
            .title
            .as_ref()
            .unwrap()
            .reference_category(),
        Some((0, 0, 2))
    );
    j.tap(KeyCode::ArrowDown, viewport);
    assert_eq!(
        j.runtime
            .slot()
            .unwrap()
            .title
            .as_ref()
            .unwrap()
            .reference_category(),
        Some((0, 1, 2))
    );
    j.tap(KeyCode::Enter, viewport);
    j.until(
        viewport,
        deadline,
        "ordinary Category1 Gameplay factory",
        |r| game(r).is_some(),
    );
    // Observe complete factory ownership before waiting for the actual VM.
    assert_eq!(game(&j.runtime).unwrap().flags, 0);
    j.until(viewport, deadline, "short scenario1 VM attachment", |r| {
        game(r).is_some_and(|g| g.host().is_some_and(|h| h.runner.native_vm_attached()))
    });
    let mut driver = Driver::new();
    driver.until(
        &mut j,
        viewport,
        deadline,
        "actual SNRSEL1 prologue bypass",
        |r| {
            if r.slot().is_some_and(|s| s.pending_scenario_select()) {
                return true;
            }
            no_script_choice(r);
            false
        },
    );
    let g = game(&j.runtime).expect("queued picker retains source until dispatch");
    let source = g.runtime_game_data().unwrap();
    assert_eq!(source.progress.active_index(), 0);
    assert_eq!(source.progress.active_entry().scenario_id, 1);
    assert_eq!(source.progress.extra_word, 1);
    assert_eq!(g.host().unwrap().runner.position().0, 0x1f7d1);
    assert!(matches!(
        g.host().unwrap().runner.pending_command(),
        Some(Cmd::SNRSEL { argument: 1 })
    ));
    let picker_commands = g.host().unwrap().runner.completed_commands();
    let (p, q) = g.reference_story_variables();
    for (slot, value) in [(0, 2), (1, 19), (9, 3), (10, 3), (11, 1), (12, 1), (24, 95)] {
        assert_eq!(p.get(slot), value, "source committed{slot}");
        assert!(!q.iter().any(|(s, _)| *s == slot as u32));
    }
    assert!(
        driver.movie_waits.is_empty(),
        "already-earned1=19 bypasses prologueMovie8"
    );
    assert_eq!(HIRUKOWASHI_SPANS.len(), 23);
    assert_eq!(
        HIRUKOWASHI_SPANS
            .iter()
            .map(|(_, hex)| hex.len() / 2)
            .sum::<usize>(),
        1170
    );
    for (at, hex) in HIRUKOWASHI_SPANS {
        pin(&j.runtime, *at, hex);
    }
    let at_picker = rows(&j.runtime);
    same_manual(&at_picker, &before);
    j.no_writes(Some(&initial_file));
    j.until(
        viewport,
        deadline,
        "real kind1 picker First/Input",
        picker_ready,
    );
    assert!(game(&j.runtime).is_none());
    let m = picker(&j.runtime).unwrap().reference_model();
    assert_eq!((m.kind, m.selected, m.count), (1, 0, 6));
    assert_ne!(
        m.mask & (1 << 3),
        0,
        "genuine entered11 enables Hirukowashi original3"
    );
    assert_eq!(m.data, source, "full SNRSEL source payload");
    same_rows(&rows(&j.runtime), &at_picker);
    assert_eq!(identities(&j.runtime), shared);
    pure_frame(&mut j);
    // Native b60bc/b61e8: Left maps original0 to1; Down maps1 to3.
    j.tap(KeyCode::ArrowLeft, viewport);
    assert!(picker_ready(&j.runtime));
    assert_eq!(picker(&j.runtime).unwrap().reference_model().selected, 1);
    j.tap(KeyCode::ArrowDown, viewport);
    assert!(picker_ready(&j.runtime));
    assert_eq!(picker(&j.runtime).unwrap().reference_model().selected, 3);
    j.tap(KeyCode::Enter, viewport);
    j.until(viewport, deadline, "actual selected203 factory", |r| {
        r.slot()
            .is_some_and(|s| s.scenario_select.is_some() && s.pending_scene_factory())
    });
    let returned = picker(&j.runtime).unwrap().reference_model().data.clone();
    let mut expected = source.clone();
    expected.progress.set_active_index(0).unwrap();
    expected.progress.entries[0].scenario_id = 203;
    expected.progress.entries[0].random_seed = returned.progress.entries[0].random_seed;
    expected.progress.entries[0].save_position = 0;
    expected.progress.entries[0].replay_target = 0;
    assert_eq!(
        returned, expected,
        "native b7310 fullsource/date/extra/dormant/responses/tails/bytes except assigned fields"
    );
    same_rows(&rows(&j.runtime), &at_picker);
    j.no_writes(Some(&initial_file));
    pure_frame(&mut j);
    assert!(matches!(
        j.step(&RawInputState::new(), viewport),
        Frame::Advanced(_)
    ));
    let g = game(&j.runtime).expect("actual native picker factory");
    assert!(picker(&j.runtime).is_none());
    assert_eq!(g.flags, 0);
    assert_eq!(
        g.runtime_game_data().unwrap(),
        returned,
        "full native picker return before VM execution"
    );
    assert_eq!(identities(&j.runtime), shared);
    j.until(viewport, deadline, "Hirukowashi VM attachment", |r| {
        game(r).is_some_and(|g| g.host().is_some_and(|h| h.runner.native_vm_attached()))
    });
    driver.until(
        &mut j,
        viewport,
        deadline,
        "actual first-clear protected Movie10",
        |r| {
            no_script_choice(r);
            pending(r, 0x7195a2)
        },
    );
    assert!(metadata(&j.runtime, 0, "昼壊し編") && metadata(&j.runtime, 1, "#1 宝探し"));
    assert!(chart_acquired(&j.runtime, 191));
    let initial_current = game(&j.runtime).unwrap().runtime_game_data().unwrap();
    assert_eq!(initial_current.progress.active_entry().scenario_id, 203);
    assert_eq!(initial_current.progress.extra_word, 1);
    assert_eq!(
        register(&j.runtime, 22),
        0,
        "ordinary source bypasses NCSELECT"
    );
    assert_eq!(
        register(&j.runtime, 25),
        0,
        "authored baseball score initializer"
    );
    assert!(matches!(
        game(&j.runtime)
            .unwrap()
            .host()
            .unwrap()
            .runner
            .pending_command(),
        Some(Cmd::MOVIEWAIT { .. })
    ));
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
    same_manual(&rows(&j.runtime), &before);
    j.no_writes(Some(&initial_file));
    pure_frame(&mut j);
    let plan = [(0x7459af, 0), (0x7468e2, 0), (0x747e53, 1), (0x74921e, 0)];
    let mut chosen = Vec::new();
    for (pc, original) in plan {
        driver.until(
            &mut j,
            viewport,
            deadline,
            "exact authored baseball SELECT",
            |r| choice_ready(r, pc),
        );
        if pc == 0x74921e {
            assert_eq!(
                register(&j.runtime, 25),
                1,
                "0,0,1 leaves score1 and exposes actual fourth choice"
            );
        }
        pure_frame(&mut j);
        choose_original(&mut j, viewport, deadline, pc, original);
        chosen.push((pc, original));
    }
    assert_eq!(chosen, plan);
    driver.until(
        &mut j,
        viewport,
        deadline,
        "actual Satoko win Chapter12",
        |r| {
            no_script_choice(r);
            metadata(r, 1, "#12 レナの告白？") && chart_acquired(r, 202)
        },
    );
    assert_eq!(register(&j.runtime, 25), 1);
    driver.until(
        &mut j,
        viewport,
        deadline,
        "actual good END and Chart204",
        |r| {
            no_script_choice(r);
            metadata(r, 1, "END (なにがなんだか)") && chart_acquired(r, 204)
        },
    );
    assert!(
        !chart_acquired(&j.runtime, 203),
        "BADEND Chart must not be earned"
    );
    let (p, q) = game(&j.runtime).unwrap().reference_story_variables();
    assert_eq!(p.get(11), 1, "clear not yet queued before credits");
    assert_eq!(p.get(24), 95);
    assert!(!q.iter().any(|(s, _)| matches!(*s, 11 | 24)));
    same_manual(&rows(&j.runtime), &before);
    j.no_writes(Some(&initial_file));
    pure_frame(&mut j);
    let bgm_wait_observed = RefCell::new(false);
    driver.until(
        &mut j,
        viewport,
        deadline,
        "protected BGM97 credits layerwait",
        |r| {
            no_script_choice(r);
            if pending(r, 0x759162) {
                let h = game(r).unwrap().host().unwrap();
                assert!(matches!(
                    h.runner.pending_command(),
                    Some(Cmd::BGMSYNC { time: 180 })
                ));
                assert_eq!(h.runner.native_read_state().unwrap().flags & 0x8000_0000, 0);
                *bgm_wait_observed.borrow_mut() = true;
            }
            pending(r, 0x759195)
        },
    );
    assert!(matches!(
        game(&j.runtime)
            .unwrap()
            .host()
            .unwrap()
            .runner
            .pending_command(),
        Some(Cmd::LAYERWAIT { .. })
    ));
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
    driver.until(&mut j, viewport, deadline, "protected WAIT0,300", |r| {
        no_script_choice(r);
        pending(r, 0x759199)
    });
    let h = game(&j.runtime).unwrap().host().unwrap();
    assert!(matches!(
        h.runner.pending_command(),
        Some(Cmd::WAIT {
            allow_interrupt: 0,
            amount: 300
        })
    ));
    assert_eq!(h.runner.native_read_state().unwrap().flags & 0x8000_0000, 0);
    pure_frame(&mut j);
    driver.until(
        &mut j,
        viewport,
        deadline,
        "actual final TIP158 Continue",
        |r| tips_ready(r, 0x759225),
    );
    let (p, q) = game(&j.runtime).unwrap().reference_story_variables();
    assert_eq!(
        p.get(11),
        3,
        "TIPS1185c commits Hirukowashi clear before UI"
    );
    assert_eq!(
        p.get(166),
        1,
        "first-clear Movie10 assignment committed by real autosaves"
    );
    assert_eq!(p.get(24), 95, "no OTSUGET/24 award in Hirukowashi");
    assert_eq!(
        p.get(12),
        1,
        "next ordinary Kageboushi still entered,not clear"
    );
    assert!(!q.iter().any(|(s, _)| matches!(*s, 11 | 24 | 166)));
    assert!(acquired(j.runtime.globals().unwrap().unwrap().data, 158));
    assert!(driver.exits.is_empty());
    same_manual(&rows(&j.runtime), &before);
    j.no_writes(Some(&initial_file));
    pure_frame(&mut j);
    continue_tip(&mut j, viewport, deadline, 0x759225);
    let exit = RefCell::new(None::<ExitBoundary>);
    driver.until(
        &mut j,
        viewport,
        deadline,
        "natural Hirukowashi EXIT save and readyTitle",
        |r| {
            if let Some(g) = game(r) {
                no_script_choice(r);
                if let Some(h) = g.host() {
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
                            commands: h.runner.completed_commands(),
                        });
                    }
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
    assert!(driver.movie_waits.contains(&0x7195a2));
    assert!(
        !driver.movie_waits.contains(&0x7195bc),
        "not a replay alternateMovie11"
    );
    for pc in [
        0x7459af, 0x7468e2, 0x747e53, 0x74921e, 0x759195, 0x759199, 0x759225,
    ] {
        assert!(
            driver.pending_pcs.contains(&pc),
            "actual owned wait{pc:08x}"
        );
    }
    for pc in [0x7491ba, 0x1f99a, 0x5cb12] {
        assert!(!driver.pending_pcs.contains(&pc), "wrong route{pc:08x}");
    }
    let after = rows(&j.runtime);
    same_manual(&exit.slots, &before);
    same_rows(&after, &exit.slots);
    assert_eq!(exit.current.progress.active_index(), 0);
    assert_eq!(exit.current.progress.extra_word, 1);
    assert_eq!(exit.current.progress.active_entry().scenario_id, 203);
    assert!(exit.current.progress.active_entry().save_position > 0);
    assert_eq!(
        exit.current.progress.entries[1], initial_current.progress.entries[1],
        "dormant current retained"
    );
    assert_eq!(identities(&j.runtime), shared);
    let globals = j.runtime.globals().unwrap().unwrap();
    assert!(globals.clock.ticks() > clock);
    let expected = [
        (0, 2),
        (1, 19),
        (2, 3),
        (3, 3),
        (4, 3),
        (5, 3),
        (6, 3),
        (7, 3),
        (8, 3),
        (9, 3),
        (10, 3),
        (11, 3),
        (12, 1),
        (24, 95),
        (32, persistent32),
        (166, 1),
    ];
    for (s, v) in expected {
        assert_eq!(globals.data.persist_data.get(s), v, "final committed{s}");
    }
    assert!(acquired(globals.data, 158));
    assert!(chart_acquired(&j.runtime, 204) && !chart_acquired(&j.runtime, 203));
    let writes = j.storage.writes.lock().unwrap().clone();
    assert_eq!(
        writes.len(),
        1,
        "one natural flag1 EXIT save,no shutdown save"
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
    for (s, v) in expected {
        assert_eq!(disk.persist_data.get(s), v, "saved final{s}");
    }
    assert!(acquired(&disk, 158));
    assert!(j.runtime.failure().is_none());
    pure_frame(&mut j);
    println!(
        "Runtime actualearnedMeakashi->Category1/shortscenario1/SNRSEL1row3/scenario203/Movie10/baseball0,0,1,0/GoodEND/Chart204/protectedBGM97credits/TIP158/EXIT1f997 one save+readyTitle:elapsed{:.3}s,{}frames/{}driven callbacks;phase completed{}+{}={} (two fresh runners, excludes pending EXIT);full100manual/101EXIT/shared/fullsource/dormant/pure720p1080p;BGMSYNCpendingobserved{}",
        started.elapsed().as_secs_f64(),
        j.runtime.frames,
        driver.callbacks,
        picker_commands,
        exit.commands,
        picker_commands + exit.commands,
        bgm_wait_observed.into_inner()
    );
}

// Exact updated original bytes from the separately frozen static audit.
const HIRUKOWASHI_SPANS: &[(usize, &str)] = &[
    // Ordinary category dispatch
    (
        0x0001e2bd,
        "4ab00d8ff70100c0f70100e0f7010000f8010029f80100581619005617190024181900f21819009627190060281900f328190090291900",
    ),
    // Kind1 scenario200 through204 dispatch
    (
        0x0001e345,
        "4600b080c8ccf701004600b080c907d15e004600b080caf96664004600b080cb6c9471004600b080cc31927500",
    ),
    // Already-earned scenario1 prologue bypass and SNRSEL1
    (0x0001f7c0, "810100014606b102d1f701004808cb0500d401000000"),
    // Actual Hirukowashi entry/autosave/Chart191/score0
    (
        0x0071946c,
        "a00009928b89f382b595d200a0010a23312095f3925482b500a1d3000180bf4100190000",
    ),
    // First-clear Movie10 vs replay alternation Movie11
    (
        0x0071957b,
        "850000008102000b4686b2029895710081020080a64606b201b2957100a200c10d0700030a82eec80d02c20d00a38280a60147cc957100a200c10d0700030b82eec80d02c20d00a38280a60047cc957100",
    ),
    // First baseball SELECT and result1 target
    (
        0x007459af,
        "8d4c0025000000900fff0b91ac82a28b8582f08163002182dc82c182b782ae905582e894b282ad0082e682ad8ca982c490548f6482c900004600b001a45e7400",
    ),
    // First result0 join and result1 score increment
    (0x00745e9f, "47636374004102190001"),
    // First result1 join
    (0x0074635e, "4763637400"),
    // Second baseball SELECT and result1 target
    (
        0x007468e2,
        "8d4e0026000000900fff1192e182df82cc905e82f1928682f08163002197cd88ea9474905582e882ca82ad00837e815b836782f0905382aa82af82e900004600b0016a6c7400",
    ),
    // Second result0 join and result1 score increment
    (0x00746c65, "47c56f74004102190001"),
    // Second result1 join
    (0x00746fc0, "47c56f7400"),
    // Third baseball SELECT and result1 target
    (
        0x00747e53,
        "8d500027000000900fff118a4f8a7082c9978882bd8b8582f08163002393af82b68352815b8358915f82a282c58149008384837d82f092a382c182c4814900004600b00172867400",
    ),
    // Third result0 BAD route jump and result1 score increment
    (0x0074866d, "4763f774004102190001"),
    // Normal v22 bypasses NCSELECT and actual score/fourth SELECT routing
    (
        0x0074914a,
        "4600c016001592740086872301017f5f00407240733235352e8366836f8362834f9770914991f08e8840728daa90ab837c834383938367825188c88fe382c590b38b4b838bb08367cd8bad90a78357838383938376a4825196a2969ec98fea8d87ca914991f08e8894ad90b6a14065008d000000000000900fff094e4353454c45435400368daa90ab837c8343839383678181825188c88fe3008daa90ab837c8343839383678181825196a2969e0083748389834f8c7091b100004600b000509274004600b0011e9274004602c01902509274008d520028000000900fff0b8db993738e7182c981630013944382b982e9008ea995aa82c591c582c200004600b00163f77400",
    ),
    // Fourth original0 Satoko success block jump
    (0x0074f75e, "47c51a7500"),
    // Bad END metadata/autosave/Chart203
    (0x0074f974, "a0010e424144454e442028947391de2900a1d3000180cb"),
    // Bad END actual no-save EXIT return
    (0x00751ac0, "479af90100"),
    // Success join/Chapter12/autosave/Chart202
    (
        0x0075379b,
        "47a0377500a0011123313220838c836982cc8d909492814800a1d3000180ca",
    ),
    // Good END metadata/autosave/Chart204
    (
        0x007578dd,
        "47e2787500a00115454e44202882c882c982aa82c882f182be82a92900a1d3000180cc",
    ),
    // Protected credits BGM97/BGMSYNC180/scroll/waits
    (
        0x0075912d,
        "a200c502c10202000182d9c3021501c014c3020c0183e8c3020d0183e8c30201019feaa68e000000008f908061000183e8830080b49480b442020000816800901ba8030090155a0400903d89030090271004ff41830100901ba8b2c30201038e98b1c3020106b202c40201018300812cc502c202008a00c505c506c507c508c509c50ac50bc50cc20500c20600c20700c20800c20900c20a00c20b00c20c00c501c10101001f90ffff8d808d30850082d0c301090183e8c3010600c3010700c30108004d01b94d01ba410009000241000a0080b44837fa01004e010a004e010900973c913c83008078a3",
    ),
    // Clear11/Trophy18/TIPS158 commit and return
    (
        0x00759217,
        "810c000b41080c0002820bbcb012d001809e8300014780f80100",
    ),
    // Second-collection clear gates and epilogue call
    (
        0x0001f880,
        "810200094686b202b5f801008102000a4686b202b5f801008102000b4686b202b5f801008102000c4686b202b5f8010048a6d905004797f90100",
    ),
    // Actual saved and unsaved EXIT endpoints
    (0x0001f997, "000001000000"),
];
