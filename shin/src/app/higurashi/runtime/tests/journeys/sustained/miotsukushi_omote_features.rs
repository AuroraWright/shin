//! Ordinary Miotsukushi Omote403 from the exact unlocked-template input.
//! Fixture unlock provenance, not an earned-unlock claim. No state mutation.
//! Full-route acceptance requires actual ending, saved output and returned Title.
use super::*;
use shin_core::format::scenario::instruction_elements::Register;
use std::path::PathBuf;

const PLAN: [(u32, usize); 11] = [
    (0x00c7004c, 1),
    (0x00c95133, 1),
    (0x00c96593, 2),
    (0x00cb64ee, 1),
    (0x00cbd333, 0),
    (0x00cbf8f3, 1),
    (0x00cc16a3, 0),
    (0x00ccd95b, 1),
    (0x00cce1b4, 1),
    (0x00ccf6a2, 1),
    (0x00ce1dd2, 0),
];
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
    assert_eq!(data.persist_data.get(125), 2);
    for slot in [18, 19, 20, 21, 22, 23] {
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
            "MIOTSU_CAPTURE {label} {}x{} bytes={}",
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
        "Miotsukushi Omote story deadline: {}",
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
    eprintln!("MIOTSU_CHOICE pc={pc:08x} original={original} rows={rows:?}");
    capture(j, out, &format!("choice-{pc:08x}"));
    j.tap(KeyCode::Enter, vp);
    j.until(vp, deadline, "real SELECT closing callback", |r| {
        !pending(r, pc)
    });
}

#[test]
#[ignore = "Exact unlocked template; normal Category3/SNRSEL3 row3/403;11explicit choices/two score gates/protected natural Movie1/BGM90credits/END371/commonEXIT+Title; real assets/Metal/exactLoopback; no earned unlock,KAKERAGET65 or physical HID claim"]
fn higurashi_runtime_miotsukushi_omote_unlocked_template_full_good_end() {
    let (saved, initial) = template();
    let out = PathBuf::from(
        std::env::var("SHIN_HIGURASHI_MIOTSU_CAPTURE").expect("owned retained capture directory"),
    );
    std::fs::create_dir_all(&out).unwrap();
    let settings = serde_json::to_vec(&saved.settings).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2 * 60 * 60);
    let vp = viewport((1920, 1080));
    let mut j = Journey::new("miotsukushi-omote-template", &saved);
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
    for row in [1, 2, 3] {
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
    j.until(vp, deadline, "actual Category3 VM", |r| {
        scenario(r) == Some(3)
            && game(r).is_some_and(|g| g.host().is_some_and(|h| h.runner.native_vm_attached()))
    });
    let mut release = false;
    while !picker_ready(&j.runtime) {
        story_step(&mut j, vp, deadline, &mut release);
    }
    {
        let p = j.runtime.slot().unwrap().scenario_select.as_ref().unwrap();
        let m = p.reference_owner().reference_model();
        assert_eq!((m.kind, m.selected, m.count), (3, 0, 9));
        assert_ne!(m.mask & 8, 0);
        assert_eq!(m.data.progress.active_entry().scenario_id, 3);
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
    j.tap(KeyCode::ArrowDown, vp);
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
        3
    );
    j.tap(KeyCode::Enter, vp);
    j.until(vp, deadline, "actual403 VM", |r| {
        scenario(r) == Some(403)
            && game(r).is_some_and(|g| g.host().is_some_and(|h| h.runner.native_vm_attached()))
    });
    let mut next = 0usize;
    let mut final_exit = None;
    let mut saw_end = false;
    let mut protected_sync = 0u64;
    let mut protected_scroll = 0u64;
    let mut protected_wait = 0u64;
    let mut credit_holds = [0u64; 6];
    let mut captured_credit = false;
    let mut saw_epilogue = false;
    let mut score_gates = [false; 2];
    let mut iterations = 0u64;
    let mut max_commands = 0;
    let mut movie_samples = 0u64;
    let mut movie_started = None;
    let mut movie_last_us = 0;
    let mut movie_passed = false;
    let mut captured_movie = false;
    while !ready_menu(&j.runtime, "top") {
        assert!(iterations < 1_000_000, "full403 main-loop budget");
        iterations += 1;
        let mut capture_label = None;
        if let Some(g) = game(&j.runtime) {
            if let Some(h) = g.host().filter(|h| h.runner.native_vm_attached()) {
                assert_eq!(
                    scenario(&j.runtime),
                    Some(403),
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
                    saw_end |= m.current()[1].as_ref() == "END (さよならのかわりに)";
                }
                if pc == 0xc68240 && h.runner.pending_command().is_some() {
                    assert!(matches!(
                        h.runner.pending_command(),
                        Some(Cmd::MOVIEWAIT { .. })
                    ));
                    assert_eq!(
                        h.runner.native_read_state().unwrap().flags & 0x8000_0000,
                        0,
                        "Authored EVBEGIN0 protects actual in-story Movie1 while Ctrl held"
                    );
                    let layers = h.runner.frame().scene.layers();
                    let target = layers.state.target(13).expect("Authored Movie logical13");
                    let owner = layers
                        .layer(target)
                        .unwrap()
                        .reference_movie()
                        .expect("Real Movie7 owner");
                    assert!(
                        owner.fault().is_none(),
                        "A decoder fault is not natural EOF"
                    );
                    let state = owner.reference_state();
                    assert!(state.1, "No forced stop or pause");
                    if state.0 == 1 {
                        movie_started.get_or_insert_with(Instant::now);
                        assert!(state.2 >= movie_last_us, "No media seek or clock reversal");
                        movie_last_us = state.2;
                        movie_samples += 1;
                        if movie_last_us >= 60_000_000 && !captured_movie {
                            capture_label = Some("protected-movie-mid");
                            captured_movie = true;
                        }
                    }
                } else if movie_started.is_some() && !movie_passed {
                    assert!(
                        movie_last_us >= 131_000_000,
                        "132.032s movie did not run naturally to its tail"
                    );
                    let elapsed = movie_started.unwrap().elapsed().as_secs_f64();
                    assert!(
                        elapsed >= 131.,
                        "No accelerated media clock or forced Movie completion"
                    );
                    movie_passed = true;
                    eprintln!(
                        "MIOTSU_MOVIE_PASS last_us={movie_last_us} playing_wall_seconds={elapsed:.6} samples={movie_samples}; protected MOVIEWAIT13,2 returned without forced input"
                    );
                }
                if matches!(h.runner.pending_command(), Some(Cmd::SELECT { .. })) {
                    let &(wanted_pc, original) = PLAN.get(next).expect("Unexpected extra SELECT");
                    assert_eq!(
                        pc, wanted_pc,
                        "Exact ordinary SELECT order; no debug/untaken choice accepted"
                    );
                    if (4..10).contains(&next) {
                        assert_eq!(
                            h.runner
                                .reference_register(Register::from_regular_register(31)),
                            Some(0),
                            "Both declared score groups remain0 under authored answers"
                        );
                    }
                    choose(&mut j, vp, deadline, wanted_pc, original, &out);
                    next += 1;
                    release = false;
                    unchanged(&j, &before, &settings, ids);
                    continue;
                }
                assert!(
                    !matches!(
                        h.runner.pending_command(),
                        Some(Cmd::CHARSELECT { .. } | Cmd::FAKESELECT {} | Cmd::KAKERAGET { .. })
                    ),
                    "Exact input125=2 bypasses KAKERAGET65; no unexpected interactive child"
                );
                for (i, range) in [(0xcc3ae3..0xccd956), (0xcd4b83..0xce1dd2)]
                    .iter()
                    .enumerate()
                {
                    if range.contains(&pc) && !score_gates[i] {
                        assert_eq!(
                            h.runner
                                .reference_register(Register::from_regular_register(31)),
                            Some(0)
                        );
                        score_gates[i] = true;
                        eprintln!("MIOTSU_SCORE_GATE group={i} score=0 after_pc={pc:08x}");
                    }
                }
                if matches!(pc, 0xdc1d2e | 0xdc1dde | 0xdc1de2)
                    && h.runner.pending_command().is_some()
                {
                    assert_eq!(
                        h.runner.native_read_state().unwrap().flags & 0x8000_0000,
                        0,
                        "Authored EVBEGIN0 protects actual ending waits even while Ctrl held"
                    );
                    match pc {
                        0xdc1d2e => {
                            assert!(matches!(
                                h.runner.pending_command(),
                                Some(Cmd::BGMSYNC { time: 660 })
                            ));
                            protected_sync += 1;
                        }
                        0xdc1dde => {
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
                }
                if let Some(i) = [0xdc1d72, 0xdc1d84, 0xdc1d96, 0xdc1da8, 0xdc1dba, 0xdc1dcc]
                    .iter()
                    .position(|&x| x == pc)
                {
                    assert!(matches!(
                        h.runner.pending_command(),
                        Some(Cmd::WAIT {
                            allow_interrupt: 0,
                            amount: 1320
                        })
                    ));
                    assert_eq!(h.runner.native_read_state().unwrap().flags & 0x8000_0000, 0);
                    credit_holds[i] += 1;
                    if i == 1 && credit_holds[i] == 220 {
                        capture_label = Some("protected-credit-scroll");
                        captured_credit = true;
                    }
                }
                saw_epilogue |= (0x8f916..0x8fdf3).contains(&pc);
                if matches!(h.runner.pending_command(), Some(Cmd::EXIT { arg1: 0, .. })) {
                    assert!(
                        saw_end && next == PLAN.len(),
                        "No early/bad return accepted"
                    );
                    final_exit = Some(pc);
                }
            }
        }
        if let Some(label) = capture_label {
            capture(&mut j, &out, label);
        }
        story_step(&mut j, vp, deadline, &mut release);
    }
    assert_eq!(next, PLAN.len());
    assert!(saw_end && saw_epilogue && score_gates.iter().all(|x| *x));
    assert!(movie_passed && movie_samples > 100 && captured_movie);
    assert_eq!(final_exit, Some(0x1f997));
    assert!(
        protected_sync > 0
            && protected_scroll > 0
            && protected_wait > 0
            && credit_holds.iter().all(|&x| x > 0)
            && captured_credit,
        "Actual protected BGM90 sync, six credit holds, scrolling and final wait observed"
    );
    unchanged(&j, &before, &settings, ids);
    let writes = j.storage.writes.lock().unwrap();
    assert_eq!(writes.len(), 1);
    let actual = std::fs::read(j.storage.paths.current.as_ref().unwrap()).unwrap();
    assert_eq!(writes[0], actual);
    let result = SaveData::decode(&actual).unwrap();
    assert_eq!(result.encode().unwrap(), actual);
    for slot in [18, 19, 20, 21, 22, 23] {
        assert_eq!(result.persist_data.get(slot), 3);
    }
    assert_eq!(result.persist_data.get(125), 2);
    drop(writes);
    capture(&mut j, &out, "returned-title");
    eprintln!(
        "MIOTSU_PASS fixture_unlocked403 eleven choices/fullEND371/protectedMovie1/BGM90/EXIT1f997/Title; main_loop_iterations={iterations} max_commands={max_commands} protected={protected_sync}/{protected_scroll}/{protected_wait} credit_holds={credit_holds:?}; no earned-unlock,KAKERAGET65,native-whole-frame or HID claim"
    );
}
