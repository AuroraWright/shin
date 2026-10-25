//! Exact accepted cumulative Tsuki disk -> real earned Chart29 -> direct
//! protected preview4/full Taraimawashi -> actual TIPS52/OTSUGET1/EXIT save/Title.
use super::*;
use crate::app::higurashi::{
    chart::owner::Owner as ChartOwner, runtime_game_data::RuntimeGameData, runtime_saves::Slots,
};
use std::{cell::RefCell, collections::BTreeSet};

const TSUKIOTOSHI: &str = "app::higurashi::runtime::tests::journeys::sustained::tsukiotoshi::higurashi_runtime_sustained_earned_chart_satoshi_reaches_full_tsukiotoshi_exit_and_title";
// Actual parent-accepted Tsuki PASS, copied before its Journey path retired.
// Parent launcher additionally pins full SHA256/build/binary/log/transitive
// accepted Tatar/Watan/first-book provenance before running this ignored test.
// Output SHA2569a9ff3ebd50421d4a5ad2da1701489c4616f305c28f9aca71de75a41ef3226a0.
const EARNED_FILE: (usize, u64) = (15364, 0x60fe_31c4_9490_807b);
// PASS SHA256a125bb0151d08838828476bb35d8727085ca2a8a31d88983b95fd720afedd783.
const EARNED_RECORD: (usize, u64) = (348936, 0x9737_c9df_d46d_d689);
const PREVIEW_WAITS: [u32; 8] = [
    0x1508ad, 0x1508c6, 0x1508df, 0x1508f8, 0x150984, 0x1509d9, 0x150b71, 0x150b8a,
];

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3)
    })
}
fn earned() -> (SaveData, Vec<u8>) {
    let (file_len, file_fnv) = EARNED_FILE;
    let (record_len, record_fnv) = EARNED_RECORD;
    let path = std::env::var("SHIN_HIGURASHI_EARNED_TSUKIOTOSHI")
        .expect("require actual accepted cumulative Tsuki output, never a completed template");
    let record = std::env::var("SHIN_HIGURASHI_EARNED_TSUKIOTOSHI_RECORD")
        .expect("require original accepted Tsuki record, not partial/rejected output");
    let proof = std::fs::read(record).unwrap();
    assert_eq!(proof.len(), record_len);
    assert_eq!(fnv(&proof), record_fnv);
    let proof: serde_json::Value = serde_json::from_slice(&proof).unwrap();
    assert_eq!(proof["exit_code"].as_i64(), Some(0));
    assert_eq!(proof["command"][1].as_str(), Some(TSUKIOTOSHI));
    assert!(
        proof["changed_pinned_sources"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(bytes.len(), file_len);
    assert_eq!(fnv(&bytes), file_fnv);
    let saved = SaveData::decode(&bytes).unwrap();
    assert_eq!(
        saved.encode().unwrap(),
        bytes,
        "exact earned input roundtrip"
    );
    // Literal CRC/XOR+v2-prefix inspection confirmed these exact accepted input fields.
    for (slot, value) in [
        (1, 1),
        (2, 3),
        (3, 3),
        (4, 3),
        (5, 3),
        (6, 3),
        (7, 0),
        (8, 0),
        (24, 13),
        (41, 0),
        (54, 1),
    ] {
        assert_eq!(
            saved.persist_data.get(slot),
            value,
            "actual earned input slot{slot}"
        );
    }
    assert_eq!(
        saved.persist_data.get(24) & 2,
        0,
        "new OTSUGET1 is genuinely unearned"
    );
    assert!(saved.auto_save_slot.is_some());
    assert!(
        saved
            .save_vectors
            .extra_words_1
            .get(29 / 32)
            .is_some_and(|word| word & (1 << (29 % 32)) != 0),
        "Chart29 actually earned"
    );
    for tip in [27, 116] {
        assert!(acquired(&saved, tip), "prior actual TIP{tip}");
    }
    for tip in [50, 51, 52] {
        assert!(!acquired(&saved, tip), "new Tarai TIP{tip}");
    }
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
fn assert_direct_route(runtime: &Runtime) {
    let g = game(runtime).unwrap_or_else(|| {
        panic!(
            "left ADV before direct Taraimawashi route: {}",
            Driver::describe(runtime)
        )
    });
    if let Some(h) = g.host() {
        assert!(
            !matches!(
                h.runner.pending_command(),
                Some(Cmd::SELECT { .. } | Cmd::CHARSELECT { .. })
            ),
            "41=0 direct route must not offer a script SELECT/CHARSELECT: {}",
            Driver::describe(runtime)
        );
    }
}
fn tips_ready(runtime: &Runtime, pc: u32) -> bool {
    assert_direct_route(runtime);
    pending(runtime, pc)
        && game(runtime)
            .unwrap()
            .reference_story_choice()
            .is_some_and(|(phase, _, _)| phase == 2)
}
fn continue_tip(j: &mut Journey, viewport: InputViewport, deadline: Instant, pc: u32) {
    for _ in 0..8 {
        assert!(tips_ready(&j.runtime, pc));
        let (_, selected, rows) = game(&j.runtime).unwrap().reference_story_choice().unwrap();
        let wanted = rows
            .iter()
            .position(|(original, _)| *original == 0)
            .unwrap_or_else(|| panic!("actual TIPS{pc:08x} Continue original0 absent: {rows:?}"));
        if selected == wanted as i32 {
            j.tap(KeyCode::Enter, viewport);
            j.until(
                viewport,
                deadline,
                "actual TIPS52 close/Continue callback",
                |r| {
                    assert_direct_route(r);
                    !pending(r, pc)
                },
            );
            return;
        }
        j.tap(KeyCode::ArrowDown, viewport);
    }
    panic!("actual TIPS{pc:08x} Continue original0 was not selected");
}
fn acquired(saved: &SaveData, tip: usize) -> bool {
    let bit = 2 * tip;
    saved
        .save_vectors
        .extra_words_0
        .get(bit / 32)
        .is_some_and(|w| w & (1 << (bit % 32)) != 0)
}
fn pin(runtime: &Runtime, at: usize, raw_hex: &str) {
    assert_eq!(raw_hex.len() % 2, 0);
    let bytes = (0..raw_hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&raw_hex[i..i + 2], 16).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        &runtime.globals().unwrap().unwrap().common.scenario.raw()[at..at + bytes.len()],
        bytes.as_slice(),
        "original authored raw bytes at{at:08x}"
    );
}
struct ExitBoundary {
    slots: Slots,
    current: RuntimeGameData,
}

#[test]
#[ignore = "actual accepted cumulative Tsuki output+record required; real Runtime/earned Chart29/forced Tarai/protected preview4/full Taraimawashi/TIPS52+OTSUGET1/native EXIT save+Title; actual assets/offscreen Metal/the default audio output; temporary writes only; two hour observation allowance"]
fn higurashi_runtime_sustained_earned_chart_reaches_full_taraimawashi_exit_and_title() {
    let (saved, initial_file) = earned();
    let started = Instant::now();
    let deadline = started + Duration::from_secs(2 * 60 * 60);
    let viewport = viewport((1920, 1080));
    let mut j = Journey::new("sustained-earned-taraimawashi", &saved);
    j.boot_title(viewport, deadline);
    let before = rows(&j.runtime);
    let shared = identities(&j.runtime);
    let clock = j.runtime.globals().unwrap().unwrap().clock.ticks();
    let before_awards = saved.persist_data.get(24);
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
    j.until(
        viewport,
        deadline,
        "actual retained Title Chart",
        chart_ready,
    );
    assert_eq!(
        chart(&j.runtime).unwrap().reference_selected_entry(),
        Some((0, 3000))
    );
    assert_eq!(cursor(&j.runtime), [18., 28.]);
    // Native node29 is logical(0,38), center(18,2156), hitY[2121,2191).
    // Observed +10 movements from28 stop at2158; no fixed callback count.
    steer(&mut j, viewport, deadline, 1, 2158., KeyCode::ArrowDown);
    assert_eq!(cursor(&j.runtime), [18., 2158.]);
    assert_eq!(
        chart(&j.runtime).unwrap().reference_selected_entry(),
        Some((29, 3221))
    );
    pure_frame(&mut j);
    same_manual(&rows(&j.runtime), &before);
    j.no_writes(Some(&initial_file));
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
    let mut driver = Driver::new();
    driver.until(
        &mut j,
        viewport,
        deadline,
        "actual Chart fresh ADV3221/common WatanA",
        |r| metadata(r, "route 綿流しA"),
    );
    assert!(j.runtime.slot().unwrap().title.is_none());
    let initial_current = game(&j.runtime).unwrap().runtime_game_data().unwrap();
    assert_eq!(initial_current.progress.active_index(), 0);
    assert_eq!(initial_current.progress.active_entry().scenario_id, 3221);
    assert_eq!(identities(&j.runtime), shared);
    for (at, raw) in [
        (0x1e57b, "4600b0900c958f6f2b00"),
        (0x2b6fae, "d300011d"),
        (0x2c7fa6, "4600c0160051802c00"),
        (
            0x2c8117,
            "4602c0220032812c0041002200004600c0170032812c008122002941001800c0224600c0180062812c004602c024005c812c0041002400004600c017005c812c008124002b41001800c0244600c0180006822c00",
        ),
        (
            0x2c8206,
            "8a0083001e810c000741080c00018207bc810c000241080c00028202bc41000000044884f31400830001471fe65400",
        ),
        (0x14f3a4, "4600b0043f081500"),
        (0x15083f, "a200"),
        (0x1508ad, "9e02"),
        (0x150b8a, "9e02"),
        (0x54e646, "a1d3000220804b"),
        (0x5581ac, "d0023233"),
        (
            0x580efc,
            "810c000741080c00028207bcb00ed0013483008078d301018054810200184606b202340f5800810c001841080c00028218bcd20183008078830001472ef80100",
        ),
        (
            0x1f82e,
            "810200034686b2027bf80100810200044686b2027bf80100810200064686b2027bf80100810200054686b2027bf80100810200074686b2027bf80100810200084686b2027bf801004813aa05004797f90100",
        ),
        (0x1f997, "000001"),
    ] {
        pin(&j.runtime, at, raw);
    }
    let preview_waits = RefCell::new(BTreeSet::<u32>::new());
    driver.until(
        &mut j,
        viewport,
        deadline,
        "direct forced Tarai/protected preview4/full book",
        |r| {
            assert_direct_route(r);
            let g = game(r).unwrap();
            if let Some(h) = g.host() {
                let pc = h.runner.position().0;
                if PREVIEW_WAITS.contains(&pc)
                    && matches!(
                        h.runner.pending_command(),
                        Some(Cmd::VOICEWAIT { status: 2 })
                    )
                {
                    assert_eq!(
                        h.runner.native_read_state().unwrap().flags & 0x8000_0000,
                        0,
                        "actual EVBEGIN0 keeps preview VOICEWAIT protected from skip"
                    );
                    preview_waits.borrow_mut().insert(pc);
                }
            }
            story_metadata(r, "盥回し編")
        },
    );
    let preview_waits = preview_waits.into_inner();
    assert!(
        !preview_waits.is_empty(),
        "observed actual protected preview4 VOICEWAIT2"
    );
    // Static sites are independently pinned; an already complete voice need
    // not remain pending, so do not invent an all-eight pending requirement.
    assert!(preview_waits.iter().all(|pc| PREVIEW_WAITS.contains(pc)));
    assert_eq!(
        game(&j.runtime)
            .unwrap()
            .reference_story_variables()
            .0
            .get(7),
        1,
        "entered, not yet cleared"
    );
    assert!(
        driver.movie_waits.is_empty(),
        "Chart29 does not replay Movie7"
    );
    pure_frame(&mut j);
    same_manual(&rows(&j.runtime), &before);
    j.no_writes(Some(&initial_file));
    driver.until(
        &mut j,
        viewport,
        deadline,
        "actual final TIPS52 Continue child",
        |r| tips_ready(r, 0x580f0a),
    );
    let (committed, queued) = game(&j.runtime).unwrap().reference_story_variables();
    // TIPS constructor1185c commits SSET7=3 before opening this actual child.
    assert_eq!(committed.get(7), 3);
    assert!(!queued.iter().any(|(slot, _)| *slot == 7));
    assert_eq!(committed.get(24), before_awards);
    assert!(!queued.iter().any(|(slot, _)| *slot == 24));
    for tip in [50, 51, 52] {
        assert!(
            acquired(j.runtime.globals().unwrap().unwrap().data, tip),
            "actual TIP{tip}"
        );
    }
    assert!(
        driver.pending_pcs.contains(&0x5581ac),
        "actual TIPS50/51 command"
    );
    assert!(driver.exits.is_empty());
    same_manual(&rows(&j.runtime), &before);
    j.no_writes(Some(&initial_file));
    pure_frame(&mut j);
    continue_tip(&mut j, viewport, deadline, 0x580f0a);
    driver.until(
        &mut j,
        viewport,
        deadline,
        "actual new OTSUGET1 before acknowledgement",
        |r| {
            assert_direct_route(r);
            pending(r, 0x580f2e) && game(r).unwrap().reference_story_notification() == Some((13, 2))
        },
    );
    let (committed, queued) = game(&j.runtime).unwrap().reference_story_variables();
    assert_eq!(committed.get(7), 3);
    assert_eq!(
        committed.get(24),
        before_awards,
        "OTSUGET does not commit queued award"
    );
    assert_eq!(
        queued.iter().find(|(slot, _)| *slot == 24).map(|(_, v)| *v),
        Some(before_awards | 2)
    );
    assert!(!queued.iter().any(|(slot, _)| *slot == 7));
    same_manual(&rows(&j.runtime), &before);
    j.no_writes(Some(&initial_file));
    pure_frame(&mut j);
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
    for pc in [0x5581ac, 0x580f0a, 0x580f2e] {
        assert!(
            driver.pending_pcs.contains(&pc),
            "actual owned wait{pc:08x}"
        );
    }
    for pc in [0x2c8006, 0x2c816b, 0x34174a, 0x36cfc7, 0x383385] {
        assert!(
            !driver.pending_pcs.contains(&pc),
            "no replay/incorrect choice{pc:08x}"
        );
    }
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
    assert_eq!(exit.current.progress.active_entry().scenario_id, 3221);
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
        (7, 3),
        (8, 0),
        (24, before_awards | 2),
        (41, 0),
        (54, 1),
    ];
    for (slot, value) in expected {
        assert_eq!(
            globals.data.persist_data.get(slot),
            value,
            "final committed slot{slot}"
        );
    }
    for tip in [27, 50, 51, 52, 116] {
        assert!(acquired(globals.data, tip), "actual acquired TIP{tip}");
    }
    for node in [29, 32, 75, 76, 83, 84] {
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
        "wire projection separate from full101 equality"
    );
    for (slot, value) in expected {
        assert_eq!(
            disk.persist_data.get(slot),
            value,
            "saved committed slot{slot}"
        );
    }
    for tip in [27, 50, 51, 52, 116] {
        assert!(acquired(&disk, tip), "saved TIP{tip}");
    }
    assert!(j.runtime.failure().is_none());
    pure_frame(&mut j);
    println!(
        "Runtime accepted cumulative Tsuki→earned Chart29/scenario3221→41=0 direct Tarai/protected preview4→full Taraimawashi/TIPS52 committed7/OTSUGET1 queued24→EXIT0001f997/one save/ready Title: elapsed{:.3}s,{} Runtimeframes/{} callbacks/{}commands;full100manuals+101 EXIT boundary/shared owners/dormant current/pure720p1080p;observed preview waits{:x?};no commonDays/Bento/Character/Movie7 replay",
        started.elapsed().as_secs_f64(),
        j.runtime.frames,
        driver.callbacks,
        driver.max_commands,
        preview_waits
    );
}
