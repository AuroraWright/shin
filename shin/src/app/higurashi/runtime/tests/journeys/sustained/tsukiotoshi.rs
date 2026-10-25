//! Exact accepted cumulative Tatar disk -> real earned Chart40 -> Satoshi1/absence0
//! -> complete authored Tsukiotoshi -> actual retained EXIT save -> ready Title.
use super::*;
use crate::app::higurashi::{
    chart::owner::Owner as ChartOwner, runtime_game_data::RuntimeGameData, runtime_saves::Slots,
};
use std::cell::RefCell;

const TATARIGOROSHI: &str = "app::higurashi::runtime::tests::journeys::sustained::tatarigoroshi::higurashi_runtime_sustained_earned_chart_satoko_reaches_full_tatarigoroshi_exit_and_title";
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3)
    })
}
fn earned() -> (SaveData, Vec<u8>) {
    let path = std::env::var("SHIN_HIGURASHI_EARNED_TATARIGOROSHI")
        .expect("require the actual accepted cumulative Tatarigoroshi progress file");
    let record = std::env::var("SHIN_HIGURASHI_EARNED_TATARIGOROSHI_RECORD")
        .expect("require its exact accepted execution record, not a partial or rejected run");
    let proof = std::fs::read(record).unwrap();
    // PASS SHA2566f9c841fe0257c4a3056a0b8dee7a415fcda7ba3f6551f45feb59ea7e88fd033.
    // Parent launcher pins full SHA256, build/binary/log and the transitive
    // Watanagashi/first-book earned chain before executing this ignored test.
    assert_eq!(proof.len(), 5386);
    assert_eq!(fnv(&proof), 0x954a_68a4_fa1d_a341);
    let proof: serde_json::Value = serde_json::from_slice(&proof).unwrap();
    assert_eq!(proof["exit_code"].as_i64(), Some(0));
    assert_eq!(proof["command"][1].as_str(), Some(TATARIGOROSHI));
    assert!(
        proof["changed_pinned_sources"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let bytes = std::fs::read(path).unwrap();
    // Exact earned SHA2567616d26155c5ef1879b3bc69f3da3839ae70e4a3736848ab9e7bbf0bb6cf3cbc.
    assert_eq!(bytes.len(), 14340);
    assert_eq!(fnv(&bytes), 0x7f07_a79a_982b_e773);
    let saved = SaveData::decode(&bytes).unwrap();
    assert_eq!(
        saved.encode().unwrap(),
        bytes,
        "ordinary Journey disk setup preserves exact earned bytes"
    );
    for (slot, value) in [
        (1, 1),
        (2, 3),
        (3, 3),
        (4, 3),
        (5, 3),
        (6, 0),
        (7, 0),
        (8, 0),
        (24, 13),
        (33, 1),
        (41, 0),
        (42, 1),
        (44, 0),
        (45, 1),
        (54, 0),
    ] {
        assert_eq!(
            saved.persist_data.get(slot),
            value,
            "accepted input slot{slot}"
        );
    }
    assert!(saved.auto_save_slot.is_some());
    assert_ne!(
        saved.save_vectors.extra_words_1[40 / 32] & (1 << (40 % 32)),
        0,
        "Chart40 was actually earned before the accepted Tatar EXIT save"
    );
    assert!(!acquired(&saved, 27));
    assert!(!acquired(&saved, 116));
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
fn story_metadata(runtime: &Runtime, label: &str) -> bool {
    assert!(
        game(runtime).is_some(),
        "left ADV before authored story label {label}: {}",
        Driver::describe(runtime)
    );
    metadata(runtime, label)
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
fn choice_ready(runtime: &Runtime, pc: u32) -> bool {
    assert!(
        game(runtime).is_some(),
        "left ADV before required SELECT{pc:08x}: {}",
        Driver::describe(runtime)
    );
    pending(runtime, pc)
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
#[ignore = "actual accepted cumulative Tatar disk/Runtime/earned Chart40/Satoshi original1/absence original0/protected preview3/full Tsuki/TIP116/native EXIT1f997 save+Title;real assets/offscreen Metal/the default audio output;temporary writes only;up to two hours"]
fn higurashi_runtime_sustained_earned_chart_satoshi_reaches_full_tsukiotoshi_exit_and_title() {
    let (saved, initial_file) = earned();
    let started = Instant::now();
    // Observation allowance only; no synthetic VM/media/policy/clock advances.
    let deadline = started + Duration::from_secs(2 * 60 * 60);
    let viewport = viewport((1920, 1080));
    let mut j = Journey::new("sustained-earned-tsukiotoshi", &saved);
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
        chart(&j.runtime).unwrap().reference_selected_entry(),
        Some((0, 3000))
    );
    assert_eq!(cursor(&j.runtime), [18., 28.]);
    // Chart40 logical(8,46), center(306,2604); observed held+10 samples
    // reach(308,2608), strictly inside the actual native hit rectangle.
    steer(&mut j, viewport, deadline, 0, 308., KeyCode::ArrowRight);
    steer(&mut j, viewport, deadline, 1, 2608., KeyCode::ArrowDown);
    assert_eq!(
        chart(&j.runtime).unwrap().reference_selected_entry(),
        Some((40, 3229))
    );
    pure_frame(&mut j);
    same_manual(&rows(&j.runtime), &before);
    j.no_writes(Some(&initial_file));
    j.tap(KeyCode::Enter, viewport);
    j.until(
        viewport,
        deadline,
        "actual Chart shortcut default-No Question",
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
        "actual Chart fresh ADV3229/Satoko4",
        |r| metadata(r, "route 沙都子4"),
    );
    assert!(j.runtime.slot().unwrap().title.is_none());
    let initial_current = game(&j.runtime).unwrap().runtime_game_data().unwrap();
    assert_eq!(initial_current.progress.active_index(), 0);
    assert_eq!(initial_current.progress.active_entry().scenario_id, 3229);
    assert_eq!(identities(&j.runtime), shared);
    for (at, bytes) in [
        (
            0x1e60f,
            &[0x46, 0x00, 0xb0, 0x90, 0x0c, 0x9d, 0xe3, 0xb1, 0x36, 0x00][..],
        ),
        (0x36b1f3, &[0xd3, 0x00, 0x01, 0x28][..]),
        (
            0x36def4,
            &[0x41, 0x00, 0x2f, 0x00, 0x01, 0x82, 0x36, 0x01][..],
        ),
        (0x36f175, &[0xd0, 0x01, 0x1b][..]),
        (
            0x3833e3,
            &[0x46, 0x00, 0xb0, 0x00, 0xf3, 0x33, 0x38, 0x00][..],
        ),
        (
            0x3833f3,
            &[
                0x81, 0x0c, 0x00, 0x06, 0x41, 0x08, 0x0c, 0x00, 0x01, 0x82, 0x06, 0xbc, 0x81, 0x0c,
                0x00, 0x02, 0x41, 0x08, 0x0c, 0x00, 0x02, 0x82, 0x02, 0xbc, 0x41, 0x00, 0x00, 0x00,
                0x03, 0x48, 0x84, 0xf3, 0x14, 0x00, 0x83, 0x00, 0x01, 0x47, 0x46, 0x71, 0x4e, 0x00,
            ][..],
        ),
        (
            0x14f39c,
            &[0x46, 0x00, 0xb0, 0x03, 0x57, 0x02, 0x15, 0x00][..],
        ),
        (0x1502c5, &[0x9e, 0x02][..]),
        (0x1506a9, &[0x9e, 0x02][..]),
        (0x4e716b, &[0xa1, 0xd3, 0x00, 0x02, 0x2c, 0x80, 0x66][..]),
        (
            0x54e605,
            &[
                0x81, 0x0c, 0x00, 0x06, 0x41, 0x08, 0x0c, 0x00, 0x02, 0x82, 0x06, 0xbc, 0xb0, 0x0f,
                0xd0, 0x01, 0x80, 0x74, 0x83, 0x00, 0x01, 0x47, 0x2e, 0xf8, 0x01, 0x00,
            ][..],
        ),
        (0x1f997, &[0x00, 0x00, 0x01][..]),
    ] {
        pin(&j.runtime, at, bytes);
    }
    // Stop before both route-critical actual SELECTs. A compact visible row
    // is resolved from original IDs; neither availability nor response is set.
    driver.until(
        &mut j,
        viewport,
        deadline,
        "Satoshi actual SELECT before autoaccept",
        |r| choice_ready(r, 0x36cfc7),
    );
    choose_original(&mut j, viewport, deadline, 0x36cfc7, 1);
    driver.until(
        &mut j,
        viewport,
        deadline,
        "real Satoshi1 continuation/Satoko5",
        |r| story_metadata(r, "route 沙都子5"),
    );
    assert_eq!(
        game(&j.runtime)
            .unwrap()
            .reference_story_variables()
            .0
            .get(54),
        1
    );
    assert!(
        acquired(j.runtime.globals().unwrap().unwrap().data, 27),
        "actual Satoshi1 TIP27"
    );
    assert!(driver.pending_pcs.contains(&0x36f175));
    same_manual(&rows(&j.runtime), &before);
    j.no_writes(Some(&initial_file));
    driver.until(
        &mut j,
        viewport,
        deadline,
        "absence actual original0 before autoaccept",
        |r| choice_ready(r, 0x383385),
    );
    choose_original(&mut j, viewport, deadline, 0x383385, 0);
    driver.until(
        &mut j,
        viewport,
        deadline,
        "actual protected preview3/full Tsuki",
        |r| story_metadata(r, "憑落し編"),
    );
    assert_eq!(
        game(&j.runtime)
            .unwrap()
            .reference_story_variables()
            .0
            .get(6),
        1,
        "entered, not yet cleared"
    );
    assert!(
        driver.pending_pcs.contains(&0x1502c5),
        "actual first protected preview3 VOICEWAIT2"
    );
    assert!(
        driver.pending_pcs.contains(&0x1506a9),
        "actual last protected preview3 VOICEWAIT2"
    );
    assert!(
        driver.movie_waits.is_empty(),
        "Chart40 does not replay earlier Movie7"
    );
    pure_frame(&mut j);
    same_manual(&rows(&j.runtime), &before);
    j.no_writes(Some(&initial_file));
    driver.until(
        &mut j,
        viewport,
        deadline,
        "actual final TIP116 Continue child",
        |r| choice_ready(r, 0x54e613),
    );
    let (committed, queued) = game(&j.runtime).unwrap().reference_story_variables();
    // Unlike Tatar's OTSUGET, actual TIPSGET c13c invokes1185c before
    // attaching this child. Its preceding SSET6=3 has already committed.
    assert_eq!(committed.get(6), 3);
    assert!(!queued.iter().any(|(slot, _)| *slot == 6));
    assert_eq!(committed.get(24), 13, "Tsuki has no new24 award/OTSUGET");
    assert!(!queued.iter().any(|(slot, _)| *slot == 24));
    assert!(acquired(j.runtime.globals().unwrap().unwrap().data, 116));
    assert!(driver.exits.is_empty());
    same_manual(&rows(&j.runtime), &before);
    j.no_writes(Some(&initial_file));
    pure_frame(&mut j);
    choose_original(&mut j, viewport, deadline, 0x54e613, 0);
    let exit = RefCell::new(None::<ExitBoundary>);
    driver.until(
        &mut j,
        viewport,
        deadline,
        "natural EXIT1f997/save/ready Title",
        |r| {
            if let Some(g) = game(r) {
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
    for pc in [0x36cfc7, 0x36f175, 0x37579d, 0x383385, 0x54e613] {
        assert!(
            driver.pending_pcs.contains(&pc),
            "actual authored owned wait{pc:08x}"
        );
    }
    assert!(
        !driver.pending_pcs.contains(&0x34174a),
        "no replay of already accepted Bento/common prefix"
    );
    assert!(driver.movie_waits.is_empty());
    let after = rows(&j.runtime);
    same_manual(&exit.slots, &before);
    same_manual(&after, &before);
    assert_eq!(after.auto.valid, exit.slots.auto.valid);
    assert_eq!(
        after.auto.data, exit.slots.auto.data,
        "complete101 EXIT transfer"
    );
    assert_eq!(exit.current.progress.active_index(), 0);
    assert_eq!(exit.current.progress.active_entry().scenario_id, 3229);
    assert!(exit.current.progress.active_entry().save_position > 0);
    assert_eq!(
        exit.current.progress.entries[1], initial_current.progress.entries[1],
        "fresh Chart dormant current preserved"
    );
    assert_eq!(identities(&j.runtime), shared);
    let globals = j.runtime.globals().unwrap().unwrap();
    assert!(globals.clock.ticks() > clock);
    let expected = [
        (1, 1),
        (2, 3),
        (3, 3),
        (4, 3),
        (5, 3),
        (6, 3),
        (7, 0),
        (8, 0),
        (24, 13),
        (33, 1),
        (41, 0),
        (42, 1),
        (44, 0),
        (45, 1),
        (54, 1),
    ];
    for (slot, value) in expected {
        assert_eq!(
            globals.data.persist_data.get(slot),
            value,
            "final committed slot{slot}"
        );
    }
    for tip in [27, 116] {
        assert!(acquired(globals.data, tip), "actual acquired TIP{tip}");
    }
    for node in [40, 102, 117] {
        assert!(
            globals
                .data
                .save_vectors
                .extra_words_1
                .get(node / 32)
                .is_some_and(|word| word & (1 << (node % 32)) != 0),
            "actual Chart{node}"
        );
    }
    let writes = j.storage.writes.lock().unwrap().clone();
    assert_eq!(
        writes.len(),
        1,
        "one native flag1 EXIT write; no shutdown save"
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
        "wire projection checked separately from full101 equality"
    );
    for (slot, value) in expected {
        assert_eq!(
            disk.persist_data.get(slot),
            value,
            "saved committed slot{slot}"
        );
    }
    for tip in [27, 116] {
        assert!(acquired(&disk, tip), "saved TIP{tip}");
    }
    assert!(j.runtime.failure().is_none());
    pure_frame(&mut j);
    println!(
        "Runtime accepted cumulative Tatar→earned Chart40/scenario3229→Satoshi1/absence0→protectedpreview3/full Tsuki→TIP116 commit→EXIT0001f997/one save/ready Title: elapsed{:.3}s,{} Runtimeframes/{} callbacks/{}commands;full100manuals+101 EXIT boundary/shared owners/dormant current/pure720p1080p;no repeated Movie7/OTSUGET award",
        started.elapsed().as_secs_f64(),
        j.runtime.frames,
        driver.callbacks,
        driver.max_commands
    );
}
