//! Actual scene-host bustup construction/publication failure and repair.
use super::*;
use crate::{
    adv::VmState,
    asset::system::{AssetLoadContext, LayeredAssetIo, cache::AssetCache, supplied_payload},
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
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

#[test]
#[ignore = "requires supplied ROM,Metal and exact Loopback Audio;temporary assets/progress only"]
fn reference_bustup_factory_and_artwork_failure_recovers_title() {
    std::thread::Builder::new()
        .name("bustup-recovery".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(run)
        .unwrap()
        .join()
        .unwrap();
}
fn globals(host: &SceneHost) -> &GlobalData {
    match host.scene.as_ref().unwrap() {
        Scene::Title(t) => &t.globals,
        Scene::Bustup(t) => &t.globals,
        _ => panic!("Unexpected bustup host"),
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
        Scene::Bustup(t) => &t.audio,
        _ => panic!("Unexpected bustup host"),
    }
}
fn title_active(host: &SceneHost) -> bool {
    matches!(&host.scene, Some(Scene::Title(t)) if t.scene.reference_active())
}
fn bustup_active(host: &SceneHost) -> bool {
    matches!(&host.scene, Some(Scene::Bustup(t)) if t.scene.reference_state().1 && !t.scene.reference_load_blocked())
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
        "Bustup errors must recover locally: {result:?};scene_none={},host_error={:?}",
        host.scene.is_none(),
        host.error
    );
    assert!(host.error.is_none());
    // Submit preparation on every simulated frame, as the live application does.
    // Waiting without submission can exhaust Metal's command-buffer queue.
    let mut upload = ctx.device.create_command_encoder(&Default::default());
    ctx.dynamic_buffer.finish(&mut upload);
    let encoder = std::mem::replace(
        ctx.encoder,
        ctx.device.create_command_encoder(&Default::default()),
    );
    ctx.queue.submit([upload.finish(), encoder.finish()]);
    ctx.device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("GPU work must complete before readback or retirement");
}
fn run() {
    crate::init_test_task_pools();
    let output = PathBuf::from(std::env::var_os("SHIN_REFERENCE_BUSTUP_FAILURE_OUTPUT").unwrap());
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
        "Bustup recovery depth",
        shin_render::DEPTH_STENCIL_FORMAT,
    ));
    let depth_view = depth.create_view(&Default::default());
    let target = device.create_texture(&descriptor(
        "Bustup recovery color",
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
                "Bustup recovery",
            );
            host.render(&mut pass);
        }
        let pixels = crate::layer::user::readback_test_texture(ctx, &target);
        ctx.dynamic_buffer.recall();
        pixels
    };
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let profiles = [
        "atlas-short",
        "icon-name",
        "num-name",
        "first-character-name",
        "last-character-name",
        "last-background-name",
        "mask-short",
        "initial-picture-short",
        "initial-bustup-short",
        "replace-bustup-short",
        "replace-primary-short",
        "replace-secondary-short",
        "unused-dressicon",
        "unused-sizeicon",
    ];
    let mut cases = Vec::new();
    for profile in profiles {
        if let Ok(only) = std::env::var("SHIN_REFERENCE_BUSTUP_ONLY") {
            if profile != only {
                continue;
            }
        }
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
        let (backgrounds, characters, _) =
            crate::title::BustupScene::reference_catalog_paths(&assets.scenario);
        let paired_index = backgrounds
            .iter()
            .enumerate()
            .skip(1)
            .find(|(_, bg)| bg.1.is_some())
            .map(|(i, _)| i);
        let dynamic = profile.starts_with("replace-");
        let positive = profile.starts_with("unused-");
        let (path, name, target_index) = match profile {
            "atlas-short" => ("/bupmode.txa".to_owned(), None, 0),
            "icon-name" => ("/bupmode.txa".to_owned(), Some("icon".to_owned()), 0),
            "num-name" => ("/bupmode.txa".to_owned(), Some("num".to_owned()), 0),
            "first-character-name" => ("/bupmode.txa".to_owned(), Some("char01".to_owned()), 0),
            "last-character-name" => (
                "/bupmode.txa".to_owned(),
                Some(format!("char{:02}", characters.len())),
                0,
            ),
            "last-background-name" => (
                "/bupmode.txa".to_owned(),
                Some(format!("bg{:03}", backgrounds.len())),
                0,
            ),
            "mask-short" => ("/default.msk".to_owned(), None, 0),
            "initial-picture-short" => (backgrounds[0].0.clone(), None, 0),
            "initial-bustup-short" => (characters[0].clone(), None, 0),
            "replace-bustup-short" => {
                assert_ne!(characters[0], characters[1]);
                (characters[1].clone(), None, 1)
            }
            "replace-primary-short" => {
                assert_ne!(backgrounds[0].0, backgrounds[1].0);
                (backgrounds[1].0.clone(), None, 1)
            }
            "replace-secondary-short" => {
                let i = paired_index.expect("Reachable paired replacement background");
                assert_ne!(backgrounds[0].1.as_deref(), backgrounds[i].1.as_deref());
                (backgrounds[i].1.clone().unwrap(), None, i)
            }
            "unused-dressicon" => ("/bupmode.txa".to_owned(), Some("dressicon".to_owned()), 0),
            "unused-sizeicon" => ("/bupmode.txa".to_owned(), Some("sizeicon".to_owned()), 0),
            _ => unreachable!(),
        };
        let disk = output.join(format!("progress-{profile}.dat"));
        let disk_bytes = save.encode();
        std::fs::write(&disk, &disk_bytes).unwrap();
        let global = GlobalData::open(Some(disk.clone()), None).unwrap();
        let setup = TitleSetup::from_save(global.save(), TitleItem::Bustup as i32);
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

        if dynamic {
            host.pending = Some(Destination::Action(TitleItem::Bustup));
            step(&mut host, &server, &mut ctx, &mut frame, 0.0, None);
            wait_active(&mut host, &server, &mut ctx, &mut frame);
            // Navigate the real controller to just before the requested resource.
            if profile != "replace-bustup-short" {
                for _ in 0..4 {
                    step(
                        &mut host,
                        &server,
                        &mut ctx,
                        &mut frame,
                        12.0,
                        Some(AppAction::AnyDown),
                    );
                }
                for _ in 1..target_index {
                    step(
                        &mut host,
                        &server,
                        &mut ctx,
                        &mut frame,
                        12.0,
                        Some(AppAction::Right),
                    );
                    wait_active(&mut host, &server, &mut ctx, &mut frame);
                }
            }
        }
        let previous_resources = match &host.scene {
            Some(Scene::Bustup(t)) => Some(t.scene.reference_resource_state()),
            _ => None,
        };
        let bytes = supplied_payload(&path).2;
        let file = directory.join(&path[1..]);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let mut bad = bytes.clone();
        if let Some(name) = &name {
            rename_texture(&mut bad, &name);
        } else {
            bad.truncate(15);
        }
        std::fs::write(&file, &bad).unwrap();
        if dynamic {
            step(
                &mut host,
                &server,
                &mut ctx,
                &mut frame,
                0.0,
                Some(AppAction::Right),
            );
            assert!(
                matches!(&host.scene, Some(Scene::Bustup(t)) if t.scene.reference_load_blocked())
            );
            let deadline = Instant::now() + Duration::from_secs(10);
            while matches!(&host.scene, Some(Scene::Bustup(t)) if t.scene.reference_load_blocked())
            {
                assert!(
                    Instant::now() < deadline,
                    "{profile}:replacement did not retire"
                );
                step(&mut host, &server, &mut ctx, &mut frame, 0.0, None);
                draw(&host, &mut ctx);
                std::thread::sleep(Duration::from_millis(1));
            }
            let Some(Scene::Bustup(t)) = &host.scene else {
                panic!("Replacement must retain the gallery")
            };
            let (failures, identities) = t.scene.reference_resource_state();
            let (old_failures, old_identities) = previous_resources.unwrap();
            assert_eq!(failures, old_failures + 1);
            assert_eq!(
                identities, old_identities,
                "No replacement slot publishes on failure"
            );
            assert!(
                !t.scene.reference_state().1,
                "Completion frame still blocks input"
            );
            step(&mut host, &server, &mut ctx, &mut frame, 0.0, None);
            assert!(bustup_active(&host), "Following frame re-arms input");
            assert_eq!(snapshot(&host), before);
            assert_eq!(std::fs::read(&disk).unwrap(), disk_bytes);
            std::fs::write(&file, &bytes).unwrap();
            // Move back then reselect using the actual menu, not a direct worker call.
            step(
                &mut host,
                &server,
                &mut ctx,
                &mut frame,
                12.0,
                Some(AppAction::Left),
            );
            wait_active(&mut host, &server, &mut ctx, &mut frame);
            let old = match &host.scene {
                Some(Scene::Bustup(t)) => t.scene.reference_resource_state().1,
                _ => unreachable!(),
            };
            step(
                &mut host,
                &server,
                &mut ctx,
                &mut frame,
                12.0,
                Some(AppAction::Right),
            );
            wait_active(&mut host, &server, &mut ctx, &mut frame);
            let current = match &host.scene {
                Some(Scene::Bustup(t)) => t.scene.reference_resource_state().1,
                _ => unreachable!(),
            };
            if profile == "replace-bustup-short" {
                assert_eq!(current[..2], old[..2]);
                assert_ne!(current[2], old[2]);
            } else {
                assert_ne!(current[..2], old[..2]);
                assert_eq!(current[2], old[2]);
            }
            draw(&host, &mut ctx);
        } else if positive {
            host.pending = Some(Destination::Action(TitleItem::Bustup));
            step(&mut host, &server, &mut ctx, &mut frame, 0.0, None);
            wait_active(&mut host, &server, &mut ctx, &mut frame);
            draw(&host, &mut ctx);
            std::fs::write(&file, &bytes).unwrap();
        } else {
            host.pending = Some(Destination::Action(TitleItem::Bustup));
            step(&mut host, &server, &mut ctx, &mut frame, 0.0, None);
            let deadline = Instant::now() + Duration::from_secs(10);
            while !matches!(host.scene, Some(Scene::Title(_))) && host.pending.is_none() {
                assert!(
                    Instant::now() < deadline,
                    "{profile}:initial failure did not return to title"
                );
                step(&mut host, &server, &mut ctx, &mut frame, 0.0, None);
                draw(&host, &mut ctx);
                std::thread::sleep(Duration::from_millis(1));
            }
            assert_eq!(snapshot(&host), before);
            assert_eq!(audio(&host).bgm_player.reference_source().2, track);
            // Shared title mask is repaired before queued title reconstruction.
            std::fs::write(&file, &bytes).unwrap();
            wait_title(&mut host, &server, &mut ctx, &mut frame);
            host.pending = Some(Destination::Action(TitleItem::Bustup));
            step(&mut host, &server, &mut ctx, &mut frame, 0.0, None);
            wait_active(&mut host, &server, &mut ctx, &mut frame);
            draw(&host, &mut ctx);
        }
        assert_eq!(snapshot(&host), before);
        assert!(Arc::ptr_eq(&audio(&host).manager, &manager));
        assert_eq!(std::fs::read(&disk).unwrap(), disk_bytes);
        step(
            &mut host,
            &server,
            &mut ctx,
            &mut frame,
            7.0,
            Some(AppAction::Cancel),
        );
        wait_title(&mut host, &server, &mut ctx, &mut frame);
        assert_eq!(snapshot(&host), before);
        assert_eq!(std::fs::read(&disk).unwrap(), disk_bytes);
        draw(&host, &mut ctx);
        std::fs::remove_file(file).unwrap();
        cases.push(
            serde_json::json!({"profile":profile,"asset":path,"required_name":name,
            "dynamic":dynamic,"unused_artwork_accepted":positive,"same_host_server_repair_retry":true,
            "global_progress_preserved_except_elapsed_time":true,"disk_progress_preserved":true,
            "audio_manager_retained":true,"host_error_latch_avoided":true,
            "previous_slots_retained_on_replacement_failure":dynamic,
            "paired_background_publication_atomic_on_failure":profile == "replace-secondary-short",
            "actual_controller_reselection_and_cancel":true}),
        );
    }
    std::fs::write(output.join("bustup-recovery.json"),serde_json::to_vec_pretty(&serde_json::json!({
        "cases":cases,"cases_passed":cases.len(),"patch_mounted":std::env::var_os("SHIN_REFERENCE_PATCH_ROM").is_some(),
        "window_created":false,"working_progress_written":false,"audio_device":"exact Loopback Audio"
    })).unwrap()).unwrap();
}
fn wait_active<'a>(
    host: &mut SceneHost,
    server: &'a Arc<AssetServer>,
    ctx: &mut crate::render::PreRenderContext<'a, '_, '_, '_>,
    frame: &mut FrameId,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !bustup_active(host) {
        assert!(Instant::now() < deadline, "Gallery did not activate");
        step(host, server, ctx, frame, 12.0, None);
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn wait_title<'a>(
    host: &mut SceneHost,
    server: &'a Arc<AssetServer>,
    ctx: &mut crate::render::PreRenderContext<'a, '_, '_, '_>,
    frame: &mut FrameId,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !title_active(host) {
        assert!(Instant::now() < deadline, "Title did not recover");
        step(host, server, ctx, frame, 12.0, None);
        std::thread::sleep(Duration::from_millis(1));
    }
}

// Match a complete TXA index entry; substring matching can rename dressicon
// instead of icon and silently turn a negative fixture into an unused control.
fn rename_texture(bytes: &mut [u8], name: &str) {
    let count = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize;
    let mut offset = 32;
    for _ in 0..count {
        offset = (offset + 3) & !3;
        let length = u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap()) as usize;
        let start = offset + 20;
        let end = start
            + bytes[start..offset + length]
                .iter()
                .position(|b| *b == 0)
                .unwrap();
        if &bytes[start..end] == name.as_bytes() {
            bytes[start] = b'!';
            return;
        }
        offset += length;
    }
    panic!("Supplied TXA index lacks {name}");
}
