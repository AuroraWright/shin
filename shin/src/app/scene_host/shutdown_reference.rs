//! Exercise real temporary storage errors through FirstBoot and its scene owner.
use super::*;
use crate::app::{
    App,
    shutdown::{Frame, Shutdown},
};
use crate::{
    asset::system::{AssetLoadContext, LayeredAssetIo, cache::AssetCache},
    render::PreRenderContext,
};
use shin_core::{format::save::SaveData, primitives::update::FrameId, time::Ticks};
use shin_render::{
    dynamic_buffer::DynamicBuffer,
    pipelines::PipelineStorage,
    resize::{SurfaceResizeSource, ViewportParams},
    shaders::types::{
        buffer::BytesAddress,
        texture::{DepthStencilTarget, TextureSamplerStore, TextureTarget, TextureTargetKind},
    },
};
use shin_window::ShinApp;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    time::{Duration, Instant},
};

#[test]
#[ignore = "requires supplied ROM, Metal and the default audio output; isolated temporary saves only"]
fn reference_shutdown_failure_retry_cancel_and_pending_save_owners() {
    run_shutdown_failure(false);
}

pub(super) fn run_shutdown_failure(bad_notice: bool) {
    crate::init_test_task_pools();
    let adapter =
        shin_tasks::block_on(wgpu::Instance::default().request_adapter(&Default::default()))
            .unwrap();
    let (device, queue) = shin_tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: if adapter.get_info().backend == wgpu::Backend::Vulkan {
            adapter.features() & wgpu::Features::PASSTHROUGH_SHADERS
        } else {
            wgpu::Features::empty()
        },
        ..Default::default()
    }))
    .unwrap();
    let mut io = LayeredAssetIo::new();
    let _notice_fixture =
        bad_notice.then(|| super::notice_failure_reference::override_notice(&mut io));
    if let Ok(patch) = std::env::var("SHIN_REFERENCE_PATCH_ROM") {
        io.try_with_rom(patch).unwrap();
    }
    io.try_with_rom(std::env::var("SHIN_REFERENCE_ROM").unwrap())
        .unwrap();
    let server = Arc::new(AssetServer::new(
        io.into(),
        AssetLoadContext {
            game: crate::cli::Game::Umineko,
            higurashi_bustup_cache: Default::default(),
            wgpu_device: device.clone(),
            wgpu_queue: queue.clone(),
            bustup_cache: AssetCache::new(),
            picture_cache: Default::default(),
            font_cache: Default::default(),
        },
    ));
    let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
        1920, 1080,
    )));
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Shutdown storage failure reference"),
        size: wgpu::Extent3d {
            width: 1920,
            height: 1080,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: shin_render::TEXTURE_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Shutdown storage failure depth"),
        size: target.size(),
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: shin_render::DEPTH_STENCIL_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let depth_view = depth.create_view(&Default::default());
    let samplers = TextureSamplerStore::new(&device);
    let mut pipelines = PipelineStorage::new(device.clone(), shin_render::TEXTURE_FORMAT);
    let mut dynamic = DynamicBuffer::new(device.clone(), BytesAddress::new(1024 * 1024));
    let mut encoder = device.create_command_encoder(&Default::default());
    let mut context = PreRenderContext {
        device: &device,
        queue: &queue,
        breakup_texture: None,
        resize_source: &resize,
        sampler_store: &samplers,
        depth_stencil: DepthStencilTarget { view: &depth_view },
        pipeline_storage: &mut pipelines,
        dynamic_buffer: &mut dynamic,
        encoder: &mut encoder,
    };
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o700));
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let assets = shin_tasks::block_on(AdvAssets::load(&server)).unwrap();
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let view = target.create_view(&Default::default());
    fn slot_bytes(data: &SaveData) -> Vec<u8> {
        let mut only_slot = SaveData::default();
        only_slot.manual_save_slots[99] = data.manual_save_slots[99].clone();
        only_slot.encode()
    }
    fn frame<'a>(
        app: &mut App,
        server: &'a Arc<AssetServer>,
        ctx: &mut PreRenderContext<'a, '_, '_, '_>,
        delta: f32,
        action: Option<AppAction>,
        point: glam::Vec2,
        held: bool,
        extent: (u16, u16),
    ) -> Frame {
        let mut input = EnumMap::default();
        if let Some(action) = action {
            input[action] = ActionState {
                is_clicked: true,
                is_clicked_or_repeated: true,
                ..Default::default()
            };
        }
        input[AppAction::PointerClick].is_held = held;
        let result = app.shutdown_frame(
            ctx,
            Ticks::from_f32(delta),
            &input,
            point,
            extent,
            1.0 / 60.0,
        );
        if result == Frame::Continue {
            app.frame_id.advance();
            app.scenes
                .update(
                    &mut UpdateContext {
                        frame_started_at: Instant::now(),
                        frame_id: app.frame_id,
                        delta_ticks: Ticks::from_f32(delta),
                        asset_server: server,
                        pre_render: ctx,
                    },
                    input,
                    point * glam::vec2(extent.0 as f32 / 1920.0, extent.1 as f32 / 1080.0),
                    point,
                    extent,
                    0.0,
                    1.0 / 60.0,
                )
                .unwrap();
        }
        let encoder = std::mem::replace(
            ctx.encoder,
            ctx.device.create_command_encoder(&Default::default()),
        );
        let mut upload = ctx.device.create_command_encoder(&Default::default());
        ctx.dynamic_buffer.finish(&mut upload);
        ctx.queue.submit([upload.finish(), encoder.finish()]);
        ctx.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("GPU work must complete before readback or retirement");
        result
    }
    fn pixels(
        app: &App,
        ctx: &mut PreRenderContext,
        target: &wgpu::Texture,
        view: &wgpu::TextureView,
        include_notice: bool,
    ) -> Vec<u8> {
        let mut pass = ctx.begin_pass(
            TextureTarget {
                kind: TextureTargetKind::Screen,
                view,
            },
            Some(ctx.depth_stencil),
            "Shutdown notice reference",
        );
        app.scenes.render(&mut pass);
        if include_notice {
            app.shutdown.render(&mut pass);
        }
        drop(pass);
        crate::layer::user::readback_test_texture(ctx, target)
    }
    fn open<'a>(
        app: &mut App,
        server: &'a Arc<AssetServer>,
        ctx: &mut PreRenderContext<'a, '_, '_, '_>,
        extent: (u16, u16),
        held: bool,
    ) {
        let before = app.scenes.reference_save().encode();
        let clock = app.scenes.reference_play_ticks();
        for _ in 0..40 {
            assert_eq!(
                frame(app, server, ctx, 1.0, None, glam::Vec2::ZERO, held, extent),
                Frame::Consumed
            );
            assert_eq!(app.scenes.reference_save().encode(), before);
            assert_eq!(app.scenes.reference_play_ticks(), clock);
            if app.shutdown.reference_failure_is_open() {
                return;
            }
        }
        panic!("Shutdown notice did not open");
    }
    fn dismiss<'a>(
        app: &mut App,
        server: &'a Arc<AssetServer>,
        ctx: &mut PreRenderContext<'a, '_, '_, '_>,
        extent: (u16, u16),
        index: usize,
        mouse: bool,
    ) {
        let point = app.shutdown.reference_button(index);
        if mouse {
            assert_eq!(
                frame(app, server, ctx, 0.0, None, point, false, extent),
                Frame::Consumed
            );
            assert_eq!(
                frame(app, server, ctx, 0.0, None, point, true, extent),
                Frame::Consumed
            );
            assert_eq!(
                frame(app, server, ctx, 0.0, None, point, false, extent),
                Frame::Consumed
            );
        } else {
            assert_eq!(
                frame(
                    app,
                    server,
                    ctx,
                    0.0,
                    Some(if index == 1 {
                        AppAction::Cancel
                    } else {
                        AppAction::Enter
                    }),
                    point,
                    false,
                    extent
                ),
                Frame::Consumed
            );
        }
        for _ in 0..60 {
            assert_eq!(
                frame(app, server, ctx, 1.0, None, point, false, extent),
                Frame::Consumed
            );
            if !app.shutdown.pauses_scene() {
                return;
            }
        }
        panic!("Shutdown acknowledgement did not finish");
    }
    for (title, pending, fail_pending) in [
        (true, false, false),
        (false, false, false),
        (true, true, false),
        (true, true, true),
    ] {
        // Explicit SaveGame retains its strict updating atlas and is already
        // covered by the preparation/callback tests. Select shutdown-only owners.
        if bad_notice && pending {
            continue;
        }
        let directory = std::env::temp_dir().join(format!(
            "shin-shutdown-{}-{title}-{pending}-{fail_pending}",
            std::process::id()
        ));
        fs::create_dir(&directory).unwrap();
        let _cleanup = Cleanup(directory.clone());
        let path = directory.join("progress.dat");
        let mut saved = SaveData::default();
        saved.play_seconds = 41;
        saved.save_menu_position = 99;
        saved.settings.v3_sysvol = 35;
        saved.settings.v18_showsongtitle = false;
        saved.manual_save_slots[99] =
            Some(crate::adv::VmState::for_scenario(19, 99).snapshot_game_data());
        saved.auto_save_slot = Some(crate::adv::VmState::for_scenario(7, 42).snapshot_game_data());
        let original = saved.encode();
        fs::write(&path, &original).unwrap();
        let scenes = if title {
            let mut globals = GlobalData::open(Some(path.clone()), None).unwrap();
            let mut settings = globals.save().settings.clone();
            settings.v0_bgmvol = 73;
            globals.apply_config_settings(&settings);
            globals.advance_play_clock(Ticks::from_f32(61.0));
            let mut scene =
                TitleScene::new(server.clone(), TitleSetup::from_save(globals.save(), 0));
            if pending {
                scene.reference_begin_config_save(&mut globals);
            }
            SceneHost {
                scene: Some(Scene::Title(TitleRuntime {
                    scene,
                    globals,
                    audio: GlobalAudio::new(manager.clone()),
                    extent: (1920, 1080),
                })),
                pending: None,
                assets: Some(assets.clone()),
                common: None,
                server: server.clone(),
                pointer: Default::default(),
                error: None,
                restoring_from_title: false,
                viewer_axes: glam::Vec2::ZERO,
            }
        } else {
            let mut live = Adv::new(
                manager.clone(),
                server.clone(),
                assets.clone(),
                Scripter::new(&assets.scenario, 0, 42),
            );
            live.configure_save_file(path.clone(), None).unwrap();
            let (vm, _) = live.reference_msgset_states();
            vm.settings.v0_bgmvol = 73;
            vm.persist.set(28, 123);
            vm.save_revision += 1;
            live.advance_play_clock(Ticks::from_f32(61.0));
            SceneHost::new(live, assets.clone(), server.clone())
        };
        let mut app = App {
            shutdown: Shutdown::default(),
            platform_suspended: false,
            frame_id: FrameId::default(),
            effect_textures: None,
            audio_manager: manager.clone(),
            asset_server: server.clone(),
            scenes,
            pointer_position: glam::Vec2::ZERO,
            character_scroll_axis: 0.0,
            touch_effect: Default::default(),
            touch_background: None,
            system_textures: None,
        };
        manager.set_suspended(false);
        let extent = if title { (1280, 720) } else { (3840, 2160) };
        if !title {
            let deadline = Instant::now() + Duration::from_secs(10);
            while app
                .scenes
                .reference_adv()
                .unwrap()
                .reference_entry()
                .save_position
                == 0
            {
                assert_eq!(
                    frame(
                        &mut app,
                        &server,
                        &mut context,
                        0.0,
                        None,
                        glam::Vec2::ZERO,
                        false,
                        extent
                    ),
                    Frame::Continue
                );
                assert!(
                    Instant::now() < deadline,
                    "Authored message checkpoint did not publish"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
            // The message's automatic save is durable. Make a newer global
            // progress change so shutdown really needs to write again.
            let (vm, _) = app
                .scenes
                .reference_adv_mut()
                .unwrap()
                .reference_msgset_states();
            vm.persist.set(28, 124);
            vm.save_revision += 1;
        }
        let original = fs::read(&path).unwrap();
        if !pending || fail_pending {
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o555)).unwrap();
        }
        assert!(!app.close_requested());
        if pending {
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut observed = 0;
            loop {
                let was_pending = app.scenes.has_save_task();
                let result = frame(
                    &mut app,
                    &server,
                    &mut context,
                    1.0,
                    None,
                    glam::Vec2::ZERO,
                    false,
                    extent,
                );
                if was_pending {
                    assert_eq!(result, Frame::Continue);
                    observed += 1;
                }
                if fail_pending
                    && app
                        .scenes
                        .reference_title()
                        .unwrap()
                        .reference_save_notice_open()
                {
                    break;
                }
                if result == Frame::Exit {
                    break;
                }
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(1));
            }
            assert!(observed > 0);
            if fail_pending {
                assert_eq!(fs::read(&path).unwrap(), original);
                for _ in 0..3 {
                    assert!(!app.close_requested());
                    assert_eq!(
                        frame(
                            &mut app,
                            &server,
                            &mut context,
                            1.0,
                            None,
                            glam::Vec2::ZERO,
                            false,
                            extent
                        ),
                        Frame::Continue
                    );
                    assert!(app.scenes.has_save_task());
                }
                fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
                assert_eq!(
                    frame(
                        &mut app,
                        &server,
                        &mut context,
                        0.0,
                        Some(AppAction::Enter),
                        glam::Vec2::ZERO,
                        false,
                        extent
                    ),
                    Frame::Continue
                );
                loop {
                    if frame(
                        &mut app,
                        &server,
                        &mut context,
                        1.0,
                        None,
                        glam::Vec2::ZERO,
                        false,
                        extent,
                    ) == Frame::Exit
                    {
                        break;
                    }
                    assert!(Instant::now() < deadline);
                }
            }
            let written = SaveData::decode(&fs::read(&path).unwrap()).unwrap();
            assert_eq!(written.settings.v0_bgmvol, 73);
            assert_eq!(written.settings.v3_sysvol, 40);
            assert_eq!(slot_bytes(&written), slot_bytes(&saved));
            assert!(written.play_seconds >= 42);
            println!(
                "Title pending SaveGame failure{fail_pending}: owner retained until worker/notice finish, latest shutdown snapshot durable."
            );
        } else {
            // Inherited press over Retry must not confirm the new notice.
            assert_eq!(
                frame(
                    &mut app,
                    &server,
                    &mut context,
                    0.0,
                    None,
                    glam::Vec2::ZERO,
                    true,
                    extent
                ),
                Frame::Consumed
            );
            assert!(manager.suspension().is_suspended());
            open(&mut app, &server, &mut context, extent, true);
            if bad_notice {
                assert!(app.shutdown.reference_uses_plain_notice());
            }
            let error_pixels = pixels(&app, &mut context, &target, &view, true);
            if bad_notice {
                super::notice_failure_reference::assert_notice_pixels(
                    &error_pixels,
                    &format!("shutdown-{title}"),
                );
            }
            let scene_pixels = pixels(&app, &mut context, &target, &view, false);
            assert!(
                error_pixels
                    .chunks_exact(4)
                    .zip(scene_pixels.chunks_exact(4))
                    .filter(|(a, b)| a != b)
                    .count()
                    > 1000,
                "Omitting the error notice must change the visible frame"
            );
            assert!(
                error_pixels
                    .chunks_exact(4)
                    .filter(|p| p[0] > 10 || p[1] > 10 || p[2] > 10)
                    .count()
                    > 1000,
                "Shutdown error must be visible"
            );
            let original_button = app.shutdown.reference_button(0);
            for _ in 0..3 {
                assert!(!app.close_requested());
            }
            assert_eq!(app.shutdown.reference_button(0), original_button);
            assert_eq!(
                frame(
                    &mut app,
                    &server,
                    &mut context,
                    1.0,
                    None,
                    original_button,
                    false,
                    extent
                ),
                Frame::Consumed
            );
            assert!(app.shutdown.reference_failure_is_open());
            assert_eq!(fs::read(&path).unwrap(), original);
            let memory = app.scenes.reference_save().encode();
            if !title {
                app.set_suspended(true);
            }
            dismiss(&mut app, &server, &mut context, extent, 1, title);
            assert!(!app.shutdown.active());
            assert_eq!(manager.suspension().is_suspended(), !title);
            app.set_suspended(false);
            assert!(!manager.suspension().is_suspended());
            assert_eq!(app.scenes.reference_save().encode(), memory);
            assert_eq!(fs::read(&path).unwrap(), original);
            assert_eq!(
                frame(
                    &mut app,
                    &server,
                    &mut context,
                    0.0,
                    None,
                    glam::Vec2::ZERO,
                    false,
                    extent
                ),
                Frame::Continue
            );
            assert_eq!(
                app.scenes.reference_save().encode(),
                memory,
                "Cancel returns to the same scene without advancing a message"
            );
            assert!(app.scenes.pending.is_none());
            // A new close retries; a failed Retry retains the same live game.
            assert!(!app.close_requested());
            assert_eq!(
                frame(
                    &mut app,
                    &server,
                    &mut context,
                    0.0,
                    None,
                    glam::Vec2::ZERO,
                    false,
                    extent
                ),
                Frame::Consumed
            );
            open(&mut app, &server, &mut context, extent, false);
            dismiss(&mut app, &server, &mut context, extent, 0, !title);
            assert_eq!(
                frame(
                    &mut app,
                    &server,
                    &mut context,
                    0.0,
                    None,
                    glam::Vec2::ZERO,
                    false,
                    extent
                ),
                Frame::Consumed
            );
            open(&mut app, &server, &mut context, extent, false);
            assert_eq!(app.scenes.reference_save().encode(), memory);
            assert_eq!(fs::read(&path).unwrap(), original);
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
            dismiss(&mut app, &server, &mut context, extent, 0, !title);
            assert_eq!(
                frame(
                    &mut app,
                    &server,
                    &mut context,
                    0.0,
                    None,
                    glam::Vec2::ZERO,
                    false,
                    extent
                ),
                Frame::Exit
            );
            let written = SaveData::decode(&fs::read(&path).unwrap()).unwrap();
            assert_eq!(written.encode(), memory);
            assert_eq!(written.settings.v0_bgmvol, 73);
            assert_eq!(written.play_seconds, 42);
            assert_eq!(slot_bytes(&written), slot_bytes(&saved));
            if !title {
                assert_eq!(written.persist_data.get(28), 124);
            }
            println!(
                "Shutdown Title{title}: real permission failure, visible notice, frozen progress/clock/audio, inherited press ignored, mouse/keyboard Cancel/repeated failed Retry/successful Retry and unchanged slots pass."
            );
        }
        manager.set_suspended(false);
        drop(app);
    }
}
