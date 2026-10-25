//! Actual scene-host gallery construction/publication failure and repair.
use super::*;
use crate::{
    adv::VmState,
    asset::system::{AssetLoadContext, LayeredAssetIo, cache::AssetCache, supplied_payload},
};
use shin_core::{
    format::save::SaveData,
    primitives::update::FrameId,
    time::Ticks,
};
use shin_render::{
    dynamic_buffer::DynamicBuffer,
    pipelines::PipelineStorage,
    resize::{SurfaceResizeSource, ViewportParams},
    shaders::types::{
        buffer::BytesAddress,
        texture::{DepthStencilTarget, TextureSamplerStore, TextureTarget, TextureTargetKind},
    },
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

#[test]
#[ignore = "requires supplied ROM,Metal and the default audio output;temporary assets/progress only"]
fn reference_gallery_factory_and_artwork_failure_recovers_title() {
    std::thread::Builder::new()
        .name("gallery-recovery".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(run)
        .unwrap()
        .join()
        .unwrap();
}
fn globals(host: &SceneHost) -> &GlobalData {
    match host.scene.as_ref().unwrap() {
        Scene::Title(t) => &t.globals,
        Scene::Cg(t) => &t.globals,
        Scene::Music(t) => &t.globals,
        Scene::Movie(t) => &t.globals,
        _ => panic!("Unexpected gallery host"),
    }
}
fn snapshot(host: &SceneHost) -> Vec<u8> {
    let mut save = globals(host).save().clone();
    save.play_seconds = 0; // Normal host updates continue elapsed play time.
    save.encode()
}
fn audio(host: &SceneHost) -> &GlobalAudio {
    match host.scene.as_ref().unwrap() {
        Scene::Title(t) => &t.audio,
        Scene::Cg(t) => &t.audio,
        Scene::Music(t) => &t.audio,
        Scene::Movie(t) => &t.audio,
        _ => panic!("Unexpected gallery host"),
    }
}
fn title_active(host: &SceneHost) -> bool {
    matches!(&host.scene, Some(Scene::Title(t)) if t.scene.reference_active())
}
fn gallery_active(host: &SceneHost) -> bool {
    match &host.scene {
        Some(Scene::Cg(t)) => t.scene.reference_state().2,
        Some(Scene::Music(t)) => t.scene.reference_state().2,
        Some(Scene::Movie(t)) => t.scene.reference_state().2,
        _ => false,
    }
}
fn step<'a>(
    host: &mut SceneHost,
    server: &'a Arc<AssetServer>,
    ctx: &mut crate::render::PreRenderContext<'a, '_, '_, '_>,
    frame: &mut FrameId,
    delta: f32,
    action: Option<AppAction>,
) {
    let mut input = EnumMap::default();
    if let Some(action) = action {
        input[action] = ActionState {
            is_clicked: true,
            is_clicked_or_repeated: true,
            ..Default::default()
        };
    }
    frame.advance();
    let result = host.update(
        &mut UpdateContext {
            frame_started_at: Instant::now(),
            frame_id: *frame,
            delta_ticks: Ticks::from_f32(delta),
            asset_server: server,
            pre_render: ctx,
        },
        input,
        glam::vec2(1900.0, 1050.0),
        glam::vec2(1900.0, 1050.0),
        (1920, 1080),
        0.0,
        0.0,
    );
    assert!(
        result.is_ok(),
        "Gallery errors must recover locally: {result:?};scene_none={},host_error={:?}",
        host.scene.is_none(),
        host.error
    );
    assert!(host.error.is_none());
}
fn run() {
    crate::init_test_task_pools();
    let output = PathBuf::from(std::env::var_os("SHIN_REFERENCE_GALLERY_FAILURE_OUTPUT").unwrap());
    let directory = output.join("overrides");
    std::fs::create_dir_all(&directory).unwrap();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::METAL,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = shin_tasks::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) = shin_tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: if adapter.get_info().backend == wgpu::Backend::Vulkan {
            adapter.features() & wgpu::Features::PASSTHROUGH_SHADERS
        } else {
            wgpu::Features::empty()
        },
        ..Default::default()
    }))
    .unwrap();
    let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
        1920, 1080,
    )));
    let descriptor = |label, format| wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: 1920,
            height: 1080,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    };
    let depth = device.create_texture(&descriptor(
        "Gallery recovery depth",
        shin_render::DEPTH_STENCIL_FORMAT,
    ));
    let depth_view = depth.create_view(&Default::default());
    let target = device.create_texture(&descriptor(
        "Gallery recovery color",
        wgpu::TextureFormat::Rgba8Unorm,
    ));
    let view = target.create_view(&Default::default());
    let samplers = TextureSamplerStore::new(&device);
    let mut pipelines = PipelineStorage::new(device.clone(), wgpu::TextureFormat::Rgba8Unorm);
    let mut dynamic = DynamicBuffer::new(device.clone(), BytesAddress::new(1024 * 1024));
    let mut encoder = device.create_command_encoder(&Default::default());
    let draw = |host: &SceneHost, ctx: &mut crate::render::PreRenderContext| {
        {
            let mut pass = ctx.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &view,
                },
                Some(ctx.depth_stencil),
                "Gallery recovery",
            );
            host.render(&mut pass);
        }
        crate::layer::user::readback_test_texture(ctx, &target)
    };
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let mut cases = Vec::new();
    for (profile, item, path, name) in [
        (
            "cg-factory-system-short",
            TitleItem::Cg,
            "/systex.txa",
            None,
        ),
        (
            "cg-factory-scroll-name",
            TitleItem::Cg,
            "/systex.txa",
            Some("scroll"),
        ),
        ("cg-atlas-short", TitleItem::Cg, "/cgmode.txa", None),
        ("cg-bg-name", TitleItem::Cg, "/cgmode.txa", Some("bg")),
        (
            "cg-thumbnail-name",
            TitleItem::Cg,
            "/cgmode.txa",
            Some("first-thumbnail"),
        ),
        ("cg-mask-short", TitleItem::Cg, "/default.msk", None),
        ("cg-num-partial", TitleItem::Cg, "/cgmode.txa", Some("num")),
        ("cg-unused-num", TitleItem::Cg, "/cgmode.txa", Some("num")),
        ("cg-unused-logo", TitleItem::Cg, "/cgmode.txa", Some("logo")),
        (
            "music-preload-system-short",
            TitleItem::Bgm,
            "/systex.txa",
            None,
        ),
        (
            "music-factory-scroll-name",
            TitleItem::Bgm,
            "/systex.txa",
            Some("scroll"),
        ),
        ("music-atlas-short", TitleItem::Bgm, "/bgmmode.txa", None),
        ("music-bg-name", TitleItem::Bgm, "/bgmmode.txa", Some("bg")),
        (
            "music-title0-name",
            TitleItem::Bgm,
            "/bgmmode.txa",
            Some("title0"),
        ),
        (
            "music-unused-title1",
            TitleItem::Bgm,
            "/bgmmode.txa",
            Some("title1"),
        ),
        (
            "music-menu-on-playable",
            TitleItem::Bgm,
            "/bgmmode.txa",
            Some("menu_on"),
        ),
        (
            "music-unused-menu-on",
            TitleItem::Bgm,
            "/bgmmode.txa",
            Some("menu_on"),
        ),
        (
            "movie-atlas-short",
            TitleItem::Movie,
            "/moviemode.txa",
            None,
        ),
        (
            "movie-bg-name",
            TitleItem::Movie,
            "/moviemode.txa",
            Some("bg"),
        ),
        (
            "movie-thumbnail-name",
            TitleItem::Movie,
            "/moviemode.txa",
            Some("mv01"),
        ),
        (
            "movie-unused-logo",
            TitleItem::Movie,
            "/moviemode.txa",
            Some("logo"),
        ),
    ] {
        let mut io = LayeredAssetIo::new();
        io.try_with_dir(&directory).unwrap();
        if let Some(patch) = std::env::var_os("SHIN_REFERENCE_PATCH_ROM") {
            io.try_with_rom(patch).unwrap();
        }
        io.try_with_rom(std::env::var_os("SHIN_REFERENCE_ROM").unwrap())
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
        let mut ctx = crate::render::PreRenderContext {
            breakup_texture: None,
            device: &device,
            queue: &queue,
            resize_source: &resize,
            sampler_store: &samplers,
            depth_stencil: DepthStencilTarget { view: &depth_view },
            pipeline_storage: &mut pipelines,
            dynamic_buffer: &mut dynamic,
            encoder: &mut encoder,
        };
        let assets = shin_tasks::block_on(AdvAssets::load(&server)).unwrap();
        let mut save = SaveData::default();
        save.persist_data.set(0, 1);
        save.save_menu_position = 91;
        save.manual_save_slots[91] = Some(VmState::for_scenario(0, 42).snapshot_game_data());
        save.settings.v0_bgmvol = 37;
        save.settings.v3_sysvol = 0;
        if profile == "music-menu-on-playable" {
            save.save_vectors.unlocked_bgms.add(
                assets.scenario.info_tables().music_box_info[0]
                    .bgm_id
                    .into_i32() as usize,
            );
        }
        if profile == "cg-unused-logo" {
            for row in &assets.scenario.info_tables().picture_box_info {
                for id in &row.picture_ids.0 {
                    save.save_vectors.unlocked_cgs.add(id.into_i32() as usize);
                }
            }
        }
        if profile == "cg-num-partial" {
            let row = assets
                .scenario
                .info_tables()
                .picture_box_info
                .iter()
                .find(|row| row.picture_ids.0.len() > 1)
                .unwrap();
            save.save_vectors
                .unlocked_cgs
                .add(row.picture_ids.0[0].into_i32() as usize);
        }
        if profile == "movie-unused-logo" {
            for id in 0..=10 {
                save.save_vectors.unlocked_movies.add(id);
            }
        }
        let disk = output.join(format!("progress-{profile}.dat"));
        let disk_bytes = save.encode();
        std::fs::write(&disk, &disk_bytes).unwrap();
        let global = GlobalData::open(Some(disk.clone()), None).unwrap();
        let setup = TitleSetup::from_save(global.save(), item as i32);
        let mut host = SceneHost {
            scene: Some(Scene::Title(TitleRuntime {
                scene: TitleScene::new(server.clone(), setup),
                globals: global,
                audio: GlobalAudio::new(manager.clone()),
                extent: (1920, 1080),
            })),
            pending: None,
            assets: Some(assets),
            common: None,
            server: server.clone(),
            pointer: Default::default(),
            error: None,
            restoring_from_title: false,
            viewer_axes: glam::Vec2::ZERO,
        };
        let mut frame = FrameId::default();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !title_active(&host) {
            assert!(Instant::now() < deadline);
            step(&mut host, &server, &mut ctx, &mut frame, 12.0, None);
            std::thread::sleep(Duration::from_millis(1));
        }
        let before = snapshot(&host);
        let track = audio(&host).bgm_player.reference_source().2;
        let bytes = supplied_payload(path).2;
        let file = directory.join(&path[1..]);
        let mut bad = bytes.clone();
        if let Some(name) = name {
            let actual = if name == "first-thumbnail" {
                host.assets
                    .as_ref()
                    .unwrap()
                    .scenario
                    .info_tables()
                    .picture_box_info[0]
                    .name
                    .as_str()
            } else {
                name
            };
            let needle = format!("{actual}\0");
            let at = bad
                .windows(needle.len())
                .position(|w| w == needle.as_bytes())
                .unwrap();
            bad[at + needle.len() - 2] = b'x';
        } else {
            bad.truncate(15);
        }
        std::fs::write(&file, &bad).unwrap();
        host.pending = Some(Destination::Action(item));
        step(&mut host, &server, &mut ctx, &mut frame, 0.0, None);
        let positive = profile.contains("unused");
        let deadline = Instant::now() + Duration::from_secs(10);
        if positive {
            while !gallery_active(&host) {
                assert!(
                    Instant::now() < deadline,
                    "{profile}:unused artwork must remain accepted"
                );
                assert!(
                    !matches!(host.scene, Some(Scene::Title(_))),
                    "Unused artwork rejected"
                );
                step(&mut host, &server, &mut ctx, &mut frame, 12.0, None);
                std::thread::sleep(Duration::from_millis(1));
            }
            draw(&host, &mut ctx);
        } else {
            while !matches!(host.scene, Some(Scene::Title(_))) && host.pending.is_none() {
                assert!(
                    Instant::now() < deadline,
                    "{profile}:failed gallery did not return to title"
                );
                step(&mut host, &server, &mut ctx, &mut frame, 0.0, None);
                draw(&host, &mut ctx);
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        assert_eq!(snapshot(&host), before);
        assert_eq!(audio(&host).bgm_player.reference_source().2, track);
        assert!(Arc::ptr_eq(&audio(&host).manager, &manager));
        assert_eq!(std::fs::read(&disk).unwrap(), disk_bytes);
        // Repair before queued root reconstruction; mask is also a title dependency.
        std::fs::write(&file, &bytes).unwrap();
        if positive {
            step(
                &mut host,
                &server,
                &mut ctx,
                &mut frame,
                7.0,
                Some(AppAction::Cancel),
            );
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while !title_active(&host) {
            assert!(Instant::now() < deadline, "{profile}:title did not recover");
            step(&mut host, &server, &mut ctx, &mut frame, 12.0, None);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(snapshot(&host), before);
        assert_eq!(std::fs::read(&disk).unwrap(), disk_bytes);
        if !positive {
            host.pending = Some(Destination::Action(item));
            step(&mut host, &server, &mut ctx, &mut frame, 0.0, None);
            let deadline = Instant::now() + Duration::from_secs(10);
            while !gallery_active(&host) {
                assert!(
                    Instant::now() < deadline,
                    "{profile}:repaired gallery did not open"
                );
                step(&mut host, &server, &mut ctx, &mut frame, 12.0, None);
                std::thread::sleep(Duration::from_millis(1));
            }
            draw(&host, &mut ctx);
            step(
                &mut host,
                &server,
                &mut ctx,
                &mut frame,
                7.0,
                Some(AppAction::Cancel),
            );
            let deadline = Instant::now() + Duration::from_secs(10);
            while !title_active(&host) {
                assert!(Instant::now() < deadline);
                step(&mut host, &server, &mut ctx, &mut frame, 12.0, None);
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        assert_eq!(snapshot(&host), before);
        assert_eq!(std::fs::read(&disk).unwrap(), disk_bytes);
        draw(&host, &mut ctx);
        std::fs::remove_file(file).unwrap();
        cases.push(serde_json::json!({"profile":profile,"asset":path,"unused_artwork_accepted":positive,
            "same_host_server_repair_retry":true,"global_progress_preserved_except_elapsed_time":true,
            "audio_manager_and_bgm_focus_track_retained":true,"disk_progress_preserved":true,"host_error_latch_avoided":true}));
    }
    std::fs::write(output.join("gallery-recovery.json"),serde_json::to_vec_pretty(&serde_json::json!({
        "cases":cases,"cases_passed":cases.len(),"patch_mounted":std::env::var_os("SHIN_REFERENCE_PATCH_ROM").is_some(),
        "window_created":false,"working_progress_written":false,"audio_device":"default output"
    })).unwrap()).unwrap();
}
