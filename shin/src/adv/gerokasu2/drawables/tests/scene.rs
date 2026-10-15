use super::transitions::{Frames, advance, assets, new_backend};
use super::*;
use crate::adv::gerokasu2::scene::{SceneServices, WipeController, WipeStage};
use shin_core::primitives::update::FrameId;

fn tile(color: i32) -> [i32; 8] {
    [color, -960, -540, 1920, 1080, 0, 0, 0]
}
fn publish(controller: &mut WipeController, backend: &mut BasicLoadBackend) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while controller.stage() != WipeStage::Wipes {
        assert!(!controller.poll(backend).unwrap());
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn solid(pixels: &[u8], color: [f32; 4]) {
    for p in pixels.chunks_exact(4) {
        for c in 0..4 {
            assert!(
                (p[c] as f32 - color[c]).abs() <= 1.0,
                "scene {p:?}/{color:?}"
            );
        }
    }
}

#[test]
#[ignore = "requires supplied ROM,Metal and exact Loopback Audio"]
fn actual_pageback_promotion_all_scene_wipers_and_copied_bank_retirement() {
    let gpu = Offscreen::new();
    let assets = assets(&gpu);
    let scenario = Arc::new(Gerokasu2Scenario::new(bytes().into()).unwrap());
    let mut frames = Frames::new(&gpu);
    for kind in 0..=9 {
        let mut backend = new_backend(&assets, &scenario);
        let mut layers = Gerokasu2Layers::default();
        let mut frame = FrameId::default();
        complete(
            &mut load(&mut backend, &mut layers, command(1, 0, tile(0xff00))),
            &mut backend,
        );
        assert!(!backend.has_back());
        solid(
            &frames.draw_scene(&gpu, &mut backend),
            [255.0, 0.0, 0.0, 255.0],
        );
        backend.pageback(false).unwrap();
        layers.begin_back();
        assert!(backend.has_back());
        complete(
            &mut load(&mut backend, &mut layers, command(1, 4, tile(0xf00f))),
            &mut backend,
        );
        assert!(
            backend
                .screen()
                .page_layer()
                .get_plane(PlaneId::new(0))
                .reference_transition_words()
                .is_empty(),
            "PAGEBACK clears AUTOWIPE"
        );
        solid(
            &frames.draw_scene(&gpu, &mut backend),
            [255.0, 0.0, 0.0, 255.0],
        );
        let settings = LoadTransition {
            kind,
            flags: 0,
            duration: 30,
            params: match kind {
                1 => [0, 500, 0, 0, 0, 0, 0, 0],
                3 => [1000, 1000, 0, 0, 0, 0, 0, 0],
                6 => [2, 0, 0, 0, 0, 0, 0, 0],
                _ => [0; 8],
            },
        };
        let mut wipe = WipeController::new(true, settings);
        publish(&mut wipe, &mut backend);
        assert!(!backend.has_back());
        for (delta, progress) in [(0.0, 0.0), (15.0, 0.5), (15.0, 1.0)] {
            frame.advance();
            advance(&mut backend, &gpu, &assets, frame, delta, true);
            let pixels = frames.draw_scene(&gpu, &mut backend);
            if kind == 0 {
                solid(
                    &pixels,
                    [255.0 * (1.0 - progress), 0.0, 255.0 * progress, 255.0],
                );
            }
            assert_eq!(wipe.poll(&mut backend).unwrap(), progress == 1.0);
        }
        assert!(backend.previous_root.is_some());
    }
    // PAGEBACK copies the target page,then strips copied bank wrappers without
    // advancing their target properties or changing active bank clocks.
    let mut backend = new_backend(&assets, &scenario);
    let mut layers = Gerokasu2Layers::default();
    complete(
        &mut load(&mut backend, &mut layers, command(1, 0, tile(0xff00))),
        &mut backend,
    );
    backend.set_transition(LoadTransition {
        duration: 60,
        flags: 2,
        ..Default::default()
    });
    complete(
        &mut load(&mut backend, &mut layers, command(1, 4, tile(0xf00f))),
        &mut backend,
    );
    let mut frame = FrameId::default();
    frame.advance();
    advance(&mut backend, &gpu, &assets, frame, 15.0, true);
    backend.pageback(false).unwrap();
    layers.begin_back();
    assert!(
        backend
            .screen()
            .page_layer()
            .get_plane(PlaneId::new(0))
            .reference_transition_words()
            .is_empty()
    );
    assert!(
        backend
            .screen()
            .reference_pages()
            .next()
            .unwrap()
            .get_plane(PlaneId::new(0))
            .has_wiper_for_layerbank(layers.target(LayerId::new(3)).unwrap().layerbank)
    );
    solid(
        &frames.draw_scene(&gpu, &mut backend),
        [191.25, 0.0, 63.75, 255.0],
    );
    backend.promote_pending_page(false).unwrap();
    solid(
        &frames.draw_scene(&gpu, &mut backend),
        [0.0, 0.0, 255.0, 255.0],
    );
    // Fresh PAGEBACK uses defaults and empty planes;active remains unchanged.
    backend.pageback(true).unwrap();
    assert!(
        backend
            .screen()
            .page_layer()
            .planes()
            .iter()
            .all(|p| p.get_used_layerbank_ids().is_empty())
    );
    solid(
        &frames.draw_scene(&gpu, &mut backend),
        [0.0, 0.0, 255.0, 255.0],
    );
    backend.promote_pending_page(false).unwrap();
    solid(
        &frames.draw_scene(&gpu, &mut backend),
        [0.0, 0.0, 0.0, 255.0],
    );
    println!(
        "GEROKASU2_SCENE_GPU kinds=10 frames={} channels={} active_vs_pending_actual_Screen_Page_promotion_wipe_wait_Default_blend_copied_bank_stop_fresh_page=verified audio=Loopback Audio",
        frames.frames, frames.channels
    );
}

#[test]
#[ignore = "requires supplied ROM,Metal and exact Loopback Audio"]
fn decoded_pageback_wipe_dont_wait_and_wipewait_run_to_exit() {
    let gpu = Offscreen::new();
    let assets = assets(&gpu);
    let source = Gerokasu2Scenario::new(bytes().into()).unwrap();
    let mut data = bytes();
    data.truncate(source.header.code_offset as usize);
    let number = |data: &mut Vec<u8>, v: i32| {
        let mut out = Cursor::new(Vec::new());
        NumberSpec::<i32>::constant(v).write_le(&mut out).unwrap();
        data.extend(out.into_inner());
    };
    let layer = |data: &mut Vec<u8>, color: i32, flags: i32| {
        data.push(0xc1);
        for v in [3, 1, flags] {
            number(data, v);
        }
        data.push(0x1f);
        for v in tile(color)[..5].iter().copied() {
            number(data, v);
        }
    };
    layer(&mut data, 0xff00, 0);
    data.extend([0xcb, 0xcb]);
    layer(&mut data, 0xf00f, 4);
    data.push(0x8e);
    for v in [0, 2, 30] {
        number(&mut data, v);
    }
    data.push(0);
    data.push(0x8f);
    data.extend([0, 0]);
    number(&mut data, 0);
    let size = data.len() as u32;
    data[4..8].copy_from_slice(&size.to_le_bytes());
    let scenario = Arc::new(Gerokasu2Scenario::new(data.into()).unwrap());
    struct Services {
        loader: BasicLoadBackend,
        assets: Arc<AssetServer>,
        device: wgpu::Device,
        queue: wgpu::Queue,
        started: Instant,
    }
    impl MessageNotifications for Services {
        fn observed_voice(&mut self, _: &str) {}
        fn completed(&mut self, _: u32) {}
    }
    impl RunnerServices for Services {
        fn layer_load_services(&mut self) -> Option<&mut (dyn LoadServices + '_)> {
            Some(&mut self.loader)
        }
        fn scene_services(&mut self) -> Option<&mut (dyn SceneServices + '_)> {
            Some(&mut self.loader)
        }
        fn command_context(
            &mut self,
            _: &Gerokasu2RuntimeCommand,
            _: bool,
        ) -> Result<MessageCommandContext> {
            self.loader.set_frame(self.started, false);
            Ok(MessageCommandContext {
                was_read: false,
                event_mode: -1,
                skip: false,
                message_speed: 50,
            })
        }
        fn message_installed(&mut self, _: u32, _: PortraitChange) -> Result<()> {
            Ok(())
        }
        fn portrait_changed(&mut self, _: PortraitChange) -> Result<()> {
            Ok(())
        }
        fn advance_layers(&mut self, _: &Gerokasu2Layers, _: Ticks, _: bool) -> Result<()> {
            unreachable!()
        }
        fn advance_layers_in_frame(
            &mut self,
            frame: FrameId,
            _: &Gerokasu2Layers,
            delta: Ticks,
            allowed: bool,
        ) -> Result<()> {
            self.loader.update(&crate::update::AdvUpdateContext {
                frame_id: frame,
                delta_ticks: delta,
                asset_server: &self.assets,
                device: &self.device,
                queue: &self.queue,
                are_animations_allowed: allowed,
                voice_callback: None,
                movie_volume_settings: Default::default(),
            });
            Ok(())
        }
        fn fast_forward_layers(&mut self, _: &Gerokasu2Layers) -> Result<()> {
            self.loader.fast_forward();
            Ok(())
        }
    }
    let mut services = Services {
        loader: new_backend(&assets, &scenario),
        assets: assets.clone(),
        device: gpu.device.clone(),
        queue: gpu.queue.clone(),
        started: Instant::now(),
    };
    let mut runner = shin_tasks::block_on(Gerokasu2Runner::load(
        assets,
        scenario,
        Arc::new(AudioManager::new_for_tests()),
        0,
        42,
        InitialMessage {
            style: 0,
            asynchronous: false,
            face: -1,
        },
        Default::default(),
    ))
    .unwrap();
    let mut frames = Frames::new(&gpu);
    let mut frame = FrameId::default();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut saw_wait = false;
    loop {
        services.started = Instant::now();
        let status = runner.pump(false, 100, 10000, &mut services).unwrap();
        if status == RunnerStatus::Exited {
            break;
        }
        assert_eq!(status, RunnerStatus::Waiting);
        assert!(Instant::now() < deadline);
        if matches!(
            runner.pending_command(),
            Some(Gerokasu2RuntimeCommand::Shared(
                shin_core::vm::command::RuntimeCommand::WIPEWAIT(_)
            ))
        ) {
            saw_wait = true;
            assert!(!runner.layers().back_started());
            assert!(!services.loader.has_back());
        }
        frame.advance();
        runner
            .advance(
                frame,
                Ticks::from_f32(15.0),
                crate::adv::gerokasu2::runner::FramePolicy {
                    paused: false,
                    scene_changing: false,
                    animations_allowed: true,
                    autoplay: false,
                    fast_skip: false,
                },
                &mut services,
            )
            .unwrap();
        frames.draw_scene(&gpu, &mut services.loader);
    }
    assert!(saw_wait);
    assert_eq!(runner.completed_commands(), 7);
    assert_eq!(services.loader.loaded_notifications().len(), 2);
    solid(
        &frames.draw_scene(&gpu, &mut services.loader),
        [0.0, 0.0, 255.0, 255.0],
    );
    println!(
        "GEROKASU2_SCENE_RUNNER commands=7 frames={} channels={} decoded_repeated_PAGEBACK_LayerLoad_WIPE_DONT_WAIT_WIPEWAIT_FrameId_promotion_retirement_EXIT=verified audio=Loopback Audio",
        frames.frames, frames.channels
    );
}

#[test]
#[ignore = "requires Metal and exact Loopback Audio;legacy default Screen/Page regression"]
fn legacy_default_screen_nested_copy_render_and_motion_regression() {
    use shin_render::{
        dynamic_buffer::DynamicBuffer,
        pipelines::PipelineStorage,
        resize::{SurfaceResizeSource, ViewportParams},
        shaders::types::{
            buffer::BytesAddress,
            texture::{DepthStencilTarget, TextureSamplerStore},
        },
    };
    let gpu = Offscreen::new();
    let assets = assets(&gpu);
    let resize = SurfaceResizeSource::new(ViewportParams::both((1920, 1080).into()));
    let depth = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Legacy Screen regression depth"),
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
    let view = depth.create_view(&Default::default());
    let samplers = TextureSamplerStore::new(&gpu.device);
    let mut pipelines = PipelineStorage::new(gpu.device.clone(), wgpu::TextureFormat::Rgba8Unorm);
    let mut dynamic = DynamicBuffer::new(gpu.device.clone(), BytesAddress::new(2 * 1024 * 1024));
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    let mut context = crate::render::PreRenderContext {
        device: &gpu.device,
        queue: &gpu.queue,
        breakup_texture: None,
        resize_source: &resize,
        sampler_store: &samplers,
        depth_stencil: DepthStencilTarget { view: &view },
        pipeline_storage: &mut pipelines,
        dynamic_buffer: &mut dynamic,
        encoder: &mut encoder,
    };
    crate::layer::ScreenLayer::assert_reference_nested_transitions(&mut context, &assets);
    context.dynamic_buffer.finish(context.encoder);
    let finished = std::mem::replace(
        context.encoder,
        gpu.device.create_command_encoder(&Default::default()),
    )
    .finish();
    gpu.queue.submit([finished]);
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    println!(
        "GEROKASU2_SCENE_LEGACY default_UserLayer_Screen_Page_nested_copies_motion_capture_pixels_retirement=verified audio=Loopback Audio"
    );
}
