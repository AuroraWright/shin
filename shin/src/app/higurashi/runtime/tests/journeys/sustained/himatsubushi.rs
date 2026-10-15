//! Conditional genuine Some output -> ordinary Category1/prologue/SNRSEL1 ->
//! full Himatsubushi. No VM, responses, media or clock mutation.
use super::*;
use crate::app::higurashi::{
    runtime_game_data::RuntimeGameData, runtime_saves::Slots,
    scenario_select::owner::Owner as PickerOwner,
};
use shin_core::format::scenario::instruction_elements::Register;
use std::cell::RefCell;

const SOME: &str = "app::higurashi::runtime::tests::journeys::sustained::someutsushi::higurashi_runtime_sustained_earned_day3_chart_natsumi_reaches_full_someutsushi_epilogue_and_title";
struct EarnedPins {
    save_len: usize,
    save_fnv: u64,
    record_len: usize,
    record_fnv: u64,
}
// This unregistered draft cannot run until the parent supplies actual accepted
// Some output/PASS. Expected future flags are not fabricated earned provenance.
const ACCEPTED_SOME: Option<EarnedPins> = Some(EarnedPins {
    save_len: 16388,
    save_fnv: 0xa6ef_6390_ddef_b4a3,
    record_len: 354313,
    record_fnv: 0x65e6_f70f_c53a_2bb4,
});
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3)
    })
}
fn earned() -> (SaveData, Vec<u8>) {
    let pins = ACCEPTED_SOME
        .as_ref()
        .expect("actual accepted Some output/PASS has not been pinned; conditional draft only");
    let path = std::env::var("SHIN_HIGURASHI_EARNED_SOMEUTSUSHI")
        .expect("actual accepted cumulative Some output required");
    let record = std::env::var("SHIN_HIGURASHI_EARNED_SOMEUTSUSHI_RECORD")
        .expect("exact original passing Some record required");
    let proof = std::fs::read(record).unwrap();
    assert_eq!(proof.len(), pins.record_len);
    assert_eq!(fnv(&proof), pins.record_fnv);
    let proof: serde_json::Value = serde_json::from_slice(&proof).unwrap();
    assert_eq!(proof["exit_code"].as_i64(), Some(0));
    assert_eq!(proof["command"][1].as_str(), Some(SOME));
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
    for (slot, value) in [
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
    ] {
        assert_eq!(saved.persist_data.get(slot), value, "actual input{slot}");
    }
    // Require the genuine first ordinary prologue, not a Quiz2-cleared input.
    assert_eq!(saved.persist_data.get(1) & 2, 0);
    for slot in 9..=12 {
        assert_eq!(
            saved.persist_data.get(slot),
            0,
            "first later-book input{slot}"
        );
    }
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
fn choose_zero(j: &mut Journey, viewport: InputViewport, deadline: Instant, pc: u32) {
    for _ in 0..8 {
        assert!(choice_ready(&j.runtime, pc));
        let (_, selected, choices) = game(&j.runtime).unwrap().reference_story_choice().unwrap();
        let wanted = choices
            .iter()
            .position(|(id, _)| *id == 0)
            .expect("required native original0 must be visible");
        if selected == wanted as i32 {
            j.tap(KeyCode::Enter, viewport);
            j.until(viewport, deadline, "actual SELECT close/callback", |r| {
                !pending(r, pc)
            });
            return;
        }
        j.tap(KeyCode::ArrowDown, viewport);
    }
    panic!("SELECT{pc:08x} original0 not selected");
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
#[ignore = "CONDITIONAL unregistered draft: requires actual accepted Some output+PASS;ordinary Category1/scenario1/Movie8 naturalEOF/SNRSEL1 row1/201/three original0 choices/fullHima/TIP49/Otsu4/EXIT1f997 retainedsave+Title;real assets/offscreenMetal/exactLoopbackAudio;temporaryPaths only;twohour observation allowance"]
fn higurashi_runtime_sustained_earned_category_reaches_full_himatsubushi_exit_and_title() {
    let (saved, initial_file) = earned();
    let started = Instant::now();
    let deadline = started + Duration::from_secs(2 * 60 * 60);
    let viewport = viewport((1920, 1080));
    let mut j = Journey::new("sustained-earned-himatsubushi", &saved);
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
    // The scene factory publishes its host before asynchronous VM setup ends.
    // Drive neutral callbacks until position/read-state diagnostics are valid.
    j.until(viewport, deadline, "ordinary prologue VM attached", |r| {
        game(r).is_some_and(|g| g.host().is_some_and(|h| h.runner.native_vm_attached()))
    });
    let mut driver = Driver::new();
    driver.until(
        &mut j,
        viewport,
        deadline,
        "ordinary scenario1 prologue Movie8",
        |r| {
            no_script_choice(r);
            pending(r, 0x5cb12)
        },
    );
    let g = game(&j.runtime).unwrap();
    assert_eq!(g.flags, 0);
    let ordinary = g.runtime_game_data().unwrap();
    assert_eq!(ordinary.progress.active_index(), 0);
    assert_eq!(ordinary.progress.active_entry().scenario_id, 1);
    assert_eq!(ordinary.progress.extra_word, 1);
    assert!(matches!(
        g.host().unwrap().runner.pending_command(),
        Some(Cmd::MOVIEWAIT { args: [13, 2] })
    ));
    assert_eq!(
        g.host().unwrap().runner.native_read_state().unwrap().flags & 0x8000_0000,
        0
    );
    // Exact authored entry/prologue/choice/ending bytes; no patched script.
    for (at, hex) in HIMATSUBUSHI_SPANS {
        pin(&j.runtime, *at, hex);
    }
    pure_frame(&mut j);
    driver.until(
        &mut j,
        viewport,
        deadline,
        "natural Movie8/prologue/SNRSEL1 factory",
        |r| {
            if r.slot().is_some_and(|s| s.pending_scenario_select()) {
                return true;
            }
            no_script_choice(r);
            false
        },
    );
    assert!(driver.movie_waits.contains(&0x5cb12));
    assert!(driver.exits.is_empty());
    let g = game(&j.runtime).expect("queued picker retains original ADV until dispatch");
    assert_eq!(g.host().unwrap().runner.position().0, 0x1f7d1);
    assert!(matches!(
        g.host().unwrap().runner.pending_command(),
        Some(Cmd::SNRSEL { .. })
    ));
    let source = g.runtime_game_data().unwrap();
    let prologue_commands = g.host().unwrap().runner.completed_commands();
    let (p, q) = g.reference_story_variables();
    assert_eq!(p.get(1), 19);
    assert_eq!(p.get(9), 1);
    assert!(!q.iter().any(|(s, _)| matches!(*s, 1 | 9)));
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
    assert_ne!(m.mask & (1 << 1), 0, "actual prologue enables row1");
    assert_eq!(m.data, source, "complete source payload into SNRSEL owner");
    same_rows(&rows(&j.runtime), &at_picker);
    assert_eq!(identities(&j.runtime), shared);
    j.no_writes(Some(&initial_file));
    pure_frame(&mut j);
    // Native two-column geometry: Left0->1, not Down0->2.
    j.tap(KeyCode::ArrowLeft, viewport);
    assert!(picker_ready(&j.runtime));
    assert_eq!(picker(&j.runtime).unwrap().reference_model().selected, 1);
    j.tap(KeyCode::Enter, viewport);
    j.until(
        viewport,
        deadline,
        "actual picker scenario201 deferred factory",
        |r| {
            r.slot()
                .is_some_and(|s| s.scenario_select.is_some() && s.pending_scene_factory())
        },
    );
    let returned = picker(&j.runtime).unwrap().reference_model().data.clone();
    let mut expected = source.clone();
    expected.progress.set_active_index(0).unwrap();
    expected.progress.entries[0].scenario_id = 201;
    expected.progress.entries[0].random_seed = returned.progress.entries[0].random_seed;
    expected.progress.entries[0].save_position = 0;
    expected.progress.entries[0].replay_target = 0;
    assert_eq!(
        returned, expected,
        "native b7310 retains date/extra/bothresponsevectors/dormant/trailing/bytebanks"
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
    j.until(viewport, deadline, "actual Himatsubushi VM attached", |r| {
        game(r).is_some_and(|g| g.host().is_some_and(|h| h.runner.native_vm_attached()))
    });
    driver.until(
        &mut j,
        viewport,
        deadline,
        "full Himatsubushi AUTOSAVE",
        |r| {
            no_script_choice(r);
            metadata(r, 0, "暇潰し編") && metadata(r, 1, "#1 誘拐事件")
        },
    );
    let initial_current = game(&j.runtime).unwrap().runtime_game_data().unwrap();
    assert_eq!(initial_current.progress.active_entry().scenario_id, 201);
    assert_eq!(initial_current.progress.extra_word, 1);
    assert_eq!(
        register(&j.runtime, 22),
        0,
        "ordinary route bypasses NCSELECT"
    );
    for (index, pc) in [0x5fb588, 0x60bd1f, 0x618cb8].into_iter().enumerate() {
        driver.until(
            &mut j,
            viewport,
            deadline,
            "required authored Hima SELECT",
            |r| choice_ready(r, pc),
        );
        assert_eq!(
            register(&j.runtime, 26),
            index as i32,
            "earlier native choices alone update score"
        );
        pure_frame(&mut j);
        choose_zero(&mut j, viewport, deadline, pc);
    }
    driver.until(
        &mut j,
        viewport,
        deadline,
        "fuller Hima ending/final TIPS49",
        |r| tips_ready(r, 0x6466c7),
    );
    assert_eq!(register(&j.runtime, 26), 3);
    assert_eq!(register(&j.runtime, 27), 1);
    let (p, q) = game(&j.runtime).unwrap().reference_story_variables();
    assert_eq!(p.get(9), 3);
    for s in [10, 11, 12] {
        assert_eq!(p.get(s), 1);
    }
    assert!(!q.iter().any(|(s, _)| matches!(*s, 9..=12)));
    assert_eq!(p.get(24), 79);
    assert!(!q.iter().any(|(s, _)| *s == 24));
    for tip in 35..=49 {
        assert!(
            acquired(j.runtime.globals().unwrap().unwrap().data, tip),
            "actual Hima TIP{tip}"
        );
    }
    assert!(
        !driver.pending_pcs.contains(&0x62e0b9),
        "points3 skips conditional rescue prompt"
    );
    same_manual(&rows(&j.runtime), &before);
    j.no_writes(Some(&initial_file));
    pure_frame(&mut j);
    continue_tip(&mut j, viewport, deadline, 0x6466c7);
    driver.until(
        &mut j,
        viewport,
        deadline,
        "actual OTSUGET4 queued Hima award",
        |r| {
            no_script_choice(r);
            pending(r, 0x6466eb) && game(r).unwrap().reference_story_notification() == Some((13, 2))
        },
    );
    let (p, q) = game(&j.runtime).unwrap().reference_story_variables();
    assert_eq!(p.get(24), 79);
    assert_eq!(q.iter().find(|(s, _)| *s == 24).map(|(_, v)| *v), Some(95));
    assert!(driver.exits.is_empty());
    j.no_writes(Some(&initial_file));
    let exit = RefCell::new(None::<ExitBoundary>);
    driver.until(
        &mut j,
        viewport,
        deadline,
        "natural Hima EXIT1f997/save/readyTitle",
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
        .expect("actual Hima EXIT retained save boundary");
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
        0x5cb12, 0x5d993, 0x5fb588, 0x60bd1f, 0x618cb8, 0x6466c7, 0x6466eb,
    ] {
        assert!(
            driver.pending_pcs.contains(&pc),
            "actual owned wait{pc:08x}"
        );
    }
    for pc in [0x5d9a2, 0x62e0b9, 0x1f99a, 0x5cb04] {
        assert!(!driver.pending_pcs.contains(&pc), "wrong route{pc:08x}");
    }
    let after = rows(&j.runtime);
    same_manual(&exit.slots, &before);
    same_rows(&after, &exit.slots);
    assert_eq!(exit.current.progress.active_index(), 0);
    assert_eq!(exit.current.progress.extra_word, 1);
    assert_eq!(exit.current.progress.active_entry().scenario_id, 201);
    assert!(exit.current.progress.active_entry().save_position > 0);
    assert_eq!(
        exit.current.progress.entries[1], initial_current.progress.entries[1],
        "dormant current survives Hima"
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
        (10, 1),
        (11, 1),
        (12, 1),
        (24, 95),
    ];
    for (s, v) in expected {
        assert_eq!(globals.data.persist_data.get(s), v, "final committed{s}");
    }
    for tip in 35..=49 {
        assert!(acquired(globals.data, tip));
    }
    for node in [148, 150, 163] {
        assert!(
            globals
                .data
                .save_vectors
                .extra_words_1
                .get(node / 32)
                .is_some_and(|w| w & (1 << (node % 32)) != 0),
            "actual Hima Chart{node}"
        );
    }
    let writes = j.storage.writes.lock().unwrap().clone();
    assert_eq!(
        writes.len(),
        1,
        "one actual Hima flag1 save; no shutdown save"
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
    for tip in 35..=49 {
        assert!(acquired(&disk, tip));
    }
    assert!(j.runtime.failure().is_none());
    pure_frame(&mut j);
    println!(
        "Runtime actualearnedSome->Category1/scenario1/Movie8naturalEOF/SNRSEL1row1/scenario201/threeoriginal0/fullHima/TIP49/Otsu4/EXIT1f997 one save+readyTitle:elapsed{:.3}s,{}frames/{}driven callbacks;phase completed{}+{}={} (two fresh runners, excludes pending commands);full100manuals/101EXIT/shared/source-payload/dormant/pure720p1080p",
        started.elapsed().as_secs_f64(),
        j.runtime.frames,
        driver.callbacks,
        prologue_commands,
        exit.commands,
        prologue_commands + exit.commands
    );
}

// Filled directly from the frozen post-Some raw audit, not a freshly generated
// expectation. The input acceptance pin above remains intentionally absent.
const HIMATSUBUSHI_SPANS: &[(usize, &str)] = &[
    // Actual retained-save Title EXIT0,1
    (0x0001f997, "000001"),
    // AfterALLsix mainepilogue sets1|16/0|2; conditionalChartEXIT
    (
        0x0005cae3,
        "810c000141080c00108201bc810c000041080c00028200bc4601c0170107cb050000000149",
    ),
    // Categoryscenario1 ->actual1f7c0 viajump table
    (
        0x0001e2bd,
        "4ab00d8ff70100c0f70100e0f7010000f8010029f80100581619005617190024181900f21819009627190060281900f328190090291900",
    ),
    // ActualCategory1 prologue gate/SNRSELkind1/epiloguereentry
    (
        0x0001f7c0,
        "810100014606b102d1f701004808cb0500d40100000048a6d9050047d1f70100",
    ),
    // PrologueactualMovie8/status2/metadata/autosave/Chart148
    (
        0x0005cb08,
        "a200c10d0700030882eec80d02c20d00a3a0001f8c788e8b92a18cf688c0959491e68eb58eba917b8db8837483408343838b00a0010970726f6c6f67756500a1d300018094",
    ),
    // Prologue1|2,9|1,OTSUGET13;returntosnrsel
    (
        0x0005d96f,
        "810c000141080c00028201bc810200094606b20199d90500810c000941080c00018209bcd20d830080784601c01701a5d9050000000049",
    ),
    // Actual201 ->fullHima
    (0x0001e34e, "4600b080c907d15e00"),
    // FullHima metadata/autosave/Chart150/counterszero
    (
        0x005ed107,
        "a0000989c992d782b595d200a0010c233120975589fb8e968c8f00a1d30001809641001a000041001b0000",
    ),
    // FirstauthoredSELECToriginal0 adds1
    (
        0x005fb588,
        "8d420020000000900fff1191e590ce82cc905c82b58f6f82c9816300198d5482a682df82c991ce899e0091b891e582c991ce899e00004600b000cbb55f004718be5f0041021a0001",
    ),
    // SecondauthoredSELECToriginal0 adds1
    (
        0x0060bd1f,
        "8d440021000000900fff138b53839695a38e808ee793af96bf82cd8163002495d4939a82f0406282bd82df82e72e403ce753e74f403e82a40091a6939a82b782e900004600b0006fbd6000470fbe600041021a0001",
    ),
    // ThirdauthoredSELECToriginal0 adds1
    (
        0x00618cb8,
        "8d460022000000900fff0f8a6a905382f093cb82a982ea8163000f96d982e90082c682da82af82e900004600b000ef8c610047a2a5610041021a0001",
    ),
    // Normalv22=0 skipsdebugselect
    (0x006247dd, "4600c0160091486200"),
    // Realpoints>=2 warningbranch
    (
        0x00624881,
        "4600b0009a4862004600b001975362004604c01a019753620041001b0001",
    ),
    // Normalv22=0 skipsseconddebugselect
    (0x0062dfe1, "4600c01600b0e06200"),
    // Realpoints>=2 bypassrescueSELECT;otherbadendbranch
    (
        0x0062e0b0,
        "4602c01a02eee062008d480023000000900fff0b90d48de282cc914991f0001196bd8ceeb2a6bdd9008c84a6894db300004600b000eee06200473dfb6200",
    ),
    // BadendgoesEXIT0,0;notfullclear
    (0x0062fe87, "83008078830001479af90100"),
    // ActualEND/autosave/Chart162
    (
        0x0063ad2e,
        "a00111454e442028966b8a4393b982c982c42900a1d3000180a2",
    ),
    // Normalbranchskipsdebugendingselector
    (0x00644db5, "4600c01600614e6400"),
    // Points3fullerendingvs<=2
    (
        0x00644e51,
        "4600b0006a4e64004600b001185b64004604c01a02185b6400",
    ),
    // FullerendingfirstactualVOICEWAIT2
    (0x00646104, "9e02"),
    // FullerendinglastactualVOICEWAIT2
    (0x0064630f, "9e02"),
    // Himaclear9|2/unlock10..12/Tip49/Chart163/24|16/actualOTSUGET4/commonreturn
    (
        0x0064665f,
        "810c000941080c00028209bcb0108102000a4606b2018b666400810c000a41080c0001820abcd20e830080788102000b4606b201a9666400810c000b41080c0001820bbcd20f830080788102000c4606b201c7666400810c000c41080c0001820cbcd21083008078d0013183008078d3010180a3810200184606b210f1666400810c001841080c00108218bcd204830080788300014780f80100",
    ),
    // Sharedsecondcollectionclear9..12 checkthenEXIT
    (
        0x0001f880,
        "810200094686b202b5f801008102000a4686b202b5f801008102000b4686b202b5f801008102000c4686b202b5f8010048a6d905004797f90100",
    ),
    // Warning branch selects TIPS47+48 and rejoins successful rescue
    (
        0x0062faae,
        "4601c01b00bffa6200d0012f47c3fa6200d0022f304793fe6200",
    ),
];
