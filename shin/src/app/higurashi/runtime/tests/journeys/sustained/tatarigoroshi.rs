//! Genuine cumulative first-book+Watanagashi earned progress -> actual Chart Day3 -> actual Satoko
//! selection -> complete authored Tatarigoroshi -> retained EXIT save -> Title.
use super::*;
use crate::app::higurashi::{
    chart::owner::Owner as ChartOwner, runtime_game_data::RuntimeGameData, runtime_saves::Slots,
};
use std::cell::RefCell;

const WATANAGASHI: &str = "app::higurashi::runtime::tests::journeys::sustained::watanagashi::higurashi_runtime_sustained_earned_chart_mion_reaches_full_watanagashi_exit_and_title";
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3)
    })
}
fn earned() -> (SaveData, Vec<u8>) {
    let path = std::env::var("SHIN_HIGURASHI_EARNED_WATANAGASHI")
        .expect("require the actual accepted cumulative Watanagashi progress file");
    let record = std::env::var("SHIN_HIGURASHI_EARNED_WATANAGASHI_RECORD")
        .expect("require the exact accepted Watanagashi execution record");
    let proof = std::fs::read(record).unwrap();
    // Original PASS record SHA25649c13781f7e4999466413baa0f1443da2255bdcacb7b789a7c62906af1b3c0ec.
    // The parent launcher independently verifies full SHA256 plus the original
    // build/binary/log and transitive first-book acceptance for these files.
    assert_eq!(proof.len(), 4867);
    assert_eq!(fnv(&proof), 0x93fa_07a6_8ad3_ab31);
    let proof: serde_json::Value = serde_json::from_slice(&proof).unwrap();
    assert_eq!(proof["exit_code"].as_i64(), Some(0));
    assert_eq!(proof["command"][1].as_str(), Some(WATANAGASHI));
    assert!(
        proof["changed_pinned_sources"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let bytes = std::fs::read(path).unwrap();
    // Exact earned output: SHA256e6783682c38400ec877cd0d6a93e6a68a85f1eb0cb0356c1d4be22e2080f0221.
    assert_eq!(bytes.len(), 14340);
    assert_eq!(fnv(&bytes), 0x7b11_0971_c7b7_5f54);
    let saved = SaveData::decode(&bytes).unwrap();
    // Journey::new performs ordinary disk setup through the encoder. Prove
    // that it will copy the earned bytes unchanged, not project new flags.
    assert_eq!(saved.encode().unwrap(), bytes);
    for (slot, value) in [
        (1, 1),
        (2, 3),
        (3, 3),
        (4, 3),
        (5, 0),
        (6, 0),
        (24, 9),
        (31, 1),
        (34, 1),
        (42, 0),
        (45, 0),
        (54, 0),
        (58, 0),
    ] {
        assert_eq!(saved.persist_data.get(slot), value, "earned slot{slot}");
    }
    assert!(saved.auto_save_slot.is_some());
    assert_ne!(
        saved.save_vectors.extra_words_1[0] & (1 << 5),
        0,
        "earned Chart5 retained through the accepted cumulative Watanagashi output"
    );
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
fn choose_satoko(j: &mut Journey, viewport: InputViewport, deadline: Instant) {
    // Satoko is ORIGINAL2 (original0 is Rena). Never synthesize availability.
    for _ in 0..5 {
        let g = game(&j.runtime).unwrap();
        assert!(pending(&j.runtime, 0x2abb0a));
        let (phase, selected, originals) = g.reference_story_character().unwrap();
        assert_eq!(phase, 2);
        assert!(g.input_permission);
        let wanted = originals
            .iter()
            .position(|original| *original == 2)
            .expect("actual Day3 choices must make original2/Satoko available");
        if selected == wanted as i32 {
            j.tap(KeyCode::Enter, viewport);
            j.until(viewport, deadline, "actual Character close/callback", |r| {
                !pending(r, 0x2abb0a)
            });
            return;
        }
        j.tap(KeyCode::ArrowDown, viewport);
    }
    panic!("actual original2/Satoko row not selected");
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
#[ignore = "actual accepted-cumulative-Watanagashi disk/normal Runtime/earned Chart5/Day3 actual choices/CHARSELECT original2/Movie7 natural EOF/full Tatar/EXIT1f997 real save+Title;real assets/offscreen Metal/the default audio output;temporary writes only;up to two hours"]
fn higurashi_runtime_sustained_earned_chart_satoko_reaches_full_tatarigoroshi_exit_and_title() {
    let (saved, initial_file) = earned();
    let started = Instant::now();
    // A wall allowance, not a synthetic advance: actual media/voice/worker
    // clocks and every authored command remain unchanged.
    let deadline = started + Duration::from_secs(2 * 60 * 60);
    let viewport = viewport((1920, 1080));
    let mut j = Journey::new("sustained-earned-tatarigoroshi", &saved);
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
    // Earned Chart5 anchor17/label19: logical(10,8), center(378,476).
    // Actual +10 held samples reach(378,478), inside its native hit rectangle.
    steer(&mut j, viewport, deadline, 0, 378., KeyCode::ArrowRight);
    steer(&mut j, viewport, deadline, 1, 478., KeyCode::ArrowDown);
    assert_eq!(
        chart(&j.runtime).unwrap().reference_selected_entry(),
        Some((5, 3203))
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
        "Chart actual fresh ADV3203/Day3",
        |r| metadata(r, "3日目"),
    );
    assert!(j.runtime.slot().unwrap().title.is_none());
    let initial_current = game(&j.runtime).unwrap().runtime_game_data().unwrap();
    assert_eq!(initial_current.progress.active_index(), 0);
    assert_eq!(initial_current.progress.active_entry().scenario_id, 3203);
    assert_eq!(identities(&j.runtime), shared);
    for (at, bytes) in [
        (
            0x1e47b,
            &[0x46, 0x00, 0xb0, 0x90, 0x0c, 0x83, 0x6c, 0xb1, 0x23, 0x00][..],
        ),
        (0x23b182, &[0xd3, 0x00, 0x01, 0x05][..]),
        (
            0x23b70b,
            &[
                0x46, 0x00, 0xb0, 0x00, 0x1b, 0xb7, 0x23, 0x00, 0x46, 0x00, 0xb0, 0x01, 0xa6, 0xbf,
                0x23, 0x00,
            ][..],
        ),
        (
            0x23e6a6,
            &[
                0x46, 0x00, 0xb0, 0x00, 0x4d, 0x03, 0x24, 0x00, 0x46, 0x00, 0xb0, 0x01, 0x81, 0x19,
                0x24, 0x00,
            ][..],
        ),
        (
            0x24034d,
            &[
                0x41, 0x00, 0x22, 0x00, 0x00, 0x82, 0x29, 0x00, 0x41, 0x00, 0x23, 0x00, 0x01, 0x82,
                0x2a, 0x01, 0x41, 0x00, 0x26, 0x00, 0x01, 0x82, 0x2d, 0x01, 0x41, 0x00, 0x25, 0x00,
                0x00, 0x82, 0x2c, 0x00,
            ][..],
        ),
        (0x2aba32, &[0x41, 0x08, 0x01, 0x00, 0x04][..]),
        (0x2abb0a, &[0xd1, 0x12, 0x00, 0x00, 0x00, 0xb1][..]),
        (0x2abb8c, &[0xc8, 0x0d, 0x02][..]),
        (0x2abbbd, &[0x47, 0x44, 0x33, 0x32, 0x00][..]),
        (
            0x3833eb,
            &[0x46, 0x00, 0xb0, 0x01, 0x1d, 0x34, 0x38, 0x00][..],
        ),
        (
            0x38341d,
            &[
                0x82, 0x21, 0x01, 0xd0, 0x01, 0x1c, 0x81, 0x0c, 0x00, 0x05, 0x41, 0x08, 0x0c, 0x00,
                0x01, 0x82, 0x05, 0xbc, 0x81, 0x0c, 0x00, 0x02, 0x41, 0x08, 0x0c, 0x00, 0x02, 0x82,
                0x02, 0xbc, 0x41, 0x00, 0x00, 0x00, 0x02, 0x48, 0x84, 0xf3, 0x14, 0x00, 0x83, 0x00,
                0x01, 0x47, 0xe0, 0x59, 0x46, 0x00,
            ][..],
        ),
        (0x14fdea, &[0x9e, 0x02][..]),
        (0x1500c1, &[0x9e, 0x02][..]),
        (
            0x4e710d,
            &[
                0x81, 0x0c, 0x00, 0x05, 0x41, 0x08, 0x0c, 0x00, 0x02, 0x82, 0x05, 0xbc, 0xb0, 0x0c,
                0xd3, 0x01, 0x01, 0x80, 0x65, 0x81, 0x02, 0x00, 0x18, 0x46, 0x06, 0xb2, 0x04, 0x3e,
                0x71, 0x4e, 0x00, 0x81, 0x0c, 0x00, 0x18, 0x41, 0x08, 0x0c, 0x00, 0x04, 0x82, 0x18,
                0xbc, 0xd2, 0x02, 0x83, 0x00, 0x80, 0x78, 0x83, 0x00, 0x01, 0x47, 0x2e, 0xf8, 0x01,
                0x00,
            ][..],
        ),
        (0x1f997, &[0x00, 0x00, 0x01][..]),
    ] {
        pin(&j.runtime, at, bytes);
    }

    driver.until(
        &mut j,
        viewport,
        deadline,
        "real Day3 Rena invitation ready before autoaccept",
        |r| choice_ready(r, 0x23b6c2),
    );
    choose_original(&mut j, viewport, deadline, 0x23b6c2, 1);
    driver.until(
        &mut j,
        viewport,
        deadline,
        "real Day3 Rika shopping ready before autoaccept",
        |r| choice_ready(r, 0x23e657),
    );
    choose_original(&mut j, viewport, deadline, 0x23e657, 0);
    driver.until(
        &mut j,
        viewport,
        deadline,
        "authored Satoko prerequisites committed",
        |r| {
            let Some(g) = r.globals().unwrap() else {
                return false;
            };
            g.data.persist_data.get(42) == 1 && g.data.persist_data.get(45) == 1
        },
    );
    for (slot, value) in [(41, 0), (42, 1), (44, 0), (45, 1)] {
        assert_eq!(
            j.runtime
                .globals()
                .unwrap()
                .unwrap()
                .data
                .persist_data
                .get(slot),
            value
        );
    }
    assert!(
        !driver.pending_pcs.contains(&0x23d4c3),
        "actual Rika route skips Rena treasure SELECT"
    );
    for label in ["4日目", "5日目", "6日目", "7日目", "8日目"] {
        driver.until(&mut j, viewport, deadline, label, |r| {
            story_metadata(r, label)
        });
        assert_eq!(identities(&j.runtime), shared);
        same_manual(&rows(&j.runtime), &before);
        j.no_writes(Some(&initial_file));
    }
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
    choose_satoko(&mut j, viewport, deadline);
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
        "Movie7 natural EOF/actual Satoko1",
        |r| story_metadata(r, "route 沙都子1"),
    );
    assert!(driver.movie_waits.contains(&0x2abb8c));
    assert!(!driver.movie_waits.contains(&0x2abbcc));
    driver.until(&mut j, viewport, deadline, "route 沙都子2", |r| {
        story_metadata(r, "route 沙都子2")
    });
    assert_eq!(identities(&j.runtime), shared);
    same_manual(&rows(&j.runtime), &before);
    j.no_writes(Some(&initial_file));
    driver.until(
        &mut j,
        viewport,
        deadline,
        "actual Satoko bento SELECT before autoaccept",
        |r| choice_ready(r, 0x34174a),
    );
    // Original0 returns to the common Watanagashi branch. Original1 is
    // available through the legitimately earned persist34 prerequisite.
    choose_original(&mut j, viewport, deadline, 0x34174a, 1);
    for label in ["route 沙都子3", "route 沙都子4", "route 沙都子5"] {
        driver.until(&mut j, viewport, deadline, label, |r| {
            story_metadata(r, label)
        });
        assert_eq!(identities(&j.runtime), shared);
        same_manual(&rows(&j.runtime), &before);
        j.no_writes(Some(&initial_file));
    }
    driver.until(
        &mut j,
        viewport,
        deadline,
        "actual Satoko absence SELECT before autoaccept",
        |r| choice_ready(r, 0x383385),
    );
    // Original1 is always available, even when compact visible row0. Native
    // original0 would enter Tsukiotoshi; do not let a default answer choose it.
    choose_original(&mut j, viewport, deadline, 0x383385, 1);
    driver.until(
        &mut j,
        viewport,
        deadline,
        "actual protected preview2/full Tatar",
        |r| story_metadata(r, "祟殺し編"),
    );
    assert_eq!(
        j.runtime
            .globals()
            .unwrap()
            .unwrap()
            .data
            .persist_data
            .get(5),
        1,
        "actual entered flag, not yet clear"
    );
    assert!(
        driver.pending_pcs.contains(&0x14fdea),
        "preview2 first VOICEWAIT2"
    );
    assert!(
        driver.pending_pcs.contains(&0x1500c1),
        "preview2 last VOICEWAIT2"
    );
    pure_frame(&mut j);
    same_manual(&rows(&j.runtime), &before);
    j.no_writes(Some(&initial_file));
    driver.until(
        &mut j,
        viewport,
        deadline,
        "actual OTSUGET2 phase2 acknowledgement",
        |r| {
            pending(r, 0x4e7138) && game(r).unwrap().reference_story_notification() == Some((13, 2))
        },
    );
    // SSETs after the final message remain queued through OTSUGET. Native
    // EXIT commits them after its readiness gates; do not force that commit.
    let (committed, queued) = game(&j.runtime).unwrap().reference_story_variables();
    for (slot, before, after) in [(5, 1, 3), (24, 9, 13)] {
        assert_eq!(committed.get(slot), before, "pre-EXIT committed slot{slot}");
        assert_eq!(
            queued
                .iter()
                .find(|(id, _)| *id == slot as u32)
                .map(|(_, value)| *value),
            Some(after),
            "actual queued ending SSET slot{slot}"
        );
    }
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
        0x23b6c2, 0x23e657, 0x2abb0a, 0x34174a, 0x383385, 0x383420, 0x477cee, 0x47a9fc, 0x489ca8,
        0x48dea2, 0x4b3b92, 0x4cdec6, 0x4d3215, 0x4e7138,
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
    assert_eq!(exit.current.progress.active_entry().scenario_id, 3203);
    assert!(exit.current.progress.active_entry().save_position > 0);
    assert_eq!(
        exit.current.progress.entries[1], initial_current.progress.entries[1],
        "fresh Chart dormant current record retained"
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
        (6, 0),
        (24, 13),
        (33, 1),
        (41, 0),
        (42, 1),
        (44, 0),
        (45, 1),
        (54, 0),
    ];
    for (slot, value) in expected {
        assert_eq!(
            globals.data.persist_data.get(slot),
            value,
            "committed final slot{slot}"
        );
    }
    for tip in 78..=90 {
        assert!(acquired(globals.data, tip), "actual Tatar TIP{tip}");
    }
    assert!(
        globals
            .data
            .save_vectors
            .extra_words_1
            .get(101 / 32)
            .is_some_and(|w| w & (1 << (101 % 32)) != 0)
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
    for (slot, value) in expected {
        assert_eq!(disk.persist_data.get(slot), value, "saved final slot{slot}");
    }
    for tip in 78..=90 {
        assert!(acquired(&disk, tip), "saved Tatar TIP{tip}");
    }
    assert!(j.runtime.failure().is_none());
    pure_frame(&mut j);
    println!(
        "Runtime earned Chart5→Day3 original1/0→CHARSELECT original2→Satoko1..5→finaloriginal1→full Tatar→EXIT0001f997/one save/Title: elapsed{:.3}s,{} Runtimeframes/{} driven callbacks/{}commands;real Movie7 EOF/protectedpreview2/TIPS/OTSUGET;full100manuals+101 EXIT boundary,shared Globals,unchanged actual first+Watan earned input,pure720p/1080p",
        started.elapsed().as_secs_f64(),
        j.runtime.frames,
        driver.callbacks,
        driver.max_commands
    );
}
