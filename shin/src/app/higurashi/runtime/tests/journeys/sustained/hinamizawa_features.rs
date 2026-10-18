//! Ordinary Hinamizawa502 from the immutable unlocked-template input.
//! Supplied unlocked fixture; no earned-clear, PCM or Nintendo frame claim.
use super::*;
use crate::adv::higurashi::music_commands::MusicCommands;
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
    assert_eq!(data.persist_data.get(0) & 16, 16);
    for slot in [162, 163, 164, 165] {
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
            "HINAMIZAWA_CAPTURE {label} {}x{} bytes={}",
            size.0,
            size.1,
            a.len()
        );
    }
    assert_eq!(j.runtime.frames, frames);
    assert_eq!(j.runtime.globals().unwrap().unwrap().clock.ticks(), ticks);
}
fn story_step(
    j: &mut Journey,
    vp: InputViewport,
    deadline: Instant,
    accelerate: bool,
    release: &mut bool,
) {
    assert!(
        Instant::now() < deadline,
        "Hinamizawa deadline: {}",
        Driver::describe(&j.runtime)
    );
    let mut raw = if accelerate {
        key(KeyCode::ControlLeft)
    } else {
        RawInputState::new()
    };
    let mut action = false;
    if let Some(g) = game(&j.runtime) {
        assert_eq!(g.reference_play_modes() & 2, 0, "No latched skip");
        assert!(
            g.reference_story_choice().is_none(),
            "No authored choices on502"
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
                            | Cmd::QUIZ { .. }
                    )
                ),
                "Unexpected interactive child"
            );
        }
        if !accelerate && !*release && g.input_permission {
            action = true;
        }
    }
    *release = action;
    if action {
        raw.keyboard.insert(KeyCode::Enter);
    }
    assert!(matches!(j.step(&raw, vp), Frame::Advanced(_)));
}

#[test]
#[ignore = "Ordinary Category4row2/502; protectedBGM145/actual BGMVOL lifecycle/fullEND443/commonEXITsaveTitle; immutable unlocked input, exactLoopback/Metal"]
fn higurashi_runtime_hinamizawa_unlocked_template_full_volume_and_protected_end() {
    let (saved, initial) = template();
    let out = PathBuf::from(
        std::env::var("SHIN_HIGURASHI_HINAMIZAWA_CAPTURE")
            .expect("owned retained capture directory"),
    );
    std::fs::create_dir_all(&out).unwrap();
    let settings = serde_json::to_vec(&saved.settings).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2 * 60 * 60);
    let vp = viewport((1920, 1080));
    let mut j = Journey::new("hinamizawa-template", &saved);
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
    let mut release = false;
    while !picker_ready(&j.runtime) {
        story_step(&mut j, vp, deadline, true, &mut release);
    }
    {
        let p = j.runtime.slot().unwrap().scenario_select.as_ref().unwrap();
        let m = p.reference_owner().reference_model();
        assert_eq!((m.kind, m.selected), (4, 0));
        assert!(matches!(m.count, 4 | 5), "Original kind4 count is4+append");
        assert_ne!(m.mask & (1 << 2), 0);
        assert_eq!(m.data.progress.active_entry().scenario_id, 4);
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
    j.until(vp, deadline, "actual502 VM", |r| {
        scenario(r) == Some(502)
            && game(r).is_some_and(|g| g.host().is_some_and(|h| h.runner.native_vm_attached()))
    });
    const SYNCS: [(u32, i32); 14] = [
        (0x10a236d, 180),
        (0x10a2499, 780),
        (0x10a24e5, 1140),
        (0x10a2529, 1560),
        (0x10a25cf, 1980),
        (0x10a2624, 2520),
        (0x10a2672, 2940),
        (0x10a2702, 3240),
        (0x10a2745, 3480),
        (0x10a277b, 3720),
        (0x10a27e7, 4320),
        (0x10a283d, 4740),
        (0x10a28b0, 5340),
        (0x10a28e7, 5700),
    ];
    let mut sync_samples = [0u64; 14];
    let mut wait_samples = [0u64; 2];
    let mut neutral_end = false;
    let mut saw_title = false;
    let mut saw_end = false;
    let mut selected_range_seen = false;
    let mut captured_ending = false;
    let mut captured_scroll = false;
    let mut fade_started = false;
    let mut fade_completed = false;
    let mut fade_tween = 0u64;
    let mut fade_playing = 0u64;
    let mut fade_observation_started: Option<Instant> = None;
    let mut first_fade_clock = None;
    let mut last_fade_clock = 0u64;
    let mut protected_scroll = 0u64;
    let mut first_y = None;
    let mut last_y = None;
    let mut final_exit = None;
    let mut max_commands = 0u64;
    let mut iterations = 0u64;
    let mut last_progress = Instant::now();
    while !ready_menu(&j.runtime, "top") {
        iterations += 1;
        assert!(iterations < 1_000_000, "Full502 main-loop budget");
        if last_progress.elapsed() >= Duration::from_secs(15) {
            eprintln!(
                "HINAMIZAWA_PROGRESS iterations={iterations} max_commands={max_commands} {}",
                Driver::describe(&j.runtime)
            );
            last_progress = Instant::now();
        }
        let mut capture_label = None;
        if let Some(g) = game(&j.runtime) {
            if let Some(h) = g.host().filter(|h| h.runner.native_vm_attached()) {
                assert_eq!(
                    scenario(&j.runtime),
                    Some(502),
                    "No unrelated/early replacement"
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
                    if (0x10718d3..0x10a29b5).contains(&pc) {
                        assert_eq!(m.current()[0].as_ref(), "雛見沢停留所");
                        saw_title = true;
                    }
                    let label = m.current()[1].as_ref();
                    assert!(!label.starts_with("BADEND"));
                    saw_end |= label == "END 二人ナラ…";
                    neutral_end |= label == "END 二人ナラ…";
                }
                // LAYERSELECT is immediate. Observe actual retained range after
                // its first main occurrence, without forcing an opcode yield.
                if (0x1081284..0x10a29b5).contains(&pc)
                    && h.runner.frame().scene.layers().state.selected_native() == [1, 12]
                {
                    if !selected_range_seen {
                        eprintln!(
                            "HINAMIZAWA_RANGE pc={pc:08x} selected=[1,12]; retained state, not direct pending opcode"
                        );
                    }
                    selected_range_seen = true;
                }
                if let Some(i) = SYNCS.iter().position(|&(p, _)| p == pc) {
                    if let Some(Cmd::BGMSYNC { time }) = h.runner.pending_command() {
                        assert_eq!(*time, SYNCS[i].1);
                        assert_eq!(h.runner.native_read_state().unwrap().flags & 0x8000_0000, 0);
                        sync_samples[i] += 1;
                        if i == 4 && sync_samples[i] == 2 {
                            capture_label = Some("protected-ending-1980");
                            captured_ending = true;
                        }
                    }
                }
                // Immediate BGMVOL at10a28ba follows sync5340. Its following
                // loading/wipe/sync/scroll boundary remains genuine gameplay;
                // no artificial hold is introduced. Descriptor1.0 is a target.
                if !fade_completed && (0x10a28bf..0x10a2996).contains(&pc) {
                    assert!(saw_end && neutral_end);
                    assert_eq!(g.reference_effective_play_modes() & 2, 0);
                    assert_eq!(h.runner.native_read_state().unwrap().flags & 0x8000_0000, 0);
                    let observed = fade_observation_started.get_or_insert_with(Instant::now);
                    assert!(
                        observed.elapsed() < Duration::from_secs(30),
                        "Missing actual BGMVOL lifecycle within30s"
                    );
                    let d = h.runner.native_playback_descriptors().unwrap().music;
                    assert_eq!((d.id, d.count), (-1, 1), "Finite BGM145 source path");
                    assert_eq!(d.volume.to_bits(), (1000.0f32 * 0.001).to_bits());
                    let status = h.audio.bgm_status();
                    if status & 3 == 2 {
                        let clock = h.audio.bgm_clock();
                        assert!(clock >= last_fade_clock, "No BGM seek or reversal");
                        last_fade_clock = clock;
                        fade_playing += 1;
                        if status & 4 != 0 {
                            fade_tween += 1;
                            fade_started = true;
                            first_fade_clock.get_or_insert(clock);
                        } else if fade_started {
                            assert!(fade_playing > fade_tween && clock > first_fade_clock.unwrap());
                            fade_completed = true;
                            eprintln!(
                                "HINAMIZAWA_VOLUME_PASS source_bgm=145 target=1 fade_ticks=180 tween_samples={fade_tween} playing_samples={fade_playing} first_us={} last_us={clock} pc={pc:08x}; lifecycle only, not measured gain/PCM",
                                first_fade_clock.unwrap()
                            );
                        }
                    }
                }
                if neutral_end && !fade_completed && pc >= 0x10a2996 {
                    panic!("Actual BGMVOL playing/tween/completion was not observed before stop");
                }
                if pc == 0x10a291b && h.runner.pending_command().is_some() {
                    match h.runner.pending_command() {
                        Some(Cmd::LAYERWAIT { id: 2, properties }) => {
                            assert_eq!(properties.as_slice(), &[1])
                        }
                        _ => panic!("Actual credits LAYERWAIT2/property1"),
                    }
                    assert_eq!(h.runner.native_read_state().unwrap().flags & 0x8000_0000, 0);
                    let layers = h.runner.frame().scene.layers();
                    let target = layers.state.target(2).expect("Actual credits logical2");
                    let y = layers
                        .layer(target)
                        .expect("Actual PIC742 owner")
                        .properties
                        .values()[1];
                    assert!(y.is_finite());
                    if let Some(previous) = last_y {
                        assert!(y >= previous - 0.001);
                    }
                    if first_y.is_none() {
                        assert!(y < -10000.0);
                        first_y = Some(y);
                    }
                    last_y = Some(y);
                    protected_scroll += 1;
                    if !captured_scroll && y >= -5000.0 {
                        assert!(y <= -4700.0, "No skipped midpoint");
                        assert!(
                            fade_completed,
                            "Actual volume lifecycle must finish before visible credits midpoint"
                        );
                        capture_label = Some("protected-credit-scroll");
                        captured_scroll = true;
                        eprintln!(
                            "HINAMIZAWA_CREDITS_CAPTURE pc={pc:08x} y={y} samples={protected_scroll}"
                        );
                    }
                }
                for (i, (p, amount)) in [(0x10a291f, 300), (0x10a2998, 120)].into_iter().enumerate()
                {
                    if pc == p && h.runner.pending_command().is_some() {
                        assert!(
                            matches!(h.runner.pending_command(),Some(Cmd::WAIT { allow_interrupt:0,amount:a }) if *a == amount)
                        );
                        assert_eq!(h.runner.native_read_state().unwrap().flags & 0x8000_0000, 0);
                        assert!(
                            fade_completed,
                            "Protected closing waits run after canonical Ctrl resumes"
                        );
                        wait_samples[i] += 1;
                    }
                }
                if matches!(h.runner.pending_command(), Some(Cmd::EXIT { arg1: 0, .. })) {
                    assert!(
                        saw_title
                            && saw_end
                            && selected_range_seen
                            && fade_completed
                            && captured_ending
                            && captured_scroll
                            && wait_samples.iter().all(|&n| n > 0)
                    );
                    assert_eq!(pc, 0x1f997, "Only full common EXIT accepted");
                    final_exit = Some(pc);
                }
            }
        }
        if let Some(label) = capture_label {
            capture(&mut j, &out, label);
            unchanged(&j, &before, &settings, ids);
        }
        // Neutral canonical input from END through BGMVOL completion. Resume
        // rawCtrl only after observing the real ramp, stressing protected
        // scrolling and final logical waits without changing any clocks.
        story_step(
            &mut j,
            vp,
            deadline,
            !neutral_end || fade_completed,
            &mut release,
        );
    }
    assert!(
        saw_title
            && saw_end
            && selected_range_seen
            && fade_started
            && fade_completed
            && captured_ending
            && captured_scroll
    );
    assert!(sync_samples.iter().all(|&n| n > 0) && sync_samples[4] >= 2);
    assert!(protected_scroll > 100 && wait_samples.iter().all(|&n| n > 0));
    assert!(first_y.unwrap() < -10000.0 && last_y.unwrap() > -50.0);
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
        result.save_vectors, saved.save_vectors,
        "All complete banks/padding unchanged"
    );
    drop(writes);
    capture(&mut j, &out, "returned-title");
    eprintln!(
        "HINAMIZAWA_PASS fixture_unlocked502/fullEND443/EXIT1f997/onewrite/Title; iterations={iterations} max_commands={max_commands} sync_samples={sync_samples:?} fade_tween={fade_tween} fade_playing={fade_playing} protected_scroll={protected_scroll} wait_samples={wait_samples:?} first_y={first_y:?} last_y={last_y:?} selected_range={selected_range_seen}; noearnedunlock/newPCM/nativewholeframe/HIDclaim"
    );
}
