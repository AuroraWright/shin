//! Ordinary Hajisarashi407 from the exact unlocked-template input.
//! Fixture unlock provenance, not an earned-unlock claim. No state mutation.
//! Full-route acceptance requires actual ending, saved output and returned Title.
use super::*;
use crate::adv::higurashi::{message_playback::MessageVoice, voice_commands::VoiceCommands};
use shin_core::format::scenario::instruction_elements::Register;
use std::path::PathBuf;

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
    assert_eq!(data.persist_data.get(0) & 8, 8);
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
            "HAJISARASHI_CAPTURE {label} {}x{} bytes={}",
            size.0,
            size.1,
            a.len()
        );
    }
    assert_eq!(j.runtime.frames, frames);
    assert_eq!(j.runtime.globals().unwrap().unwrap().clock.ticks(), ticks);
}
// Canonical raw input only. ReleaseCtrl from actual#7 metadata before RND.
// NoA/skip while the selected branchVOICEWAIT is pending: real audio retires it.
fn story_step(
    j: &mut Journey,
    vp: InputViewport,
    deadline: Instant,
    accelerate: bool,
    release: &mut bool,
) {
    assert!(
        Instant::now() < deadline,
        "Hajisarashi deadline: {}",
        Driver::describe(&j.runtime)
    );
    let mut raw = if accelerate {
        key(KeyCode::ControlLeft)
    } else {
        RawInputState::new()
    };
    let mut action = None;
    if let Some(g) = game(&j.runtime) {
        assert_eq!(g.reference_play_modes() & 2, 0, "No latched skip");
        assert!(
            g.reference_story_choice().is_none(),
            "No authored story choice on407"
        );
        let mut branch_wait = false;
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
                            | Cmd::QUIZ { .. }
                    )
                ),
                "Unexpected interactive child"
            );
            branch_wait = BRANCHES.iter().any(|b| b.1 == h.runner.position().0)
                && matches!(
                    h.runner.pending_command(),
                    Some(Cmd::VOICEWAIT { status: 2 })
                );
        }
        if !accelerate && !branch_wait && !*release && g.input_permission {
            action = Some(KeyCode::Enter);
        }
        if branch_wait {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    *release = action.is_some();
    if let Some(k) = action {
        raw.keyboard.insert(k);
    }
    assert!(matches!(j.step(&raw, vp), Frame::Advanced(_)));
}
// InclusiveRND0..3: first3values indexJT;3falls through to the fourth voice.
const BRANCHES: [(i32, u32, &str); 4] = [
    (0, 0x10031e1, "S23/22/HCHI_0000"),
    (1, 0x1003232, "S23/22/HCHI_0010"),
    (2, 0x100328b, "S23/22/HCHI_0020"),
    (3, 0x100319a, "S14/22/221400003"),
];

#[test]
#[ignore = "ordinaryCategory3row7/407; realRND0..3 branchvoice/naturalVOICEWAIT/fullEND418/EXIT+saveTitle; immutableunlockedtemplate/exactLoopback/Metal"]
fn higurashi_runtime_hajisarashi_unlocked_template_full_random_voice_and_end() {
    let (saved, initial) = template();
    let out = PathBuf::from(
        std::env::var("SHIN_HIGURASHI_HAJISARASHI_CAPTURE")
            .expect("owned retained capture directory"),
    );
    std::fs::create_dir_all(&out).unwrap();
    let settings = serde_json::to_vec(&saved.settings).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2 * 60 * 60);
    let vp = viewport((1920, 1080));
    let mut j = Journey::new("hajisarashi-template", &saved);
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
        story_step(&mut j, vp, deadline, true, &mut release);
    }
    {
        let p = j.runtime.slot().unwrap().scenario_select.as_ref().unwrap();
        let m = p.reference_owner().reference_model();
        assert_eq!((m.kind, m.selected, m.count), (3, 0, 9));
        assert_ne!(m.mask & (1 << 7), 0);
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
    for wanted in [3, 5, 7] {
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
            wanted
        );
    }
    j.tap(KeyCode::Enter, vp);
    j.until(vp, deadline, "actual407 VM", |r| {
        scenario(r) == Some(407)
            && game(r).is_some_and(|g| g.host().is_some_and(|h| h.runner.native_vm_attached()))
    });
    let mut release = false;
    let mut neutral_random = false;
    let mut actual_branch = None;
    let mut voice_samples = 0u64;
    let mut last_voice_us = 0u64;
    let mut voice_completed = false;
    let mut captured_voice = false;
    let mut captured_end = false;
    let mut saw_title = false;
    let mut saw_end = false;
    let mut saw_epilogue = false;
    let mut final_exit = None;
    let mut max_commands = 0;
    let mut iterations = 0u64;
    let mut last_progress = Instant::now();
    while !ready_menu(&j.runtime, "top") {
        iterations += 1;
        assert!(iterations < 1_000_000, "Full407 main-loop budget");
        if last_progress.elapsed() >= Duration::from_secs(15) {
            eprintln!(
                "HAJISARASHI_PROGRESS iterations={iterations} max_commands={max_commands} {}",
                Driver::describe(&j.runtime)
            );
            last_progress = Instant::now();
        }
        let mut capture_label = None;
        if let Some(g) = game(&j.runtime) {
            if let Some(h) = g.host().filter(|h| h.runner.native_vm_attached()) {
                assert_eq!(
                    scenario(&j.runtime),
                    Some(407),
                    "No early/unrelated replacement"
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
                    // Attachment may be exposed before its first SAVEINFO constructor.
                    // After that exact first instruction, require the authored title.
                    if (0xfeb4c1..0x10154a6).contains(&pc) {
                        assert_eq!(m.current()[0].as_ref(), "羞晒し編");
                        saw_title = true;
                    }
                    let label = m.current()[1].as_ref();
                    assert!(!label.starts_with("BADEND"));
                    neutral_random |= label == "#7 覚醒！ 欲望の鎖を解き放て！！";
                    saw_end |= label == "END (A New Story. A New Legend.)";
                }
                if let Some(&(value, wait_pc, name)) = BRANCHES.iter().find(|b| b.1 == pc) {
                    assert!(matches!(
                        h.runner.pending_command(),
                        Some(Cmd::VOICEWAIT { status: 2 })
                    ));
                    assert!(neutral_random);
                    assert_eq!(
                        g.reference_effective_play_modes() & 2,
                        0,
                        "Real voice must not be skipped"
                    );
                    assert_eq!(
                        h.runner
                            .reference_register(Register::from_regular_register(0)),
                        Some(value),
                        "Actual inclusiveRND result selects this voice branch"
                    );
                    if let Some(previous) = actual_branch {
                        assert_eq!(previous, (value, wait_pc, name));
                    }
                    actual_branch = Some((value, wait_pc, name));
                    let status = h.audio.status_bits();
                    if status & 2 != 0 {
                        let pos = h.audio.position_micros();
                        assert!(pos >= last_voice_us, "No voice seek/clock reversal");
                        last_voice_us = pos;
                        voice_samples += 1;
                        if pos >= 50_000 && !captured_voice {
                            capture_label = Some("random-voice-playing");
                            captured_voice = true;
                        }
                    }
                } else if let Some((value, wait_pc, name)) = actual_branch {
                    if !voice_completed {
                        assert!(
                            voice_samples > 1 && last_voice_us > 0 && captured_voice,
                            "Need real playing voice before actual naturalVOICEWAIT exit"
                        );
                        voice_completed = true;
                        eprintln!(
                            "HAJISARASHI_RANDOM_PASS value={value} wait_pc={wait_pc:08x} name={name} last_us={last_voice_us} samples={voice_samples}; naturalwaitexit with neutralrawinput, no RNG/VM/voice/clock setters"
                        );
                    }
                }
                // An authored later completed scene, ordinaryWAIT0/60, not protected.
                // StopCtrl from END metadata until this capture to preserve its hold.
                if saw_end && pc == 0x101505a && h.runner.pending_command().is_some() {
                    assert!(matches!(
                        h.runner.pending_command(),
                        Some(Cmd::WAIT {
                            allow_interrupt: 0,
                            amount: 60
                        })
                    ));
                    assert_eq!(g.reference_effective_play_modes() & 2, 0);
                    if !captured_end {
                        capture_label = Some("normal-ending-scene");
                        captured_end = true;
                    }
                }
                saw_epilogue |= (0x8f916..0x8fdf3).contains(&pc);
                if matches!(h.runner.pending_command(), Some(Cmd::EXIT { arg1: 0, .. })) {
                    assert!(saw_end && captured_end && voice_completed && saw_epilogue);
                    assert_eq!(pc, 0x1f997, "Actual common fullroute EXIT");
                    final_exit = Some(pc);
                }
            }
        }
        if let Some(label) = capture_label {
            capture(&mut j, &out, label);
            unchanged(&j, &before, &settings, ids);
        }
        story_step(
            &mut j,
            vp,
            deadline,
            (!neutral_random || voice_completed) && (!saw_end || captured_end),
            &mut release,
        );
    }
    assert!(
        voice_completed && captured_voice && captured_end && saw_title && saw_end && saw_epilogue
    );
    assert_eq!(final_exit, Some(0x1f997));
    unchanged(&j, &before, &settings, ids);
    let writes = j.storage.writes.lock().unwrap();
    assert_eq!(writes.len(), 1);
    let actual = std::fs::read(j.storage.paths.current.as_ref().unwrap()).unwrap();
    assert_eq!(writes[0], actual);
    let result = SaveData::decode(&actual).unwrap();
    assert_eq!(result.encode().unwrap(), actual);
    assert_eq!(serde_json::to_vec(&result.settings).unwrap(), settings);
    for slot in 0..192 {
        assert_eq!(
            result.persist_data.get(slot),
            saved.persist_data.get(slot),
            "Suppliedflagslot{slot}"
        );
    }
    assert_eq!(
        result.save_vectors.select_last_choices,
        saved.save_vectors.select_last_choices
    );
    assert_eq!(
        result.save_vectors.unlocked_cgs,
        saved.save_vectors.unlocked_cgs
    );
    assert_eq!(
        result.save_vectors.unlocked_bgms,
        saved.save_vectors.unlocked_bgms
    );
    assert_eq!(
        result.save_vectors.unlocked_movies,
        saved.save_vectors.unlocked_movies
    );
    drop(writes);
    capture(&mut j, &out, "returned-title");
    eprintln!(
        "HAJISARASHI_PASS fixture_unlocked407/fullEND418/oneactualRNDvoicebranch/fourthcollection/EXIT1f997/onewrite/Title; iterations={iterations} max_commands={max_commands} actual_branch={actual_branch:?} voice_samples={voice_samples} last_voice_us={last_voice_us}; noearnedunlock/allrandomoutcomes/protectedending/nativewholeframe/HIDclaim"
    );
}
