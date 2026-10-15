//! Actual scene-host CG/Movie child resource failure and repair.
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
fn reference_gallery_child_failure_retires_and_reopens() {
    std::thread::Builder::new()
        .name("gallery-child-recovery".into())
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
        Scene::Movie(t) => &t.globals,
        _ => panic!("Unexpected gallery child host"),
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
        Scene::Movie(t) => &t.audio,
        _ => panic!("Unexpected gallery child host"),
    }
}
fn title_active(host: &SceneHost) -> bool {
    matches!(&host.scene, Some(Scene::Title(t)) if t.scene.reference_active())
}
fn gallery_state(host: &SceneHost) -> (bool, f32, bool, usize, usize, bool) {
    match host.scene.as_ref().unwrap() {
        Scene::Cg(t) => t.scene.reference_state(),
        Scene::Movie(t) => t.scene.reference_state(),
        _ => panic!("Expected retained gallery"),
    }
}
fn child_active(host: &SceneHost) -> bool {
    match host.scene.as_ref().unwrap() {
        Scene::Cg(t) => t.scene.reference_child_state().is_some_and(|s| s.0),
        Scene::Movie(t) => t.scene.reference_state().5,
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
        "Gallery child errors must recover locally: {result:?};scene_none={},host_error={:?}",
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
    let output =
        PathBuf::from(std::env::var_os("SHIN_REFERENCE_GALLERY_CHILD_FAILURE_OUTPUT").unwrap());
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
        "Gallery child recovery depth",
        shin_render::DEPTH_STENCIL_FORMAT,
    ));
    let depth_view = depth.create_view(&Default::default());
    let target = device.create_texture(&descriptor(
        "Gallery child recovery color",
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
                "Gallery child recovery",
            );
            host.render(&mut pass);
        }
        let pixels = crate::layer::user::readback_test_texture(ctx, &target);
        ctx.dynamic_buffer.recall();
        pixels
    };
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());


    let mut cases = Vec::new();
    for profile in [
        "cg-first-short",
        "cg-first-zero-width",
        "cg-next-short",
        "cg-next-zero-height",
        "cg-check-locked",
        "cg-check-unlocked",
        "movie-short",
        "movie-first-nal-invalid",
        "movie-concurrent-first-nal-invalid",
    ] {
        println!("gallery child profile: {profile}");
        let concurrent = profile.contains("concurrent");
        let cg = profile.starts_with("cg");
        let check = profile.contains("check");
        let locked_control = profile == "cg-check-locked";
        let replacement = profile.contains("next");
        let item = if cg { TitleItem::Cg } else { TitleItem::Movie };
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
        let info = assets.scenario.info_tables();
        let row = info
            .picture_box_info
            .iter()
            .position(|r| r.picture_ids.0.len() >= 2 && r.picture_ids.0[0] != r.picture_ids.0[1])
            .unwrap();
        let ids: Vec<_> = info.picture_box_info[row]
            .picture_ids
            .0
            .iter()
            .copied()
            .collect();
        let path = if check {
            "/cgmode.txa".to_owned()
        } else if cg {
            info.picture_info[ids[usize::from(replacement)].into_i32() as usize].path()
        } else {
            info.movie_info[0].path()
        };
        let mut save = SaveData::default();
        save.persist_data.set(0, 1);
        save.save_menu_position = 91;
        save.manual_save_slots[91] = Some(VmState::for_scenario(0, 42).snapshot_game_data());
        save.settings.v0_bgmvol = 37;
        save.settings.v3_sysvol = 0;
        if profile != "cg-check-locked" {
            for id in &ids {
                save.save_vectors.unlocked_cgs.add(id.into_i32() as usize);
            }
        }
        save.save_vectors.unlocked_movies.add(0);
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
        wait(&mut host, &server, &mut ctx, &mut frame, title_active);
        let before = snapshot(&host);
        let bytes = supplied_payload(&path).2;
        let file = directory.join(&path[1..]);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let mut bad = bytes.clone();
        if check {
            rename_texture(&mut bad, "check");
        } else if profile.ends_with("zero-width") {
            bad[16..18].fill(0);
        } else if profile.ends_with("zero-height") {
            bad[18..20].fill(0);
        } else if profile.ends_with("nal-invalid") {
            // Retain the actual MP4 tables and AVC setup; corrupt only the first
            // authored video's sample length prefix, beyond container parsing.
            let mp4 = shin_video::mp4::Mp4::new(std::io::Cursor::new(bytes.clone())).unwrap();
            let mut track = mp4.video_track;
            let sample = track.next_sample().unwrap().unwrap();
            assert!(sample.bytes.len() > 4);
            let at = bad
                .windows(sample.bytes.len())
                .position(|w| w == sample.bytes.as_ref())
                .unwrap();
            bad[at..at + 4].fill(0xff);
        } else {
            bad.truncate(15);
        }
        if check {
            std::fs::write(&file, &bad).unwrap();
        }
        host.pending = Some(Destination::Action(item));
        step(&mut host, &server, &mut ctx, &mut frame, 0.0, None);
        if check && !locked_control {
            let deadline = Instant::now() + Duration::from_secs(10);
            while !matches!(host.scene, Some(Scene::Title(_))) && host.pending.is_none() {
                assert!(
                    Instant::now() < deadline,
                    "{profile}:missing indicator art published"
                );
                step(&mut host, &server, &mut ctx, &mut frame, 0.0, None);
                draw(&host, &mut ctx);
                std::thread::sleep(Duration::from_millis(1));
            }
            assert_eq!(snapshot(&host), before);
            std::fs::write(&file, &bytes).unwrap();
            wait(&mut host, &server, &mut ctx, &mut frame, title_active);
            host.pending = Some(Destination::Action(item));
            step(&mut host, &server, &mut ctx, &mut frame, 0.0, None);
        }
        wait(&mut host, &server, &mut ctx, &mut frame, |h| {
            gallery_state(h).2
        });
        if cg {
            for _ in 0..row / 4 {
                step(
                    &mut host,
                    &server,
                    &mut ctx,
                    &mut frame,
                    0.0,
                    Some(AppAction::AnyDown),
                );
                for _ in 0..3 {
                    step(&mut host, &server, &mut ctx, &mut frame, 12.0, None);
                }
            }
            for _ in 0..row % 4 {
                step(
                    &mut host,
                    &server,
                    &mut ctx,
                    &mut frame,
                    0.0,
                    Some(AppAction::Right),
                );
                for _ in 0..3 {
                    step(&mut host, &server, &mut ctx, &mut frame, 12.0, None);
                }
            }
            assert_eq!(gallery_state(&host).3, row);
        }
        let track = audio(&host).bgm_player.reference_source().2;
        let gallery_pixels = draw(&host, &mut ctx);
        if !check && !replacement {
            std::fs::write(&file, &bad).unwrap();
        }
        if profile.ends_with("nal-invalid") {
            // Parse succeeds: this case targets the real paused-player worker.
            let parsed =
                shin_tasks::block_on(server.load::<crate::asset::movie::Movie, _>(&path)).unwrap();
            drop(parsed);
        }
        if locked_control {
            step(
                &mut host,
                &server,
                &mut ctx,
                &mut frame,
                0.0,
                Some(AppAction::Enter),
            );
            for _ in 0..3 {
                step(&mut host, &server, &mut ctx, &mut frame, 12.0, None);
            }
            assert_eq!(
                gallery_state(&host).4,
                0,
                "Locked row must not create a viewer"
            );
            assert!(gallery_state(&host).2);
            draw(&host, &mut ctx);
            assert_eq!(snapshot(&host), before);
            assert_eq!(audio(&host).bgm_player.reference_source().2, track);
            std::fs::write(&file, &bytes).unwrap();
        } else {
            step(
                &mut host,
                &server,
                &mut ctx,
                &mut frame,
                0.0,
                Some(AppAction::Enter),
            );
            if concurrent {
                step(
                    &mut host,
                    &server,
                    &mut ctx,
                    &mut frame,
                    0.0,
                    Some(AppAction::Enter),
                );
            }
            wait(&mut host, &server, &mut ctx, &mut frame, |h| {
                gallery_state(h).4 >= if concurrent { 2 } else { 1 }
            });
            if check || replacement {
                wait(&mut host, &server, &mut ctx, &mut frame, child_active);
                draw(&host, &mut ctx);
            }
            if replacement {
                std::fs::write(&file, &bad).unwrap();
                step(
                    &mut host,
                    &server,
                    &mut ctx,
                    &mut frame,
                    0.0,
                    Some(AppAction::Enter),
                );
                // The previous PIC remains visible while its replacement is pending.
                assert!(gallery_state(&host).4 > 0);
                draw(&host, &mut ctx);
            }
            if !check {
                let deadline = Instant::now() + Duration::from_secs(10);
                while gallery_state(&host).4 > 0 {
                    assert!(
                        Instant::now() < deadline,
                        "{profile}:failed child did not retire"
                    );
                    step(&mut host, &server, &mut ctx, &mut frame, 0.0, None);
                    std::thread::sleep(Duration::from_millis(1));
                }
                assert!(
                    !gallery_state(&host).2,
                    "Failure completion must not enable parent input in the same snapshot"
                );
                let retained = draw(&host, &mut ctx);
                assert!(
                    retained == gallery_pixels,
                    "{profile}:gallery rendering changed across child failure"
                );
                let failures = match host.scene.as_ref().unwrap() {
                    Scene::Cg(t) => t.scene.reference_resource_failures(),
                    Scene::Movie(t) => t.scene.reference_resource_failures(),
                    _ => unreachable!(),
                };
                assert_eq!(failures.len(), if concurrent { 2 } else { 1 });
                if profile.contains("zero-") {
                    assert!(
                        failures[0].contains("zero effective extent"),
                        "{:?}",
                        failures
                    );
                }
                if profile.ends_with("nal-invalid") {
                    assert!(
                        failures[0].contains("Converting MP4 sample to Annex B"),
                        "{:?}",
                        failures
                    );
                }
                step(&mut host, &server, &mut ctx, &mut frame, 0.0, None);
                assert!(gallery_state(&host).2);
                assert_eq!(snapshot(&host), before);
                assert_eq!(audio(&host).bgm_player.reference_source().2, track);
                assert!(Arc::ptr_eq(&audio(&host).manager, &manager));
                assert_eq!(std::fs::read(&disk).unwrap(), disk_bytes);
                std::fs::write(&file, &bytes).unwrap();
                // Actual list controller reopens the same child with the same server.
                step(
                    &mut host,
                    &server,
                    &mut ctx,
                    &mut frame,
                    0.0,
                    Some(AppAction::Enter),
                );
                wait(&mut host, &server, &mut ctx, &mut frame, child_active);
                draw(&host, &mut ctx);
                if replacement {
                    step(
                        &mut host,
                        &server,
                        &mut ctx,
                        &mut frame,
                        0.0,
                        Some(AppAction::Enter),
                    );
                    wait(&mut host, &server, &mut ctx, &mut frame, |h| {
                        match h.scene.as_ref().unwrap() {
                            Scene::Cg(t) => t
                                .scene
                                .reference_child_state()
                                .is_some_and(|s| s.0 && s.2 == 1),
                            _ => false,
                        }
                    });
                    draw(&host, &mut ctx);
                }
            }
            if !cg {
                // Preparation is ready before movie fade permits skip input.
                for _ in 0..10 {
                    step(&mut host, &server, &mut ctx, &mut frame, 12.0, None);
                }
            }
            step(
                &mut host,
                &server,
                &mut ctx,
                &mut frame,
                0.0,
                Some(AppAction::Cancel),
            );
            wait(&mut host, &server, &mut ctx, &mut frame, |h| {
                gallery_state(h).2
            });
        }
        assert_eq!(snapshot(&host), before);
        step(
            &mut host,
            &server,
            &mut ctx,
            &mut frame,
            0.0,
            Some(AppAction::Cancel),
        );
        wait(&mut host, &server, &mut ctx, &mut frame, title_active);
        assert_eq!(snapshot(&host), before);
        assert_eq!(std::fs::read(&disk).unwrap(), disk_bytes);
        assert!(Arc::ptr_eq(&audio(&host).manager, &manager));
        if file.exists() {
            std::fs::remove_file(&file).unwrap();
        }
        cases.push(
            serde_json::json!({"profile":profile,"asset":path,"same_host_server_retry":!locked_control,"unused_indicator_artwork_accepted":locked_control,
            "gallery_input_and_render_recovered":true,"host_error_latch_avoided":true,
            "progress_and_audio_manager_preserved":true,"returned_to_title":true}),
        );
    }
    std::fs::write(output.join("gallery-child-recovery.json"),serde_json::to_vec_pretty(&serde_json::json!({
        "cases":cases,"cases_passed":cases.len(),"patch_mounted":std::env::var_os("SHIN_REFERENCE_PATCH_ROM").is_some(),
        "window_created":false,"working_progress_written":false,"audio_device":"exact Loopback Audio"
    })).unwrap()).unwrap();
}
#[track_caller]
fn wait<'a>(
    host: &mut SceneHost,
    server: &'a Arc<AssetServer>,
    ctx: &mut crate::render::PreRenderContext<'a, '_, '_, '_>,
    frame: &mut FrameId,
    condition: impl Fn(&SceneHost) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !condition(host) {
        assert!(
            Instant::now() < deadline,
            "Wait timed out; gallery/title state did not advance"
        );
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
