//! Actual scene-host start construction/publication failure and repair.
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
fn reference_start_factory_and_artwork_failure_recovers_title() {
    std::thread::Builder::new()
        .name("start-recovery".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(run)
        .unwrap()
        .join()
        .unwrap();
}
fn globals(host: &SceneHost) -> &GlobalData {
    match host.scene.as_ref().unwrap() {
        Scene::Title(t) => &t.globals,
        Scene::Picker(t) => &t.globals,
        _ => panic!("Unexpected start host"),
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
        Scene::Picker(t) => &t.audio,
        _ => panic!("Unexpected start host"),
    }
}
fn title_active(host: &SceneHost) -> bool {
    matches!(&host.scene, Some(Scene::Title(t)) if t.scene.reference_active())
}
fn start_active(host: &SceneHost) -> bool {
    matches!(&host.scene, Some(Scene::Picker(t)) if t.scene.accepts_input())
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
        "Start errors must recover locally: {result:?};scene_none={},host_error={:?}",
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
    let output = PathBuf::from(std::env::var_os("SHIN_REFERENCE_START_FAILURE_OUTPUT").unwrap());
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
        "Start recovery depth",
        shin_render::DEPTH_STENCIL_FORMAT,
    ));
    let depth_view = depth.create_view(&Default::default());
    let target = device.create_texture(&descriptor(
        "Start recovery color",
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
                "Start recovery",
            );
            host.render(&mut pass);
        }
        let pixels = crate::layer::user::readback_test_texture(ctx, &target);
        ctx.dynamic_buffer.recall();
        pixels
    };
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let profiles = [
        ("atlas-short", "/snrsel.txa", None),
        ("bg-name", "/snrsel.txa", Some("bg")),
        ("help-name", "/snrsel.txa", Some("help_snrsel")),
        ("first-details-name", "/snrsel.txa", Some("details_00")),
        ("locked-details-name", "/snrsel.txa", Some("details_48")),
        ("lock-name", "/snrsel.txa", Some("lock")),
        ("system-short", "/systex.txa", None),
        ("scroll-name", "/systex.txa", Some("scroll")),
        ("curtain-short", "/curtain.msk", None),
        ("tabs-name", "/snrsel.txa", Some("tab_on")),
        ("new-name", "/snrsel.txa", Some("new")),
        ("star-name", "/snrsel.txa", Some("star")),
        ("details-last-name", "/snrsel.txa", Some("details_47")),
        ("unused-single-tab", "/snrsel.txa", Some("tab_on")),
        ("unused-star", "/snrsel.txa", Some("star")),
        ("unused-new", "/snrsel.txa", Some("new")),
        (
            "unused-locked-details-all-open",
            "/snrsel.txa",
            Some("details_48"),
        ),
        ("unused-lock-all-open", "/snrsel.txa", Some("lock")),
        ("dialog-short", "/systex.txa", None),
        ("unlock-dialog-short", "/systex.txa", None),
    ];
    let mut cases = Vec::new();
    for (profile, path, name) in profiles {
        if profile == "unlock-dialog-short"
            && std::env::var_os("SHIN_REFERENCE_PATCH_ROM").is_none()
        {
            continue;
        }
        if let Ok(only) = std::env::var("SHIN_REFERENCE_START_ONLY") {
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
        if profile.contains("all-open") || profile == "details-last-name" {
            for i in 1..=48 {
                save.persist_data.set(i, 2);
            }
        }
        if profile == "tabs-name" {
            save.persist_data.set(9, 2);
        }
        if profile == "new-name" {
            save.persist_data.set(2, 1);
        }
        if profile == "star-name" || profile == "dialog-short" {
            save.persist_data.set(1, 3);
        }
        let positive = profile.starts_with("unused-");
        let child = profile == "dialog-short" || profile == "unlock-dialog-short";
        let disk = output.join(format!("progress-{profile}.dat"));
        let disk_bytes = save.encode();
        std::fs::write(&disk, &disk_bytes).unwrap();
        let global = GlobalData::open(Some(disk.clone()), None).unwrap();
        let setup = TitleSetup::from_save(global.save(), TitleItem::Start as i32);
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

        if child {
            host.pending = Some(Destination::Action(TitleItem::Start));
            step(&mut host, &server, &mut ctx, &mut frame, 0.0, None);
            wait_active(&mut host, &server, &mut ctx, &mut frame);
        }
        let old_selection = match &host.scene {
            Some(Scene::Picker(t)) => Some(t.scene.reference_groups_and_selection()),
            _ => None,
        };
        let bytes = supplied_payload(path).2;
        let file = directory.join(&path[1..]);
        let mut bad = bytes.clone();
        if let Some(name) = name {
            rename_texture(&mut bad, name);
        } else {
            bad.truncate(15);
        }
        std::fs::write(&file, &bad).unwrap();
        if child {
            let action = if profile == "dialog-short" {
                AppAction::Enter
            } else {
                AppAction::UnlockScenarios
            };
            step(&mut host, &server, &mut ctx, &mut frame, 12.0, Some(action));
            // The scenario confirmation is deferred until its chapter flash finishes.
            if profile == "dialog-short" {
                assert!(!start_active(&host));
                for _ in 0..6 {
                    step(&mut host, &server, &mut ctx, &mut frame, 12.0, None);
                }
            }
            let Some(Scene::Picker(t)) = &host.scene else {
                panic!("Dialog failure must retain the Start menu")
            };
            assert!(!t.scene.reference_has_dialog());
            assert!(
                t.scene.accepts_input(),
                "Failure must release the dialog input gate"
            );
            assert_eq!(
                Some(t.scene.reference_groups_and_selection()),
                old_selection
            );
            assert_eq!(snapshot(&host), before);
            assert_eq!(std::fs::read(&disk).unwrap(), disk_bytes);
            draw(&host, &mut ctx);
            std::fs::write(&file, &bytes).unwrap();
            step(&mut host, &server, &mut ctx, &mut frame, 12.0, Some(action));
            let deadline = Instant::now() + Duration::from_secs(10);
            while !matches!(&host.scene, Some(Scene::Picker(t)) if t.scene.reference_has_dialog()) {
                assert!(
                    Instant::now() < deadline,
                    "{profile}:repaired dialog did not open"
                );
                step(&mut host, &server, &mut ctx, &mut frame, 12.0, None);
            }
            draw(&host, &mut ctx);
            // Finish the twelve-tick opening before asking the dialog to cancel.
            for _ in 0..3 {
                step(&mut host, &server, &mut ctx, &mut frame, 12.0, None);
            }
            step(
                &mut host,
                &server,
                &mut ctx,
                &mut frame,
                12.0,
                Some(AppAction::Cancel),
            );
            wait_active(&mut host, &server, &mut ctx, &mut frame);
            assert_eq!(snapshot(&host), before);
        } else {
            host.pending = Some(Destination::Action(TitleItem::Start));
            step(&mut host, &server, &mut ctx, &mut frame, 0.0, None);
            if positive {
                wait_active(&mut host, &server, &mut ctx, &mut frame);
                draw(&host, &mut ctx);
                std::fs::write(&file, &bytes).unwrap();
            } else {
                let deadline = Instant::now() + Duration::from_secs(10);
                while !matches!(host.scene, Some(Scene::Title(_))) && host.pending.is_none() {
                    assert!(
                        Instant::now() < deadline,
                        "{profile}:failed Start did not return to title"
                    );
                    step(&mut host, &server, &mut ctx, &mut frame, 0.0, None);
                    draw(&host, &mut ctx);
                    std::thread::sleep(Duration::from_millis(1));
                }
                assert_eq!(snapshot(&host), before);
                assert_eq!(audio(&host).bgm_player.reference_source().2, track);
                std::fs::write(&file, &bytes).unwrap();
                wait_title(&mut host, &server, &mut ctx, &mut frame);
                host.pending = Some(Destination::Action(TitleItem::Start));
                step(&mut host, &server, &mut ctx, &mut frame, 0.0, None);
                wait_active(&mut host, &server, &mut ctx, &mut frame);
                draw(&host, &mut ctx);
            }
        }
        // Healthy controller navigation remains usable after every failure/control.
        step(
            &mut host,
            &server,
            &mut ctx,
            &mut frame,
            12.0,
            Some(AppAction::AnyDown),
        );
        step(
            &mut host,
            &server,
            &mut ctx,
            &mut frame,
            12.0,
            Some(AppAction::Up),
        );
        draw(&host, &mut ctx);
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
        cases.push(serde_json::json!({"profile":profile,"asset":path,"required_name":name,
            "unused_artwork_accepted":positive,"dialog_failure_retains_picker":child,
            "same_host_server_repair_retry":true,"global_progress_preserved_except_elapsed_time":true,
            "audio_manager_retained":true,"disk_progress_preserved":true,"host_error_latch_avoided":true,
            "actual_controller_navigation_and_cancel":true}));
    }
    std::fs::write(output.join("start-recovery.json"),serde_json::to_vec_pretty(&serde_json::json!({
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
    while !start_active(host) {
        assert!(Instant::now() < deadline, "Start did not activate");
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
