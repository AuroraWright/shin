//! Ordinary Kamikanshi501 from the exact unlocked-template input.
//! Fixture unlock provenance, not an earned-unlock claim. No state mutation.
//! Full-route acceptance requires actual ending, saved output and returned Title.
use super::*;
use shin_core::format::scenario::instruction_elements::Register;
use std::path::PathBuf;

fn template() -> (SaveData, Vec<u8>) {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../reverse-engineering/higurashi/chapter-playthrough-20261015/unlocked-input.dat"
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
    assert_eq!(data.persist_data.get(0) & 16, 16);
    for slot in [162, 163, 164, 165] {
        assert_eq!(data.persist_data.get(slot), 3);
    }
    for id in [143usize, 144] {
        assert_ne!(
            data.save_vectors.unlocked_bgms[id / 32] & (1 << (id % 32)),
            0
        );
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
            "KAMIKANSHI_CAPTURE {label} {}x{} bytes={}",
            size.0,
            size.1,
            a.len()
        );
    }
    assert_eq!(j.runtime.frames, frames);
    assert_eq!(j.runtime.globals().unwrap().unwrap().clock.ticks(), ticks);
}
// Automated canonical raw Ctrl, not physical HID evidence. No J/latch,
// clock, response, VM or state assignment; this route has no story choices.
fn story_step(j: &mut Journey, vp: InputViewport, deadline: Instant) {
    assert!(
        Instant::now() < deadline,
        "Kamikanshi deadline: {}",
        Driver::describe(&j.runtime)
    );
    if let Some(g) = game(&j.runtime) {
        assert_eq!(g.reference_play_modes() & 2, 0, "No latched skip");
        assert!(
            g.reference_story_choice().is_none(),
            "No authored choice on this route"
        );
        if let Some(h) = g.host().filter(|h| h.runner.native_vm_attached()) {
            assert!(
                !matches!(
                    h.runner.pending_command(),
                    Some(
                        Cmd::SELECT { .. }
                            | Cmd::CHARSELECT { .. }
                            | Cmd::FAKESELECT {}
                            | Cmd::TIPSGET { .. }
                            | Cmd::KAKERA {}
                            | Cmd::KAKERAGET { .. }
                    )
                ),
                "Unexpected interactive command"
            );
        }
    }
    assert!(matches!(
        j.step(&key(KeyCode::ControlLeft), vp),
        Frame::Advanced(_)
    ));
}

#[test]
#[ignore = "Ordinary full-route fixture; ordinary Category4/SNRSEL4 row1/501 from immutable unlocked template; protected BGM154 ending/fullEND433/commonEXIT+saveTitle; idempotent UNLOCK1[143,144]; exactLoopback/Metal, no earned or HID claim"]
fn higurashi_runtime_kamikanshi_unlocked_template_full_protected_end() {
    let (saved, initial) = template();
    let out = PathBuf::from(
        std::env::var("SHIN_HIGURASHI_KAMIKANSHI_CAPTURE")
            .expect("owned retained capture directory"),
    );
    std::fs::create_dir_all(&out).unwrap();
    let settings = serde_json::to_vec(&saved.settings).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2 * 60 * 60);
    let vp = viewport((1920, 1080));
    let mut j = Journey::new("kamikanshi-template", &saved);
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
    for row in [1, 2, 3, 4] {
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
    j.until(vp, deadline, "actual Category4 VM", |r| {
        scenario(r) == Some(4)
            && game(r).is_some_and(|g| g.host().is_some_and(|h| h.runner.native_vm_attached()))
    });
    while !picker_ready(&j.runtime) {
        story_step(&mut j, vp, deadline);
    }
    {
        let p = j.runtime.slot().unwrap().scenario_select.as_ref().unwrap();
        let m = p.reference_owner().reference_model();
        assert_eq!((m.kind, m.selected), (4, 0));
        assert!(matches!(m.count, 4 | 5), "Original kind4 count is4+append");
        assert_ne!(m.mask & 2, 0);
        assert_eq!(m.data.progress.active_entry().scenario_id, 4);
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
    j.until(vp, deadline, "actual501 VM", |r| {
        scenario(r) == Some(501)
            && game(r).is_some_and(|g| g.host().is_some_and(|h| h.runner.native_vm_attached()))
    });
    const SYNCS: [(u32, i32); 10] = [
        (0x107100f, 180),
        (0x10710ef, 660),
        (0x10711c0, 1020),
        (0x1071281, 1380),
        (0x107131d, 1440),
        (0x10713d9, 1560),
        (0x1071401, 1650),
        (0x107149d, 2010),
        (0x107156d, 2430),
        (0x10717ee, 2910),
    ];
    let mut sync_samples = [0u64; 10];
    let mut captured = [false; 2];
    let mut protected_final_wait = 0u64;
    let mut saw_end = false;
    let mut final_exit = None;
    let mut max_commands = 0u64;
    let mut iterations = 0u64;
    let mut last_progress = Instant::now();
    let mut directly_sampled_unlock = 0u64;
    while !ready_menu(&j.runtime, "top") {
        assert!(iterations < 1_000_000, "Full501 main-loop budget");
        iterations += 1;
        if last_progress.elapsed() >= Duration::from_secs(15) {
            eprintln!(
                "KAMIKANSHI_PROGRESS iterations={iterations} max_commands={max_commands} {}",
                Driver::describe(&j.runtime)
            );
            last_progress = Instant::now();
        }
        let mut capture_label = None;
        if let Some(g) = game(&j.runtime) {
            if let Some(h) = g.host().filter(|h| h.runner.native_vm_attached()) {
                assert_eq!(
                    scenario(&j.runtime),
                    Some(501),
                    "Unexpected replacement/early return"
                );
                for reg in [22, 23] {
                    assert_eq!(
                        h.runner
                            .reference_register(Register::from_regular_register(reg)),
                        Some(0)
                    );
                }
                max_commands = max_commands.max(h.runner.completed_commands());
                let pc = h.runner.position().0;
                if let Some(m) = h.runner.native_save_metadata() {
                    assert!(!m.current()[1].as_ref().starts_with("BADEND"));
                    saw_end |= m.current()[1].as_ref() == "END";
                }
                if let Some(i) = SYNCS.iter().position(|&(p, _)| p == pc) {
                    if let Some(Cmd::BGMSYNC { time }) = h.runner.pending_command() {
                        assert_eq!(*time, SYNCS[i].1);
                        assert_eq!(
                            h.runner.native_read_state().unwrap().flags & 0x8000_0000,
                            0,
                            "EVBEGIN0 protects BGM154 even while raw Ctrl held"
                        );
                        sync_samples[i] += 1;
                        // Meaningful later montage holds, not an initial offscreen scroll.
                        if i == 5 && sync_samples[i] == 2 {
                            capture_label = Some("protected-ending-1560");
                            captured[0] = true;
                        }
                        if i == 9 && sync_samples[i] == 2 {
                            capture_label = Some("protected-ending-2910");
                            captured[1] = true;
                        }
                    }
                }
                if pc == 0x107189f && h.runner.pending_command().is_some() {
                    assert!(matches!(
                        h.runner.pending_command(),
                        Some(Cmd::WAIT {
                            allow_interrupt: 0,
                            amount: 120
                        })
                    ));
                    assert_eq!(h.runner.native_read_state().unwrap().flags & 0x8000_0000, 0);
                    protected_final_wait += 1;
                }
                // UNLOCK normally completes inside the same unmodified runner pump.
                // Record direct visibility only if it actually exists; zero is not a pass
                // for a missing dynamic sample. Full-path reachability is static evidence.
                if let Some(Cmd::UNLOCK { kind, ids }) = h.runner.pending_command() {
                    assert_eq!(pc, 0x10718a4);
                    assert_eq!(*kind, 1);
                    assert_eq!(ids.as_slice(), &[143, 144]);
                    directly_sampled_unlock += 1;
                }
                if matches!(h.runner.pending_command(), Some(Cmd::EXIT { arg1: 0, .. })) {
                    assert!(saw_end && protected_final_wait > 0 && captured.iter().all(|x| *x));
                    assert_eq!(pc, 0x1f997, "No early or different EXIT accepted");
                    final_exit = Some(pc);
                }
            }
        }
        if let Some(label) = capture_label {
            capture(&mut j, &out, label);
            unchanged(&j, &before, &settings, ids);
        }
        story_step(&mut j, vp, deadline);
    }
    assert!(saw_end && captured.iter().all(|x| *x));
    assert!(sync_samples[0] > 0 && sync_samples[9] >= 2 && protected_final_wait > 0);
    assert_eq!(final_exit, Some(0x1f997));
    unchanged(&j, &before, &settings, ids);
    let writes = j.storage.writes.lock().unwrap();
    assert_eq!(writes.len(), 1);
    let actual = std::fs::read(j.storage.paths.current.as_ref().unwrap()).unwrap();
    assert_eq!(writes[0], actual);
    let result = SaveData::decode(&actual).unwrap();
    assert_eq!(result.encode().unwrap(), actual);
    for slot in 0..192 {
        assert_eq!(
            result.persist_data.get(slot),
            saved.persist_data.get(slot),
            "Supplied existing clear/availability flag{slot}"
        );
    }
    assert_eq!(serde_json::to_vec(&result.settings).unwrap(), settings);
    assert_eq!(
        result.save_vectors.unlocked_bgms, saved.save_vectors.unlocked_bgms,
        "UNLOCK143/144 are alreadyset; retain complete bank length/padding"
    );
    assert_eq!(
        result.save_vectors.unlocked_cgs,
        saved.save_vectors.unlocked_cgs
    );
    assert_eq!(
        result.save_vectors.unlocked_movies,
        saved.save_vectors.unlocked_movies
    );
    drop(writes);
    capture(&mut j, &out, "returned-title");
    eprintln!(
        "KAMIKANSHI_PASS fixture_unlocked501/nochoices/fullEND433/protectedBGM154/EXIT1f997/onewrite/Title; main_loop_iterations={iterations} max_commands={max_commands} sync_samples={sync_samples:?} final_wait={protected_final_wait}; UNLOCK1[143,144] idempotent suppliedbits; directly_sampled_unlock={directly_sampled_unlock}; immediate dispatch not claimed directly observed if0; no earned-unlock,new-acquisition,native-whole-frame or HID claim"
    );
}
