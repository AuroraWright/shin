//! Full ordinary Origin10002 with four authored Mono stages.
//! Exact supplied unlocked template, not earned-unlock provenance. No game-state setters.
use super::*;
use crate::adv::higurashi::read_state::native_message_id;
use shin_core::format::scenario::instruction_elements::Register;
use std::path::PathBuf;

const LEVELS: [f32; 4] = [666., 466., 266., 66.];
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
    let saved = SaveData::decode(&bytes).unwrap();
    assert_eq!(saved.encode().unwrap(), bytes);
    assert!(saved.persist_data.get(176) >= 1);
    (saved, bytes)
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
fn preserved(
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
fn screen(r: &Runtime) -> Option<[f32; 65]> {
    Some(
        game(r)?
            .host()?
            .runner
            .frame()
            .scene
            .layers()
            .front_properties()
            .values(),
    )
}
fn capture(j: &mut Journey, out: &std::path::Path, label: &str) {
    let frames = j.runtime.frames;
    let ticks = j.runtime.globals().unwrap().unwrap().clock.ticks();
    let props = screen(&j.runtime).map(|p| p.map(f32::to_bits));
    for size in [(1280, 720), (1920, 1080)] {
        let a = j.gpu.pixels(&j.runtime, size, false);
        assert_eq!(
            a,
            j.gpu.pixels(&j.runtime, size, true),
            "pure completed cache presentation"
        );
        std::fs::write(out.join(format!("{label}-{}x{}.rgba", size.0, size.1)), &a).unwrap();
        eprintln!(
            "ORIGIN_WATA_CAPTURE {label} {}x{} bytes={}",
            size.0,
            size.1,
            a.len()
        );
    }
    assert_eq!(j.runtime.frames, frames);
    assert_eq!(j.runtime.globals().unwrap().unwrap().clock.ticks(), ticks);
    assert_eq!(screen(&j.runtime).map(|p| p.map(f32::to_bits)), props);
}
// Canonical keyboard only. The final Day12/epilogue segment runs without Ctrl
// so normal WAIT0 and WIPEWAIT cannot be elided by eligible skip suppression.
// A/release drives actual message blocks; every story child must be TIPS Continue.
fn story_step(
    j: &mut Journey,
    vp: InputViewport,
    deadline: Instant,
    accelerate: bool,
    release: &mut bool,
) {
    assert!(
        Instant::now() < deadline,
        "Origin Watanagashi deadline: {}",
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
                    Some(Cmd::SELECT { .. } | Cmd::CHARSELECT { .. } | Cmd::FAKESELECT {})
                ),
                "No ordinary story choices in actual10002; unexpected interaction needs plan"
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
                let wanted = rows
                    .iter()
                    .position(|(id, _)| *id == 0)
                    .expect("TIPS Continue original0");
                action = Some(if selected == wanted as i32 {
                    KeyCode::Enter
                } else {
                    KeyCode::ArrowDown
                });
            }
        } else if !*release
            && (matches!(g.reference_story_notification(), Some((12..=14, 2)))
                || (!accelerate && g.input_permission))
        {
            action = Some(KeyCode::Enter);
        }
    }
    *release = action.is_some();
    if let Some(k) = action {
        raw.keyboard.insert(k);
    }
    assert!(matches!(j.step(&raw, vp), Frame::Advanced(_)));
}

#[test]
#[ignore = "Exact unlocked template; ordinary Originpickerrow1/full10002; canonicalnoCtrl epilogue/fourMono screen stages/realWAIT+WIPE boundaries/actualEXIT+saveTitle; requires real assets; exactLoopback/Metal"]
fn higurashi_runtime_origin_watanagashi_unlocked_template_full_mono_epilogue() {
    let (saved, initial) = template();
    let out = PathBuf::from(
        std::env::var("SHIN_HIGURASHI_ORIGIN_WATA_CAPTURE")
            .expect("owned retainedcapture directory"),
    );
    std::fs::create_dir_all(&out).unwrap();
    let settings = serde_json::to_vec(&saved.settings).unwrap();
    let vp = viewport((1920, 1080));
    let deadline = Instant::now() + Duration::from_secs(2 * 60 * 60);
    let mut j = Journey::new("origin-watanagashi-template", &saved);
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
    j.tap(KeyCode::ArrowUp, vp);
    assert_eq!(
        j.runtime
            .slot()
            .unwrap()
            .title
            .as_ref()
            .unwrap()
            .reference_top_selection(),
        Some(8)
    );
    j.tap(KeyCode::Enter, vp);
    j.until(vp, deadline, "actual Originpicker", picker_ready);
    {
        let m = j
            .runtime
            .slot()
            .unwrap()
            .scenario_select
            .as_ref()
            .unwrap()
            .reference_owner()
            .reference_model();
        assert_eq!((m.kind, m.selected, m.count), (5, 0, 10));
        assert_ne!(m.mask & 2, 0);
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
    j.until(vp, deadline, "actual10002 VM", |r| {
        scenario(r) == Some(10002)
            && game(r).is_some_and(|g| g.host().is_some_and(|h| h.runner.native_vm_attached()))
    });
    let mut release = false;
    let mut neutral_tail = false;
    let mut neutral_seen = false;
    let mut stages = 0usize;
    let mut restored = false;
    let mut epilogue2 = false;
    let mut exit = None;
    let mut max_commands = 0;
    let mut iterations = 0u64;
    let mut last_progress = Instant::now();
    while !ready_menu(&j.runtime, "top") {
        iterations += 1;
        if last_progress.elapsed() >= Duration::from_secs(15) {
            eprintln!(
                "ORIGIN_WATA_PROGRESS iterations={iterations} max_commands={max_commands} stages={stages} {}",
                Driver::describe(&j.runtime)
            );
            last_progress = Instant::now();
        }
        assert!(iterations < 1_000_000, "Origin Watanagashi callbackbudget");
        let mut take_stage = None;
        if let Some(g) = game(&j.runtime) {
            if let Some(h) = g.host().filter(|h| h.runner.native_vm_attached()) {
                assert_eq!(
                    scenario(&j.runtime),
                    Some(10002),
                    "No unintended TIP/story replacement"
                );
                assert_eq!(
                    h.runner
                        .reference_register(Register::from_regular_register(22)),
                    Some(0)
                );
                max_commands = max_commands.max(h.runner.completed_commands());
                if let Some(m) = h.runner.native_save_metadata() {
                    let label = m.current()[1].as_ref();
                    // Stop canonical Ctrl a whole authored chapter segment before
                    // MSG164556, never by changing VM position or run budget.
                    neutral_tail |= label == "１２日目３" || label == "エピローグ１";
                    epilogue2 |= label == "エピローグ２";
                }
                if neutral_tail && g.reference_effective_play_modes() & 2 == 0 {
                    neutral_seen = true;
                }
                let scene = &h.runner.frame().scene;
                let p = scene.layers().front_properties().values();
                // advance_screen retires a completed wiper after the VM poll.
                // Other earlier Mono helpers legitimately use W=996. Bind this
                // observation to the authored message containing the four cuts.
                // Thus real WIPEWAIT remains pending for this completed frame,
                // before its next poll can advance MSGSIGNAL/reset the shader.
                // This also covers stage66, which has no following common WAIT.
                if h.runner.frame().message.playback.message_id == native_message_id(164556)
                    && scene.transition().is_none()
                    && p[21] == 1.
                    && matches!(h.runner.pending_command(), Some(Cmd::WIPEWAIT {}))
                    && h.runner.position().0 == 0x57b8c
                {
                    assert!(neutral_tail && neutral_seen);
                    assert_eq!(g.reference_effective_play_modes() & 2, 0);
                    assert_eq!(p[23], p[24]);
                    assert_eq!(p[24], p[25]);
                    assert_eq!(p[26], 1000.);
                    let stage = LEVELS
                        .iter()
                        .position(|x| *x == p[23])
                        .expect("ExactauthoredMono level");
                    if stage == stages {
                        eprintln!(
                            "ORIGIN_WATA_MONO stage={stage} pc=00057b8c RGB={} W={} nativeflags={:08x}",
                            p[23],
                            p[26],
                            h.runner.native_read_state().unwrap().flags
                        );
                        take_stage = Some(stage);
                    } else {
                        assert!(stage < stages, "Missing an earlier actualMono stage");
                    }
                }
                if stages == 4 && scene.transition().is_none() && p[21] == 0. {
                    restored = true;
                }
                if matches!(h.runner.pending_command(), Some(Cmd::EXIT { arg1: 0, .. })) {
                    assert_eq!(
                        h.runner.position().0,
                        0x12724a8,
                        "ActualfullmainEXIT, not appendedTIP or earlyreturn"
                    );
                    assert!(stages == 4 && restored && epilogue2);
                    exit = Some(h.runner.completed_commands());
                }
            }
        }
        if let Some(stage) = take_stage {
            capture(&mut j, &out, &format!("mono-{}", LEVELS[stage] as u32));
            stages += 1;
            preserved(&j, &before, &settings, ids);
        }
        // Resume skip only after all four actualMono captures and native21=0
        // restoration have been observed; no claim these waits are EVBEGIN-protected.
        story_step(
            &mut j,
            vp,
            deadline,
            !neutral_tail || (stages == 4 && restored),
            &mut release,
        );
    }
    assert_eq!(stages, 4);
    assert!(neutral_seen && restored && epilogue2 && exit.is_some());
    preserved(&j, &before, &settings, ids);
    let writes = j.storage.writes.lock().unwrap();
    assert_eq!(writes.len(), 1);
    let actual = std::fs::read(j.storage.paths.current.as_ref().unwrap()).unwrap();
    assert_eq!(writes[0], actual);
    let result = SaveData::decode(&actual).unwrap();
    assert_eq!(result.encode().unwrap(), actual);
    assert_ne!(result.persist_data.get(170) & 2, 0);
    drop(writes);
    capture(&mut j, &out, "returned-title");
    eprintln!(
        "ORIGIN_WATA_PASS fixture_unlocked10002/fourMono666,466,266,66/W1/defaultRestored/actualEXIT12724a8/onewrite/Title; max_commands={max_commands} iterations={iterations} pending_exit_completed={exit:?}; noearnedunlock/newshader/controller/Nintendo-frameclaim"
    );
}
