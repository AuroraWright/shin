//! Production entry/poll/input/composition/voice and fresh-ADV backlog journeys.
use super::system_menu::{Driver, driver_with_scenario};
use super::*;
use crate::adv::gerokasu2::{
    backlog::{chapter::Phase as ChapterPhase, lifecycle::Phase},
    message_clock::VoiceRequest,
};
use crate::app::gerokasu2::backlog::{self as live, Reference};
use shin_core::vm::command::types::AudioWaitStatus;

fn state(app: &Gerokasu2App) -> Option<Reference> {
    app.backlog.as_ref().and_then(live::Slot::reference)
}
fn await_state(d: &mut Driver<'_>, label: &str, ready: impl Fn(&Gerokasu2App) -> bool) {
    let end = Instant::now() + Duration::from_secs(30);
    while !ready(&d.app) {
        d.settle(1);
        assert!(Instant::now() < end, "{label}");
    }
}
fn voice_name() -> String {
    let rom = RomReader::new(
        StatelessFile::new(
            std::fs::File::open(std::env::var_os("SHIN_GEROKASU2_ROM").unwrap()).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    rom.traverse()
        .find_map(|(p, _)| {
            (p.starts_with("/voice/") && p.ends_with(".nxa")).then(|| p[7..p.len() - 4].to_owned())
        })
        .unwrap()
}
fn scenario() -> Arc<Gerokasu2Scenario> {
    scenario_rows(12)
}
fn scenario_rows(count: u32) -> Arc<Gerokasu2Scenario> {
    let original = Gerokasu2Scenario::new(bytes().into()).unwrap();
    let mut code = bytes();
    code.truncate(original.header.code_offset as usize);
    code.push(0xc1);
    for value in [0, 1, 0] {
        num(&mut code, value);
    }
    code.push(0xff);
    for value in [0xffff, -960, -540, 1920, 1080, 0, 0, 0] {
        num(&mut code, value);
    }
    for (op, args) in [
        (0x90, vec![0, 0, 0, 1000]),
        (0x95, vec![0, 0, 0, 0, 1000, 0, 1000]),
    ] {
        code.push(op);
        for arg in args {
            num(&mut code, arg);
        }
    }
    code.push(0xa0);
    num(&mut code, 0);
    let title = "履歴の章";
    code.extend(((title.len() + 1) as u16).to_le_bytes());
    code.extend(title.as_bytes());
    code.push(0);
    for i in 1..=count {
        if i == 1 || i == 7 || i == 125 {
            code.push(0xa1);
        }
        msg(
            &mut code,
            i,
            &format!(
                "@r{}履歴{i}を表示しています。@k",
                if i == 1 {
                    format!("@v{}.", voice_name())
                } else {
                    String::new()
                }
            ),
        );
    }
    code.extend([0, 0, 0]);
    let size = code.len() as u32;
    code[4..8].copy_from_slice(&size.to_le_bytes());
    Arc::new(Gerokasu2Scenario::new(code.into()).unwrap())
}
fn story_ready(d: &mut Driver<'_>) {
    d.settle(20);
    let end = Instant::now() + Duration::from_secs(20);
    while !retained(d.app.host.as_ref().unwrap()).contains("履歴5") {
        assert!(
            Instant::now() < end,
            "story did not advance naturally to fifth message"
        );
        d.press(AppAction::Enter);
        d.settle(15);
    }
    d.settle(15);
    assert!(d.app.host.as_ref().unwrap().scene.system_menu_allowed());
}
fn open(d: &mut Driver<'_>) {
    d.press(AppAction::Backlog);
    await_state(d, "browser factory/Start did not complete", |a| {
        state(a).is_some_and(|s| s.phase == Phase::Active)
    });
    d.settle(2);
    assert_eq!(d.app.modal.pause_depth, 1);
    assert!(state(&d.app).unwrap().rows.len() >= 4);
    d.draw();
}
fn pointer(d: &mut Driver<'_>, canvas: Vec2, held_down: bool) {
    let v = viewport();
    d.app.point = v.offset + canvas * v.size / Vec2::new(1920., 1080.);
    d.step(if held_down {
        held(AppAction::PointerClick)
    } else {
        EnumMap::default()
    });
}
fn source_voice(d: &mut Driver<'_>) {
    let voice = d.app.host.as_mut().unwrap().scene.runner_mut().voice_mut();
    assert!(voice.play(VoiceRequest {
        filename: voice_name(),
        volume: 0.8,
        lipsync: false,
        segment_start: 0,
        segment_duration: 0
    }));
    await_state(d, "source voice did not load", |a| {
        !a.host
            .as_ref()
            .unwrap()
            .scene
            .runner()
            .voice()
            .wait_status()
            .contains(AudioWaitStatus::LOADING)
    });
}
fn question(d: &mut Driver<'_>) {
    d.press(AppAction::SystemMenu);
    await_state(d, "backlog jump question did not appear", |a| {
        state(a).is_some_and(|s| s.question)
    });
    d.settle(20);
    d.draw();
}

#[test]
#[ignore = "backlog factory failure before modal acquisition on Metal/the default audio output; temporary missing-assets directory"]
fn ger_app_backlog_failed_factory_retains_live_scene_without_modal_side_effects() {
    let gpu = Offscreen::new();
    let (temporary, _path, mut d) = driver_with_scenario(&gpu, scenario());
    story_ready(&mut d);
    let original = d.app.assets.clone();
    let mut io = LayeredAssetIo::new();
    io.try_with_dir(&temporary.0).unwrap();
    d.app.assets = Arc::new(AssetServer::new(
        io.into(),
        AssetLoadContext {
            game: Game::Gerokasu2,
            wgpu_device: gpu.device.clone(),
            wgpu_queue: gpu.queue.clone(),
            bustup_cache: AssetCache::new(),
            higurashi_bustup_cache: Default::default(),
            picture_cache: Default::default(),
            font_cache: Default::default(),
        },
    ));
    let progress = d
        .app
        .host
        .as_ref()
        .unwrap()
        .scene
        .save_session()
        .progress
        .clone();
    let commands = d
        .app
        .host
        .as_ref()
        .unwrap()
        .scene
        .runner()
        .completed_commands();
    d.press(AppAction::Backlog);
    d.app.assets = original;
    await_state(
        &mut d,
        "failed backlog factory did not release preparation",
        |a| a.backlog.is_none(),
    );
    assert_eq!(d.app.modal.pause_depth, 0);
    assert_eq!(d.app.modal.conceal_depth, 0);
    assert!(d.app.backdrop.is_none());
    assert_eq!(
        d.app.host.as_ref().unwrap().scene.save_session().progress,
        progress
    );
    assert_eq!(
        d.app
            .host
            .as_ref()
            .unwrap()
            .scene
            .runner()
            .completed_commands(),
        commands
    );
    open(&mut d);
    d.press(AppAction::Cancel);
    await_state(&mut d, "retry browser did not close", |a| {
        a.backlog.is_none()
    });
    d.draw();
    println!(
        "GER_BACKLOG_FACTORY_APP failure_no_mode_pause_conceal_capture_mutation_source_retained_retry=verified frames={} audio=default output",
        d.frames.frames
    );
}

#[test]
#[ignore = "actual live backlog entry, ChapterSelector/pointer/private voice/early modal return on Metal/the default audio output; temporary progress"]
fn ger_app_backlog_opens_routes_chapter_voice_pointer_and_resumes_before_retirement() {
    let gpu = Offscreen::new();
    let (_temporary, _path, mut d) = driver_with_scenario(&gpu, scenario());
    story_ready(&mut d);
    d.app
        .save_store
        .as_mut()
        .unwrap()
        .data
        .settings
        .v4_voicefocus = true;
    let mut both = held(AppAction::SystemMenu);
    both[AppAction::Backlog] = held(AppAction::Backlog)[AppAction::Backlog];
    d.step(both);
    d.step(EnumMap::default());
    d.ready();
    assert!(d.app.backlog.is_none());
    assert!(d.app.system.is_some());
    d.press(AppAction::Cancel);
    d.settle(40);
    source_voice(&mut d);
    let boundary = d
        .app
        .host
        .as_ref()
        .unwrap()
        .scene
        .save_session()
        .timeline
        .boundary;
    let commands = d
        .app
        .host
        .as_ref()
        .unwrap()
        .scene
        .runner()
        .completed_commands();
    open(&mut d);
    let frozen = d
        .app
        .host
        .as_ref()
        .unwrap()
        .scene
        .runner()
        .voice()
        .timeline()
        .position;
    let row = state(&d.app)
        .unwrap()
        .rows
        .iter()
        .position(|r| r.kind == 0 && r.voice_count != 0)
        .unwrap();
    while state(&d.app).unwrap().selected > row {
        d.press(AppAction::Up);
        d.settle(5);
    }
    while state(&d.app).unwrap().selected < row {
        d.press(AppAction::AnyDown);
        d.settle(5);
    }
    // InputArea emits Press after150ms. That callback starts a four-tick
    // reveal even when the row is already visible; Release must survive the
    // resulting busy scroll and still reach the portrait voice action.
    let s = state(&d.app).unwrap();
    let y = s.rows[row].top - s.scroll + 160. + s.rows[row].height / 2.;
    assert!((160. ..992.).contains(&y));
    let portrait = Vec2::new(300., y);
    pointer(&mut d, portrait, true);
    for _ in 0..14 {
        pointer(&mut d, portrait, true);
        if state(&d.app).unwrap().scroll_busy {
            break;
        }
    }
    assert!(state(&d.app).unwrap().scroll_busy);
    assert!(!state(&d.app).unwrap().voice);
    pointer(&mut d, portrait, false);
    await_state(&mut d, "private voice did not advance", |a| {
        state(a).is_some_and(|s| s.voice_micros > 0)
    });
    assert_eq!(
        d.app
            .host
            .as_ref()
            .unwrap()
            .scene
            .runner()
            .voice()
            .timeline()
            .position,
        frozen
    );
    d.settle(5);
    assert!(
        d.app
            .menu_bgm
            .lock()
            .shared_audio()
            .lock()
            .bgm_player
            .reference_focus_gain()
            < 1.
    );
    assert_eq!(
        d.app
            .host
            .as_ref()
            .unwrap()
            .scene
            .runner()
            .completed_commands(),
        commands
    );
    d.press(AppAction::Enter);
    d.settle(12);
    d.press(AppAction::Backlog);
    d.settle(7);
    assert_eq!(state(&d.app).unwrap().chapter, Some(ChapterPhase::Active));
    d.draw();
    d.press(AppAction::PageDown);
    d.settle(5);
    d.draw();
    d.press(AppAction::Cancel);
    await_state(
        &mut d,
        "chapter did not reach its done-setting frame",
        |a| state(a).is_some_and(|s| s.chapter == Some(ChapterPhase::Finished)),
    );
    d.draw();
    let selected = state(&d.app).unwrap().selected;
    let (action, expected) = if selected > 0 {
        (AppAction::Up, selected - 1)
    } else {
        (AppAction::AnyDown, 1)
    };
    // Child is retained on its final frame, then Main accepts a fresh press
    // in the same frame that108 sweeps the finished child.
    d.step(held(action));
    assert!(state(&d.app).unwrap().chapter.is_none());
    assert_eq!(state(&d.app).unwrap().selected, expected);
    d.settle(5);
    assert_eq!(d.app.modal.pause_depth, 1);
    let s = state(&d.app).unwrap();
    let y = (s.rows[s.selected].top - s.scroll + 190.).clamp(180., 950.);
    pointer(&mut d, Vec2::new(300., y), true);
    d.app.focus_lost();
    pointer(&mut d, Vec2::new(300., y), false);
    d.app.set_suspended(true);
    let before = state(&d.app).unwrap().scroll;
    d.settle(3);
    assert_eq!(state(&d.app).unwrap().scroll, before);
    d.app.set_suspended(false);
    d.settle(2);
    // Native pointer close leaves private voice owned until the outgoing owner
    // retires. reason0 resumes the ADV before its ten-tick fade completes.
    pointer(&mut d, Vec2::new(1800., 80.), true);
    pointer(&mut d, Vec2::new(1800., 80.), false);
    await_state(&mut d, "ordinary backlog Exit did not release modal", |a| {
        a.modal.pause_depth == 0
    });
    assert!(d.app.backlog.is_some());
    assert_eq!(state(&d.app).unwrap().phase, Phase::Fading);
    d.draw();
    assert_eq!(
        d.app
            .host
            .as_ref()
            .unwrap()
            .scene
            .save_session()
            .timeline
            .boundary,
        boundary
    );
    await_state(&mut d, "browser did not retire after its fade", |a| {
        a.backlog.is_none()
    });
    assert_eq!(d.app.modal.conceal_depth, 0);
    d.settle(3);
    d.draw();
    println!(
        "GER_BACKLOG_APP entry_native_input_held_portrait_release_during_reveal_ChapterSelector_stencil_next_sweep_Main_pointer_focus_suspend_private_voice_BGM_focus_source_frozen_early_return_retirement=verified frames={} audio=default output",
        d.frames.frames
    );
}

#[test]
#[ignore = "live backlog defaultNo/cancel/failed and successful memory-only jump publication on Metal/the default audio output; temporary progress"]
fn ger_app_backlog_jump_preserves_old_source_until_memory_destination_succeeds() {
    let gpu = Offscreen::new();
    let gate = live::measurement::Gate::new(100);
    let (temporary, _path, mut d) = driver_with_scenario(&gpu, scenario_rows(320));
    d.app.backlog_options.gate = Some(gate.clone());
    // The native preference otherwise stops history at the first unread row.
    d.app
        .save_store
        .as_mut()
        .unwrap()
        .data
        .settings
        .v11_disallowskipunread = false;
    story_ready(&mut d);
    open(&mut d);
    assert_eq!(state(&d.app).unwrap().measured, (100, true, false));
    question(&mut d);
    d.press(AppAction::Enter);
    d.settle(25);
    assert!(!state(&d.app).unwrap().question);
    assert_eq!(d.app.modal.pause_depth, 1);
    assert!(d.app.loading_game.is_none());
    d.press(AppAction::Up);
    d.settle(5);
    let before = state(&d.app).unwrap();
    let saved = before.frozen.progress.active_entry().save_position;
    let expected = before.rows[before.selected].index;
    question(&mut d);
    d.press(AppAction::Cancel);
    d.settle(25);
    assert!(d.app.loading_game.is_none());
    let original_assets = d.app.assets.clone();
    let mut io = LayeredAssetIo::new();
    io.try_with_dir(&temporary.0).unwrap();
    d.app.assets = Arc::new(AssetServer::new(
        io.into(),
        AssetLoadContext {
            game: Game::Gerokasu2,
            wgpu_device: gpu.device.clone(),
            wgpu_queue: gpu.queue.clone(),
            bustup_cache: AssetCache::new(),
            higurashi_bustup_cache: Default::default(),
            picture_cache: Default::default(),
            font_cache: Default::default(),
        },
    ));
    let source_commands = d
        .app
        .host
        .as_ref()
        .unwrap()
        .scene
        .runner()
        .completed_commands();
    question(&mut d);
    d.press(AppAction::Left);
    d.press(AppAction::Enter);
    await_state(&mut d, "Yes callback did not retain a memory jump", |a| {
        state(a).is_some_and(|s| s.jump.is_some())
    });
    let jump = state(&d.app).unwrap().jump.unwrap();
    assert_eq!(jump.progress.active_entry().replay_target, saved);
    assert_ne!(jump.progress.active_entry().save_position, saved);
    await_state(
        &mut d,
        "failed destination did not recover retained browser",
        |a| state(a).is_some_and(|s| s.phase == Phase::Active) && a.loading_game.is_none(),
    );
    assert_eq!(d.app.modal.pause_depth, 1);
    assert_eq!(
        d.app
            .host
            .as_ref()
            .unwrap()
            .scene
            .runner()
            .completed_commands(),
        source_commands
    );
    gate.advance(150);
    await_state(
        &mut d,
        "failed jump did not resume remaining measurement",
        |a| state(a).is_some_and(|s| s.measured == (150, true, false) && s.allowed.len() == 150),
    );
    d.app.assets = original_assets;
    d.settle(2);
    question(&mut d);
    d.press(AppAction::Left);
    d.press(AppAction::Enter);
    await_state(&mut d, "successful memory jump did not publish", |a| {
        a.backlog.is_none() && a.loading_game.is_none()
    });
    assert_eq!(d.app.modal.pause_depth, 0);
    assert!(d.app.title.is_none());
    assert_eq!(
        d.app
            .host
            .as_ref()
            .unwrap()
            .scene
            .save_session()
            .progress
            .active_entry()
            .replay_target,
        saved
    );
    d.settle(20);
    d.draw();
    assert!(d.app.host.as_ref().unwrap().scene.system_menu_allowed());
    println!(
        "GER_BACKLOG_JUMP_APP row={expected} old_saved={saved} defaultNo_cancel_failure_retains_scope_and_source_measurement_100_to_150_fresh_token_retry_memory_pair_freshADV_mode0_publication=verified frames={} audio=default output",
        d.frames.frames
    );
}

#[test]
#[ignore = "actual incremental backlog publication/growth/cancellation on Metal/the default audio output; temporary progress"]
fn ger_app_backlog_incremental_measurement_publishes_grows_and_retires_worker() {
    let gpu = Offscreen::new();
    let gate = live::measurement::Gate::new(100);
    let (_temporary, _path, mut d) = driver_with_scenario(&gpu, scenario_rows(320));
    d.app.backlog_options.gate = Some(gate.clone());
    // The native preference otherwise stops history at the first unread row.
    d.app
        .save_store
        .as_mut()
        .unwrap()
        .data
        .settings
        .v11_disallowskipunread = false;
    story_ready(&mut d);
    open(&mut d);
    let before = state(&d.app).unwrap();
    assert_eq!(before.measured, (100, true, false));
    assert_eq!(before.allowed.len(), 100);
    assert!(before.rows.len() > 300);
    assert_eq!(before.rows[before.allowed.end].height, 0.);
    assert_eq!(before.rows[before.allowed.end].voice_count, 0);
    d.press(AppAction::AnyDown);
    let selected = state(&d.app).unwrap().selected;
    d.settle(5);
    d.press(AppAction::Backlog);
    d.settle(7);
    assert_eq!(state(&d.app).unwrap().chapter, Some(ChapterPhase::Active));
    let before_pixels = d.capture();
    gate.advance(150);
    await_state(&mut d, "live geometry did not grow to150", |a| {
        state(a).is_some_and(|s| s.measured == (150, true, false) && s.allowed.len() == 150)
    });
    let after = state(&d.app).unwrap();
    assert_eq!(after.selected, selected);
    assert_eq!(after.frozen, before.frozen);
    for i in before.allowed {
        assert_eq!(after.rows[i].top.to_bits(), before.rows[i].top.to_bits());
        assert_eq!(
            after.rows[i].height.to_bits(),
            before.rows[i].height.to_bits()
        );
    }
    let after_pixels = d.capture();
    assert_ne!(
        before_pixels, after_pixels,
        "live Chapter/scrollbar must reflect growing measured bounds"
    );
    d.press(AppAction::Cancel);
    await_state(&mut d, "incremental chapter did not retire", |a| {
        state(a).is_some_and(|s| s.chapter.is_none())
    });
    d.settle(2);
    d.press(AppAction::Cancel);
    assert_eq!(
        state(&d.app).unwrap().measured,
        (150, true, false),
        "close flash must not cancel measurement before Exit"
    );
    await_state(&mut d, "incremental browser did not Exit", |a| {
        a.modal.pause_depth == 0
    });
    assert!(state(&d.app).unwrap().measured.2);
    await_state(&mut d, "canceled geometry worker did not retire", |a| {
        state(a).is_none_or(|s| !s.measured.1)
    });
    if let Some(s) = state(&d.app) {
        assert_eq!(s.measured.0, 150);
    }
    await_state(&mut d, "incremental browser did not finish fade", |a| {
        a.backlog.is_none()
    });
    assert!(!gate.blocked());
    assert_eq!(d.app.modal.conceal_depth, 0);
    d.draw();
    println!(
        "GER_BACKLOG_INCREMENTAL_APP metadata_over300_ready100_busy_growth150_fixed_rows_selection_Chapter_close_flash_Exit_cancel_worker_retired=verified frames={} audio=default output",
        d.frames.frames
    );
}
