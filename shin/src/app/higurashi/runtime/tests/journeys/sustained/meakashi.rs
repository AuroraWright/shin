//! Earned Hima -> ordinary Category1/SNRSEL1 row2 -> full Meakashi.
//! Actual accepted input/PASS are pinned; no VM, responses, media or clock mutation.
use super::*;
use crate::app::higurashi::{
    runtime_game_data::RuntimeGameData, runtime_saves::Slots,
    scenario_select::owner::Owner as PickerOwner,
};
use shin_core::format::scenario::instruction_elements::Register;
use std::cell::RefCell;

const HIMA: &str = "app::higurashi::runtime::tests::journeys::sustained::himatsubushi::higurashi_runtime_sustained_earned_category_reaches_full_himatsubushi_exit_and_title";
struct EarnedPins {
    save_len: usize,
    save_fnv: u64,
    record_len: usize,
    record_fnv: u64,
}
// Exact actual accepted Hima output/PASS; no completion flags are seeded.
const ACCEPTED_HIMA: Option<EarnedPins> = Some(EarnedPins {
    save_len: 16388,
    save_fnv: 0xf4f65db2cc0fd5a5,
    record_len: 178246,
    record_fnv: 0xae8da3e4db536217,
});
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3)
    })
}
fn earned() -> (SaveData, Vec<u8>) {
    let pins = ACCEPTED_HIMA
        .as_ref()
        .expect("actual accepted Hima output/PASS has not been pinned; conditional draft only");
    let path = std::env::var("SHIN_HIGURASHI_EARNED_HIMATSUBUSHI")
        .expect("actual accepted cumulative Hima output required");
    let record = std::env::var("SHIN_HIGURASHI_EARNED_HIMATSUBUSHI_RECORD")
        .expect("exact original passing Hima record required");
    let proof = std::fs::read(record).unwrap();
    assert_eq!(proof.len(), pins.record_len);
    assert_eq!(fnv(&proof), pins.record_fnv);
    let proof: serde_json::Value = serde_json::from_slice(&proof).unwrap();
    assert_eq!(proof["exit_code"].as_i64(), Some(0));
    assert_eq!(proof["command"][1].as_str(), Some(HIMA));
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
        saved.save_vectors.extra_words_1.get(189 / 32)
            .is_none_or(|word| word & (1 << (189 % 32)) == 0),
        "actual earned input must not already have Meakashi good-ending Chart189"
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
        (10, 1),
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
        !metadata(runtime, 1, "BADEND"),
        "wrong Meakashi ending: {}",
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
fn choose_one(j: &mut Journey, viewport: InputViewport, deadline: Instant, pc: u32) {
    for _ in 0..8 {
        assert!(choice_ready(&j.runtime, pc));
        let (_, selected, choices) = game(&j.runtime).unwrap().reference_story_choice().unwrap();
        let wanted = choices
            .iter()
            .position(|(id, _)| *id == 1)
            .expect("required native original1 must be visible");
        if selected == wanted as i32 {
            j.tap(KeyCode::Enter, viewport);
            j.until(viewport, deadline, "actual SELECT close/callback", |r| {
                !pending(r, pc)
            });
            return;
        }
        j.tap(KeyCode::ArrowDown, viewport);
    }
    panic!("SELECT{pc:08x} original1 not selected");
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
#[ignore = "actual accepted Hima output+PASS required;ordinary Category1/SNRSEL1row2/scenario202/actualpersist32gatedoriginal1/GoodEND/TIP111/protectedBGM98credits/TIP112113114/EXIT1f997 save+Title;realassets/offscreenMetal/audio output;temporaryPaths only;twohour observation allowance"]
fn higurashi_runtime_sustained_earned_category_reaches_full_meakashi_exit_and_title() {
    let (saved, initial_file) = earned();
    let persistent32 = saved.persist_data.get(32);
    let started = Instant::now();
    let deadline = started + Duration::from_secs(2 * 60 * 60);
    let viewport = viewport((1920, 1080));
    let mut j = Journey::new("sustained-earned-meakashi", &saved);
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
        "ordinary Category1 creates scenario1 Gameplay",
        |r| game(r).is_some(),
    );
    // A published Gameplay can retain its Host before the native VM exists.
    // Driver logs position(), so let the actual First attach it before logging.
    j.until(
        viewport,
        deadline,
        "short scenario1 native VM attachment",
        |r| game(r).is_some_and(|g| g.host().is_some_and(|h| h.runner.native_vm_attached())),
    );
    let mut driver = Driver::new();
    driver.until(
        &mut j,
        viewport,
        deadline,
        "already-earned prologue bypass/actual SNRSEL1 factory",
        |r| {
            if r.slot().is_some_and(|s| s.pending_scenario_select()) {
                return true;
            }
            no_script_choice(r);
            false
        },
    );
    let g = game(&j.runtime).expect("queued picker retains ordinary ADV until dispatch");
    assert_eq!(g.flags, 0);
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
    assert_eq!(p.get(1), 19);
    assert_eq!(p.get(9), 3);
    assert_eq!(p.get(10), 1);
    assert_eq!(p.get(32), persistent32);
    assert!(!q.iter().any(|(s, _)| matches!(*s, 1 | 9 | 10 | 32)));
    assert!(
        driver.movie_waits.is_empty(),
        "ordinary1=19 bypasses already-earned Movie8"
    );
    assert_eq!(MEAKASHI_SPANS.len(), 22);
    assert_eq!(
        MEAKASHI_SPANS
            .iter()
            .map(|(_, hex)| hex.len() / 2)
            .sum::<usize>(),
        512,
        "all frozen literal route spans are present"
    );
    for (at, hex) in MEAKASHI_SPANS {
        pin(&j.runtime, *at, hex);
    }
    let at_picker = rows(&j.runtime);
    same_manual(&at_picker, &before);
    j.no_writes(Some(&initial_file));
    j.until(
        viewport,
        deadline,
        "actual kind1 picker First/Input",
        picker_ready,
    );
    assert!(game(&j.runtime).is_none());
    let m = picker(&j.runtime).unwrap().reference_model();
    assert_eq!((m.kind, m.selected, m.count), (1, 0, 6));
    assert_ne!(
        m.mask & (1 << 2),
        0,
        "actual Hima entered10 enables Meakashi row2"
    );
    assert_eq!(m.data, source, "full SNRSEL source payload retained");
    same_rows(&rows(&j.runtime), &at_picker);
    assert_eq!(identities(&j.runtime), shared);
    j.no_writes(Some(&initial_file));
    pure_frame(&mut j);
    // Native geometry: Down0->2. Left0->1 repeats Hima; A at0 is prologue200.
    j.tap(KeyCode::ArrowDown, viewport);
    assert!(picker_ready(&j.runtime));
    assert_eq!(picker(&j.runtime).unwrap().reference_model().selected, 2);
    j.tap(KeyCode::Enter, viewport);
    j.until(
        viewport,
        deadline,
        "actual picker scenario202 deferred factory",
        |r| {
            r.slot()
                .is_some_and(|s| s.scenario_select.is_some() && s.pending_scene_factory())
        },
    );
    let returned = picker(&j.runtime).unwrap().reference_model().data.clone();
    let mut expected = source.clone();
    expected.progress.set_active_index(0).unwrap();
    expected.progress.entries[0].scenario_id = 202;
    expected.progress.entries[0].random_seed = returned.progress.entries[0].random_seed;
    expected.progress.entries[0].save_position = 0;
    expected.progress.entries[0].replay_target = 0;
    assert_eq!(
        returned, expected,
        "native b7310 retains date/extra/bothresponses/dormant/tails/bytes"
    );
    same_rows(&rows(&j.runtime), &at_picker);
    j.no_writes(Some(&initial_file));
    pure_frame(&mut j);
    assert!(matches!(
        j.step(&RawInputState::new(), viewport),
        Frame::Advanced(_)
    ));
    let g = game(&j.runtime).expect("actual picker ADV factory created Gameplay");
    assert!(picker(&j.runtime).is_none());
    assert_eq!(g.flags, 0);
    assert_eq!(
        g.runtime_game_data().unwrap(),
        returned,
        "full native picker return before VM execution"
    );
    assert_eq!(identities(&j.runtime), shared);
    // Keep the complete factory payload check above before any VM execution;
    // only then wait for the real Host/First to attach the new native VM.
    j.until(
        viewport,
        deadline,
        "full Meakashi native VM attachment",
        |r| game(r).is_some_and(|g| g.host().is_some_and(|h| h.runner.native_vm_attached())),
    );
    driver.until(
        &mut j,
        viewport,
        deadline,
        "actual full Meakashi AUTOSAVE",
        |r| {
            no_script_choice(r);
            metadata(r, 0, "目明し編") && metadata(r, 1, "#1 雛見沢帰還")
        },
    );
    let initial_current = game(&j.runtime).unwrap().runtime_game_data().unwrap();
    assert_eq!(initial_current.progress.active_entry().scenario_id, 202);
    assert_eq!(initial_current.progress.extra_word, 1);
    assert_eq!(
        register(&j.runtime, 22),
        0,
        "ordinary v22 skips debug NCSELECTs"
    );
    assert_eq!(
        game(&j.runtime)
            .unwrap()
            .reference_story_variables()
            .0
            .get(32),
        persistent32
    );
    same_manual(&rows(&j.runtime), &before);
    j.no_writes(Some(&initial_file));
    pure_frame(&mut j);
    if persistent32 != 0 {
        driver.until(
            &mut j,
            viewport,
            deadline,
            "native persist32-gated Shion SELECT",
            |r| choice_ready(r, 0x707325),
        );
        pure_frame(&mut j);
        choose_one(&mut j, viewport, deadline, 0x707325);
    }
    // Persist32==0 jumps straight to this good branch without any SELECT.
    // Its actual TIP111 and GoodEND are required; no invisible prompt is made.
    driver.until(
        &mut j,
        viewport,
        deadline,
        "actual good-branch TIPS111 Continue",
        |r| tips_ready(r, 0x70fc1e),
    );
    assert_eq!(
        game(&j.runtime)
            .unwrap()
            .reference_story_variables()
            .0
            .get(32),
        persistent32
    );
    assert!(acquired(j.runtime.globals().unwrap().unwrap().data, 111));
    continue_tip(&mut j, viewport, deadline, 0x70fc1e);
    driver.until(
        &mut j,
        viewport,
        deadline,
        "actual GoodEND AUTOSAVE/Chart189",
        |r| {
            no_script_choice(r);
            // SAVEINFO/AUTOSAVE precede CHART189. Observe both effects,
            // including when a genuine command budget separates them.
            metadata(r, 1, "END") && chart_acquired(r, 189)
        },
    );
    assert_eq!(
        game(&j.runtime)
            .unwrap()
            .reference_story_variables()
            .0
            .get(10),
        1
    );
    assert_eq!(
        game(&j.runtime)
            .unwrap()
            .reference_story_variables()
            .0
            .get(24),
        95
    );
    assert!(
        j.runtime
            .globals()
            .unwrap()
            .unwrap()
            .data
            .save_vectors
            .extra_words_1
            .get(189 / 32)
            .is_some_and(|w| w & (1 << (189 % 32)) != 0)
    );
    same_manual(&rows(&j.runtime), &before);
    j.no_writes(Some(&initial_file));
    pure_frame(&mut j);
    let bgm_wait_observed = RefCell::new(false);
    driver.until(
        &mut j,
        viewport,
        deadline,
        "actual protected Meakashi credits layer wait",
        |r| {
            no_script_choice(r);
            if pending(r, 0x717fdb) {
                let h = game(r).unwrap().host().unwrap();
                assert!(matches!(
                    h.runner.pending_command(),
                    Some(Cmd::BGMSYNC { time: 780 })
                ));
                assert_eq!(h.runner.native_read_state().unwrap().flags & 0x8000_0000, 0);
                *bgm_wait_observed.borrow_mut() = true;
            }
            pending(r, 0x71800e)
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
    assert!(matches!(
        game(&j.runtime)
            .unwrap()
            .host()
            .unwrap()
            .runner
            .pending_command(),
        Some(Cmd::LAYERWAIT { .. })
    ));
    driver.until(
        &mut j,
        viewport,
        deadline,
        "actual protected credits WAIT0,300",
        |r| {
            no_script_choice(r);
            pending(r, 0x718012)
        },
    );
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
        "actual final TIPS112113114 Continue",
        |r| tips_ready(r, 0x71945c),
    );
    let (p, q) = game(&j.runtime).unwrap().reference_story_variables();
    assert_eq!(p.get(10), 3, "TIPS constructor commits Meakashi clear");
    assert_eq!(p.get(24), 95, "Meakashi has no24 award");
    assert_eq!(p.get(11), 1);
    assert_eq!(p.get(12), 1);
    assert!(!q.iter().any(|(s, _)| matches!(*s, 10 | 24)));
    for tip in 91..=114 {
        assert!(
            acquired(j.runtime.globals().unwrap().unwrap().data, tip),
            "actual Meakashi TIP{tip}"
        );
    }
    assert!(driver.exits.is_empty());
    same_manual(&rows(&j.runtime), &before);
    j.no_writes(Some(&initial_file));
    pure_frame(&mut j);
    continue_tip(&mut j, viewport, deadline, 0x71945c);
    let exit = RefCell::new(None::<ExitBoundary>);
    driver.until(
        &mut j,
        viewport,
        deadline,
        "natural Meakashi EXIT1f997/save/readyTitle",
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
        .expect("actual Meakashi EXIT retained save boundary");
    assert!(game(&j.runtime).is_none());
    assert_eq!(
        j.runtime.slot().unwrap().active_scene().unwrap(),
        ActiveScene::Title
    );
    assert_eq!(
        driver.exits.iter().map(|(pc, _)| *pc).collect::<Vec<_>>(),
        [0x1f997]
    );
    assert!(
        driver.movie_waits.is_empty(),
        "no Movie in this actual ordinary entry/body"
    );
    for pc in [0x70fc1e, 0x71800e, 0x718012, 0x71945c] {
        assert!(
            driver.pending_pcs.contains(&pc),
            "actual owned wait{pc:08x}"
        );
    }
    if persistent32 == 0 {
        assert!(
            !driver.pending_pcs.contains(&0x707325),
            "native32zero bypass"
        );
    } else {
        assert!(
            driver.pending_pcs.contains(&0x707325),
            "actual original1 branch"
        );
    }
    for pc in [0x6b4389, 0x70728e, 0x1f99a, 0x5cb12] {
        assert!(!driver.pending_pcs.contains(&pc), "wrong path{pc:08x}");
    }
    let after = rows(&j.runtime);
    same_manual(&exit.slots, &before);
    same_rows(&after, &exit.slots);
    assert_eq!(exit.current.progress.active_index(), 0);
    assert_eq!(exit.current.progress.extra_word, 1);
    assert_eq!(exit.current.progress.active_entry().scenario_id, 202);
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
        (11, 1),
        (12, 1),
        (24, 95),
        (32, persistent32),
    ];
    for (s, v) in expected {
        assert_eq!(globals.data.persist_data.get(s), v, "final committed{s}");
    }
    for tip in 91..=114 {
        assert!(acquired(globals.data, tip));
    }
    let writes = j.storage.writes.lock().unwrap().clone();
    assert_eq!(
        writes.len(),
        1,
        "one actual Meakashi flag1 save; no shutdown save"
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
    for tip in 91..=114 {
        assert!(acquired(&disk, tip));
    }
    assert!(j.runtime.failure().is_none());
    pure_frame(&mut j);
    println!(
        "Runtime actualearnedHima->Category1/shortscenario1/SNRSEL1row2/scenario202/persist32={persistent32}/GoodEND/TIP111/protectedBGM98credits/TIP112113114/EXIT1f997 one save+readyTitle:elapsed{:.3}s,{}frames/{}driven callbacks;phase completed{}+{}={} (two fresh runners, excludes pending commands);full100manuals/101EXIT/shared/source-payload/dormant/pure720p1080p;BGMSYNCpendingobserved{}",
        started.elapsed().as_secs_f64(),
        j.runtime.frames,
        driver.callbacks,
        picker_commands,
        exit.commands,
        picker_commands + exit.commands,
        bgm_wait_observed.into_inner()
    );
}

// Exact literals from the frozen meakashi-next-route-audit.json. No decoded
// script or runtime-produced expectation replaces these original byte spans.
const MEAKASHI_SPANS: &[(usize, &str)] = &[
    // Ordinary scenario1 dispatch and category branch table
    (
        0x0001e2bd,
        "4ab00d8ff70100c0f70100e0f7010000f8010029f80100581619005617190024181900f21819009627190060281900f328190090291900",
    ),
    // Picker rows200/201/202 actual scenario dispatches
    (
        0x0001e345,
        "4600b080c8ccf701004600b080c907d15e004600b080caf9666400",
    ),
    // Entered prologue skipsMovie8 then SNRSEL1
    (0x0001f7c0, "810100014606b102d1f701004808cb0500d401000000"),
    // Full Meakashi entry metadata/autosave/Chart164
    (
        0x006466f9,
        "a0000996da96be82b595d200a0010e23312090978ca991f28b418ad200a1d3000180a4",
    ),
    // First debug selector skipped by normalv22=0
    (0x006b4338, "4600c0160014446b00"),
    // First narrative branch uses actualpersistent32
    (0x006b4414, "810200204600b2008b476b00"),
    // Persistent32present narrative branch join
    (0x006b4786, "4764526b00"),
    // Persistent32absent narrative branch join
    (0x006b525f, "4764526b00"),
    // Second debug selector skipped by normalv22=0
    (0x00707237, "4600c0160019737000"),
    // Persistent32 gate/only normal SELECT/result1target
    (
        0x00707319,
        "810200204600b200489f70008d4a0024000000900fff0b8e8d89b982cc965c9196004582bb82cc8e9e81418e8482cd8c5c88ea82cc8e8b90fc82c98b4382aa82c282a282bd81420082bb82b582c48e8482cd81418c5c88ea82c9905582e895d482c182bd814200004600b001489f7000",
    ),
    // Original0 choice block jumpsBADEND
    (0x00709f43, "4727fc7000"),
    // Original1 TIP111 joinGoodEND and separateBADENDheader
    (
        0x0070fc1e,
        "d001806f478b087100a00107424144454e4400a1d3000180be",
    ),
    // BADEND jumpsaftermath vs actualGoodENDheader
    (0x00710886, "479f137100a00104454e4400a1d3000180bd"),
    // GoodEND block jumpssharedclearpath
    (0x0071139a, "473a467100"),
    // BADEND aftermath EXIT0,0 before goodcredits
    (0x00714635, "479af90100"),
    // Protected credits EVBEGIN/BGM98
    (0x00717ecc, "a200908062000183e8"),
    // Actual credits timing and twoqueuedverticalcontrols
    (
        0x00717fdb,
        "94830c42020000816800902e680300902ab40400903d89030090271004ff41830100902e68b2c30201038e98b1c3020106b202c40201018300812c",
    ),
    // Protected credits EVEND
    (0x0071808f, "a3"),
    // Final10clear/Trophy17/TIPS112113114/return
    (
        0x0071944e,
        "810c000a41080c0002820abcb011d0038070807180728300014780f80100",
    ),
    // Shared secondcollection gate then Title
    (
        0x0001f880,
        "810200094686b202b5f801008102000a4686b202b5f801008102000b4686b202b5f801008102000c4686b202b5f8010048a6d905004797f90100",
    ),
    // Good actual retainedsave EXIT0,1
    (0x0001f997, "000001"),
    // Bad actual EXIT0,0
    (0x0001f99a, "000000"),
];
