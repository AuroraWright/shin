//! Ordinary Tsumihoroboshi301 from the exact unlocked-template input.
//! Fixture unlock provenance, not an earned-unlock claim. No state mutation.
//! Full-route acceptance requires actual ending, saved output and returned Title.
use super::*;
use shin_core::format::scenario::instruction_elements::Register;
use std::path::PathBuf;

const PLAN: [(u32, usize); 3] = [(0x893988, 1), (0x8e1c28, 0), (0x908fcc, 1)];
fn template() -> (SaveData, Vec<u8>) {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../test-fixtures/higurashi/chapter-playthrough-20261015/unlocked-input.dat"
    ))
    .unwrap();
    assert_eq!(bytes.len(), 31748);
    assert_eq!(
        bytes
            .iter()
            .fold(0xcbf29ce484222325u64, |h, b| (h ^ u64::from(*b))
                .wrapping_mul(0x100000001b3)),
        0x83007166ab0dfcf0
    );
    let data = SaveData::decode(&bytes).unwrap();
    assert_eq!(data.encode().unwrap(), bytes);
    for slot in [28, 30, 33, 36, 38, 40] {
        assert_eq!(data.persist_data.get(slot), 1);
    }
    for slot in [13, 14, 15, 16] {
        assert_eq!(data.persist_data.get(slot), 3);
    }
    (data, bytes)
}
fn scenario(r: &Runtime) -> Option<i32> {
    game(r)?
        .runtime_game_data()
        .ok()
        .map(|d| d.progress.active_entry().scenario_id)
}
fn picker_ready(r: &Runtime) -> bool {
    r.slot()
        .and_then(|s| s.scenario_select.as_ref())
        .is_some_and(|p| {
            let o = p.reference_owner();
            let s = o.reference_state();
            let m = o.reference_model();
            s.0 && s.2 && s.3.is_none() && m.ready && !m.closing
        })
}
fn pending(r: &Runtime, pc: u32) -> bool {
    game(r).and_then(|g| g.host()).is_some_and(|h| {
        h.runner.native_vm_attached()
            && h.runner.position().0 == pc
            && h.runner.pending_command().is_some()
    })
}
fn unchanged(
    j: &Journey,
    before: &crate::app::higurashi::runtime_saves::Slots,
    settings: &[u8],
    ids: [usize; 5],
) {
    let g = j.runtime.globals().unwrap().unwrap();
    assert_eq!(serde_json::to_vec(&g.data.settings).unwrap(), settings);
    assert_eq!(
        [
            Arc::as_ptr(g.common) as usize,
            Arc::as_ptr(g.server) as usize,
            Arc::as_ptr(g.manager) as usize,
            Arc::as_ptr(g.sounds) as usize,
            Arc::as_ptr(g.storage.unwrap()) as *const () as usize
        ],
        ids
    );
    for (i, (a, b)) in g
        .runtime_saves
        .unwrap()
        .manual
        .iter()
        .zip(&before.manual)
        .enumerate()
    {
        assert_eq!(a.valid, b.valid, "manual{i} validity");
        assert_eq!(a.data, b.data, "manual{i} complete payload");
    }
}
fn capture(j: &mut Journey, out: &std::path::Path, label: &str) {
    let frames = j.runtime.frames;
    let ticks = j.runtime.globals().unwrap().unwrap().clock.ticks();
    for size in [(1280, 720), (1920, 1080)] {
        let a = j.gpu.pixels(&j.runtime, size, false);
        assert_eq!(
            a,
            j.gpu.pixels(&j.runtime, size, true),
            "pure completed framebuffer presentation"
        );
        std::fs::write(out.join(format!("{label}-{}x{}.rgba", size.0, size.1)), &a).unwrap();
        eprintln!(
            "TSUMI_CAPTURE {label} {}x{} bytes={}",
            size.0,
            size.1,
            a.len()
        );
    }
    assert_eq!(j.runtime.frames, frames);
    assert_eq!(j.runtime.globals().unwrap().unwrap().clock.ticks(), ticks);
}
// Automated canonical keyboard Ctrl plus explicit acquisition-notice/Continue
// input. No direct skip/clock/VM/response assignment and no arbitrary SELECT.
fn story_step(j: &mut Journey, vp: InputViewport, deadline: Instant, release: &mut bool) {
    assert!(
        Instant::now() < deadline,
        "Tsumihoroboshi story deadline: {}",
        Driver::describe(&j.runtime)
    );
    let mut raw = key(KeyCode::ControlLeft);
    let mut action = None;
    if let Some(g) = game(&j.runtime) {
        assert_eq!(g.reference_play_modes() & 2, 0, "No latched skip");
        if let Some(h) = g.host().filter(|h| h.runner.native_vm_attached()) {
            assert!(
                !matches!(
                    h.runner.pending_command(),
                    Some(Cmd::SELECT { .. } | Cmd::CHARSELECT { .. } | Cmd::FAKESELECT {})
                ),
                "Interactive story choice must be handled by exact plan"
            );
        }
        if let Some((phase, selected, rows)) = g.reference_story_choice() {
            assert!(
                g.host().is_some_and(|h| matches!(
                    h.runner.pending_command(),
                    Some(Cmd::TIPSGET { .. })
                ))
            );
            if phase == 2 && g.input_permission && !*release {
                let target = rows
                    .iter()
                    .position(|(id, _)| *id == 0)
                    .expect("TIPS Continue original0");
                action = Some(if selected == target as i32 {
                    KeyCode::Enter
                } else {
                    KeyCode::ArrowDown
                });
            }
        } else if matches!(g.reference_story_notification(), Some((12..=14, 2))) && !*release {
            action = Some(KeyCode::Enter);
        }
    }
    *release = action.is_some();
    if let Some(k) = action {
        raw.keyboard.insert(k);
    }
    assert!(matches!(j.step(&raw, vp), Frame::Advanced(_)));
}
fn choose(
    j: &mut Journey,
    vp: InputViewport,
    deadline: Instant,
    pc: u32,
    original: usize,
    out: &std::path::Path,
) {
    assert!(pending(&j.runtime, pc));
    // Release temporary Ctrl; real entrance/closing owners still run normally.
    j.step(&RawInputState::new(), vp);
    j.until(vp, deadline, "declared SELECT ready", |r| {
        pending(r, pc)
            && game(r).is_some_and(|g| {
                g.input_permission && g.reference_story_choice().is_some_and(|(p, _, _)| p == 2)
            })
    });
    for _ in 0..4 {
        let (_, selected, rows) = game(&j.runtime).unwrap().reference_story_choice().unwrap();
        let wanted = rows
            .iter()
            .position(|(id, _)| *id == original)
            .expect("Declared original choice visible");
        if selected == wanted as i32 {
            break;
        }
        j.tap(KeyCode::ArrowDown, vp);
    }
    let (_, selected, rows) = game(&j.runtime).unwrap().reference_story_choice().unwrap();
    assert_eq!(rows[selected as usize].0, original);
    eprintln!("TSUMI_CHOICE pc={pc:08x} original={original} rows={rows:?}");
    capture(j, out, &format!("choice-{pc:08x}"));
    j.tap(KeyCode::Enter, vp);
    j.until(vp, deadline, "real SELECT closing callback", |r| {
        !pending(r, pc)
    });
}

#[test]
#[ignore = "Exact unlocked template; normal Category2/SNRSEL2 row1/301; explicit original choices1,0,1/protected BGM89 credits/finalTIPS130+115/commonEXIT+Title; real assets/Metal/exactLoopback; not earned unlock or physical HID"]
fn higurashi_runtime_tsumihoroboshi_unlocked_template_full_good_end() {
    let (saved, initial) = template();
    let out = PathBuf::from(
        std::env::var("SHIN_HIGURASHI_TSUMI_CAPTURE").expect("owned retained capture directory"),
    );
    std::fs::create_dir_all(&out).unwrap();
    let settings = serde_json::to_vec(&saved.settings).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2 * 60 * 60);
    let vp = viewport((1920, 1080));
    let mut j = Journey::new("tsumihoroboshi-template", &saved);
    j.boot_title(vp, deadline);
    let g = j.runtime.globals().unwrap().unwrap();
    let before = g.runtime_saves.unwrap().clone();
    let ids = [
        Arc::as_ptr(g.common) as usize,
        Arc::as_ptr(g.server) as usize,
        Arc::as_ptr(g.manager) as usize,
        Arc::as_ptr(g.sounds) as usize,
        Arc::as_ptr(g.storage.unwrap()) as *const () as usize,
    ];
    j.no_writes(Some(&initial));
    if j.runtime
        .slot()
        .unwrap()
        .title
        .as_ref()
        .unwrap()
        .reference_top_selection()
        == Some(1)
    {
        j.tap(KeyCode::ArrowLeft, vp);
    }
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
    j.tap(KeyCode::Enter, vp);
    j.until(vp, deadline, "Category ready", |r| ready_menu(r, "cat"));
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
    for row in [1, 2] {
        j.tap(KeyCode::ArrowDown, vp);
        assert_eq!(
            j.runtime
                .slot()
                .unwrap()
                .title
                .as_ref()
                .unwrap()
                .reference_category(),
            Some((0, row, 2))
        );
    }
    j.tap(KeyCode::Enter, vp);
    j.until(vp, deadline, "actual Category2 VM", |r| {
        scenario(r) == Some(2)
            && game(r).is_some_and(|g| g.host().is_some_and(|h| h.runner.native_vm_attached()))
    });
    let mut release = false;
    while !picker_ready(&j.runtime) {
        story_step(&mut j, vp, deadline, &mut release);
    }
    {
        let p = j.runtime.slot().unwrap().scenario_select.as_ref().unwrap();
        let m = p.reference_owner().reference_model();
        assert_eq!((m.kind, m.selected, m.count), (2, 0, 6));
        assert_ne!(m.mask & 2, 0);
        assert_eq!(m.data.progress.active_entry().scenario_id, 2);
    }
    j.tap(KeyCode::ArrowLeft, vp);
    assert_eq!(
        j.runtime
            .slot()
            .unwrap()
            .scenario_select
            .as_ref()
            .unwrap()
            .reference_owner()
            .reference_model()
            .selected,
        1
    );
    j.tap(KeyCode::Enter, vp);
    j.until(vp, deadline, "actual301 VM", |r| {
        scenario(r) == Some(301)
            && game(r).is_some_and(|g| g.host().is_some_and(|h| h.runner.native_vm_attached()))
    });
    let mut next = 0usize;
    let mut final_tip = false;
    let mut final_exit = None;
    let mut saw_end = false;
    let mut protected_sync = 0u64;
    let mut protected_scroll = 0u64;
    let mut protected_wait = 0u64;
    let mut saw_epilogue = false;
    let mut frames = 0u64;
    let mut max_commands = 0;
    while !ready_menu(&j.runtime, "top") {
        assert!(frames < 1_000_000, "full301 callback budget");
        frames += 1;
        let mut capture_scroll = false;
        if let Some(g) = game(&j.runtime) {
            if let Some(h) = g.host().filter(|h| h.runner.native_vm_attached()) {
                assert_eq!(
                    scenario(&j.runtime),
                    Some(301),
                    "Early or unrelated scene replacement"
                );
                for reg in [22, 23] {
                    assert_eq!(
                        h.runner
                            .reference_register(Register::from_regular_register(reg)),
                        Some(0),
                        "Ordinary full entry, no debug NCSELECT or Chart section start"
                    );
                }
                max_commands = max_commands.max(h.runner.completed_commands());
                let pc = h.runner.position().0;
                if let Some(m) = h.runner.native_save_metadata() {
                    assert!(
                        !m.current()[1].as_ref().starts_with("BADEND"),
                        "Bad ending is not full good completion"
                    );
                    saw_end |= m.current()[1].as_ref() == "END";
                }
                if matches!(h.runner.pending_command(), Some(Cmd::SELECT { .. })) {
                    let &(wanted_pc, original) = PLAN.get(next).expect("Unexpected extra SELECT");
                    assert_eq!(
                        pc, wanted_pc,
                        "Exact ordinary SELECT order; no debug/untaken choice accepted"
                    );
                    // All three opening automatic successful flags are established by real authored code.
                    for reg in [28, 29, 30] {
                        assert_eq!(
                            h.runner
                                .reference_register(Register::from_regular_register(reg)),
                            Some(1)
                        );
                    }
                    choose(&mut j, vp, deadline, wanted_pc, original, &out);
                    next += 1;
                    release = false;
                    unchanged(&j, &before, &settings, ids);
                    continue;
                }
                assert!(!matches!(
                    h.runner.pending_command(),
                    Some(Cmd::CHARSELECT { .. } | Cmd::FAKESELECT {})
                ));
                if matches!(pc, 0x92218c | 0x9221bf | 0x9221c3)
                    && h.runner.pending_command().is_some()
                {
                    assert_eq!(
                        h.runner.native_read_state().unwrap().flags & 0x8000_0000,
                        0,
                        "Authored EVBEGIN0 protects actual ending waits even while Ctrl held"
                    );
                    match pc {
                        0x92218c => {
                            assert!(matches!(
                                h.runner.pending_command(),
                                Some(Cmd::BGMSYNC { time: 660 })
                            ));
                            protected_sync += 1;
                        }
                        0x9221bf => {
                            assert!(matches!(
                                h.runner.pending_command(),
                                Some(Cmd::LAYERWAIT { .. })
                            ));
                            protected_scroll += 1;
                        }
                        _ => {
                            assert!(matches!(
                                h.runner.pending_command(),
                                Some(Cmd::WAIT {
                                    allow_interrupt: 0,
                                    amount: 300
                                })
                            ));
                            protected_wait += 1;
                        }
                    }
                    capture_scroll = pc == 0x9221bf && protected_scroll == 2;
                }
                if pc == 0x9222fb && matches!(h.runner.pending_command(), Some(Cmd::TIPSGET { .. }))
                {
                    final_tip = true;
                }
                saw_epilogue |= (0x60166..0x6247f).contains(&pc);
                if matches!(h.runner.pending_command(), Some(Cmd::EXIT { arg1: 0, .. })) {
                    assert!(
                        final_tip && next == PLAN.len(),
                        "No early/bad return accepted"
                    );
                    final_exit = Some(pc);
                }
            }
        }
        if capture_scroll {
            capture(&mut j, &out, "protected-credit-scroll");
        }
        story_step(&mut j, vp, deadline, &mut release);
    }
    assert_eq!(next, PLAN.len());
    assert!(saw_end && final_tip && saw_epilogue);
    assert_eq!(final_exit, Some(0x1f997));
    assert!(
        protected_sync > 0 && protected_scroll >= 2 && protected_wait > 0,
        "Actual protected BGM89 sync, scrolling and final wait observed"
    );
    unchanged(&j, &before, &settings, ids);
    let writes = j.storage.writes.lock().unwrap();
    assert_eq!(writes.len(), 1);
    let actual = std::fs::read(j.storage.paths.current.as_ref().unwrap()).unwrap();
    assert_eq!(writes[0], actual);
    let result = SaveData::decode(&actual).unwrap();
    assert_eq!(result.encode().unwrap(), actual);
    for slot in [13, 14, 15, 16] {
        assert_eq!(result.persist_data.get(slot), 3);
    }
    drop(writes);
    capture(&mut j, &out, "returned-title");
    eprintln!(
        "TSUMI_PASS fixture_unlocked301 originals1,0,1/fullEND249/finalTIPS130+115/EXIT1f997/Title; callbacks={frames} max_commands={max_commands} protected={protected_sync}/{protected_scroll}/{protected_wait}; no earned-unlock/native-whole-frame/HID claim"
    );
}
