//! Real-ROM TitleDemo worker/movie rendering, input and deferred host replacement.
use super::*;
use shin_core::{format::save::SaveData, primitives::update::FrameId, time::Ticks};

#[test]
#[ignore = "prepares disposable on/off progress files for the background GUI check"]
fn reference_prepare_background_policy_progress() {
    let dir = std::path::PathBuf::from(std::env::var_os("SHIN_BACKGROUND_TEST_DIR").unwrap());
    assert!(dir.is_absolute() && dir.starts_with(std::env::temp_dir()));
    std::fs::create_dir_all(&dir).unwrap();
    for enabled in [false, true] {
        let mut data = SaveData::default();
        data.settings.v18_showsongtitle = enabled;
        let encoded = data.encode();
        assert_eq!(
            SaveData::decode(&encoded)
                .unwrap()
                .settings
                .v18_showsongtitle,
            enabled
        );
        let file = dir.join(if enabled {
            "enabled.dat"
        } else {
            "disabled.dat"
        });
        use std::io::Write;
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(file)
            .unwrap()
            .write_all(&encoded)
            .unwrap();
    }
}

#[test]
#[ignore = "requires supplied ROM, FFmpeg and GPU (SHIN_REFERENCE_ROM)"]
fn reference_startup_movie_owner_and_host_handoff() {
    use crate::{
        asset::system::{AssetLoadContext, LayeredAssetIo, cache::AssetCache},
        render::PreRenderContext,
    };
    use shin_render::{
        dynamic_buffer::DynamicBuffer,
        pipelines::PipelineStorage,
        resize::{SurfaceResizeSource, ViewportParams},
        shaders::types::{
            buffer::BytesAddress,
            texture::{DepthStencilTarget, TextureSamplerStore},
        },
    };
    crate::init_test_task_pools();
    let adapter =
        shin_tasks::block_on(wgpu::Instance::default().request_adapter(&Default::default()))
            .unwrap();
    let (device, queue) = shin_tasks::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            required_features: adapter.features() & wgpu::Features::SPIRV_SHADER_PASSTHROUGH,
            ..Default::default()
        },
        None,
    ))
    .unwrap();
    let mut io = LayeredAssetIo::new();
    io.try_with_rom(std::env::var("SHIN_REFERENCE_ROM").unwrap())
        .unwrap();
    let server = Arc::new(AssetServer::new(
        io.into(),
        AssetLoadContext {
            wgpu_device: device.clone(),
            wgpu_queue: queue.clone(),
            bustup_cache: AssetCache::new(),
            picture_cache: Default::default(),
            font_cache: Default::default(),
        },
    ));
    let assets = shin_tasks::block_on(AdvAssets::load(&server)).unwrap();
    let info = assets
        .scenario
        .info_tables()
        .movie_info
        .first()
        .expect("Native startup movie0");
    eprintln!("Native startup movie: {info:?}");
    let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
        1920, 1080,
    )));
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Startup reference depth"),
        size: wgpu::Extent3d {
            width: 1920,
            height: 1080,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: shin_render::DEPTH_STENCIL_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let depth_view = depth.create_view(&Default::default());
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Startup reference pixels"),
        size: depth.size(),
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: shin_render::TEXTURE_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let samplers = TextureSamplerStore::new(&device);
    let mut pipelines = PipelineStorage::new(device.clone(), shin_render::TEXTURE_FORMAT);
    let mut dynamic = DynamicBuffer::new(device.clone(), BytesAddress::new(1024 * 1024));
    let mut encoder = device.create_command_encoder(&Default::default());
    let mut ctx = PreRenderContext {
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
    fn step<'a>(
        host: &mut SceneHost,
        ctx: &mut PreRenderContext<'a, '_, '_, '_>,
        server: &'a Arc<AssetServer>,
        frame: &mut FrameId,
        delta: f32,
        action: Option<AppAction>,
        held: bool,
    ) {
        let mut input = EnumMap::default();
        if let Some(action) = action {
            input[action] = ActionState {
                is_clicked: true,
                is_clicked_or_repeated: true,
                ..Default::default()
            };
        }
        input[AppAction::PointerClick].is_held = held;
        frame.advance();
        host.update(
            &mut UpdateContext {
                frame_started_at: std::time::Instant::now(),
                frame_id: *frame,
                delta_ticks: Ticks::from_f32(delta),
                asset_server: server,
                pre_render: ctx,
            },
            input,
            glam::vec2(20.0, 20.0),
            glam::vec2(20.0, 20.0),
            (1920, 1080),
            0.0,
            delta as f64 / 60.0,
        )
        .unwrap();
    }
    fn pixels(host: &SceneHost, ctx: &mut PreRenderContext, target: &wgpu::Texture) -> Vec<u8> {
        let view = target.create_view(&Default::default());
        {
            let mut pass = ctx.begin_pass(
                shin_render::shaders::types::texture::TextureTarget {
                    view: &view,
                    kind: shin_render::shaders::types::texture::TextureTargetKind::Screen,
                },
                Some(ctx.depth_stencil),
                "Startup reference",
            );
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::WHITE),
                Some(0),
                Some(1.0),
            );
            host.render(&mut pass);
        }
        crate::layer::user::readback_test_texture(ctx, target)
    }
    let folder = std::env::temp_dir().join(format!("shin-startup-owner-{}", std::process::id()));
    assert!(!folder.exists());
    std::fs::create_dir(&folder).unwrap();
    let boot_servers: Vec<_> = (0..3)
        .map(|_| {
            let mut io = LayeredAssetIo::new();
            io.try_with_rom(std::env::var("SHIN_REFERENCE_ROM").unwrap())
                .unwrap();
            Arc::new(AssetServer::new(
                io.into(),
                AssetLoadContext {
                    wgpu_device: device.clone(),
                    wgpu_queue: queue.clone(),
                    bustup_cache: AssetCache::new(),
                    picture_cache: Default::default(),
                    font_cache: Default::default(),
                },
            ))
        })
        .collect();
    let mut authored_manual: Option<GameDataEntry> = None;
    for mode in 0..6 {
        let server = if mode >= 3 {
            &boot_servers[mode - 3]
        } else {
            &server
        };
        let mut save = SaveData::default();
        save.play_seconds = 123;
        save.settings.v0_bgmvol = 0; // Keep real movie audio clock while muting output.
        save.settings.v3_sysvol = 0;
        if mode != 0 {
            let game = shin_core::format::save::GameData::from_entry(GameDataEntry {
                scenario_id: 0,
                random_seed: 1234,
                save_position: if mode >= 3 { 0 } else { 1 },
                route_data: Default::default(),
            });
            if mode != 4 {
                save.auto_save_slot = Some(game.clone());
            }
            save.manual_save_slots[99] = Some(game);
            if mode == 5 {
                let entry = authored_manual
                    .as_ref()
                    .expect("Start must publish a real authored entry before Load")
                    .clone();
                assert_eq!(entry.save_position, 1);
                assert_ne!(entry.random_seed, 1234);
                save.manual_save_slots[99] =
                    Some(shin_core::format::save::GameData::from_entry(entry));
            }
            save.save_menu_position = 99;
        }
        let path = folder.join(format!("{mode}.dat"));
        let globals = if mode >= 3 {
            // Read must be deferred until common resources finish, not init.
            std::fs::write(&path, b"not yet a valid save").unwrap();
            GlobalData::default()
        } else if mode != 0 {
            std::fs::write(&path, save.encode()).unwrap();
            GlobalData::open(Some(path.clone()), None).unwrap()
        } else {
            GlobalData::open(None, Some(save.clone())).unwrap()
        };
        let audio = GlobalAudio::new(Arc::new(shin_audio::AudioManager::new_for_tests()));
        let track = audio.system_player.track.id();
        let bgm_track = audio.bgm_player.reference_source().2;
        let mut host = if mode >= 3 {
            SceneHost::new_first_boot(audio, server.clone(), Some(path.clone()))
        } else {
            SceneHost::new_startup(globals, audio, assets.clone(), server.clone())
        };
        assert!(host.reference_adv().is_none());
        let mut frame = FrameId::default();
        if mode >= 3 {
            assert!(host.effect_textures().is_none());
            assert_eq!(host.reference_play_ticks(), 0.0);
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            let mut saw_bank_before_common = false;
            while host.common.is_none() {
                step(
                    &mut host,
                    &mut ctx,
                    &server,
                    &mut frame,
                    1.0,
                    Some(AppAction::Enter),
                    true,
                );
                saw_bank_before_common |=
                    host.reference_audio().system_player.bank.is_some() && host.common.is_none();
                assert!(matches!(host.scene, Some(Scene::Boot(_))));
                assert!(host.pending.is_none());
                assert!(
                    !host
                        .reference_save()
                        .save_vectors
                        .unlocked_movies
                        .contains(0)
                );
                assert!(
                    pixels(&host, &mut ctx, &target)
                        .chunks_exact(4)
                        .all(|p| p[..3] == [0, 0, 0])
                );
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            assert!(
                saw_bank_before_common,
                "Native sysse publishes before fonts/common assets finish"
            );
            assert!(host.effect_textures().is_some());
            assert_eq!(
                server.reference_requested_paths(),
                [
                    "/sysse.bin",
                    "/system.fnt",
                    "/newrodin-medium.fnt",
                    "/newrodin-bold.fnt",
                    "/systex.txa",
                    "/msgtex.txa",
                    "/effects.txa",
                    "/main.snr"
                ]
            );
            let boot_ticks = host.reference_play_ticks();
            assert!(boot_ticks > 0.0);
            std::fs::write(&path, save.encode()).unwrap();
            while host.pending.is_none() {
                step(
                    &mut host,
                    &mut ctx,
                    &server,
                    &mut frame,
                    0.0,
                    Some(AppAction::Enter),
                    true,
                );
                assert!(matches!(host.scene, Some(Scene::Boot(_))));
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            assert!(matches!(host.pending, Some(Destination::Demo)));
            assert_eq!(
                host.reference_play_ticks(),
                123.0 * 60.0,
                "LoadGame replaces pre-load elapsed clock"
            );
            assert_eq!(host.reference_save().encode(), save.encode());
            assert!(host.reference_audio().system_player.bank.is_some());
            step(
                &mut host,
                &mut ctx,
                &server,
                &mut frame,
                0.0,
                Some(AppAction::Enter),
                true,
            );
            assert!(matches!(host.scene, Some(Scene::Demo(_))));
        }
        let vectors = &host.reference_save().save_vectors;
        assert!(vectors.unlocked_movies.contains(0));
        assert!(
            vectors
                .unlocked_cgs
                .contains(info.linked_picture_id.into_i32() as usize)
        );
        if let Some(bgm) = info.linked_bgm_id.repr() {
            assert!(vectors.unlocked_bgms.contains(bgm.into_i32() as usize));
        }
        let expected = host.reference_save();
        let black = pixels(&host, &mut ctx, &target);
        assert!(black.chunks_exact(4).all(|p| p[..3] == [0, 0, 0]));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !matches!(&host.scene,Some(Scene::Demo(demo)) if demo.scene.reference_ready()) {
            step(
                &mut host,
                &mut ctx,
                &server,
                &mut frame,
                0.0,
                Some(AppAction::Enter),
                true,
            );
            assert!(
                host.pending.is_none(),
                "Loading and publication ignore input"
            );
            assert!(
                std::time::Instant::now() < deadline,
                "Startup movie loading timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        // Holding across publication cannot manufacture a new pointer capture.
        step(&mut host, &mut ctx, &server, &mut frame, 0.0, None, false);
        assert_eq!(
            match &host.scene {
                Some(Scene::Demo(d)) => d.scene.reference_opacity(),
                _ => panic!(),
            },
            1.0
        );
        assert_eq!(host.reference_play_ticks(), 123.0 * 60.0);
        if mode != 2 {
            // The authored opening starts black. Reach visible artwork on its
            // real audio-tied clock so the fade pixel oracle cannot pass by
            // comparing two blank frames.
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
            loop {
                step(&mut host, &mut ctx, &server, &mut frame, 3.0, None, false);
                let image = pixels(&host, &mut ctx, &target);
                if image
                    .chunks_exact(4)
                    .filter(|p| p[..3].iter().any(|&v| v > 32))
                    .count()
                    > 1000
                {
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "Authored opening never displayed visible artwork"
                );
                std::thread::sleep(std::time::Duration::from_millis(30));
            }
        }
        if mode == 2 {
            if let Some(Scene::Demo(demo)) = &host.scene {
                demo.scene.reference_finish();
            }
            step(
                &mut host,
                &mut ctx,
                &server,
                &mut frame,
                0.0,
                Some(AppAction::Enter),
                false,
            );
            assert!(
                host.reference_audio().system_player.sounds.is_empty(),
                "Completion precedes skip cue"
            );
        } else {
            if mode == 1 {
                step(
                    &mut host,
                    &mut ctx,
                    &server,
                    &mut frame,
                    0.0,
                    Some(AppAction::Enter),
                    true,
                );
                assert!(
                    host.reference_audio().system_player.sounds.is_empty(),
                    "Pointer capture suppresses controller"
                );
                step(&mut host, &mut ctx, &server, &mut frame, 0.0, None, false);
            } else {
                step(
                    &mut host,
                    &mut ctx,
                    &server,
                    &mut frame,
                    0.0,
                    Some(AppAction::Cancel),
                    false,
                );
            }
            assert!(host.reference_audio().system_player.bank.is_some());
            let full = pixels(&host, &mut ctx, &target);
            if let Some(Scene::Demo(demo)) = &mut host.scene {
                demo.scene.reference_advance_fade(Ticks::from_f32(45.0));
            }
            let half = pixels(&host, &mut ctx, &target);
            for (full, half) in full.chunks_exact(4).zip(half.chunks_exact(4)) {
                for c in 0..3 {
                    assert!((half[c] as f32 - full[c] as f32 * (127.0 / 255.0)).abs() <= 2.0);
                }
            }
            // Advance the actual host for the remaining clock, before the next
            // monitor poll. Frozen frame above isolates opacity from movie time.
            step(&mut host, &mut ctx, &server, &mut frame, 45.0, None, false);
            assert!(host.pending.is_none());
            assert!(
                pixels(&host, &mut ctx, &target)
                    .chunks_exact(4)
                    .all(|p| p[..3] == [0, 0, 0])
            );
            step(&mut host, &mut ctx, &server, &mut frame, 0.0, None, false);
        }
        assert!(matches!(host.pending, Some(Destination::StartupTitle(-1))));
        assert!(matches!(host.scene, Some(Scene::Demo(_))));
        step(&mut host, &mut ctx, &server, &mut frame, 0.0, None, false);
        assert!(host.reference_title().is_some());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !host.reference_title().unwrap().is_loaded() {
            step(&mut host, &mut ctx, &server, &mut frame, 0.0, None, false);
            assert!(
                std::time::Instant::now() < deadline,
                "Initial title assets did not load"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(
            host.reference_title().unwrap().motion.phase(),
            crate::title::TitlePhase::FirstWait
        );
        assert_eq!(
            host.reference_title().unwrap().motion.selected(),
            usize::from(mode != 0 && mode != 4)
        );
        assert_eq!(host.reference_audio().system_player.track.id(), track);
        assert_eq!(
            host.reference_audio().bgm_player.reference_source().2,
            bgm_track
        );
        let mut retained = host.reference_save();
        retained.play_seconds = expected.play_seconds;
        assert_eq!(retained.encode(), expected.encode());
        if mode == 3 {
            assert!(
                host.assets.is_none(),
                "No ADV menu atlas requested for Boot/Demo/Title"
            );
            assert!(
                !server
                    .reference_requested_paths()
                    .iter()
                    .any(|path| path == "/adv.txa")
            );
            let before = host.reference_save().encode();
            let common = host.common.as_ref().unwrap().clone();
            host.pending = Some(Destination::Action(TitleItem::Continue));
            host.apply_pending(&mut ctx, (1920, 1080)).unwrap();
            assert!(matches!(host.scene, Some(Scene::PreparingAdv(_))));
            assert_eq!(host.reference_save().encode(), before);
            assert_eq!(host.reference_audio().system_player.track.id(), track);
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            while host.reference_adv().is_none() {
                step(
                    &mut host,
                    &mut ctx,
                    &server,
                    &mut frame,
                    0.0,
                    Some(AppAction::Enter),
                    false,
                );
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            assert!(host.reference_adv().unwrap().is_loading_saved_scene());
            assert!(Arc::ptr_eq(
                &host.assets.as_ref().unwrap().fonts.system_font,
                &common.fonts.system_font
            ));
            assert!(Arc::ptr_eq(
                &host.assets.as_ref().unwrap().scenario,
                &common.scenario
            ));
            assert!(Arc::ptr_eq(
                &host.assets.as_ref().unwrap().messagebox_textures,
                &common.messagebox
            ));
            assert_eq!(
                server
                    .reference_requested_paths()
                    .iter()
                    .filter(|path| path.as_str() == "/adv.txa")
                    .count(),
                1
            );
            // Complete reconstruction at zero elapsed time so shutdown cannot
            // hide a lost dirty-progress revision behind a clock-only write.
            while !host.reference_adv().unwrap().reference_scene_reveal() {
                step(&mut host, &mut ctx, server, &mut frame, 0.0, None, false);
                assert!(std::time::Instant::now() < deadline);
                assert!(
                    host.reference_adv()
                        .unwrap()
                        .saved_scene_load_error()
                        .is_none()
                );
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            // Saved mode can retire the old cue bus during reconstruction;
            // its global bank stays resident across that native policy.
            assert!(Arc::ptr_eq(
                host.reference_audio().system_player.bank.as_ref().unwrap(),
                &common._sysse
            ));
        }
        if mode == 5 {
            let manual_entry = authored_manual.as_ref().unwrap();
            while host.reference_title().unwrap().motion.phase() != crate::title::TitlePhase::Active
            {
                step(&mut host, &mut ctx, server, &mut frame, 10.0, None, false);
                pixels(&host, &mut ctx, &target);
                assert!(std::time::Instant::now() < deadline);
            }
            step(
                &mut host,
                &mut ctx,
                server,
                &mut frame,
                0.0,
                Some(AppAction::AnyDown),
                false,
            );
            assert_eq!(
                host.reference_title().unwrap().motion.frame().caption,
                Some(TitleItem::Load)
            );
            let selected_title = host.reference_title().unwrap().motion.selected();
            for ordinary_close in [true, false] {
                step(
                    &mut host,
                    &mut ctx,
                    server,
                    &mut frame,
                    7.0,
                    Some(AppAction::Enter),
                    false,
                );
                step(
                    &mut host,
                    &mut ctx,
                    server,
                    &mut frame,
                    0.0,
                    Some(AppAction::Cancel),
                    false,
                );
                assert!(host.pending.is_none());
                assert_eq!(
                    host.reference_title()
                        .unwrap()
                        .reference_load()
                        .unwrap()
                        .model()
                        .phase(),
                    crate::adv::save_menu::SaveMenuPhase::Loading
                );
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
                while host
                    .reference_title()
                    .unwrap()
                    .reference_load()
                    .unwrap()
                    .model()
                    .phase()
                    == crate::adv::save_menu::SaveMenuPhase::Loading
                {
                    step(
                        &mut host,
                        &mut ctx,
                        server,
                        &mut frame,
                        0.0,
                        Some(AppAction::Cancel),
                        false,
                    );
                    pixels(&host, &mut ctx, &target);
                    assert!(std::time::Instant::now() < deadline);
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                assert_eq!(
                    host.reference_title()
                        .unwrap()
                        .reference_load()
                        .unwrap()
                        .model()
                        .progress(),
                    0.0
                );
                step(&mut host, &mut ctx, server, &mut frame, 10.0, None, false);
                assert_eq!(
                    host.reference_title()
                        .unwrap()
                        .reference_load()
                        .unwrap()
                        .model()
                        .selected(),
                    99
                );
                assert_eq!(
                    host.reference_title().unwrap().motion.frame().black_alpha,
                    0.5
                );
                if ordinary_close {
                    // Native d97a8 consumes Delete in Load mode without acting.
                    let before_delete = host.reference_save().encode();
                    step(
                        &mut host,
                        &mut ctx,
                        server,
                        &mut frame,
                        0.0,
                        Some(AppAction::DeleteSave),
                        false,
                    );
                    assert_eq!(
                        host.reference_title()
                            .unwrap()
                            .reference_load()
                            .unwrap()
                            .model()
                            .phase(),
                        crate::adv::save_menu::SaveMenuPhase::Active
                    );
                    assert!(
                        !host
                            .reference_title()
                            .unwrap()
                            .reference_load()
                            .unwrap()
                            .reference_dialog_is_open()
                    );
                    assert_eq!(host.reference_save().encode(), before_delete);
                    step(
                        &mut host,
                        &mut ctx,
                        server,
                        &mut frame,
                        7.0,
                        Some(AppAction::Cancel),
                        false,
                    );
                    step(&mut host, &mut ctx, server, &mut frame, 0.0, None, false);
                    assert_eq!(
                        host.reference_title().unwrap().motion.frame().black_alpha,
                        0.5
                    );
                    step(
                        &mut host,
                        &mut ctx,
                        server,
                        &mut frame,
                        10.0,
                        Some(AppAction::AnyDown),
                        false,
                    );
                    assert!(host.reference_title().unwrap().reference_load().is_none());
                    assert_eq!(
                        host.reference_title().unwrap().motion.selected(),
                        selected_title
                    );
                    assert_eq!(
                        host.reference_title().unwrap().motion.frame().black_alpha,
                        0.0
                    );
                    step(&mut host, &mut ctx, server, &mut frame, 0.0, None, false);
                    continue;
                }
                while !host
                    .reference_title()
                    .unwrap()
                    .reference_load()
                    .unwrap()
                    .reference_preview_ready(99)
                {
                    step(&mut host, &mut ctx, server, &mut frame, 0.0, None, false);
                    pixels(&host, &mut ctx, &target);
                    assert!(std::time::Instant::now() < deadline);
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                let shown = pixels(&host, &mut ctx, &target);
                assert!(
                    shown
                        .chunks_exact(4)
                        .filter(|p| p[..3].iter().any(|&c| c > 80))
                        .count()
                        > 10000
                );
                // Empty slot cannot open a confirmation or hand off an entry.
                step(
                    &mut host,
                    &mut ctx,
                    server,
                    &mut frame,
                    20.0,
                    Some(AppAction::Up),
                    false,
                );
                step(
                    &mut host,
                    &mut ctx,
                    server,
                    &mut frame,
                    0.0,
                    Some(AppAction::Enter),
                    false,
                );
                assert_eq!(
                    host.reference_title()
                        .unwrap()
                        .reference_load()
                        .unwrap()
                        .model()
                        .phase(),
                    crate::adv::save_menu::SaveMenuPhase::Active
                );
                assert!(host.pending.is_none());
                step(
                    &mut host,
                    &mut ctx,
                    server,
                    &mut frame,
                    20.0,
                    Some(AppAction::AnyDown),
                    false,
                );
                for yes in [false, true] {
                    step(
                        &mut host,
                        &mut ctx,
                        server,
                        &mut frame,
                        7.0,
                        Some(AppAction::Enter),
                        false,
                    );
                    step(&mut host, &mut ctx, server, &mut frame, 0.0, None, false);
                    step(&mut host, &mut ctx, server, &mut frame, 12.0, None, false);
                    step(&mut host, &mut ctx, server, &mut frame, 0.0, None, false);
                    assert!(
                        host.reference_title()
                            .unwrap()
                            .reference_load()
                            .unwrap()
                            .reference_dialog_is_open()
                    );
                    pixels(&host, &mut ctx, &target);
                    if yes {
                        step(
                            &mut host,
                            &mut ctx,
                            server,
                            &mut frame,
                            0.0,
                            Some(AppAction::Left),
                            false,
                        );
                    }
                    step(
                        &mut host,
                        &mut ctx,
                        server,
                        &mut frame,
                        7.0,
                        Some(AppAction::Enter),
                        false,
                    );
                    step(&mut host, &mut ctx, server, &mut frame, 0.0, None, false);
                    assert!(
                        host.pending.is_none(),
                        "Load waits for dialog final1 and menu close"
                    );
                    step(&mut host, &mut ctx, server, &mut frame, 12.0, None, false);
                    step(&mut host, &mut ctx, server, &mut frame, 5.0, None, false);
                    if !yes {
                        assert_eq!(
                            host.reference_title()
                                .unwrap()
                                .reference_load()
                                .unwrap()
                                .model()
                                .phase(),
                            crate::adv::save_menu::SaveMenuPhase::Active
                        );
                    }
                }
                step(&mut host, &mut ctx, server, &mut frame, 10.0, None, false);
                assert!(
                    matches!(&host.pending, Some(Destination::Load(entry)) if entry.entry.random_seed == manual_entry.random_seed)
                );
                assert!(host.reference_adv().is_none());
                assert_eq!(
                    host.reference_title().unwrap().motion.frame().black_alpha,
                    0.5,
                    "successful Load suppresses parent callbacks"
                );
                assert_eq!(
                    host.reference_title().unwrap().motion.phase(),
                    crate::title::TitlePhase::Child
                );
                assert!(
                    !server
                        .reference_requested_paths()
                        .iter()
                        .any(|p| p == "/adv.txa")
                );
                let before = host.reference_save().encode();
                step(
                    &mut host,
                    &mut ctx,
                    server,
                    &mut frame,
                    0.0,
                    Some(AppAction::Enter),
                    false,
                );
                assert!(matches!(host.scene, Some(Scene::PreparingAdv(_))));
                assert_eq!(host.reference_save().encode(), before);
                assert_eq!(host.reference_audio().system_player.track.id(), track);
                while host.reference_adv().is_none()
                    || !host.reference_adv().unwrap().reference_scene_reveal()
                {
                    step(&mut host, &mut ctx, server, &mut frame, 0.0, None, false);
                    assert!(std::time::Instant::now() < deadline);
                    if let Some(adv) = host.reference_adv() {
                        assert!(adv.saved_scene_load_error().is_none());
                    }
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                assert!(host.reference_adv().unwrap().is_loading_saved_scene());
                step(
                    &mut host,
                    &mut ctx,
                    server,
                    &mut frame,
                    30.0,
                    Some(AppAction::Enter),
                    false,
                );
                while host.reference_adv().unwrap().is_loading_saved_scene() {
                    step(&mut host, &mut ctx, server, &mut frame, 0.0, None, false);
                    assert!(std::time::Instant::now() < deadline);
                }
                pixels(&host, &mut ctx, &target);
                let entry = host.reference_adv().unwrap().reference_entry();
                assert_eq!(
                    (entry.scenario_id, entry.random_seed, entry.save_position),
                    (
                        manual_entry.scenario_id,
                        manual_entry.random_seed,
                        manual_entry.save_position
                    )
                );
                assert_eq!(
                    server
                        .reference_requested_paths()
                        .iter()
                        .filter(|p| *p == "/adv.txa")
                        .count(),
                    1
                );
                assert!(Arc::ptr_eq(
                    host.reference_audio().system_player.bank.as_ref().unwrap(),
                    &host.common.as_ref().unwrap()._sysse
                ));
                eprintln!(
                    "Cold Title Load: original menu/authored slot preview, ordinary close0/1, empty-slot/Delete rejection, default No and final Yes, successful close suppresses callbacks, next-frame lazy ADV restores actual Start's seed{} position{} instead of autosave1234",
                    manual_entry.random_seed, manual_entry.save_position
                );
            }
        }
        if mode == 4 {
            assert!(host.assets.is_none());
            while host.reference_title().unwrap().motion.phase() != crate::title::TitlePhase::Active
            {
                step(&mut host, &mut ctx, server, &mut frame, 10.0, None, false);
                assert!(std::time::Instant::now() < deadline);
            }
            // Native item5 creates Config as a retained Title task child.
            while host.reference_title().unwrap().motion.frame().caption != Some(TitleItem::Config)
            {
                step(
                    &mut host,
                    &mut ctx,
                    server,
                    &mut frame,
                    0.0,
                    Some(AppAction::AnyDown),
                    false,
                );
                pixels(&host, &mut ctx, &target);
                assert!(std::time::Instant::now() < deadline);
            }
            step(&mut host, &mut ctx, server, &mut frame, 30.0, None, false);
            let selected_config = host.reference_title().unwrap().motion.selected();
            for changed in [true, false] {
                step(
                    &mut host,
                    &mut ctx,
                    server,
                    &mut frame,
                    7.0,
                    Some(AppAction::Enter),
                    false,
                );
                step(
                    &mut host,
                    &mut ctx,
                    server,
                    &mut frame,
                    0.0,
                    Some(AppAction::Cancel),
                    false,
                );
                assert!(host.pending.is_none());
                assert!(
                    host.reference_title()
                        .unwrap()
                        .reference_config()
                        .unwrap()
                        .model()
                        .is_none()
                );
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
                while host
                    .reference_title()
                    .unwrap()
                    .reference_config()
                    .unwrap()
                    .model()
                    .is_none()
                {
                    step(
                        &mut host,
                        &mut ctx,
                        server,
                        &mut frame,
                        0.0,
                        Some(AppAction::Cancel),
                        false,
                    );
                    pixels(&host, &mut ctx, &target);
                    assert!(std::time::Instant::now() < deadline);
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                assert_eq!(
                    host.reference_title()
                        .unwrap()
                        .reference_config()
                        .unwrap()
                        .model()
                        .unwrap()
                        .progress(),
                    0.0
                );
                step(&mut host, &mut ctx, server, &mut frame, 10.0, None, false);
                assert_eq!(
                    host.reference_title()
                        .unwrap()
                        .reference_config()
                        .unwrap()
                        .model()
                        .unwrap()
                        .phase(),
                    crate::adv::config::ConfigPhase::Active
                );
                assert_eq!(
                    host.reference_title().unwrap().motion.frame().black_alpha,
                    0.5
                );
                let shown = pixels(&host, &mut ctx, &target);
                assert!(
                    shown
                        .chunks_exact(4)
                        .filter(|p| p[0] > 100 && p[1] > 100 && p[2] > 100)
                        .count()
                        > 10000,
                    "Original Config artwork must be visible"
                );
                let disk_before = std::fs::read(&path).unwrap();
                if changed {
                    step(
                        &mut host,
                        &mut ctx,
                        server,
                        &mut frame,
                        0.0,
                        Some(AppAction::Right),
                        false,
                    );
                    assert_eq!(
                        host.reference_save().settings.v3_sysvol,
                        5,
                        "Config edit publishes to globals on input frame"
                    );
                    assert_eq!(
                        std::fs::read(&path).unwrap(),
                        disk_before,
                        "Edit must wait for Config-close SaveGame"
                    );
                }
                step(
                    &mut host,
                    &mut ctx,
                    server,
                    &mut frame,
                    7.0,
                    Some(AppAction::Cancel),
                    false,
                );
                step(&mut host, &mut ctx, server, &mut frame, 0.0, None, false);
                assert_eq!(
                    host.reference_title()
                        .unwrap()
                        .reference_config()
                        .unwrap()
                        .model()
                        .unwrap()
                        .phase(),
                    crate::adv::config::ConfigPhase::Closing
                );
                assert_eq!(
                    host.reference_title().unwrap().motion.frame().black_alpha,
                    0.5
                );
                let disk = std::fs::read(&path).unwrap();
                if changed {
                    let saved = SaveData::decode(&disk).unwrap();
                    assert_eq!(saved.settings.v3_sysvol, 5);
                    assert!(saved.save_vectors.unlocked_movies.contains(0));
                    assert!(saved.manual_save_slots[99].is_some());
                    assert!(saved.auto_save_slot.is_none());
                } else {
                    assert_eq!(
                        disk, disk_before,
                        "Unchanged Config close has no SaveGame boundary"
                    );
                }
                step(&mut host, &mut ctx, server, &mut frame, 5.0, None, false);
                assert_eq!(
                    host.reference_title().unwrap().motion.frame().black_alpha,
                    0.5,
                    "response0 keeps Title dimmed"
                );
                step(
                    &mut host,
                    &mut ctx,
                    server,
                    &mut frame,
                    5.0,
                    Some(AppAction::AnyDown),
                    false,
                );
                assert!(host.reference_title().unwrap().reference_config().is_none());
                assert_eq!(
                    host.reference_title().unwrap().reference_child_task_count(),
                    1
                );
                assert_eq!(
                    host.reference_title().unwrap().motion.selected(),
                    selected_config
                );
                assert_eq!(
                    host.reference_title().unwrap().motion.frame().black_alpha,
                    0.0
                );
                step(&mut host, &mut ctx, server, &mut frame, 0.0, None, false);
                assert_eq!(
                    host.reference_title().unwrap().reference_child_task_count(),
                    0
                );
            }
            eprintln!(
                "Cold Title Config: actual artwork, retained globals/audio/title, immediate edits, changed-close save including movie unlock/manual slot, unchanged-close no save, response0/1 and next-poll task retirement"
            );
            // Exercise the native retained Title Manual task before Start.
            let globals_before = host.reference_save();
            while host.reference_title().unwrap().motion.frame().caption != Some(TitleItem::Manual)
            {
                step(
                    &mut host,
                    &mut ctx,
                    server,
                    &mut frame,
                    0.0,
                    Some(AppAction::AnyDown),
                    false,
                );
                pixels(&host, &mut ctx, &target);
                assert!(std::time::Instant::now() < deadline);
            }
            let selected = host.reference_title().unwrap().motion.selected();
            // Settle caption/focus before isolating only the black-overlay clock.
            step(&mut host, &mut ctx, server, &mut frame, 30.0, None, false);
            step(
                &mut host,
                &mut ctx,
                server,
                &mut frame,
                7.0,
                Some(AppAction::Enter),
                false,
            );
            assert!(host.reference_title().unwrap().reference_manual().is_none());
            step(
                &mut host,
                &mut ctx,
                server,
                &mut frame,
                0.0,
                Some(AppAction::Cancel),
                false,
            );
            assert!(host.pending.is_none());
            assert!(host.reference_adv().is_none());
            assert!(
                host.reference_title()
                    .unwrap()
                    .reference_manual()
                    .unwrap()
                    .model()
                    .is_none()
            );
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            while host
                .reference_title()
                .unwrap()
                .reference_manual()
                .unwrap()
                .model()
                .is_none()
            {
                step(
                    &mut host,
                    &mut ctx,
                    server,
                    &mut frame,
                    0.0,
                    Some(AppAction::Cancel),
                    false,
                );
                pixels(&host, &mut ctx, &target);
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            assert_eq!(
                host.reference_title()
                    .unwrap()
                    .reference_manual()
                    .unwrap()
                    .model()
                    .unwrap()
                    .progress(),
                0.0
            );
            // At progress0 all Manual content is outside the screen/transparent.
            // The rendered Title behind it must use native truncated127 alpha.
            let full = pixels(&host, &mut ctx, &target);
            // Isolate Title's clock without moving Manual pixels or assets.
            if let Some(Scene::Title(title)) = &mut host.scene {
                title.scene.motion.advance_clocks(Ticks::from_f32(5.0));
            }
            let dimmed = pixels(&host, &mut ctx, &target);
            let worst = full
                .chunks_exact(4)
                .zip(dimmed.chunks_exact(4))
                .flat_map(|(a, b)| {
                    (0..3).map(move |c| {
                        let expected = (f32::from(a[c]) * (128.0 / 255.0)).round() as u8;
                        expected.abs_diff(b[c])
                    })
                })
                .max()
                .unwrap();
            assert!(
                worst <= 2,
                "Manual Title half-black alpha oracle: maxbyteerror{worst}"
            );
            step(&mut host, &mut ctx, server, &mut frame, 5.0, None, false);
            assert_eq!(
                host.reference_title().unwrap().motion.frame().black_alpha,
                0.5
            );
            assert_eq!(host.reference_title().unwrap().motion.selected(), selected);
            step(&mut host, &mut ctx, server, &mut frame, 5.0, None, false);
            assert_eq!(
                host.reference_title()
                    .unwrap()
                    .reference_manual()
                    .unwrap()
                    .model()
                    .unwrap()
                    .phase(),
                crate::adv::manual::ManualPhase::Active
            );
            let shown = pixels(&host, &mut ctx, &target);
            assert!(
                full.iter()
                    .zip(&shown)
                    .filter(|(a, b)| a.abs_diff(**b) > 10)
                    .count()
                    > 10000
            );
            step(
                &mut host,
                &mut ctx,
                server,
                &mut frame,
                15.0,
                Some(AppAction::Right),
                false,
            );
            assert_eq!(
                host.reference_title()
                    .unwrap()
                    .reference_manual()
                    .unwrap()
                    .model()
                    .unwrap()
                    .selected(),
                1
            );
            assert_eq!(host.reference_title().unwrap().motion.selected(), selected);
            step(
                &mut host,
                &mut ctx,
                server,
                &mut frame,
                7.0,
                Some(AppAction::Cancel),
                false,
            );
            step(&mut host, &mut ctx, server, &mut frame, 0.0, None, false);
            assert_eq!(
                host.reference_title()
                    .unwrap()
                    .reference_manual()
                    .unwrap()
                    .model()
                    .unwrap()
                    .phase(),
                crate::adv::manual::ManualPhase::Closing
            );
            assert_eq!(
                host.reference_title().unwrap().motion.frame().black_alpha,
                0.5,
                "response0 retains Title dimming"
            );
            step(
                &mut host,
                &mut ctx,
                server,
                &mut frame,
                10.0,
                Some(AppAction::AnyDown),
                false,
            );
            assert_eq!(
                host.reference_title().unwrap().reference_child_task_count(),
                1,
                "completed native task stays owned until next poll"
            );
            assert!(host.reference_title().unwrap().reference_manual().is_none());
            assert_eq!(
                host.reference_title().unwrap().motion.selected(),
                selected,
                "final close frame gates Title input"
            );
            assert_eq!(
                host.reference_title().unwrap().motion.frame().black_alpha,
                0.0
            );
            assert_eq!(host.reference_audio().system_player.track.id(), track);
            step(&mut host, &mut ctx, server, &mut frame, 0.0, None, false);
            assert_eq!(
                host.reference_title().unwrap().reference_child_task_count(),
                0
            );
            let mut globals_after = host.reference_save();
            globals_after.play_seconds = globals_before.play_seconds;
            assert_eq!(globals_before.encode(), globals_after.encode());
            assert!(
                !server
                    .reference_requested_paths()
                    .iter()
                    .any(|p| p == "/adv.txa")
            );
            while host.reference_title().unwrap().motion.frame().caption != Some(TitleItem::Start) {
                step(
                    &mut host,
                    &mut ctx,
                    server,
                    &mut frame,
                    0.0,
                    Some(AppAction::Up),
                    false,
                );
                pixels(&host, &mut ctx, &target);
                assert!(std::time::Instant::now() < deadline);
            }
            eprintln!(
                "Cold Title Manual: retained title/globals/audio, original artwork, loading/input gates, paging, response0/1 dimming, close-frame input suppression"
            );
            step(
                &mut host,
                &mut ctx,
                server,
                &mut frame,
                37.0,
                Some(AppAction::Enter),
                false,
            );
            step(&mut host, &mut ctx, server, &mut frame, 0.0, None, false);
            assert!(host.reference_title().is_some());
            let prior = pixels(&host, &mut ctx, &target);
            step(&mut host, &mut ctx, server, &mut frame, 0.0, None, false);
            assert!(host.reference_picker().is_some());
            let capture = pixels(&host, &mut ctx, &target);
            assert!(prior.iter().zip(&capture).all(|(a, b)| a.abs_diff(*b) <= 1));
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            while !host.reference_picker().unwrap().is_loaded() {
                step(&mut host, &mut ctx, server, &mut frame, 0.0, None, false);
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            step(
                &mut host,
                &mut ctx,
                server,
                &mut frame,
                60.0,
                Some(AppAction::Enter),
                false,
            );
            assert!(!host.reference_picker().unwrap().accepts_input());
            step(&mut host, &mut ctx, server, &mut frame, 0.0, None, false);
            assert!(host.reference_picker().unwrap().accepts_input());
            step(
                &mut host,
                &mut ctx,
                server,
                &mut frame,
                37.0,
                Some(AppAction::Enter),
                false,
            );
            step(&mut host, &mut ctx, server, &mut frame, 0.0, None, false);
            assert_eq!(host.reference_episode_request(), Some(0));
            let prior = pixels(&host, &mut ctx, &target);
            step(&mut host, &mut ctx, server, &mut frame, 0.0, None, false);
            assert!(host.reference_transition());
            let capture = pixels(&host, &mut ctx, &target);
            assert!(prior.iter().zip(&capture).all(|(a, b)| a.abs_diff(*b) <= 1));
            step(
                &mut host,
                &mut ctx,
                server,
                &mut frame,
                359.5,
                Some(AppAction::Cancel),
                false,
            );
            assert!(host.reference_fresh_entry().is_none());
            step(
                &mut host,
                &mut ctx,
                server,
                &mut frame,
                0.5,
                Some(AppAction::Enter),
                false,
            );
            let entry = host.reference_fresh_entry().unwrap().clone();
            assert_eq!((entry.scenario_id, entry.save_position), (0, 0));
            assert!(entry.route_data.reader().read_choice().is_err());
            assert!(
                pixels(&host, &mut ctx, &target)
                    .chunks_exact(4)
                    .all(|p| p[..3] == [0, 0, 0])
            );
            assert!(
                !server
                    .reference_requested_paths()
                    .iter()
                    .any(|p| p == "/adv.txa")
            );
            step(&mut host, &mut ctx, server, &mut frame, 0.0, None, false);
            assert!(matches!(host.scene, Some(Scene::PreparingAdv(_))));
            while host.reference_adv().is_none() {
                step(&mut host, &mut ctx, server, &mut frame, 0.0, None, false);
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            assert_eq!(
                host.reference_adv()
                    .unwrap()
                    .reference_entry()
                    .save_position,
                0
            );
            let mut revealed = false;
            while host.reference_adv().unwrap().is_loading_saved_scene() {
                step(&mut host, &mut ctx, server, &mut frame, 0.0, None, false);
                assert!(
                    std::time::Instant::now() < deadline,
                    "Fresh real-ROM loader stalled: {:?}",
                    host.reference_adv().unwrap().saved_scene_load_error()
                );
                if host.reference_adv().unwrap().reference_scene_reveal() && !revealed {
                    assert_eq!(
                        host.reference_adv()
                            .unwrap()
                            .reference_entry()
                            .save_position,
                        0
                    );
                    step(&mut host, &mut ctx, server, &mut frame, 30.0, None, false);
                    revealed = true;
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            assert!(revealed);
            assert!(
                host.reference_adv()
                    .unwrap()
                    .saved_scene_load_error()
                    .is_none()
            );
            while host
                .reference_adv()
                .unwrap()
                .reference_entry()
                .save_position
                == 0
            {
                step(&mut host, &mut ctx, server, &mut frame, 0.0, None, false);
                assert!(
                    std::time::Instant::now() < deadline,
                    "Real authored first command stalled"
                );
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            let actual = host.reference_adv().unwrap().reference_entry();
            assert_eq!(
                (actual.scenario_id, actual.random_seed),
                (entry.scenario_id, entry.random_seed)
            );
            assert_eq!(
                server
                    .reference_requested_paths()
                    .iter()
                    .filter(|p| *p == "/adv.txa")
                    .count(),
                1
            );
            authored_manual = Some(actual.clone());
            eprintln!(
                "Cold real-ROM Start: FirstBoot, TitleDemo, Title Start, picker, Episode0, lazy ADV, 30tick reveal, authored script position {}",
                actual.save_position
            );
        }
        drop(host);
        if mode != 0 {
            let disk = SaveData::decode(&std::fs::read(&path).unwrap()).unwrap();
            assert!(disk.save_vectors.unlocked_movies.contains(0), "mode {mode}");
            assert_eq!(disk.settings.v0_bgmvol, 0);
            if mode != 4 {
                assert_eq!(
                    disk.auto_save_slot.as_ref().unwrap().entry.random_seed,
                    1234
                );
            }
            assert_eq!(
                disk.manual_save_slots[99]
                    .as_ref()
                    .unwrap()
                    .entry
                    .save_position,
                if mode == 5 {
                    authored_manual.as_ref().unwrap().save_position
                } else if mode >= 3 {
                    0
                } else {
                    1
                }
            );
            assert_eq!(disk.save_menu_position, 99);
        }
    }
    std::fs::remove_dir_all(folder).unwrap();
}

#[test]
#[ignore = "requires supplied ROM and GPU"]
fn reference_title_load_thumbnail_size_and_mouse_confirmation() {
    use crate::{
        asset::system::{AssetLoadContext, LayeredAssetIo, cache::AssetCache},
        render::PreRenderContext,
    };
    use shin_render::{
        dynamic_buffer::DynamicBuffer,
        pipelines::PipelineStorage,
        resize::{SurfaceResizeSource, ViewportParams},
        shaders::types::{
            buffer::BytesAddress,
            texture::{DepthStencilTarget, TextureSamplerStore},
        },
    };
    crate::init_test_task_pools();
    let adapter =
        shin_tasks::block_on(wgpu::Instance::default().request_adapter(&Default::default()))
            .unwrap();
    let (device, queue) = shin_tasks::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            required_features: adapter.features() & wgpu::Features::SPIRV_SHADER_PASSTHROUGH,
            ..Default::default()
        },
        None,
    ))
    .unwrap();
    let mut io = LayeredAssetIo::new();
    io.try_with_rom(std::env::var("SHIN_REFERENCE_ROM").unwrap())
        .unwrap();
    if let Ok(patch) = std::env::var("SHIN_REFERENCE_PATCH_ROM") {
        io.try_with_rom(patch).unwrap();
    }
    let server = Arc::new(AssetServer::new(
        io.into(),
        AssetLoadContext {
            wgpu_device: device.clone(),
            wgpu_queue: queue.clone(),
            bustup_cache: AssetCache::new(),
            picture_cache: Default::default(),
            font_cache: Default::default(),
        },
    ));
    let assets = shin_tasks::block_on(AdvAssets::load(&server)).unwrap();
    let info = assets
        .scenario
        .info_tables()
        .movie_info
        .first()
        .expect("Native startup movie0");
    eprintln!("Native startup movie: {info:?}");
    let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
        1920, 1080,
    )));
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Startup reference depth"),
        size: wgpu::Extent3d {
            width: 1920,
            height: 1080,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: shin_render::DEPTH_STENCIL_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let depth_view = depth.create_view(&Default::default());
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Startup reference pixels"),
        size: depth.size(),
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: shin_render::TEXTURE_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let samplers = TextureSamplerStore::new(&device);
    let mut pipelines = PipelineStorage::new(device.clone(), shin_render::TEXTURE_FORMAT);
    let mut dynamic = DynamicBuffer::new(device.clone(), BytesAddress::new(1024 * 1024));
    let mut encoder = device.create_command_encoder(&Default::default());
    let mut ctx = PreRenderContext {
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

    fn step<'a>(
        host: &mut SceneHost,
        ctx: &mut PreRenderContext<'a, '_, '_, '_>,
        server: &'a Arc<AssetServer>,
        frame: &mut FrameId,
        delta: f32,
        action: Option<AppAction>,
        point: glam::Vec2,
        held: bool,
        extent: (u16, u16),
    ) {
        let mut input = EnumMap::default();
        if let Some(action) = action {
            input[action] = ActionState {
                is_clicked: true,
                is_clicked_or_repeated: true,
                ..Default::default()
            };
        }
        input[AppAction::PointerClick].is_held = held;
        frame.advance();
        let physical = point * glam::vec2(extent.0 as f32 / 1920.0, extent.1 as f32 / 1080.0);
        host.update(
            &mut UpdateContext {
                frame_started_at: std::time::Instant::now(),
                frame_id: *frame,
                delta_ticks: Ticks::from_f32(delta),
                asset_server: server,
                pre_render: ctx,
            },
            input,
            physical,
            point,
            extent,
            0.0,
            1.0 / 60.0,
        )
        .unwrap();
    }
    for extent in [(1280, 720), (3840, 2160)] {
        let mut save = SaveData::default();
        save.settings.v0_bgmvol = 0;
        save.settings.v3_sysvol = 0;
        save.manual_save_slots[0] = Some(shin_core::format::save::GameData::from_entry(
            crate::adv::save_menu_screen::reference_authored_thumbnail_entry(&assets.scenario),
        ));
        let globals = GlobalData::open(None, Some(save)).unwrap();
        let mut title = TitleScene::new(server.clone(), TitleSetup::from_save(globals.save(), 0));
        title.resize(extent);
        let audio = GlobalAudio::new(Arc::new(shin_audio::AudioManager::new_for_tests()));
        let mut host = SceneHost::new_startup(
            GlobalData::default(),
            GlobalAudio::new(audio.manager.clone()),
            assets.clone(),
            server.clone(),
        );
        host.scene = Some(Scene::Title(TitleRuntime {
            scene: title,
            globals,
            audio,
            extent,
        }));
        let mut frame = FrameId::default();
        let outside = glam::vec2(1900.0, 1050.0);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(40);
        while host.reference_title().unwrap().motion.phase() != crate::title::TitlePhase::Active {
            step(
                &mut host, &mut ctx, &server, &mut frame, 10.0, None, outside, false, extent,
            );
            assert!(std::time::Instant::now() < deadline);
        }
        step(
            &mut host,
            &mut ctx,
            &server,
            &mut frame,
            0.0,
            Some(AppAction::AnyDown),
            outside,
            false,
            extent,
        );
        assert_eq!(
            host.reference_title().unwrap().motion.frame().caption,
            Some(TitleItem::Load)
        );
        step(
            &mut host,
            &mut ctx,
            &server,
            &mut frame,
            7.0,
            Some(AppAction::Enter),
            outside,
            false,
            extent,
        );
        while host
            .reference_title()
            .unwrap()
            .reference_load()
            .is_none_or(|screen| !screen.reference_preview_ready(0))
        {
            step(
                &mut host, &mut ctx, &server, &mut frame, 10.0, None, outside, false, extent,
            );
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert_eq!(
            host.reference_title()
                .unwrap()
                .reference_load()
                .unwrap()
                .reference_preview_dimensions(0),
            Some((288, 162))
        );
        // Sample real title composition, not merely a ready flag.
        let view = target.create_view(&Default::default());
        {
            let mut pass = ctx.begin_pass(
                shin_render::shaders::types::texture::TextureTarget {
                    kind: shin_render::shaders::types::texture::TextureTargetKind::Screen,
                    view: &view,
                },
                Some(ctx.depth_stencil),
                "Main menu load thumbnails",
            );
            host.render(&mut pass);
        }
        let pixels = crate::layer::user::readback_test_texture(&mut ctx, &target);
        std::fs::write(format!("/tmp/shin-title-load-{}.rgba", extent.0), &pixels).unwrap();
        let colors: std::collections::BTreeSet<_> = (195..353)
            .flat_map(|y| (168..452).map(move |x| (y * 1920 + x) * 4))
            .map(|at| [pixels[at], pixels[at + 1], pixels[at + 2]])
            .collect();
        assert!(
            colors.len() > 64,
            "Main menu thumbnail lost scene detail: {} colors",
            colors.len()
        );

        for yes in [false, true] {
            step(
                &mut host,
                &mut ctx,
                &server,
                &mut frame,
                7.0,
                Some(AppAction::Enter),
                outside,
                false,
                extent,
            );
            step(
                &mut host, &mut ctx, &server, &mut frame, 0.0, None, outside, false, extent,
            );
            step(
                &mut host, &mut ctx, &server, &mut frame, 12.0, None, outside, false, extent,
            );
            step(
                &mut host, &mut ctx, &server, &mut frame, 0.0, None, outside, false, extent,
            );
            let screen = host.reference_title().unwrap().reference_load().unwrap();
            assert!(screen.reference_dialog_is_open());
            let point = screen.reference_dialog_button_center(if yes { 0 } else { 1 });
            step(
                &mut host, &mut ctx, &server, &mut frame, 0.0, None, point, true, extent,
            );
            step(
                &mut host, &mut ctx, &server, &mut frame, 7.0, None, point, false, extent,
            );
            step(
                &mut host, &mut ctx, &server, &mut frame, 0.0, None, point, false, extent,
            );
            step(
                &mut host, &mut ctx, &server, &mut frame, 12.0, None, point, false, extent,
            );
            step(
                &mut host, &mut ctx, &server, &mut frame, 5.0, None, point, false, extent,
            );
            if !yes {
                assert!(
                    !host
                        .reference_title()
                        .unwrap()
                        .reference_load()
                        .unwrap()
                        .reference_dialog_is_open()
                );
                assert_eq!(
                    host.reference_title()
                        .unwrap()
                        .reference_load()
                        .unwrap()
                        .model()
                        .phase(),
                    crate::adv::save_menu::SaveMenuPhase::Active
                );
                assert!(host.pending.is_none());
            } else {
                step(
                    &mut host, &mut ctx, &server, &mut frame, 10.0, None, point, false, extent,
                );
                assert!(
                    matches!(host.pending,Some(Destination::Load(ref entry)) if entry.entry.random_seed==42)
                );
            }
        }
        eprintln!(
            "Main menu load: extent{extent:?}, 288x162 thumbnail, mouse No/Yes and final load handoff passed"
        );
    }
}
