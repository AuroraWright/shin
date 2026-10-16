//! Ordinary Yoigoshi302 full good-route fixture.
//! Exact supplied unlocked fixture; no earned progression or new PCM oracle.
//! Canonical inputs only; source/wire/frame acceptance is required after a run.
use super::*;
use crate::adv::higurashi::{effect_commands::EffectCommands, read_state::native_message_id};
use shin_core::format::scenario::instruction_elements::Register;
use std::path::PathBuf;

const PLAN: [(u32, usize); 3] = [(0x95c6ee, 0), (0x95fe1c, 1), (0x97672c, 1)];
const TIPS: [(u32, &[i32]); 6] = [
    (0x9337d5, &[167]),
    (0x93fb1e, &[168, 169]),
    (0x952c4e, &[170]),
    (0x968d8c, &[172]),
    (0x99940d, &[173, 174, 175]),
    (0x9f8944, &[177]),
];

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
    assert_eq!(data.persist_data.get(0) & 4, 4);
    for slot in [32, 40] {
        assert_eq!(data.persist_data.get(slot), 1);
    }
    for slot in [13, 14, 15, 16] {
        assert_eq!(data.persist_data.get(slot), 3);
    }
    assert!(data.save_vectors.select_last_choices.is_empty());
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
            "YOIGOSHI_CAPTURE {label} {}x{} bytes={}",
            size.0,
            size.1,
            a.len()
        );
    }
    assert_eq!(j.runtime.frames, frames);
    assert_eq!(j.runtime.globals().unwrap().unwrap().clock.ticks(), ticks);
}
// Canonical controls only. Neutral/A-release advances #13 before the fade;
// the first message after SEVOL is held with no Enter until its real audio
// tween finishes. No clock/VM/media/RNG/mode/response assignment.
fn story_step(
    j: &mut Journey,
    vp: InputViewport,
    deadline: Instant,
    accelerate: bool,
    hold_fade_message: bool,
    release: &mut bool,
) {
    assert!(
        Instant::now() < deadline,
        "Yoigoshi deadline: {}",
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
        if let Some(h) = g.host().filter(|h| h.runner.native_vm_attached()) {
            assert!(
                !matches!(
                    h.runner.pending_command(),
                    Some(
                        Cmd::SELECT { .. }
                            | Cmd::CHARSELECT { .. }
                            | Cmd::FAKESELECT {}
                            | Cmd::KAKERA {}
                            | Cmd::KAKERAGET { .. }
                            | Cmd::QUIZ { .. }
                    )
                ),
                "Story SELECT must be handled by exact declared plan"
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
        } else if !accelerate && !hold_fade_message && g.input_permission && !*release {
            action = Some(KeyCode::Enter);
        }
    }
    *release = action.is_some();
    if let Some(k) = action {
        raw.keyboard.insert(k);
    }
    assert!(matches!(j.step(&raw, vp), Frame::Advanced(_)));
}
fn choose(j: &mut Journey, vp: InputViewport, deadline: Instant, pc: u32, original: usize) {
    assert!(pending(&j.runtime, pc));
    assert!(matches!(
        j.step(&RawInputState::new(), vp),
        Frame::Advanced(_)
    ));
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
    eprintln!("YOIGOSHI_CHOICE pc={pc:08x} original={original} rows={rows:?}");
    j.tap(KeyCode::Enter, vp);
    j.until(vp, deadline, "actual SELECT closing callback", |r| {
        !pending(r, pc)
    });
}

#[test]
#[ignore = "Ordinary Category2row2/302; choices0,1,1/TIPSContinue0/SEVOL4 actual tween lifecycle/protectedBGM93scroll/fullEND/thirdcollection/EXITsaveTitle; immutable unlocked fixture, exactLoopback/Metal; explicit opt-in full-route test"]
fn higurashi_runtime_yoigoshi_unlocked_template_full_good_end() {
    let (saved, initial) = template();
    let out = PathBuf::from(
        std::env::var("SHIN_HIGURASHI_YOIGOSHI_CAPTURE").expect("owned capture directory"),
    );
    std::fs::create_dir_all(&out).unwrap();
    let settings = serde_json::to_vec(&saved.settings).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2 * 60 * 60);
    let vp = viewport((1920, 1080));
    let mut j = Journey::new("yoigoshi-template", &saved);
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
        story_step(&mut j, vp, deadline, true, false, &mut release);
    }
    {
        let p = j.runtime.slot().unwrap().scenario_select.as_ref().unwrap();
        let m = p.reference_owner().reference_model();
        assert_eq!((m.kind, m.selected, m.count), (2, 0, 6));
        assert_ne!(m.mask & (1 << 2), 0);
        assert_eq!(m.data.progress.active_entry().scenario_id, 2);
    }
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
        2
    );
    j.tap(KeyCode::Enter, vp);
    j.until(vp, deadline, "actual302 VM", |r| {
        scenario(r) == Some(302)
            && game(r).is_some_and(|g| g.host().is_some_and(|h| h.runner.native_vm_attached()))
    });
    let mut next = 0usize;
    let mut tips_seen = [false; 6];
    let mut neutral_fade = false;
    let mut fade_started = false;
    let mut fade_completed = false;
    let mut fade_captured = false;
    let mut fade_observation_started: Option<Instant> = None;
    let mut fade_tween_samples = 0u64;
    let mut fade_playing_samples = 0u64;
    let mut saw_end = false;
    let mut saw_epilogue = false;
    let mut protected_scroll = 0u64;
    let mut wait_samples = [0u64; 2];
    let mut first_credit_y = None;
    let mut last_credit_y = None;
    let mut credits_captured = false;
    let mut final_exit = None;
    let mut iterations = 0u64;
    let mut max_commands = 0u64;
    let mut last_progress = Instant::now();
    while !ready_menu(&j.runtime, "top") {
        iterations += 1;
        assert!(iterations < 1_000_000, "Full302 main-loop budget");
        if last_progress.elapsed() >= Duration::from_secs(15) {
            eprintln!(
                "YOIGOSHI_PROGRESS iterations={iterations} max_commands={max_commands} {}",
                Driver::describe(&j.runtime)
            );
            last_progress = Instant::now();
        }
        let mut capture_label = None;
        let mut hold_fade_message = false;
        if let Some(g) = game(&j.runtime) {
            if let Some(h) = g.host().filter(|h| h.runner.native_vm_attached()) {
                assert_eq!(
                    scenario(&j.runtime),
                    Some(302),
                    "No early/unrelated replacement"
                );
                for reg in [22, 23] {
                    assert_eq!(
                        h.runner
                            .reference_register(Register::from_regular_register(reg)),
                        Some(0),
                        "Ordinary entry, no debug or section start"
                    );
                }
                max_commands = max_commands.max(h.runner.completed_commands());
                let pc = h.runner.position().0;
                if let Some(m) = h.runner.native_save_metadata() {
                    let label = m.current()[1].as_ref();
                    assert!(
                        !label.starts_with("BADEND"),
                        "A bad ending is not full good completion"
                    );
                    neutral_fade |= label == "#13 姉との別れ";
                    saw_end |= label == "END";
                }
                if matches!(h.runner.pending_command(), Some(Cmd::SELECT { .. })) {
                    let &(wanted_pc, original) = PLAN.get(next).expect("Unexpected extra SELECT");
                    assert_eq!(
                        pc, wanted_pc,
                        "Exact ordinary SELECT order; no debug or untaken branch"
                    );
                    if pc == 0x97672c {
                        // Actual SGET32 at976720 writes v2 before the ordinary choice.
                        assert_eq!(
                            h.runner
                                .reference_register(Register::from_regular_register(2)),
                            Some(1)
                        );
                    }
                    choose(&mut j, vp, deadline, wanted_pc, original);
                    next += 1;
                    release = false;
                    unchanged(&j, &before, &settings, ids);
                    continue;
                }
                assert!(!matches!(
                    h.runner.pending_command(),
                    Some(
                        Cmd::CHARSELECT { .. }
                            | Cmd::FAKESELECT {}
                            | Cmd::KAKERA {}
                            | Cmd::KAKERAGET { .. }
                            | Cmd::QUIZ { .. }
                    )
                ));
                if let Some(Cmd::TIPSGET { ids: actual_ids }) = h.runner.pending_command() {
                    let i = TIPS
                        .iter()
                        .position(|&(p, _)| p == pc)
                        .expect("Unexpected TIPS branch");
                    assert_eq!(actual_ids.as_slice(), TIPS[i].1);
                    if !tips_seen[i] {
                        eprintln!("YOIGOSHI_TIPS pc={pc:08x} ids={actual_ids:?}");
                        tips_seen[i] = true;
                    }
                }
                // SEVOL is immediate. Observe its real following message and live
                // audio lifecycle; the descriptor is target0.4, not actual gain.
                if !fade_completed
                    && h.runner.frame().message.playback.message_id == native_message_id(99842)
                {
                    assert!(
                        neutral_fade && (0x9ede1a..0x9ee5cc).contains(&pc),
                        "Before actual next SESTOPALL"
                    );
                    assert_eq!(g.reference_effective_play_modes() & 2, 0);
                    hold_fade_message = true;
                    let started = fade_observation_started.get_or_insert_with(Instant::now);
                    assert!(
                        started.elapsed() < Duration::from_secs(30),
                        "No observed active SEVOL tween completed within30s; playing={fade_playing_samples} tween={fade_tween_samples}"
                    );
                    let descriptor = h.runner.native_playback_descriptors().unwrap().effects[4];
                    assert_eq!(descriptor.id, 26);
                    assert_eq!(descriptor.volume.to_bits(), (400.0f32 * 0.001).to_bits());
                    assert_eq!(h.audio.effect_descriptor_id(4), 26);
                    let status = h.audio.effect_status(4);
                    if status & 3 == 2 {
                        assert!(h.audio.active_effect_slots().contains(&4));
                        fade_playing_samples += 1;
                        if status & 4 != 0 {
                            fade_tween_samples += 1;
                            fade_started = true;
                            if !fade_captured {
                                capture_label = Some("se-volume-fade");
                                fade_captured = true;
                            }
                        } else if fade_started {
                            assert!(
                                fade_captured && fade_tween_samples > 0 && fade_playing_samples > 1
                            );
                            fade_completed = true;
                            eprintln!(
                                "YOIGOSHI_FADE_PASS slot=4 id=26 target=0.4 tween_samples={fade_tween_samples} playing_samples={fade_playing_samples} message=99842 pc={pc:08x}; actual lifecycle, not intermediate gain/PCM samples"
                            );
                        }
                    }
                }
                if neutral_fade && !fade_completed && pc >= 0x9ee5cc {
                    panic!(
                        "Missed actual playing SEVOL tween before stop; no invented observation accepted"
                    );
                }
                if pc == 0x9f88b1 && h.runner.pending_command().is_some() {
                    assert!(saw_end && fade_completed && next == PLAN.len());
                    match h.runner.pending_command() {
                        Some(Cmd::LAYERWAIT { id: 2, properties }) => {
                            assert_eq!(properties.as_slice(), &[1])
                        }
                        _ => panic!("Actual authored credits LAYERWAIT2/property1"),
                    }
                    assert_eq!(
                        h.runner.native_read_state().unwrap().flags & 0x8000_0000,
                        0,
                        "EVBEGIN0 protects scroll while raw Ctrl held"
                    );
                    let layers = h.runner.frame().scene.layers();
                    let target = layers.state.target(2).expect("Actual credits logical2");
                    let y = layers
                        .layer(target)
                        .expect("Real PIC732 owner")
                        .properties
                        .values()[1];
                    assert!(y.is_finite());
                    if let Some(previous) = last_credit_y {
                        assert!(y >= previous - 0.001, "Natural credits Y is monotonic");
                    }
                    if first_credit_y.is_none() {
                        assert!(y < -5500.0, "Observe initial authored offscreen placement");
                        first_credit_y = Some(y);
                    }
                    last_credit_y = Some(y);
                    protected_scroll += 1;
                    // Read the actual tweened owner, not its constructor target.
                    // Mid-scroll Y is far beyond the offscreen start. Visual
                    // inspection of retained content remains a separate gate.
                    if !credits_captured && y >= -3000.0 {
                        assert!(y <= -2700.0, "No skipped midpoint accepted");
                        capture_label = Some("protected-credit-scroll");
                        credits_captured = true;
                        eprintln!(
                            "YOIGOSHI_CREDITS_CAPTURE pc={pc:08x} y={y} samples={protected_scroll}"
                        );
                    }
                }
                for (i, (p, amount)) in [(0x9f88b5, 300), (0x9f892e, 120)].into_iter().enumerate() {
                    if pc == p && h.runner.pending_command().is_some() {
                        assert!(
                            matches!(h.runner.pending_command(), Some(Cmd::WAIT { allow_interrupt: 0, amount: a }) if *a == amount)
                        );
                        assert_eq!(h.runner.native_read_state().unwrap().flags & 0x8000_0000, 0);
                        wait_samples[i] += 1;
                    }
                }
                saw_epilogue |= (0x60166..0x6247f).contains(&pc);
                if matches!(h.runner.pending_command(), Some(Cmd::EXIT { arg1: 0, .. })) {
                    assert!(
                        next == PLAN.len()
                            && tips_seen.iter().all(|x| *x)
                            && saw_end
                            && saw_epilogue
                            && fade_completed
                            && credits_captured
                            && wait_samples.iter().all(|&n| n > 0)
                    );
                    assert_eq!(pc, 0x1f997, "No bad or section EXIT accepted");
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
            !neutral_fade || fade_completed,
            hold_fade_message && !fade_completed,
            &mut release,
        );
    }
    assert_eq!(next, PLAN.len());
    assert!(
        tips_seen.iter().all(|x| *x)
            && saw_end
            && saw_epilogue
            && fade_started
            && fade_completed
            && fade_captured
            && credits_captured
    );
    assert!(protected_scroll > 100 && wait_samples.iter().all(|&n| n > 0));
    assert!(first_credit_y.unwrap() < -5500.0 && last_credit_y.unwrap() > -50.0);
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
            "Supplied flag{slot}"
        );
    }
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
    // Empty input history legitimately grows in32-entry blocks after SELECT.
    let mut expected_last = saved.save_vectors.select_last_choices.clone();
    expected_last.resize(64, 0);
    expected_last[46] = 1;
    expected_last[47] = 2;
    expected_last[49] = 2;
    assert_eq!(
        result.save_vectors.select_last_choices, expected_last,
        "Full padded history; untaken index48 remains0"
    );
    drop(writes);
    capture(&mut j, &out, "returned-title");
    eprintln!(
        "YOIGOSHI_PASS fixture_unlocked302/fullEND/thirdcollection/EXIT1f997/onewrite/Title; iterations={iterations} max_commands={max_commands} protected_scroll={protected_scroll} wait_samples={wait_samples:?} credits_y_first={first_credit_y:?} credits_y_last={last_credit_y:?} tips_seen={tips_seen:?}; SEVOL lifecycle only, no newPCM/earnedunlock/allbranches/nativewholeframe/HID claim"
    );
}
