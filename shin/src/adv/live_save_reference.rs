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

/// Desktop storage adapter exercised through the actual ADV update/input owner.
/// Native successful save timing and Nintendo OS error UI remain separate.
#[cfg(unix)]
pub(super) fn assert_automatic_save_failure<'a>(
    server: &'a Arc<AssetServer>,
    assets: &AdvAssets,
    context: &mut PreRenderContext<'a, '_, '_, '_>,
    target: &wgpu::Texture,
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
        context.device.poll(wgpu::Maintain::Wait);
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
        context.device.poll(wgpu::Maintain::Wait);
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
        while live.is_loading_saved_scene() {
            step(&mut live, context, server, &mut frame, 1.0, None);
            assert!(
                std::time::Instant::now() < deadline,
                "Actual manual Load restoration timed out: {:?}",
                live.restore_error
            );
            std::thread::yield_now();
        }
        assert!(live.restore_error.is_none());
        assert_eq!(live.vm_state.save_position, 1);
        assert_eq!(live.vm_state.snapshot_entry().random_seed, 42);
        assert!(live.play_ticks() >= before_load);
        assert!(
            live.save_menu_data().manual_save_slots[0].is_some(),
            "scene replacement must retain global manual slots"
        );
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
