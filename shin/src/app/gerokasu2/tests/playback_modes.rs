use super::{system_menu::driver_with_scenario, *};

fn host<'a>(d: &'a system_menu::Driver<'_>) -> &'a GerHost {
    d.app.host.as_ref().unwrap()
}
fn host_mut<'a>(d: &'a mut system_menu::Driver<'_>) -> &'a mut GerHost {
    d.app.host.as_mut().unwrap()
}
fn modes(d: &system_menu::Driver<'_>) -> (bool, bool) {
    let p = d.app.host.as_ref().unwrap().scene.playback();
    (p.modes.autoplay, p.modes.skip)
}
fn cues(d: &system_menu::Driver<'_>) -> usize {
    d.app
        .menu_bgm
        .lock()
        .shared_audio()
        .lock()
        .system_player
        .sounds
        .len()
}
/// Press once and report whether the native 258F0 cue started.
fn press_cued(d: &mut system_menu::Driver<'_>, action: AppAction) -> bool {
    let before = cues(d);
    d.step(held(action));
    let cued = cues(d) > before;
    d.step(EnumMap::default());
    cued
}

#[test]
#[ignore = "real story L/R/A/B/X/Y playback-mode and HiddenWindow controls through the app path on Metal/exact Loopback Audio"]
fn ger_app_real_story_playback_mode_controls_match_native_dispatch() {
    let gpu = Offscreen::new();
    let scenario = Arc::new(Gerokasu2Scenario::new(bytes().into()).unwrap());
    let (_temporary, _path, mut d) = driver_with_scenario(&gpu, scenario);
    let deadline = Instant::now() + Duration::from_secs(60);
    while !d.app.host.as_ref().unwrap().scene.system_menu_allowed() {
        d.step(held(AppAction::Enter));
        d.settle(10);
        assert!(Instant::now() < deadline, "first story message");
    }
    d.settle(5);

    // L toggles autoplay with `son`; A keeps it latched (11650 excludes A).
    assert!(press_cued(&mut d, AppAction::ToggleAuto));
    assert_eq!(modes(&d), (true, false));
    d.press(AppAction::Enter);
    assert_eq!(modes(&d), (true, false), "A must not cancel autoplay");
    // Autoplay advances after the voice ends, without further input.
    let before = d.app.host.as_ref().unwrap().scene.runner().completed_commands();
    while d.app.host.as_ref().unwrap().scene.runner().completed_commands() == before {
        d.step(EnumMap::default());
        std::thread::sleep(Duration::from_millis(16));
        assert!(Instant::now() < deadline, "autoplay did not advance");
    }
    // B clears an active mode with `cancel`.
    assert!(press_cued(&mut d, AppAction::Cancel));
    assert_eq!(modes(&d), (false, false));
    // Idle B opens HiddenWindow (1346C) without a cue: conceal is held and
    // the scene ignores A until X/Y/A/B releases it. The press does nothing else.
    d.settle(5);
    assert!(!press_cued(&mut d, AppAction::Cancel), "idle B has no mode cue");
    assert!(host(&d).hidden_window && d.app.modal.conceal_depth == 1);
    assert!(host(&d).scene.playback().hidden_ui);
    let before = host(&d).scene.runner().completed_commands();
    d.press(AppAction::Enter);
    assert!(!host(&d).hidden_window && d.app.modal.conceal_depth == 0);
    d.settle(5);
    assert_eq!(host(&d).scene.runner().completed_commands(), before, "A only restored");
    assert!(d.app.system.is_none() && d.app.backlog.is_none());
    d.press(AppAction::Cancel);
    assert!(host(&d).hidden_window);
    d.press(AppAction::SystemMenu);
    assert!(!host(&d).hidden_window && d.app.system.is_none() && d.app.loading_system.is_none());

    // R toggles skip; X clears it instead of opening the backlog.
    assert!(press_cued(&mut d, AppAction::ToggleSkip));
    assert_eq!(modes(&d), (false, true));
    assert!(press_cued(&mut d, AppAction::Backlog));
    assert_eq!(modes(&d), (false, false));
    assert!(d.app.backlog.is_none());

    // Y clears an active mode with `cancel`, then opens the menu.
    assert!(press_cued(&mut d, AppAction::ToggleAuto));
    assert!(press_cued(&mut d, AppAction::SystemMenu));
    assert_eq!(modes(&d), (false, false));
    assert!(d.app.system.is_none() && d.app.loading_system.is_none());
    d.press(AppAction::SystemMenu);
    d.settle(5);
    assert!(d.app.system.is_some() || d.app.loading_system.is_some());
    println!("GEROKASU2_PLAYBACK_MODE_CONTROLS L/R=son|cancel A=kept B/X/Y=clear menu=Y idleB=hide A/Y=restore");
}

/// Retired cues are pruned on update; count a new cue only from silence.
fn quiet(d: &mut system_menu::Driver<'_>) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while cues(d) != 0 {
        d.step(EnumMap::default());
        std::thread::sleep(Duration::from_millis(8));
        assert!(Instant::now() < deadline, "system cues did not finish");
    }
}
fn reconstructed(d: &mut system_menu::Driver<'_>, deadline: Instant) {
    // The decision flashes, then the final callback runs after five ticks.
    for _ in 0..60 {
        if host(d).chapter.is_none() {
            break;
        }
        d.step(EnumMap::default());
    }
    assert!(host(d).chapter.is_none(), "Yes did not close the Question");
    assert!(host(d).scene.reconstruction_phase().is_some(), "Yes must start mode 0");
    while host(d).scene.reconstruction_phase().is_some() {
        d.step(EnumMap::default());
        std::thread::sleep(Duration::from_millis(4));
        assert!(Instant::now() < deadline, "chapter reconstruction did not finish");
    }
    d.settle(5);
}

#[test]
#[ignore = "real story Plus/Minus chapter Questions and mode-0 reconstruction on Metal/exact Loopback Audio"]
fn ger_app_real_story_chapter_skip_matches_native_dispatch() {
    use crate::adv::gerokasu2::replay::chapter_skip;
    let gpu = Offscreen::new();
    let scenario = Arc::new(Gerokasu2Scenario::new(bytes().into()).unwrap());
    let (_temporary, _path, mut d) = driver_with_scenario(&gpu, scenario.clone());
    let deadline = Instant::now() + Duration::from_secs(120);
    while !host(&d).scene.system_menu_allowed() {
        d.step(held(AppAction::Enter));
        d.settle(10);
        assert!(Instant::now() < deadline, "first story message");
    }
    d.settle(5);
    let source = host(&d).scene.save_session().timeline.boundary;

    // Unread with skip-unread disallowed (default): EBB0 finds no target and
    // 1264C plays only the error cue, keeping the active mode.
    assert!(host(&d).scene.playback().disallow_skip_unread);
    assert!(press_cued(&mut d, AppAction::ToggleAuto));
    assert!(press_cued(&mut d, AppAction::NextChapter));
    assert!(host(&d).chapter.is_none());
    assert_eq!(modes(&d), (true, false));
    assert!(press_cued(&mut d, AppAction::ToggleAuto));
    d.settle(5);

    // Allowed: decide cue, Question with the native text and No selected.
    // The scene's buttons belong to the child; B cancels it.
    host_mut(&mut d).scene.playback_mut().disallow_skip_unread = false;
    let expected = {
        let h = host(&d);
        let p = h.scene.playback();
        chapter_skip::next(&scenario, h.scene.save_session(), &p.seen_messages, true)
            .unwrap()
            .unwrap()
            .save
            .progress
            .active_entry()
            .save_position
    };
    quiet(&mut d);
    assert!(press_cued(&mut d, AppAction::NextChapter));
    assert!(host(&d).chapter.as_ref().is_some_and(|q| q.forward));
    d.settle(20);
    d.press(AppAction::Backlog);
    d.press(AppAction::SystemMenu);
    assert!(d.app.backlog.is_none() && d.app.system.is_none() && d.app.loading_system.is_none());
    d.press(AppAction::Cancel);
    d.settle(20);
    assert!(host(&d).chapter.is_none() && !host(&d).hidden_window);
    assert!(host(&d).scene.reconstruction_phase().is_none());
    assert_eq!(host(&d).scene.save_session().timeline.boundary, source);

    // Yes forks at the clone's last_skip and reconstructs with mode 0.
    quiet(&mut d);
    assert!(press_cued(&mut d, AppAction::NextChapter));
    d.settle(20);
    d.press(AppAction::Left);
    d.press(AppAction::Enter);
    reconstructed(&mut d, deadline);
    let timeline = host(&d).scene.save_session().timeline;
    assert_eq!(timeline.boundary, expected);
    assert!(expected > source);

    // Minus: the chapter just entered has no messages, so 18A84 returns to
    // the previous chapter start.
    let back = if timeline.chapter_messages > 1 {
        timeline.current_chapter
    } else {
        timeline.previous_chapter
    };
    quiet(&mut d);
    assert!(press_cued(&mut d, AppAction::PreviousChapter));
    assert!(host(&d).chapter.as_ref().is_some_and(|q| !q.forward));
    d.settle(20);
    d.press(AppAction::Left);
    d.press(AppAction::Enter);
    reconstructed(&mut d, deadline);
    let restored = host(&d).scene.save_session().timeline.boundary;
    assert!(restored >= back && restored < expected, "{restored} {back}");
    println!(
        "GEROKASU2_CHAPTER_SKIP_APP source={source} plus={expected} minus_target={back} minus_boundary={restored} unread_plus=error question=B/Yes"
    );
}

fn pointer(d: &mut system_menu::Driver<'_>, canvas: Vec2, down: bool) {
    let v = viewport();
    d.app.point = v.offset + canvas * v.size / Vec2::new(1920., 1080.);
    d.step(if down {
        held(AppAction::PointerClick)
    } else {
        EnumMap::default()
    });
}
fn tap(d: &mut system_menu::Driver<'_>, canvas: Vec2) {
    pointer(d, canvas, true);
    pointer(d, canvas, false);
}
fn swipe(d: &mut system_menu::Driver<'_>, from: Vec2, to: Vec2) {
    for i in 0..=4 {
        pointer(d, from.lerp(to, i as f32 / 4.), true);
    }
    pointer(d, to, false);
}
/// Mean RGB of a canvas rectangle in the 320x180 capture.
fn region(pixels: &[u8], left: f32, top: f32, right: f32, bottom: f32) -> [u32; 3] {
    let (x0, x1) = ((left / 6.) as usize, (right / 6.) as usize);
    let (y0, y1) = ((top / 6.) as usize, (bottom / 6.) as usize);
    let mut sum = [0u32; 3];
    for y in y0..y1 {
        for x in x0..x1 {
            for c in 0..3 {
                sum[c] += pixels[(y * 320 + x) * 4 + c] as u32;
            }
        }
    }
    let n = ((x1 - x0) * (y1 - y0)) as u32;
    sum.map(|s| s / n)
}

#[test]
#[ignore = "real story indicators, navi toolbar, swipes, HiddenWindow tap and touch effects on Metal/exact Loopback Audio"]
fn ger_app_real_story_touch_controls_match_native_callbacks() {
    let gpu = Offscreen::new();
    let scenario = Arc::new(Gerokasu2Scenario::new(bytes().into()).unwrap());
    let (_temporary, _path, mut d) = driver_with_scenario(&gpu, scenario);
    let deadline = Instant::now() + Duration::from_secs(90);
    while !host(&d).scene.system_menu_allowed() {
        d.step(held(AppAction::Enter));
        d.settle(10);
        assert!(Instant::now() < deadline, "first story message");
    }
    d.settle(5);
    let indicator = |p: &[u8]| region(p, 1824., 24., 1896., 96.);
    let toolbar = |p: &[u8]| region(p, 1332., 160., 1908., 412.);
    let before = d.capture();

    // L shows the auto indicator at the skip slot (no skip indicator).
    assert!(press_cued(&mut d, AppAction::ToggleAuto));
    d.settle(2);
    let auto = d.capture();
    assert_ne!(indicator(&before), indicator(&auto), "auto indicator drawn");

    // Left swipe: `page` cue and the toolbar slides in at rate 0.1.
    quiet(&mut d);
    swipe(&mut d, Vec2::new(1500., 650.), Vec2::new(1100., 650.));
    assert!(host(&d).root.navi_shown && cues(&d) > 0);
    d.settle(12);
    assert!(host(&d).root.navi_ready());
    let shown = d.capture();
    assert_ne!(toolbar(&auto), toolbar(&shown), "toolbar drawn");

    // Toolbar Auto (ID 0) toggles the mode off with `cancel`.
    quiet(&mut d);
    tap(&mut d, Vec2::new(1332. + 96., 160. + 60.));
    assert_eq!(modes(&d), (false, false));
    assert!(cues(&d) > 0);
    // Toolbar Skip (ID 1) toggles skip on; the indicator moves to 1824.
    tap(&mut d, Vec2::new(1524. + 96., 160. + 60.));
    assert_eq!(modes(&d), (false, true));
    tap(&mut d, Vec2::new(1524. + 96., 160. + 60.));
    assert_eq!(modes(&d), (false, false));
    d.settle(10);

    // Toolbar Plus with unread text plays only the error cue.
    tap(&mut d, Vec2::new(1620. + 96., 292. + 60.));
    assert!(host(&d).chapter.is_none());
    // Toolbar Minus opens the Question; B closes it.
    tap(&mut d, Vec2::new(1428. + 96., 292. + 60.));
    assert!(host(&d).chapter.as_ref().is_some_and(|q| !q.forward));
    d.settle(20);
    d.press(AppAction::Cancel);
    d.settle(20);
    assert!(host(&d).chapter.is_none());

    // Right swipe with the toolbar shown hides it; no menu opens.
    swipe(&mut d, Vec2::new(900., 650.), Vec2::new(1300., 650.));
    assert!(!host(&d).root.navi_shown);
    d.settle(12);
    assert!(d.app.system.is_none() && d.app.loading_system.is_none());

    // Down swipe hides the window. A drag does not restore it; a tap does,
    // and that tap does nothing else.
    swipe(&mut d, Vec2::new(960., 300.), Vec2::new(960., 700.));
    d.settle(2);
    assert!(host(&d).hidden_window && d.app.modal.conceal_depth == 1);
    swipe(&mut d, Vec2::new(600., 500.), Vec2::new(1000., 500.));
    d.settle(2);
    assert!(host(&d).hidden_window, "a drag must not restore the window");
    let commands = host(&d).scene.runner().completed_commands();
    tap(&mut d, Vec2::new(960., 540.));
    d.settle(2);
    assert!(!host(&d).hidden_window && d.app.modal.conceal_depth == 0);
    assert_eq!(host(&d).scene.runner().completed_commands(), commands);
    d.settle(5);

    // Toolbar Backlog opens after its flash.
    swipe(&mut d, Vec2::new(1500., 650.), Vec2::new(1100., 650.));
    d.settle(12);
    tap(&mut d, Vec2::new(1716. + 96., 160. + 60.));
    assert!(d.app.backlog.is_none(), "backlog waits for the flash");
    for _ in 0..30 {
        if d.app.backlog.is_some() {
            break;
        }
        d.step(EnumMap::default());
    }
    assert!(d.app.backlog.is_some(), "toolbar backlog request");
    println!(
        "GEROKASU2_TOUCH_CONTROLS indicator={:?}->{:?} toolbar={:?}->{:?} left=navi right=hide down=hidden tap=restore backlog=flash",
        indicator(&before),
        indicator(&auto),
        toolbar(&auto),
        toolbar(&shown)
    );
}

#[test]
#[ignore = "real story right swipe SystemMenu, message tap and global touch effects on Metal/exact Loopback Audio"]
fn ger_app_real_story_swipe_menu_tap_and_touch_effects() {
    let gpu = Offscreen::new();
    let scenario = Arc::new(Gerokasu2Scenario::new(bytes().into()).unwrap());
    let (_temporary, _path, mut d) = driver_with_scenario(&gpu, scenario);
    let deadline = Instant::now() + Duration::from_secs(90);
    while !host(&d).scene.system_menu_allowed() {
        d.step(held(AppAction::Enter));
        d.settle(10);
        assert!(Instant::now() < deadline, "first story message");
    }
    d.settle(5);
    // A tap advances the message like A.
    let commands = host(&d).scene.runner().completed_commands();
    while host(&d).scene.runner().completed_commands() == commands {
        tap(&mut d, Vec2::new(960., 700.));
        d.settle(3);
        assert!(Instant::now() < deadline, "tap did not advance");
    }
    while !host(&d).scene.system_menu_allowed() {
        d.step(EnumMap::default());
        assert!(Instant::now() < deadline, "next story message");
    }
    d.settle(5);

    // Touch effects follow Config's touch row (v17): stars on drags and a
    // ripple capture on taps.
    let size = winit::dpi::PhysicalSize::new(1920, 1080);
    let effect = |d: &mut system_menu::Driver<'_>, p: Vec2, down: bool| {
        d.app.point = p;
        let mut encoder = d.gpu.device.create_command_encoder(&Default::default());
        let mut ctx = PreRenderContext {
            device: &d.gpu.device,
            queue: &d.gpu.queue,
            breakup_texture: None,
            resize_source: &d.frames.resize,
            sampler_store: &d.frames.samplers,
            depth_stencil: DepthStencilTarget {
                view: &d.frames.depth,
            },
            pipeline_storage: &mut d.frames.pipelines,
            dynamic_buffer: &mut d.frames.dynamic,
            encoder: &mut encoder,
        };
        d.app.prepare_touch_frame(&mut ctx, Ticks::from_f32(1.), down, size);
        let mut uploads = d.gpu.device.create_command_encoder(&Default::default());
        d.frames.dynamic.finish(&mut uploads);
        d.gpu.queue.submit([uploads.finish(), encoder.finish()]);
        d.frames.dynamic.recall();
    };
    d.app.save_store.as_mut().unwrap().data.settings.v17_showscenetitle = false;
    effect(&mut d, Vec2::new(500., 500.), true);
    effect(&mut d, Vec2::new(500., 500.), false);
    assert!(d.app.touch_background.is_none() && !d.app.touch_effect.has_stars());
    d.app.save_store.as_mut().unwrap().data.settings.v17_showscenetitle = true;
    // 252C8 constructs TouchEffect disabled; 5D0FC enables it after FirstBoot.
    d.app.first_boot = Some(
        shin_tasks::block_on(super::super::first_boot::FirstBoot::load(d.app.assets.clone()))
            .unwrap(),
    );
    effect(&mut d, Vec2::new(500., 500.), true);
    effect(&mut d, Vec2::new(500., 500.), false);
    effect(&mut d, Vec2::new(500., 500.), false);
    assert!(d.app.touch_background.is_none(), "no ripple during FirstBoot");
    d.app.first_boot = None;
    // 自動スリープの抑制 (v18) keeps the window running unfocused.
    use shin_window::ShinApp;
    d.app.save_store.as_mut().unwrap().data.settings.v18_showsongtitle = false;
    assert!(!d.app.run_in_background());
    d.app.save_store.as_mut().unwrap().data.settings.v18_showsongtitle = true;
    assert!(d.app.run_in_background());
    effect(&mut d, Vec2::new(500., 500.), true);
    effect(&mut d, Vec2::new(500., 500.), false);
    effect(&mut d, Vec2::new(500., 500.), false);
    assert!(d.app.touch_background.is_some(), "tap ripple captures the frame");
    for i in 0..6 {
        effect(&mut d, Vec2::new(500. + 40. * i as f32, 500.), true);
    }
    assert!(d.app.touch_effect.has_stars(), "drag stars");
    effect(&mut d, Vec2::new(740., 500.), false);
    d.app.system_textures = Some(
        shin_tasks::block_on(d.app.assets.load("/systex.txa")).unwrap(),
    );
    {
        let mut encoder = d.gpu.device.create_command_encoder(&Default::default());
        let mut ctx = PreRenderContext {
            device: &d.gpu.device,
            queue: &d.gpu.queue,
            breakup_texture: None,
            resize_source: &d.frames.resize,
            sampler_store: &d.frames.samplers,
            depth_stencil: DepthStencilTarget {
                view: &d.frames.depth,
            },
            pipeline_storage: &mut d.frames.pipelines,
            dynamic_buffer: &mut d.frames.dynamic,
            encoder: &mut encoder,
        };
        let mut pass = ctx.begin_pass(
            TextureTarget {
                kind: TextureTargetKind::Screen,
                view: &d.frames.view,
            },
            Some(ctx.depth_stencil),
            "Ger touch effect frame",
        );
        d.app.render_touch_frame(&mut pass).unwrap();
        drop(pass);
        let mut uploads = d.gpu.device.create_command_encoder(&Default::default());
        d.frames.dynamic.finish(&mut uploads);
        d.gpu.queue.submit([uploads.finish(), encoder.finish()]);
        d.frames.dynamic.recall();
    }

    // Right swipe with the toolbar hidden opens the SystemMenu.
    d.app.point = Vec2::ZERO;
    swipe(&mut d, Vec2::new(900., 650.), Vec2::new(1300., 650.));
    for _ in 0..30 {
        if d.app.system.is_some() || d.app.loading_system.is_some() {
            break;
        }
        d.step(EnumMap::default());
    }
    assert!(d.app.system.is_some() || d.app.loading_system.is_some());
    println!("GEROKASU2_TOUCH_MENU_EFFECTS tap=advance ripple=capture drag=stars right=SystemMenu");
}

#[test]
#[ignore = "real story Up (13538 mode-1 back) and Down (advance) on Metal/exact Loopback Audio"]
fn ger_app_real_story_up_returns_one_message_and_down_advances() {
    let gpu = Offscreen::new();
    let scenario = Arc::new(Gerokasu2Scenario::new(bytes().into()).unwrap());
    let (_temporary, _path, mut d) = driver_with_scenario(&gpu, scenario);
    let deadline = Instant::now() + Duration::from_secs(120);
    let ready = |d: &mut system_menu::Driver<'_>| {
        while !host(d).scene.system_menu_allowed() {
            d.step(EnumMap::default());
            std::thread::sleep(Duration::from_millis(2));
            assert!(Instant::now() < deadline, "message not ready");
        }
        d.settle(5);
    };
    while !host(&d).scene.system_menu_allowed() {
        d.step(held(AppAction::Enter));
        d.settle(10);
        assert!(Instant::now() < deadline, "first story message");
    }
    d.settle(5);
    // With fewer than two normal messages Up only plays `error`.
    if host(&d).scene.save_session().timeline.normal_messages < 2 {
        quiet(&mut d);
        assert!(press_cued(&mut d, AppAction::Up));
        assert!(host(&d).scene.reconstruction_phase().is_none());
    }
    // Down advances like A (bit25).
    while host(&d).scene.save_session().timeline.normal_messages < 3 {
        let before = host(&d).scene.save_session().timeline.boundary;
        d.press(AppAction::AnyDown);
        d.settle(3);
        // A first press may only complete the text.
        if host(&d).scene.save_session().timeline.boundary == before {
            d.press(AppAction::AnyDown);
        }
        ready(&mut d);
    }
    let t = host(&d).scene.save_session().timeline;
    assert!(host(&d).scene.save_session().descriptors.message.text.is_some());
    // Up: `decide`, mode-1 reconstruction to the previous message.
    quiet(&mut d);
    assert!(press_cued(&mut d, AppAction::Up));
    assert!(host(&d).scene.reconstruction_phase().is_some(), "Up must start mode 1");
    while host(&d).scene.reconstruction_phase().is_some() {
        d.step(EnumMap::default());
        std::thread::sleep(Duration::from_millis(4));
        assert!(Instant::now() < deadline, "mode 1 did not finish");
    }
    ready(&mut d);
    let back = host(&d).scene.save_session().timeline;
    assert_eq!(back.boundary, t.previous_message, "{t:?} {back:?}");
    // Held skip on this read message suppresses Up (11B00).
    assert!(host(&d).scene.playback().was_read);
    let mut skip = held(AppAction::HoldSkip);
    skip[AppAction::Up] = held(AppAction::Up)[AppAction::Up];
    d.step(skip);
    assert!(host(&d).scene.reconstruction_phase().is_none());
    println!(
        "GEROKASU2_UP_DOWN down=advance boundary={} up={} previous={}",
        t.boundary, back.boundary, t.previous_message
    );
}
