//! Actual ADV/SystemMenu manual save, deletion and asynchronous Load integration.
use super::{Adv, assets::AdvAssets, save_menu::SaveMenuPhase};
use crate::{
    app::AppAction, asset::system::AssetServer, render::PreRenderContext, update::UpdateContext,
};
use shin_core::{
    format::{save::SaveData, scenario::instruction_elements::FromNumber},
    primitives::update::FrameId,
    time::Ticks,
    vm::Scripter,
};
use std::sync::Arc;

/// Reproduce the user's first-slot then third-slot1.03 Load with copied data.
pub(super) fn assert_authored_sequential_load_wipe<'a>(
    server: &'a Arc<AssetServer>,
    assets: &AdvAssets,
    context: &mut PreRenderContext<'a, '_, '_, '_>,
    target: &wgpu::Texture,
) {
    struct Trace {
        id: FrameId,
        last: std::time::Instant,
        point: glam::Vec2,
        held: bool,
        realtime: bool,
        fades: Vec<f32>,
        saved_half: bool,
        setup_stall: bool,
        publication_frame: bool,
    }
    fn step<'a>(
        live: &mut crate::app::ReferenceApp,
        context: &mut PreRenderContext<'a, '_, '_, '_>,
        server: &'a Arc<AssetServer>,
        frame: &mut Trace,
        delta: f32,
        action: Option<AppAction>,
        target: &wgpu::Texture,
    ) {
        if frame.realtime {
            std::thread::sleep(std::time::Duration::from_millis(8));
        }
        let now = std::time::Instant::now();
        let elapsed = if frame.realtime {
            now.duration_since(frame.last).as_secs_f64()
        } else {
            delta as f64 / 60.0
        };
        frame.last = now;
        let was_loading = live.pending_restore.is_some();
        let ticks = if frame.setup_stall && (was_loading || frame.publication_frame) {
            // Actual .app baseline measured43ticks at publication and37ticks
            // after the first cold draw. Neither belongs to the visible wipe.
            Ticks::from_f32(if was_loading { 43.0 } else { 37.0 })
        } else if frame.realtime {
            Ticks::from_seconds(elapsed as f32)
        } else {
            Ticks::from_f32(delta)
        };
        let mut input = enum_map::EnumMap::default();
        if let Some(action) = action {
            input[action] = shin_input::ActionState {
                is_clicked: true,
                is_clicked_or_repeated: true,
                ..Default::default()
            };
        }
        input[AppAction::PointerClick].is_held = frame.held;
        let extent = if frame.realtime {
            (3840, 2160)
        } else {
            (1920, 1080)
        };
        frame.publication_frame = false;
        frame.id.advance();
        live.update_frame(
            &mut UpdateContext {
                frame_started_at: now,
                frame_id: frame.id,
                delta_ticks: ticks,
                asset_server: server,
                pre_render: context,
            },
            input, frame.point, extent, elapsed,
        );
        frame.publication_frame = was_loading && live.pending_restore.is_none();
        if frame.realtime && was_loading && live.pending_restore.is_none() {
            eprintln!(
                "Publication: dt{}ticks, CPU{:.3}ms, fade_elapsed{:?}",
                ticks.as_f32(),
                now.elapsed().as_secs_f64() * 1000.0,
                live.reference_load_transition_elapsed()
            );
        }
        if frame.realtime && !was_loading && live.pending_restore.is_some() {
            eprintln!(
                "Queued restore: dt{}ticks, CPU{:.3}ms",
                ticks.as_f32(),
                now.elapsed().as_secs_f64() * 1000.0
            );
        }
        if frame.realtime {
            if live.has_ripples() && live.reference_load_transition_elapsed().is_some() {
                eprintln!("App ripple composition during wipe");
            }
            if let Some(elapsed) = live.reference_load_transition_elapsed() {
                frame.fades.push(elapsed);
            }
            use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
            let view = target.create_view(&Default::default());
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        kind: TextureTargetKind::Screen,
                        view: &view,
                    },
                    Some(context.depth_stencil),
                    "Real-time4K mouse Load frame",
                );
                live.render(&mut pass);
            }
        }
        let encoder = std::mem::replace(
            context.encoder,
            context.device.create_command_encoder(&Default::default()),
        );
        let mut upload = context.device.create_command_encoder(&Default::default());
        context.dynamic_buffer.finish(&mut upload);
        context.queue.submit([upload.finish(), encoder.finish()]);
        context
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        if frame.realtime && !was_loading && live.pending_restore.is_some() {
            let image = pixels(live, context, target);
            let path = std::path::PathBuf::from(
                std::env::var_os("SHIN_REFERENCE_SEQUENTIAL_LOAD_SAVE").unwrap(),
            );
            std::fs::write(path.with_file_name("mouse-outgoing.rgba"), image).unwrap();
        }
        if frame.realtime
            && !frame.saved_half
            && live
                .reference_load_transition_elapsed()
                .is_some_and(|t| (10.0..20.0).contains(&t))
        {
            let image = pixels(live, context, target);
            let path = std::path::PathBuf::from(
                std::env::var_os("SHIN_REFERENCE_SEQUENTIAL_LOAD_SAVE").unwrap(),
            );
            std::fs::write(path.with_file_name("mouse-half.rgba"), image).unwrap();
            frame.saved_half = true;
        }
    }
    fn click<'a>(
        live: &mut crate::app::ReferenceApp,
        context: &mut PreRenderContext<'a, '_, '_, '_>,
        server: &'a Arc<AssetServer>,
        frame: &mut Trace,
        target: &wgpu::Texture,
        point: glam::Vec2,
    ) {
        frame.point = point;
        frame.held = true;
        step(live, context, server, frame, 0.0, None, target);
        frame.held = false;
        step(live, context, server, frame, 0.0, None, target);
    }
    fn pixels(live: &crate::app::ReferenceApp, context: &mut PreRenderContext, target: &wgpu::Texture) -> Vec<u8> {
        use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
        let view = target.create_view(&Default::default());
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &view,
                },
                Some(context.depth_stencil),
                "Authored sequential Load composed pixels",
            );
            live.render(&mut pass);
        }
        crate::layer::user::readback_test_texture(context, target)
    }
    let path =
        std::path::PathBuf::from(std::env::var_os("SHIN_REFERENCE_SEQUENTIAL_LOAD_SAVE").unwrap());
    assert!(
        path.starts_with("/private/tmp"),
        "Use a copy of working progress"
    );
    let mut data = SaveData::decode(&std::fs::read(&path).unwrap()).unwrap();
    let third = data.manual_save_slots[2].as_ref().unwrap().clone();
    let repeat_target = std::env::var_os("SHIN_REFERENCE_SEQUENTIAL_LOAD_REPEAT_TARGET").is_some();
    let first = if repeat_target {
        third.clone()
    } else {
        data.manual_save_slots[0].as_ref().unwrap().clone()
    };
    for (slot, game) in [(1, &first), (3, &third)] {
        let scene = super::reconstruction::ReconstructedScene::from_game_data_for_load(
            &assets.scenario,
            game,
            &data,
            1_000_000,
        )
        .unwrap();
        match scene.stop {
            super::reconstruction::ReconstructionStop::Command(command) => eprintln!(
                "Authored slot{slot}: scenario{}, boundary{}, command{command:?}",
                game.entry.scenario_id, game.entry.save_position
            ),
            super::reconstruction::ReconstructionStop::Response(response) => eprintln!(
                "Authored slot{slot}: scenario{}, boundary{}, response{response:?}",
                game.entry.scenario_id, game.entry.save_position
            ),
        }
    }
    let realtime = std::env::var_os("SHIN_REFERENCE_SEQUENTIAL_LOAD_REALTIME").is_some();
    data.save_menu_position = if realtime { 0 } else { 2 };
    let audio = Arc::new(shin_audio::AudioManager::new_for_tests());
    let mut live = Adv::new(
        audio,
        server.clone(),
        assets.clone(),
        Scripter::new(&assets.scenario, 0, 42),
    );
    live.install_global_data(super::GlobalData::open(None, Some(data.clone())).unwrap());
    live.load_game_data(&data, &first, server.clone()).unwrap();
    let mut live = crate::app::ReferenceApp::new(live, assets.clone(), server.clone());
    let mut frame = Trace {
        id: FrameId::default(),
        last: std::time::Instant::now(),
        point: glam::vec2(1900.0, 1050.0),
        held: false,
        realtime,
        fades: Vec::new(),
        saved_half: false,
        setup_stall: std::env::var_os("SHIN_REFERENCE_SEQUENTIAL_LOAD_SETUP_STALL").is_some(),
        publication_frame: false,
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(40);
    while live.pending_restore.is_some() {
        step(&mut live, context, server, &mut frame, 0.0, None, target);
        assert!(
            std::time::Instant::now() < deadline,
            "First saved slot never loaded"
        );
        std::thread::yield_now();
    }
    assert!(live.saved_scene_load_error().is_none());
    step(&mut live, context, server, &mut frame, 30.0, None, target);
    if realtime {
        while live.is_loading_saved_scene() {
            step(&mut live, context, server, &mut frame, 0.0, None, target);
            assert!(std::time::Instant::now() < deadline, "Initial Load stalled");
        }
        let before_menu = pixels(&live, context, target);
        std::fs::write(path.with_file_name("mouse-before-menu.rgba"), &before_menu).unwrap();
        frame.fades.clear();
        frame.saved_half = false;
        let labels = super::save_menu_scene::SavedSlotScene::reconstruct(
            &assets.scenario,
            &third.entry,
            &data,
            1_000_000,
        )
        .unwrap()
        .labels;
        eprintln!("Mouse target label: {labels:?}");
        assert!(
            matches!(labels,super::save_menu_text::SaveSlotLabels::Ready{ref title,ref subtitle}
            if title.contains("縁寿の選択") || subtitle.contains("縁寿の選択")),
            "Slot3 is not the user's named target"
        );
        step(
            &mut live,
            context,
            server,
            &mut frame,
            0.0,
            Some(AppAction::SystemMenu),
            target,
        );
        while live.system_menu.as_ref().unwrap().screen.model.phase()
            != super::system_menu::SystemMenuPhase::Active
        {
            step(&mut live, context, server, &mut frame, 0.0, None, target);
            assert!(
                std::time::Instant::now() < deadline,
                "Pause menu never opened"
            );
        }
        click(
            &mut live,
            context,
            server,
            &mut frame,
            target,
            glam::vec2(222.0, 260.0),
        );
        while live
            .system_menu
            .as_mut()
            .unwrap()
            .reference_save_menu()
            .is_none_or(|screen| screen.model().phase() != SaveMenuPhase::Active)
        {
            step(&mut live, context, server, &mut frame, 0.0, None, target);
            assert!(
                std::time::Instant::now() < deadline,
                "Mouse Load button did not open Load"
            );
        }
        let scroll = live
            .system_menu
            .as_mut()
            .unwrap()
            .reference_save_menu()
            .unwrap()
            .model()
            .scroll();
        click(
            &mut live,
            context,
            server,
            &mut frame,
            target,
            glam::vec2(600.0, 188.0 + 2.0 * 186.0 - scroll + 86.0),
        );
        while !live
            .system_menu
            .as_mut()
            .unwrap()
            .reference_save_menu()
            .unwrap()
            .reference_dialog_is_open()
        {
            step(&mut live, context, server, &mut frame, 0.0, None, target);
            assert!(
                std::time::Instant::now() < deadline,
                "Slot3 mouse dialog did not open"
            );
        }
        assert_eq!(
            live.system_menu
                .as_mut()
                .unwrap()
                .reference_save_menu()
                .unwrap()
                .model()
                .selected(),
            2
        );
        let yes = live
            .system_menu
            .as_mut()
            .unwrap()
            .reference_save_menu()
            .unwrap()
            .reference_dialog_button_center(0);
        click(&mut live, context, server, &mut frame, target, yes);
        while live.system_menu.is_some() || live.is_loading_saved_scene() {
            step(&mut live, context, server, &mut frame, 0.0, None, target);
            assert!(std::time::Instant::now() < deadline, "Mouse Load stalled");
        }
        eprintln!("Actually presented4K mouse wipe phases: {:?}", frame.fades);
        assert!(
            frame
                .fades
                .iter()
                .filter(|&&t| (0.0..30.0).contains(&t))
                .count()
                >= 5,
            "Real-time mouse Load did not present a visible wipe"
        );
        assert!(
            frame.fades.iter().any(|&t| (10.0..20.0).contains(&t)),
            "Real-time mouse Load skipped the wipe midpoint"
        );
        let after_load = pixels(&live, context, target);
        std::fs::write(path.with_file_name("mouse-after-load.rgba"), &after_load).unwrap();
        let halfway = std::fs::read(path.with_file_name("mouse-half.rgba")).unwrap();
        let outgoing = std::fs::read(path.with_file_name("mouse-outgoing.rgba")).unwrap();
        for y in 0..1200 {
            for (x, expected) in [(200, &after_load), (3640, &outgoing)] {
                let at = (y * 3840 + x) * 4;
                for c in 0..3 {
                    assert!(
                        halfway[at + c].abs_diff(expected[at + c]) <= 1,
                        "Composed mouse wipe direction/source mismatch at({x},{y}),channel{c}"
                    );
                }
            }
        }
        eprintln!("Composed mouse wipe:2400 background direction/source samples pass");
        if repeat_target {
            let changed = before_menu
                .chunks_exact(4)
                .zip(halfway.chunks_exact(4))
                .take(3840 * 1200)
                .filter(|(a, b)| (0..3).any(|c| a[c].abs_diff(b[c]) > 3))
                .count();
            eprintln!(
                "Same 縁寿の選択 background wipe: {changed} changed pixels above the message window"
            );
            assert!(
                changed > 20_000,
                "Reloading 縁寿の選択 presented a transition clock but no visible background wipe"
            );
        }
        assert_eq!(live.vm_state.save_position, third.entry.save_position);
        assert!(live.saved_scene_load_error().is_none());
        return;
    }
    let first_pixels = pixels(&live, context, target);
    std::fs::write(path.with_file_name("sequential-first.rgba"), &first_pixels).unwrap();
    for (delta, action) in [
        (0.0, Some(AppAction::SystemMenu)),
        (22.0, None),
        (0.0, None),
        (0.0, Some(AppAction::AnyDown)),
        (0.0, Some(AppAction::Enter)),
        (7.0, None),
        (0.0, None),
    ] {
        step(
            &mut live, context, server, &mut frame, delta, action, target,
        );
    }
    while live
        .system_menu
        .as_mut()
        .unwrap()
        .reference_save_menu()
        .unwrap()
        .model()
        .phase()
        == SaveMenuPhase::Loading
    {
        step(&mut live, context, server, &mut frame, 0.0, None, target);
        assert!(
            std::time::Instant::now() < deadline,
            "Load menu never ready"
        );
        std::thread::yield_now();
    }
    step(&mut live, context, server, &mut frame, 10.0, None, target);
    assert_eq!(
        live.system_menu
            .as_mut()
            .unwrap()
            .reference_save_menu()
            .unwrap()
            .model()
            .selected(),
        2
    );
    for (delta, action) in [
        (0.0, Some(AppAction::Enter)),
        (7.0, None),
        (0.0, None),
        (12.0, None),
        (0.0, None),
        (0.0, Some(AppAction::Left)),
        (0.0, Some(AppAction::Enter)),
        (7.0, None),
        (0.0, None),
        (12.0, None),
        (5.0, None),
        (10.0, None),
    ] {
        step(
            &mut live, context, server, &mut frame, delta, action, target,
        );
    }
    assert!(live.system_menu.is_none() && live.pending_restore.is_some());
    while live.pending_restore.is_some() {
        step(&mut live, context, server, &mut frame, 0.0, None, target);
        assert!(
            std::time::Instant::now() < deadline,
            "Third saved slot never loaded"
        );
        std::thread::yield_now();
    }
    assert!(live.saved_scene_load_error().is_none());
    assert_eq!(live.reference_load_transition_masked(), Some(true));
    let zero = pixels(&live, context, target);
    step(&mut live, context, server, &mut frame, 15.0, None, target);
    let half = pixels(&live, context, target);
    step(&mut live, context, server, &mut frame, 15.0, None, target);
    let full = pixels(&live, context, target);
    for (name, image) in [("zero", &zero), ("half", &half), ("full", &full)] {
        std::fs::write(
            path.with_file_name(format!("sequential-{name}.rgba")),
            image,
        )
        .unwrap();
    }
    let changed = |a: &[u8], b: &[u8]| {
        a.chunks_exact(4)
            .zip(b.chunks_exact(4))
            .filter(|(a, b)| (0..3).any(|c| a[c].abs_diff(b[c]) > 2))
            .count()
    };
    eprintln!(
        "Authored composed wipe: changed zero/full{}, half/full{}, zero/half{};choice={}, backroot={}",
        changed(&zero, &full),
        changed(&half, &full),
        changed(&zero, &half),
        live.adv_state.choice_menu.is_some(),
        live.adv_state.back_layer_group.is_some()
    );
    assert!(
        changed(&zero, &full) > 5_000,
        "Actual saved-load wipe has no visible outgoing frame"
    );
    assert!(
        changed(&half, &full) > 5_000 && changed(&zero, &half) > 5_000,
        "Actual saved-load wipe has no visible intermediate phase"
    );
    assert!(!live.is_loading_saved_scene());
    assert_eq!(live.vm_state.save_position, third.entry.save_position);
}

/// Desktop storage adapter exercised through the actual ADV update/input owner.
/// Native successful save timing and Nintendo OS error UI remain separate.
#[cfg(unix)]
pub(super) fn assert_automatic_save_failure<'a>(
    server: &'a Arc<AssetServer>,
    assets: &AdvAssets,
    context: &mut PreRenderContext<'a, '_, '_, '_>,
    target: &wgpu::Texture,
    bad_notice: bool,
) {
    use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o700));
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn step<'a>(
        live: &mut Adv,
        context: &mut PreRenderContext<'a, '_, '_, '_>,
        server: &'a Arc<AssetServer>,
        frame: &mut FrameId,
        delta: f32,
        action: Option<AppAction>,
        pointer: glam::Vec2,
        held: bool,
        extent: (u16, u16),
    ) {
        let mut input = enum_map::EnumMap::default();
        if let Some(action) = action {
            input[action] = shin_input::ActionState {
                is_clicked: true,
                is_clicked_or_repeated: true,
                ..Default::default()
            };
        }
        let physical = pointer * glam::vec2(extent.0 as f32 / 1920.0, extent.1 as f32 / 1080.0);
        live.sample_modal_pointer(physical, extent, held, 1.0 / 60.0);
        // SceneHost passes physical samples to InputArea and canvas positions
        // to dialog hit testing; match both production coordinate contracts.
        live.handle_pointer_state(pointer, false, held, 1.0 / 60.0);
        frame.advance();
        live.update(
            &mut UpdateContext {
                frame_started_at: std::time::Instant::now(),
                frame_id: *frame,
                delta_ticks: Ticks::from_f32(delta),
                asset_server: server,
                pre_render: context,
            },
            input,
        );
        live.advance_play_clock(Ticks::from_f32(delta));
        let encoder = std::mem::replace(
            context.encoder,
            context.device.create_command_encoder(&Default::default()),
        );
        let mut upload = context.device.create_command_encoder(&Default::default());
        context.dynamic_buffer.finish(&mut upload);
        context.queue.submit([upload.finish(), encoder.finish()]);
        context.device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU work must complete before readback or retirement");
    }
    let audio = Arc::new(shin_audio::AudioManager::new_for_tests());
    for (mouse, nested, resume) in [
        (false, false, false),
        (true, true, false),
        (false, false, true),
    ] {
        let extent = if mouse { (3840, 2160) } else { (1280, 720) };
        let directory = std::env::temp_dir().join(format!(
            "shin-auto-save-failure-{}-{mouse}-{resume}",
            std::process::id()
        ));
        fs::create_dir(&directory).unwrap();
        let _cleanup = Cleanup(directory.clone());
        let path = directory.join("progress.dat");
        let mut saved = SaveData::default();
        saved.settings.v3_sysvol = 0;
        saved.save_menu_position = 99;
        saved.manual_save_slots[99] =
            Some(super::VmState::for_scenario(19, 99).snapshot_game_data());
        saved.auto_save_slot = Some(super::VmState::for_scenario(7, 42).snapshot_game_data());
        let original = saved.encode();
        let saved = SaveData::decode(&original).unwrap();
        fs::write(&path, &original).unwrap();
        let mut live = Adv::new(
            audio.clone(),
            server.clone(),
            assets.clone(),
            Scripter::new(&assets.scenario, 0, 42),
        );
        live.configure_save_file(path.clone(), None).unwrap();
        let mut frame = FrameId::default();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while live.vm_state.save_position == 0 {
            step(
                &mut live,
                context,
                server,
                &mut frame,
                0.0,
                None,
                glam::Vec2::ZERO,
                false,
                extent,
            );
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(!live.finished && live.pending_save.is_none());
        // Entering the live message may commit read/gallery progress. Protect
        // the bytes at the actual failing boundary, rather than an older seed.
        let original = fs::read(&path).unwrap();
        // The message checkpoint is live; isolate the automatic boundary from
        // explicit SaveGame and prevent writes via a real OS permission error.
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o555)).unwrap();
        if nested {
            live.adv_state.acquire_audio_pause();
            live.adv_state.acquire_audio_pause();
        }
        let pauses = live.adv_state.audio_pause_count;
        live.vm_state.persist.set(28, 123);
        live.vm_state.capture_autosave();
        let pending = format!("{:?}", live.vm_state.auto_save_slot);
        live.sample_modal_pointer(glam::Vec2::ZERO, extent, true, 1.0 / 60.0);
        frame.advance();
        if resume {
            live.vm_state.resume_position = Some(1);
            live.queue_resume_scene(&mut UpdateContext {
                frame_started_at: std::time::Instant::now(),
                frame_id: frame,
                delta_ticks: Ticks::ZERO,
                asset_server: server,
                pre_render: context,
            });
        } else {
            live.update(
                &mut UpdateContext {
                    frame_started_at: std::time::Instant::now(),
                    frame_id: frame,
                    delta_ticks: Ticks::ZERO,
                    asset_server: server,
                    pre_render: context,
                },
                enum_map::EnumMap::default(),
            );
        }
        assert!(
            live.progress_error.is_some(),
            "Automatic failure must be visible"
        );
        if bad_notice {
            assert!(
                live.progress_error
                    .as_ref()
                    .unwrap()
                    .reference_uses_plain_frame()
            );
        }
        assert!(
            live.pending_save.is_none(),
            "An automatic error is not an explicit save worker"
        );
        assert_eq!(live.adv_state.audio_pause_count, pauses + 1);
        assert_eq!(fs::read(&path).unwrap(), original);
        let position = live.vm_state.save_position;
        for _ in 0..40 {
            step(
                &mut live,
                context,
                server,
                &mut frame,
                1.0,
                None,
                glam::Vec2::ZERO,
                true,
                extent,
            );
            assert_eq!(live.vm_state.save_position, position);
            assert_eq!(live.vm_state.persist.get(28), 123);
            assert_eq!(format!("{:?}", live.vm_state.auto_save_slot), pending);
            assert!(live.pending_title_setup().is_none());
            if resume {
                assert!(
                    live.pending_restore.is_some(),
                    "Restore publication waits for acknowledgement"
                );
            }
        }
        let button = live.progress_error.as_ref().unwrap().reference_button();
        {
            use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
            let view = target.create_view(&Default::default());
            let mut pass = context.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &view,
                },
                Some(context.depth_stencil),
                "Automatic save failure acknowledgement",
            );
            live.render(&mut pass);
        }
        let pixels = crate::layer::user::readback_test_texture(context, target);
        let glyphs = (400..630)
            .flat_map(|y| (400..1520).map(move |x| (y * 1920 + x) * 4))
            .filter(|&at| pixels[at..at + 3].iter().all(|&v| v > 245))
            .count();
        assert!(glyphs > 100, "Save error prompt must render: {glyphs}");
        if bad_notice {
            super::storage_notice_failure_reference::assert_notice_pixels(
                &pixels,
                &format!("automatic-progress-{mouse}-{resume}"),
            );
        }
        // A release inherited from the initiating mouse press cannot accept.
        step(
            &mut live,
            context,
            server,
            &mut frame,
            1.0,
            Some(AppAction::Enter),
            button,
            false,
            extent,
        );
        step(
            &mut live,
            context,
            server,
            &mut frame,
            30.0,
            Some(AppAction::Cancel),
            button,
            false,
            extent,
        );
        assert!(live.progress_error.is_some());
        if mouse {
            step(
                &mut live, context, server, &mut frame, 0.0, None, button, true, extent,
            );
            assert!(live.progress_error.is_some());
            step(
                &mut live, context, server, &mut frame, 0.0, None, button, false, extent,
            );
        } else {
            step(
                &mut live,
                context,
                server,
                &mut frame,
                0.0,
                Some(AppAction::Enter),
                button,
                false,
                extent,
            );
        }
        for _ in 0..100 {
            if live.progress_error.is_none() {
                break;
            }
            step(
                &mut live, context, server, &mut frame, 1.0, None, button, false, extent,
            );
            assert_eq!(live.vm_state.save_position, position);
        }
        assert!(
            live.progress_error.is_none(),
            "Final acknowledgement must release the dialog"
        );
        assert_eq!(live.adv_state.audio_pause_count, pauses);
        assert_eq!(fs::read(&path).unwrap(), original);
        if !resume {
            step(
                &mut live, context, server, &mut frame, 0.0, None, button, false, extent,
            );
            assert!(
                live.progress_error.is_none(),
                "Do not repeatedly report the same failed revision"
            );
        }
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        if resume {
            while live.is_loading_saved_scene() {
                step(
                    &mut live, context, server, &mut frame, 1.0, None, button, false, extent,
                );
                assert!(std::time::Instant::now() < deadline);
            }
            assert!(live.restore_error.is_none());
        }
        live.vm_state.persist.set(28, 456);
        live.vm_state.capture_autosave();
        step(
            &mut live, context, server, &mut frame, 0.0, None, button, false, extent,
        );
        assert!(live.progress_error.is_none());
        let durable = SaveData::decode(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(durable.persist_data.get(28), 456);
        assert_eq!(durable.save_menu_position, 99);
        assert_eq!(
            format!("{:?}", durable.manual_save_slots[99]),
            format!("{:?}", saved.manual_save_slots[99])
        );
        assert_eq!(
            durable.auto_save_slot.as_ref().unwrap().entry.save_position,
            live.vm_state.save_position
        );
        if nested {
            live.adv_state.release_audio_pause();
            live.adv_state.release_audio_pause();
        }
        drop(live);
        eprintln!(
            "Automatic save failure: mouse={mouse}, nested={nested}, resume={resume}, rendered glyphs={glyphs}; latest globals/auto and existing slot preserved"
        );
    }
}

pub(super) fn assert_live_save_menu<'a>(
    server: &'a Arc<AssetServer>,
    assets: &AdvAssets,
    context: &mut PreRenderContext<'a, '_, '_, '_>,
    target: &wgpu::Texture,
) {
    fn step<'a>(
        live: &mut Adv,
        context: &mut PreRenderContext<'a, '_, '_, '_>,
        server: &'a Arc<AssetServer>,
        frame: &mut FrameId,
        delta: f32,
        action: Option<AppAction>,
    ) {
        let mut input = enum_map::EnumMap::default();
        if let Some(action) = action {
            input[action] = shin_input::ActionState {
                is_clicked: true,
                is_clicked_or_repeated: true,
                ..Default::default()
            };
        }
        let pointer = glam::vec2(1900.0, 1050.0);
        live.sample_modal_pointer(pointer, (1920, 1080), false, delta as f64 / 60.0);
        live.handle_pointer_state(pointer, false, false, delta as f64 / 60.0);
        frame.advance();
        live.update(
            &mut UpdateContext {
                frame_started_at: std::time::Instant::now(),
                frame_id: *frame,
                delta_ticks: Ticks::from_f32(delta),
                asset_server: server,
                pre_render: context,
            },
            input,
        );
        live.advance_play_clock(Ticks::from_f32(delta));
        // Submit each prepared frame as App does. Pending workers must not
        // exhaust Metal's finite command-buffer pool while their UI is polled.
        let encoder = std::mem::replace(
            context.encoder,
            context.device.create_command_encoder(&Default::default()),
        );
        let mut upload = context.device.create_command_encoder(&Default::default());
        context.dynamic_buffer.finish(&mut upload);
        context.queue.submit([upload.finish(), encoder.finish()]);
        context.device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU work must complete before readback or retirement");
    }
    fn child(live: &mut Adv) -> &mut super::save_menu_screen::SaveMenuScreen {
        live.system_menu
            .as_mut()
            .unwrap()
            .reference_save_menu()
            .expect("Actual SystemMenu Save/Load factory missing")
    }
    fn open_child<'a>(
        live: &mut Adv,
        context: &mut PreRenderContext<'a, '_, '_, '_>,
        server: &'a Arc<AssetServer>,
        frame: &mut FrameId,
        load: bool,
    ) {
        if live.system_menu.is_none() {
            step(
                live,
                context,
                server,
                frame,
                0.0,
                Some(AppAction::SystemMenu),
            );
        }
        step(live, context, server, frame, 22.0, None);
        step(live, context, server, frame, 0.0, None);
        if load {
            step(live, context, server, frame, 0.0, Some(AppAction::AnyDown));
        }
        step(live, context, server, frame, 0.0, Some(AppAction::Enter));
        step(live, context, server, frame, 7.0, None);
        step(live, context, server, frame, 0.0, None);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while child(live).model().phase() == SaveMenuPhase::Loading {
            step(live, context, server, frame, 0.0, None);
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert_eq!(child(live).model().progress(), 0.0);
        step(live, context, server, frame, 10.0, None);
    }
    fn confirm<'a>(
        live: &mut Adv,
        context: &mut PreRenderContext<'a, '_, '_, '_>,
        server: &'a Arc<AssetServer>,
        frame: &mut FrameId,
        action: AppAction,
    ) {
        step(live, context, server, frame, 0.0, Some(action));
        step(live, context, server, frame, 7.0, None);
        step(live, context, server, frame, 0.0, None);
        step(live, context, server, frame, 12.0, None);
        step(live, context, server, frame, 0.0, None);
        assert!(child(live).reference_dialog_is_open());
        step(live, context, server, frame, 0.0, Some(AppAction::Left));
        step(live, context, server, frame, 0.0, Some(AppAction::Enter));
        step(live, context, server, frame, 7.0, None);
        step(live, context, server, frame, 0.0, None);
    }
    fn finish<'a>(
        live: &mut Adv,
        context: &mut PreRenderContext<'a, '_, '_, '_>,
        server: &'a Arc<AssetServer>,
        frame: &mut FrameId,
    ) {
        step(live, context, server, frame, 12.0, None);
        step(live, context, server, frame, 5.0, None);
        let position = live.vm_state.save_position;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while live.pending_save.is_some() {
            let action = live
                .pending_save
                .as_ref()
                .filter(|save| save.task.reference_failure_is_open())
                .map(|_| AppAction::Enter);
            step(live, context, server, frame, 1.0, action);
            assert_eq!(
                live.vm_state.save_position, position,
                "SaveGame must retain the paused VM"
            );
            assert!(
                std::time::Instant::now() < deadline,
                "Pending SaveGame did not finish"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
    let audio = Arc::new(shin_audio::AudioManager::new_for_tests());
    for (disk, launch) in [(true, false), (false, false), (true, true), (false, true)] {
        if std::env::var_os("SHIN_REFERENCE_ADV_LOAD_MENU_ONLY").is_some() && launch {
            continue;
        }
        let mut live = Adv::new(
            audio.clone(),
            server.clone(),
            assets.clone(),
            Scripter::new(&assets.scenario, 0, 42),
        );
        assert!(live.take_title_global_data().is_err());
        let mut frame = FrameId::default();
        let folder =
            std::env::temp_dir().join(format!("shin-live-save-menu-{}", std::process::id()));
        let backup = folder.with_extension("backup");
        let path = folder.join("save.dat");
        if disk {
            assert!(!folder.exists() && !backup.exists());
            std::fs::create_dir(&folder).unwrap();
            live.configure_save_file(path.clone(), None).unwrap();
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while live.vm_state.save_position == 0 {
            step(&mut live, context, server, &mut frame, 1.0, None);
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        let retained = live.vm_state.snapshot_entry();
        assert_eq!(retained.save_position, 1);
        open_child(&mut live, context, server, &mut frame, false);
        assert!(live.pending_system_menu_request().is_none());
        assert_eq!(
            (
                live.adv_state.ui_hide_count,
                live.adv_state.audio_pause_count
            ),
            (1, 1)
        );
        live.vm_state.save_position += 10; // Force divergence after the menu retained its entry.
        confirm(&mut live, context, server, &mut frame, AppAction::Enter);
        assert_eq!(child(&mut live).model().phase(), SaveMenuPhase::Writing);
        let saved = live.save_menu_data().manual_save_slots[0]
            .as_ref()
            .unwrap()
            .clone();
        assert_eq!(
            (
                saved.entry.scenario_id,
                saved.entry.random_seed,
                saved.entry.save_position
            ),
            (retained.scenario_id, 42, 1)
        );
        assert!(saved.date_time.and_utc().timestamp() > 1_000_000_000);
        assert!(live.pending_save.is_some());
        if disk {
            let data = SaveData::decode(&std::fs::read(&path).unwrap()).unwrap();
            assert!(
                data.manual_save_slots[0].is_none(),
                "Worker must not write before the updating dialog opens"
            );
        }
        step(&mut live, context, server, &mut frame, 12.0, None);
        step(&mut live, context, server, &mut frame, 0.0, None);
        assert!(live.pending_save.is_some());
        {
            use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
            let view = target.create_view(&Default::default());
            let mut pass = context.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &view,
                },
                Some(context.depth_stencil),
                "Live updating SaveGame dialog",
            );
            live.render(&mut pass);
        }
        let pixels = crate::layer::user::readback_test_texture(context, target);
        let glyphs = (460..590)
            .flat_map(|y| (400..1520).map(move |x| (y * 1920 + x) * 4))
            .filter(|&at| pixels[at..at + 3].iter().all(|&v| v > 245))
            .count();
        assert!(
            glyphs > 100,
            "Updating prompt must actually render ({glyphs})"
        );
        std::fs::write(
            format!("/private/tmp/shin-live-save-updating-{disk}-{launch}.rgba"),
            pixels,
        )
        .unwrap();
        // Cancel/Enter/held skipping cannot close the no-answer updating dialog.
        for action in [
            AppAction::Cancel,
            AppAction::Enter,
            AppAction::ToggleSkip,
            AppAction::SystemMenu,
        ] {
            step(&mut live, context, server, &mut frame, 0.0, Some(action));
            assert!(live.pending_save.is_some());
            assert_eq!(child(&mut live).model().phase(), SaveMenuPhase::Writing);
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !child(&mut live).reference_preview_ready(0) {
            // Zero delta retains the closing updating dialog while the new
            // slot thumbnail is prepared by its independently polled worker.
            step(&mut live, context, server, &mut frame, 0.0, None);
            assert!(live.pending_save.is_some());
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        finish(&mut live, context, server, &mut frame);
        assert_eq!(child(&mut live).model().phase(), SaveMenuPhase::Active);
        assert!(
            child(&mut live).reference_preview_ready(0),
            "Successful SaveGame must retain the thumbnail already prepared during the write"
        );
        if disk {
            let data = SaveData::decode(&std::fs::read(&path).unwrap()).unwrap();
            assert_eq!(
                data.manual_save_slots[0]
                    .as_ref()
                    .unwrap()
                    .entry
                    .save_position,
                1
            );
            assert_eq!(data.save_menu_position, 0);
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !child(&mut live).reference_preview_ready(0) {
            step(&mut live, context, server, &mut frame, 0.0, None);
            assert!(
                std::time::Instant::now() < deadline,
                "Actual retained save boundary thumbnail timed out"
            );
            std::thread::yield_now();
        }
        // Full actual ADV composition includes the real child and scene thumbnail.
        {
            use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
            let view = target.create_view(&Default::default());
            let mut pass = context.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &view,
                },
                Some(context.depth_stencil),
                "Live ADV SaveMenu child",
            );
            live.render(&mut pass);
        }
        std::fs::write(
            format!(
                "/private/tmp/shin-live-save-menu-{}.rgba",
                if disk { "disk" } else { "volatile" }
            ),
            crate::layer::user::readback_test_texture(context, target),
        )
        .unwrap();
        if disk {
            std::fs::rename(&folder, &backup).unwrap();
            std::fs::write(&folder, b"blocked save parent").unwrap();
            confirm(
                &mut live,
                context,
                server,
                &mut frame,
                AppAction::DeleteSave,
            );
            finish(&mut live, context, server, &mut frame);
            assert!(
                live.save_menu_data().manual_save_slots[0].is_some(),
                "failed native SaveGame must retain slot"
            );
            assert_eq!(
                SaveData::decode(&std::fs::read(backup.join("save.dat")).unwrap())
                    .unwrap()
                    .manual_save_slots[0]
                    .as_ref()
                    .unwrap()
                    .entry
                    .save_position,
                1
            );
            std::fs::remove_file(&folder).unwrap();
            std::fs::rename(&backup, &folder).unwrap();
        }
        confirm(
            &mut live,
            context,
            server,
            &mut frame,
            AppAction::DeleteSave,
        );
        finish(&mut live, context, server, &mut frame);
        assert!(live.save_menu_data().manual_save_slots[0].is_none());
        if disk {
            assert!(
                SaveData::decode(&std::fs::read(&path).unwrap())
                    .unwrap()
                    .manual_save_slots[0]
                    .is_none()
            );
        }
        confirm(&mut live, context, server, &mut frame, AppAction::Enter);
        finish(&mut live, context, server, &mut frame);
        step(
            &mut live,
            context,
            server,
            &mut frame,
            0.0,
            Some(AppAction::Cancel),
        );
        step(&mut live, context, server, &mut frame, 7.0, None);
        step(&mut live, context, server, &mut frame, 0.0, None);
        assert!(
            live.system_menu
                .as_mut()
                .unwrap()
                .reference_save_menu()
                .is_some(),
            "close0 retains child"
        );
        step(&mut live, context, server, &mut frame, 10.0, None);
        assert!(
            live.system_menu
                .as_mut()
                .unwrap()
                .reference_save_menu()
                .is_none(),
            "close1 returns to SystemMenu"
        );
        open_child(&mut live, context, server, &mut frame, true);
        confirm(&mut live, context, server, &mut frame, AppAction::Enter);
        assert!(
            !live.is_loading_saved_scene(),
            "Load cannot occur at dialog intermediate callback"
        );
        finish(&mut live, context, server, &mut frame);
        let before_load = live.play_ticks();
        step(&mut live, context, server, &mut frame, 10.0, None);
        assert!(live.system_menu.is_none() && live.pending_restore.is_some());
        assert_eq!(
            (
                live.adv_state.ui_hide_count,
                live.adv_state.audio_pause_count
            ),
            (0, 0)
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let mut saw_masked_transition = false;
        while live.is_loading_saved_scene() {
            step(&mut live, context, server, &mut frame, 1.0, None);
            saw_masked_transition |= live.reference_load_transition_masked() == Some(true);
            assert!(
                std::time::Instant::now() < deadline,
                "Actual manual Load restoration timed out: {:?}",
                live.restore_error
            );
            std::thread::yield_now();
        }
        assert!(live.restore_error.is_none());
        assert!(saw_masked_transition, "Pause-menu Load must install the prepared default mask");
        assert_eq!(live.vm_state.save_position, 1);
        assert_eq!(live.vm_state.snapshot_entry().random_seed, 42);
        assert!(live.play_ticks() >= before_load);
        assert!(
            live.save_menu_data().manual_save_slots[0].is_some(),
            "scene replacement must retain global manual slots"
        );
        if std::env::var_os("SHIN_REFERENCE_ADV_LOAD_MENU_ONLY").is_some() {
            drop(live);
            if disk { std::fs::remove_dir_all(&folder).unwrap(); }
            assert!(!backup.exists());
            eprintln!("Actual pause-menu Save/Delete/Load: disk={disk}, masked30tick handoff, input/audio scopes, retained slots and resource invalidation pass");
            continue;
        }
        // Actual ADV Config close uses the same pending owner as Title Config.
        step(
            &mut live,
            context,
            server,
            &mut frame,
            0.0,
            Some(AppAction::SystemMenu),
        );
        step(&mut live, context, server, &mut frame, 22.0, None);
        step(&mut live, context, server, &mut frame, 0.0, None);
        for _ in 0..2 {
            step(
                &mut live,
                context,
                server,
                &mut frame,
                0.0,
                Some(AppAction::AnyDown),
            );
        }
        step(
            &mut live,
            context,
            server,
            &mut frame,
            7.0,
            Some(AppAction::Enter),
        );
        step(&mut live, context, server, &mut frame, 0.0, None);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while live
            .system_menu
            .as_ref()
            .unwrap()
            .reference_config_if_any()
            .and_then(|screen| screen.model())
            .is_none()
        {
            step(&mut live, context, server, &mut frame, 0.0, None);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        step(&mut live, context, server, &mut frame, 10.0, None);
        let previous = live.vm_state.settings.v3_sysvol;
        step(
            &mut live,
            context,
            server,
            &mut frame,
            0.0,
            Some(AppAction::Right),
        );
        assert_eq!(live.vm_state.settings.v3_sysvol, previous + 5);
        step(
            &mut live,
            context,
            server,
            &mut frame,
            7.0,
            Some(AppAction::Cancel),
        );
        step(&mut live, context, server, &mut frame, 0.0, None);
        assert!(live.pending_save.is_some());
        assert_eq!(
            live.system_menu
                .as_ref()
                .unwrap()
                .reference_config()
                .model()
                .unwrap()
                .phase(),
            super::config::ConfigPhase::WaitingSave
        );
        finish(&mut live, context, server, &mut frame);
        assert_eq!(live.vm_state.settings.v3_sysvol, previous + 5);
        if disk {
            assert_eq!(
                SaveData::decode(&std::fs::read(&path).unwrap())
                    .unwrap()
                    .settings
                    .v3_sysvol,
                previous + 5
            );
        }
        step(&mut live, context, server, &mut frame, 10.0, None);
        step(&mut live, context, server, &mut frame, 22.0, None);
        step(&mut live, context, server, &mut frame, 0.0, None);
        assert!(!live.system_menu.as_ref().unwrap().has_child());
        step(
            &mut live,
            context,
            server,
            &mut frame,
            22.0,
            Some(AppAction::Cancel),
        );
        step(&mut live, context, server, &mut frame, 0.0, None);
        assert!(live.system_menu.is_none());
        step(&mut live, context, server, &mut frame, 10.0, None);
        // Actual title-return factory owns a generic dialog. No and Cancel
        // reopen at intermediate0 while the dialog still finishes its exit.
        fn open_title<'a>(
            live: &mut Adv,
            context: &mut PreRenderContext<'a, '_, '_, '_>,
            server: &'a Arc<AssetServer>,
            frame: &mut FrameId,
        ) {
            if live.system_menu.is_none() {
                step(
                    live,
                    context,
                    server,
                    frame,
                    0.0,
                    Some(AppAction::SystemMenu),
                );
                step(live, context, server, frame, 22.0, None);
                step(live, context, server, frame, 0.0, None);
                step(live, context, server, frame, 0.0, Some(AppAction::Up));
            }
            step(live, context, server, frame, 0.0, Some(AppAction::Enter));
            step(live, context, server, frame, 7.0, None);
            step(live, context, server, frame, 0.0, None);
            assert!(
                live.system_menu
                    .as_ref()
                    .unwrap()
                    .reference_title_return()
                    .is_some()
            );
            assert!(live.pending_system_menu_request().is_none());
            step(live, context, server, frame, 12.0, None);
            step(live, context, server, frame, 0.0, None);
            assert!(
                live.system_menu
                    .as_ref()
                    .unwrap()
                    .reference_title_return()
                    .unwrap()
                    .reference_is_open()
            );
        }
        for action in [AppAction::Enter, AppAction::Cancel] {
            open_title(&mut live, context, server, &mut frame);
            if disk && matches!(action, AppAction::Enter) {
                use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
                let view = target.create_view(&Default::default());
                {
                    let mut pass = context.begin_pass(
                        TextureTarget {
                            kind: TextureTargetKind::Screen,
                            view: &view,
                        },
                        Some(context.depth_stencil),
                        "Live title-return No dialog",
                    );
                    live.render(&mut pass);
                }
                std::fs::write(
                    "/private/tmp/shin-live-title-return-dialog.rgba",
                    crate::layer::user::readback_test_texture(context, target),
                )
                .unwrap();
            }
            step(&mut live, context, server, &mut frame, 0.0, Some(action));
            step(&mut live, context, server, &mut frame, 7.0, None);
            step(&mut live, context, server, &mut frame, 0.0, None);
            assert_eq!(
                live.system_menu.as_ref().unwrap().screen.model.phase(),
                super::system_menu::SystemMenuPhase::Opening
            );
            assert!(
                live.system_menu
                    .as_ref()
                    .unwrap()
                    .reference_title_return()
                    .is_some()
            );
            assert!(live.pending_system_menu_request().is_none());
            step(&mut live, context, server, &mut frame, 12.0, None);
            step(&mut live, context, server, &mut frame, 5.0, None);
            assert!(
                live.system_menu
                    .as_ref()
                    .unwrap()
                    .reference_title_return()
                    .is_none()
            );
            step(&mut live, context, server, &mut frame, 22.0, None);
            step(&mut live, context, server, &mut frame, 0.0, None);
            assert_eq!(
                live.system_menu.as_ref().unwrap().screen.model.phase(),
                super::system_menu::SystemMenuPhase::Active
            );
        }
        open_title(&mut live, context, server, &mut frame);
        step(
            &mut live,
            context,
            server,
            &mut frame,
            0.0,
            Some(AppAction::Left),
        );
        step(
            &mut live,
            context,
            server,
            &mut frame,
            0.0,
            Some(AppAction::Enter),
        );
        step(&mut live, context, server, &mut frame, 7.0, None);
        step(&mut live, context, server, &mut frame, 0.0, None);
        assert!(
            live.pending_system_menu_request().is_none(),
            "intermediate Yes must not save/dispatch title"
        );
        let revision = live.vm_state.save_revision;
        live.vm_state.settings.v3_sysvol = 27;
        step(&mut live, context, server, &mut frame, 12.0, None);
        assert!(live.pending_system_menu_request().is_none());
        step(&mut live, context, server, &mut frame, 5.0, None);
        assert!(live.pending_save.is_some());
        assert!(
            live.pending_title_setup().is_none(),
            "Title cannot replace an unfinished SaveGame"
        );
        finish(&mut live, context, server, &mut frame);
        assert_eq!(
            live.pending_system_menu_request(),
            Some(&super::system_menu_screen::SystemMenuRequest::ReturnToTitle)
        );
        let title_setup = live
            .pending_title_setup()
            .expect("Actual title construction snapshot missing");
        assert_eq!(
            title_setup.progress,
            live.vm_state.persist.get(0).clamp(0, 255) as u8
        );
        assert_eq!(
            title_setup.items[title_setup.selected],
            crate::title::TitleItem::Continue
        );
        assert!(
            live.vm_state.auto_save_slot.is_some(),
            "MSGSET captures an autosave before the explicit chapter AUTOSAVE"
        );
        assert!(!title_setup.returning_from_child);
        assert_eq!(live.vm_state.save_revision, revision);
        assert_eq!(
            (
                live.adv_state.ui_hide_count,
                live.adv_state.audio_pause_count
            ),
            (1, 1)
        );
        assert!(
            !live.is_finished(),
            "a title scene request must not fabricate script EXIT/restart"
        );
        if disk {
            let saved = SaveData::decode(&std::fs::read(&path).unwrap()).unwrap();
            assert_eq!(
                saved.settings.v3_sysvol, 27,
                "title SaveGame must write even with unchanged progress revision"
            );
            assert!(saved.manual_save_slots[0].is_some());
        }
        let expected = live.save_menu_data();
        let before = live.play_ticks();
        assert!(live.take_title_audio().is_err());
        live.adv_state.play_system_sound(
            server,
            "move",
            shin_core::vm::command::types::Volume::default(),
        );
        live.adv_state
            .audio
            .system_player
            .sounds
            .last()
            .unwrap()
            .set_paused(true);
        let music_track = live.adv_state.audio.bgm_player.reference_source().2;
        let effect_tracks = (0..crate::audio::SE_SLOT_COUNT)
            .map(|slot| live.adv_state.audio.se_player.reference_source(slot).2)
            .collect::<Vec<_>>();
        let cue_track = live.adv_state.audio.system_player.track.id();
        let bank = live.adv_state.audio.system_player.bank.clone().unwrap();
        assert!(live.adv_state.audio.se_player.reference_is_paused());
        let mut host = crate::app::SceneHost::new(live, assets.clone(), server.clone());
        fn host_step<'a>(
            host: &mut crate::app::SceneHost,
            context: &mut PreRenderContext<'a, '_, '_, '_>,
            server: &'a Arc<AssetServer>,
            frame: &mut FrameId,
            delta: f32,
            action: Option<AppAction>,
        ) {
            let mut input = enum_map::EnumMap::default();
            if let Some(action) = action {
                input[action] = shin_input::ActionState {
                    is_clicked: true,
                    is_clicked_or_repeated: true,
                    ..Default::default()
                };
            }
            frame.advance();
            host.update(
                &mut UpdateContext {
                    frame_started_at: std::time::Instant::now(),
                    frame_id: *frame,
                    delta_ticks: Ticks::from_f32(delta),
                    asset_server: server,
                    pre_render: context,
                },
                input,
                glam::vec2(950.0, 525.0),
                glam::vec2(1900.0, 1050.0),
                (960, 540),
                0.0,
                delta as f64 / 60.0,
            )
            .unwrap();
        }
        host_step(&mut host, context, server, &mut frame, 0.0, None);
        assert!(host.reference_adv().is_some());
        assert!(host.reference_title().is_none());
        host_step(&mut host, context, server, &mut frame, 0.0, None);
        assert!(host.reference_adv().is_none());
        assert!(host.reference_title().is_some());
        assert_eq!(host.reference_save().encode(), expected.encode());
        assert_eq!(host.reference_play_ticks(), before);
        assert!(!host.reference_audio().se_player.reference_is_paused());
        assert_eq!(
            host.reference_audio().bgm_player.reference_source().2,
            music_track
        );
        assert_eq!(host.reference_audio().system_player.track.id(), cue_track);
        assert!(Arc::ptr_eq(
            host.reference_audio().system_player.bank.as_ref().unwrap(),
            &bank
        ));
        assert!(!host.reference_audio().system_player.sounds.is_empty());
        assert_eq!(
            (0..crate::audio::SE_SLOT_COUNT)
                .map(|slot| host.reference_audio().se_player.reference_source(slot).2)
                .collect::<Vec<_>>(),
            effect_tracks
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !host.reference_title().unwrap().is_loaded() {
            host_step(&mut host, context, server, &mut frame, 0.0, None);
            assert!(
                std::time::Instant::now() < deadline,
                "Host title loading timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        for delta in [120.0, 0.0, 0.0, 74.0, 0.0] {
            host_step(&mut host, context, server, &mut frame, delta, None);
        }
        assert_eq!(
            host.reference_title().unwrap().motion.phase(),
            crate::title::TitlePhase::Active
        );
        assert_eq!(
            host.reference_title().unwrap().motion.frame().caption,
            Some(crate::title::TitleItem::Continue)
        );
        assert_eq!(host.reference_play_ticks(), before + 194.0);
        {
            let view = target.create_view(&Default::default());
            let mut pass = context.begin_pass(
                shin_render::shaders::types::texture::TextureTarget {
                    kind: shin_render::shaders::types::texture::TextureTargetKind::Screen,
                    view: &view,
                },
                Some(context.depth_stencil),
                "Actual host title",
            );
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::BLACK),
                Some(0),
                Some(1.0),
            );
            host.render(&mut pass);
        }
        let title_pixels = crate::layer::user::readback_test_texture(context, target);
        assert!(
            title_pixels
                .chunks_exact(4)
                .filter(|pixel| pixel[..3] != [0, 0, 0])
                .count()
                > 1_000_000
        );
        std::fs::write(
            std::env::temp_dir().join(format!("shin-host-title-{disk}.rgba")),
            title_pixels,
        )
        .unwrap();
        // Exercise real Title Start -> ScenarioSelect -> Title(0), retaining
        // the same global objects before the existing Continue restoration.
        host_step(
            &mut host,
            context,
            server,
            &mut frame,
            0.0,
            Some(AppAction::Up),
        );
        assert_eq!(
            host.reference_title().unwrap().motion.frame().caption,
            Some(crate::title::TitleItem::Start)
        );
        host_step(
            &mut host,
            context,
            server,
            &mut frame,
            37.0,
            Some(AppAction::Enter),
        );
        assert!(host.reference_picker().is_none());
        host_step(&mut host, context, server, &mut frame, 0.0, None);
        assert!(
            host.reference_title().is_some(),
            "Start queues replacement for next frame"
        );
        let render_host = |host: &crate::app::SceneHost, context: &mut PreRenderContext| {
            let view = target.create_view(&Default::default());
            let mut pass = context.begin_pass(
                shin_render::shaders::types::texture::TextureTarget {
                    kind: shin_render::shaders::types::texture::TextureTargetKind::Screen,
                    view: &view,
                },
                Some(context.depth_stencil),
                "Start scene host frame",
            );
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::BLACK),
                Some(0),
                Some(1.0),
            );
            host.render(&mut pass);
        };
        render_host(&host, context);
        let before_picker = crate::layer::user::readback_test_texture(context, target);
        host_step(&mut host, context, server, &mut frame, 0.0, None);
        assert!(host.reference_title().is_none());
        assert!(host.reference_adv().is_none());
        assert!(host.reference_picker().is_some());
        render_host(&host, context);
        let retained = crate::layer::user::readback_test_texture(context, target);
        let worst = before_picker
            .iter()
            .zip(&retained)
            .map(|(&a, &b)| a.abs_diff(b))
            .max()
            .unwrap();
        assert!(
            worst <= 1,
            "Prior Title frame changed during picker construction: maxbyteerror{worst}"
        );
        assert_eq!(
            host.reference_audio().bgm_player.reference_source().2,
            music_track
        );
        assert_eq!(host.reference_audio().system_player.track.id(), cue_track);
        assert!(Arc::ptr_eq(
            host.reference_audio().system_player.bank.as_ref().unwrap(),
            &bank
        ));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !host.reference_picker().unwrap().is_loaded() {
            host_step(&mut host, context, server, &mut frame, 0.0, None);
            assert!(
                std::time::Instant::now() < deadline,
                "Host picker loading timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        host_step(
            &mut host,
            context,
            server,
            &mut frame,
            60.0,
            Some(AppAction::Enter),
        );
        assert!(!host.reference_picker().unwrap().accepts_input());
        host_step(&mut host, context, server, &mut frame, 0.0, None);
        assert!(host.reference_picker().unwrap().accepts_input());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while host.reference_audio().bgm_player.reference_source().0
            != Some(shin_core::format::scenario::info::BgmId::from_number(5))
        {
            host_step(&mut host, context, server, &mut frame, 0.0, None);
            assert!(
                std::time::Instant::now() < deadline,
                "Picker BGM5 request timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        render_host(&host, context);
        let pixels = crate::layer::user::readback_test_texture(context, target);
        std::fs::write(
            std::env::temp_dir().join(format!("shin-host-picker-{disk}.rgba")),
            pixels,
        )
        .unwrap();
        assert_eq!(host.reference_play_ticks(), before + 291.0);
        let mut picker_expected = expected.clone();
        picker_expected.play_seconds = host.reference_save().play_seconds;
        assert_eq!(host.reference_save().encode(), picker_expected.encode());
        if launch {
            host_step(
                &mut host,
                context,
                server,
                &mut frame,
                37.0,
                Some(AppAction::Enter),
            );
            host_step(&mut host, context, server, &mut frame, 0.0, None);
            assert_eq!(host.reference_episode_request(), Some(0));
            assert!(host.reference_picker().is_some());
            render_host(&host, context);
            let prior = crate::layer::user::readback_test_texture(context, target);
            host_step(&mut host, context, server, &mut frame, 0.0, None);
            assert!(host.reference_transition());
            assert!(host.reference_picker().is_none());
            assert!(host.reference_adv().is_none());
            render_host(&host, context);
            let capture = crate::layer::user::readback_test_texture(context, target);
            assert!(
                prior
                    .iter()
                    .zip(&capture)
                    .all(|(&a, &b)| a.abs_diff(b) <= 1),
                "Episode must retain completed picker frame"
            );
            assert_eq!(host.reference_play_ticks(), before + 328.0);
            assert_eq!(
                host.reference_audio().bgm_player.reference_source().2,
                music_track
            );
            assert_eq!(host.reference_audio().system_player.track.id(), cue_track);
            assert!(Arc::ptr_eq(
                host.reference_audio().system_player.bank.as_ref().unwrap(),
                &bank
            ));
            host_step(
                &mut host,
                context,
                server,
                &mut frame,
                359.5,
                Some(AppAction::Cancel),
            );
            assert!(host.reference_transition());
            assert!(host.reference_fresh_entry().is_none());
            host_step(
                &mut host,
                context,
                server,
                &mut frame,
                0.5,
                Some(AppAction::Enter),
            );
            assert!(
                host.reference_transition(),
                "Fresh ADV queues for next frame"
            );
            let entry = host.reference_fresh_entry().unwrap().clone();
            assert_eq!((entry.scenario_id, entry.save_position), (0, 0));
            assert!(entry.route_data.reader().read_choice().is_err());
            render_host(&host, context);
            let black = crate::layer::user::readback_test_texture(context, target);
            assert!(black.chunks_exact(4).all(|pixel| pixel[..3] == [0, 0, 0]));
            host_step(&mut host, context, server, &mut frame, 0.0, None);
            assert!(!host.reference_transition());
            assert!(host.reference_adv().is_some());
            assert_eq!(
                host.reference_adv().unwrap().vm_state.save_position,
                0,
                "Initial loader must not execute script before reveal"
            );
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            let mut revealed = false;
            while host.reference_adv().unwrap().is_loading_saved_scene() {
                host_step(&mut host, context, server, &mut frame, 0.0, None);
                assert!(
                    std::time::Instant::now() < deadline,
                    "Fresh episode loading timed out"
                );
                std::thread::sleep(std::time::Duration::from_millis(1));
                if host
                    .reference_adv()
                    .unwrap()
                    .adv_state
                    .scene_load_fade
                    .is_some()
                    && !revealed
                {
                    assert_eq!(host.reference_adv().unwrap().vm_state.save_position, 0);
                    host_step(&mut host, context, server, &mut frame, 30.0, None);
                    revealed = true;
                }
            }
            assert!(revealed, "Fresh native mode0 retains30tick reveal");
            assert!(
                host.reference_adv()
                    .unwrap()
                    .saved_scene_load_error()
                    .is_none()
            );
            while host.reference_adv().unwrap().vm_state.save_position == 0 {
                host_step(&mut host, context, server, &mut frame, 0.0, None);
                assert!(
                    std::time::Instant::now() < deadline,
                    "Fresh episode first command timed out"
                );
            }
            let actual = host.reference_adv().unwrap().vm_state.snapshot_entry();
            assert_eq!(
                (actual.scenario_id, actual.random_seed, actual.save_position),
                (entry.scenario_id, entry.random_seed, 1)
            );
            assert_eq!(host.reference_play_ticks(), before + 718.0);
            assert_eq!(host.reference_adv().unwrap().persistence.is_some(), disk);
            let mut launch_expected = expected.clone();
            launch_expected.play_seconds = host.reference_save().play_seconds;
            // The first MSGSET publishes the new episode's autosave.
            launch_expected.auto_save_slot = host.reference_save().auto_save_slot;
            assert_eq!(host.reference_save().encode(), launch_expected.encode());
            assert_eq!(
                host.reference_audio().bgm_player.reference_source().2,
                music_track
            );
            assert_eq!(
                (0..crate::audio::SE_SLOT_COUNT)
                    .map(|slot| host.reference_audio().se_player.reference_source(slot).2)
                    .collect::<Vec<_>>(),
                effect_tracks
            );
            let seconds = host.reference_save().play_seconds;
            drop(host);
            if disk {
                let saved = SaveData::decode(&std::fs::read(&path).unwrap()).unwrap();
                assert_eq!(saved.play_seconds, seconds);
                assert_eq!(saved.settings.v3_sysvol, 27);
                assert!(saved.manual_save_slots[0].is_some());
                std::fs::remove_file(path).unwrap();
                std::fs::remove_dir(folder).unwrap();
            }
            eprintln!(
                "Actual Start launch {}: completed picker capture, deferred native360tick transition, seeded Episode0 mode0 loader/30tick reveal/first MSGSET, exact718tick globalclock, retained globals/audio/persistence",
                if disk { "disk" } else { "volatile" }
            );
            continue;
        }
        host_step(
            &mut host,
            context,
            server,
            &mut frame,
            0.0,
            Some(AppAction::Cancel),
        );
        host_step(&mut host, context, server, &mut frame, 0.0, None);
        assert!(
            host.reference_picker().is_some(),
            "Back replacement must wait until next frame"
        );
        host_step(&mut host, context, server, &mut frame, 0.0, None);
        assert!(host.reference_picker().is_none());
        assert!(host.reference_title().is_some());
        assert_eq!(
            host.reference_audio().bgm_player.reference_source().2,
            music_track
        );
        assert_eq!(host.reference_audio().system_player.track.id(), cue_track);
        assert_eq!(
            (0..crate::audio::SE_SLOT_COUNT)
                .map(|slot| host.reference_audio().se_player.reference_source(slot).2)
                .collect::<Vec<_>>(),
            effect_tracks
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !host.reference_title().unwrap().is_loaded() {
            host_step(&mut host, context, server, &mut frame, 0.0, None);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        for delta in [120.0, 0.0, 0.0, 74.0, 0.0] {
            host_step(&mut host, context, server, &mut frame, delta, None);
        }
        assert_eq!(
            host.reference_title().unwrap().motion.phase(),
            crate::title::TitlePhase::Active
        );
        assert_eq!(
            host.reference_title().unwrap().motion.frame().caption,
            Some(crate::title::TitleItem::Start)
        );
        assert_eq!(host.reference_play_ticks(), before + 485.0);
        host_step(
            &mut host,
            context,
            server,
            &mut frame,
            0.0,
            Some(AppAction::AnyDown),
        );
        assert_eq!(
            host.reference_title().unwrap().motion.frame().caption,
            Some(crate::title::TitleItem::Continue)
        );
        host_step(
            &mut host,
            context,
            server,
            &mut frame,
            7.0,
            Some(AppAction::Enter),
        );
        host_step(&mut host, context, server, &mut frame, 0.0, None);
        assert_eq!(
            host.reference_title().unwrap().motion.phase(),
            crate::title::TitlePhase::Continuing
        );
        host_step(&mut host, context, server, &mut frame, 45.0, None);
        assert_eq!(
            host.reference_title().unwrap().motion.frame().black_alpha,
            0.5
        );
        host_step(&mut host, context, server, &mut frame, 45.0, None);
        host_step(&mut host, context, server, &mut frame, 0.0, None);
        assert!(
            host.reference_title().is_some(),
            "Continue factory must wait until next frame"
        );
        host_step(&mut host, context, server, &mut frame, 0.0, None);
        assert!(host.reference_title().is_none());
        assert!(host.reference_adv().is_some());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while host.reference_adv().unwrap().is_loading_saved_scene() {
            host_step(&mut host, context, server, &mut frame, 0.0, None);
            assert!(
                std::time::Instant::now() < deadline,
                "Host Continue restoration timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
            if host
                .reference_adv()
                .unwrap()
                .adv_state
                .scene_load_fade
                .is_some()
            {
                host_step(&mut host, context, server, &mut frame, 30.0, None);
            }
        }
        assert!(
            host.reference_adv()
                .unwrap()
                .saved_scene_load_error()
                .is_none()
        );
        assert_eq!(host.reference_save().settings.v3_sysvol, 27);
        assert!(host.reference_save().manual_save_slots[0].is_some());
        assert!(host.reference_save().auto_save_slot.is_some());
        assert_eq!(host.reference_play_ticks(), before + 612.0);
        assert_eq!(host.reference_adv().unwrap().persistence.is_some(), disk);
        assert_eq!(
            host.reference_audio().bgm_player.reference_source().2,
            music_track
        );
        assert_eq!(
            (0..crate::audio::SE_SLOT_COUNT)
                .map(|slot| host.reference_audio().se_player.reference_source(slot).2)
                .collect::<Vec<_>>(),
            effect_tracks
        );
        assert!(
            host.reference_audio()
                .system_player
                .retired
                .iter()
                .any(|tail| tail._track.id() == cue_track)
        );
        let seconds = host.reference_save().play_seconds;
        drop(host);
        if disk {
            let saved = SaveData::decode(&std::fs::read(&path).unwrap()).unwrap();
            assert_eq!(saved.play_seconds, seconds);
            assert_eq!(saved.settings.v3_sysvol, 27);
            assert!(saved.manual_save_slots[0].is_some());
        }
        eprintln!(
            "Actual App scene host: deferred ADV/title/Continue frame boundaries, retained disk/volatile globals and audio tracks/bank/cue, native title render/intro/90tick fade, real autosave restore/30tick reveal, exact612tick global clock including Start/picker/back round trip and shutdown save pass"
        );
        if disk {
            std::fs::remove_file(path).unwrap();
            std::fs::remove_dir(folder).unwrap();
        }
        eprintln!(
            "Live ADV Save/Load: {} committed retained entry, Delete{}, ordinary close0/1, real async Load and global clock/slot preservation",
            if disk { "disk" } else { "volatile" },
            if disk { " failure rollback/retry" } else { "" }
        );
    }
}

/// Focused actual host handoff with ready, gated pending, one-shot and fading
/// story effects. No routes, save writes or unrelated menus are exercised.
pub(super) fn assert_title_return_retires_story_effects<'a>(
    server: &'a Arc<AssetServer>,
    assets: &AdvAssets,
    context: &mut PreRenderContext<'a, '_, '_, '_>,
) {
    use shin_core::{
        format::scenario::info::SeId,
        time::Tween,
        vm::command::types::{AudioWaitStatus, Pan, Volume},
    };
    let original: Arc<shin_core::format::scenario::Scenario> =
        server.load_sync("/main.snr").unwrap();
    let info = &original.info_tables().se_info;
    let index = info
        .iter()
        .position(|item| {
            let path = item.path().to_lowercase();
            path.contains("wind") || path.contains("kaze") || path.contains("風")
        })
        .unwrap_or(0);
    let id = SeId::from_number(index as i32);
    let path = info[index].path();
    let source: Arc<shin_audio::AudioFile> = server.load_sync(&path).unwrap();
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    for paused in [false, true] {
        let mut live = Adv::new(
            manager.clone(),
            server.clone(),
            assets.clone(),
            Scripter::new(&assets.scenario, 0, 42),
        );
        // The existing save-owner trace proves how this state is reached.
        live.finished = true;
        live.exit_save_finished = true;
        let effects = &mut live.adv_state.audio.se_player;
        effects.play(
            1,
            id,
            source.clone(),
            true,
            Volume::default(),
            Pan::default(),
            1.0,
            Tween::IMMEDIATE,
        );
        effects.play(
            2,
            id,
            source.clone(),
            false,
            Volume::default(),
            Pan::default(),
            1.0,
            Tween::IMMEDIATE,
        );
        effects.stop(2, Tween::linear(Ticks::from_f32(120.0)));
        let once = effects.request_once(
            server.clone(),
            path.clone(),
            id,
            Volume::default(),
            Pan::default(),
        );
        effects.request(
            server.clone(),
            path.clone(),
            0,
            id,
            true,
            Volume::default(),
            Pan::default(),
            1.0,
            Tween::IMMEDIATE,
        );
        let gate = effects.reference_hold_loading();
        assert!(
            effects
                .get_wait_status(1)
                .contains(AudioWaitStatus::PLAYING)
        );
        assert!(
            effects
                .get_wait_status(0)
                .contains(AudioWaitStatus::LOADING)
        );
        assert!(effects.reference_scene_effect_count() >= 4);
        let tracks: Vec<_> = (0..crate::audio::SE_SLOT_COUNT)
            .map(|slot| effects.reference_source(slot).2)
            .collect();
        if paused {
            live.adv_state.acquire_audio_pause();
            live.adv_state.acquire_audio_pause();
        }
        let cue_track = live.adv_state.audio.system_player.track.id();
        let bgm_track = live.adv_state.audio.bgm_player.reference_source().2;
        let mut host = crate::app::SceneHost::new(live, assets.clone(), server.clone());
        for number in 0..2 {
            host.update(
                &mut UpdateContext {
                    frame_started_at: std::time::Instant::now(),
                    frame_id: FrameId::default(),
                    delta_ticks: Ticks::ZERO,
                    asset_server: server,
                    pre_render: context,
                },
                Default::default(),
                glam::Vec2::ZERO,
                glam::Vec2::ZERO,
                (1920, 1080),
                0.0,
                0.0,
            )
            .unwrap();
            assert_eq!(host.reference_title().is_some(), number == 1);
        }
        assert!(
            gate.send(()).is_err(),
            "Retired pending effect cannot publish on title"
        );
        let audio = host.reference_audio();
        assert!(!audio.se_player.reference_is_paused());
        assert_eq!(
            audio.se_player.reference_scene_effect_count(),
            0,
            "Story effects cannot resume in the title"
        );
        assert!(audio.se_player.get_once_wait_status(once).is_empty());
        for slot in 0..crate::audio::SE_SLOT_COUNT {
            assert!(audio.se_player.get_wait_status(slot as i32).is_empty());
            assert_eq!(audio.se_player.reference_source(slot).2, tracks[slot]);
        }
        assert_eq!(audio.system_player.track.id(), cue_track);
        assert_eq!(audio.bgm_player.reference_source().2, bgm_track);
        eprintln!(
            "Title return: paused{paused}, ready loop/fading tail/one-shot/gated pending retired; shared tracks retained; supplied SE {path}"
        );
    }
}

include!("title_return_fade_reference.rs");
