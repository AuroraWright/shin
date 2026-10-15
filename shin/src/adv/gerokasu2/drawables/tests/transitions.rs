use super::*;
use crate::{render::PreRenderContext, update::AdvUpdateContext};
use shin_core::{primitives::update::FrameId, vm::command::types::PLANES_COUNT};
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
    io::Write,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

pub(super) fn assets(gpu: &Offscreen) -> Arc<AssetServer> {
    let mut io = LayeredAssetIo::new();
    io.try_with_rom(&std::path::PathBuf::from(
        std::env::var_os("SHIN_GEROKASU2_ROM").unwrap(),
    ))
    .unwrap();
    Arc::new(AssetServer::new(
        io.into(),
        AssetLoadContext {
            game: crate::cli::Game::Gerokasu2,
            wgpu_device: gpu.device.clone(),
            wgpu_queue: gpu.queue.clone(),
            bustup_cache: crate::asset::system::cache::AssetCache::new(),
            higurashi_bustup_cache: Default::default(),
            picture_cache: Default::default(),
            font_cache: Default::default(),
        },
    ))
}
pub(super) fn new_backend(
    assets: &Arc<AssetServer>,
    scenario: &Arc<Gerokasu2Scenario>,
) -> BasicLoadBackend {
    BasicLoadBackend::new(
        assets.clone(),
        scenario.clone(),
        LoadCounters::new(0, [0; 48]),
    )
}
fn tile(color: i32) -> [i32; 8] {
    [color, -960, -540, 1920, 1080, 0, 0, 0]
}
pub(super) fn advance(
    backend: &mut BasicLoadBackend,
    gpu: &Offscreen,
    assets: &Arc<AssetServer>,
    frame: FrameId,
    delta: f32,
    allowed: bool,
) {
    backend.update(&AdvUpdateContext {
        frame_id: frame,
        delta_ticks: Ticks::from_f32(delta),
        asset_server: assets,
        device: &gpu.device,
        queue: &gpu.queue,
        are_animations_allowed: allowed,
        voice_callback: None,
        movie_volume_settings: Default::default(),
    });
}

struct Native {
    child: Child,
    input: Option<ChildStdin>,
    output: ChildStdout,
    snapshots: usize,
    words: usize,
}
impl Native {
    fn new() -> Self {
        let mut child =
            Command::new(std::env::var_os("SHIN_GEROKASU2_REPLACEMENT_ORACLE").unwrap())
                .args([
                    std::env::var_os("SHIN_GEROKASU2_EXECUTABLE").unwrap(),
                    std::env::var_os("SHIN_GEROKASU2_MEDIUM_FONT").unwrap(),
                    std::env::var_os("SHIN_GEROKASU2_SYSTEM_FONT").unwrap(),
                ])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
        Self {
            input: child.stdin.take(),
            output: child.stdout.take().unwrap(),
            child,
            snapshots: 0,
            words: 0,
        }
    }
    fn compare(&mut self, row: [u32; 8], backend: &BasicLoadBackend) {
        self.input
            .as_mut()
            .unwrap()
            .write_all(
                &row.into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        let trace = backend.planes()[0].reference_transition_words();
        let mut expected = b"GRW1".to_vec();
        expected.extend((trace.len() as u32).to_le_bytes());
        expected.extend(1u32.to_le_bytes());
        for entry in &trace {
            expected.extend(entry[..5].iter().flat_map(|w| w.to_le_bytes()));
        }
        let mut actual = vec![0; expected.len()];
        self.output.read_exact(&mut actual).unwrap();
        assert_eq!(actual, expected, "replacement snapshot {}", self.snapshots);
        self.snapshots += 1;
        self.words += (expected.len() - 4) / 4;
    }
}
impl Drop for Native {
    fn drop(&mut self) {
        self.input.take();
        let status = self.child.wait().unwrap();
        if !std::thread::panicking() {
            assert!(status.success());
        }
    }
}

pub(super) struct Frames {
    target: wgpu::Texture,
    view: wgpu::TextureView,
    depth: wgpu::TextureView,
    resize: SurfaceResizeSource,
    samplers: TextureSamplerStore,
    pipelines: PipelineStorage,
    dynamic: DynamicBuffer,
    pub(super) frames: usize,
    pub(super) channels: usize,
}
impl Frames {
    pub(super) fn new(gpu: &Offscreen) -> Self {
        let descriptor = |format, usage| wgpu::TextureDescriptor {
            label: Some("Ger retained replacement frame"),
            size: wgpu::Extent3d {
                width: 320,
                height: 180,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        };
        let target = gpu.device.create_texture(&descriptor(
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        ));
        let depth = gpu.device.create_texture(&descriptor(
            shin_render::DEPTH_STENCIL_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        ));
        Self {
            view: target.create_view(&Default::default()),
            target,
            depth: depth.create_view(&Default::default()),
            resize: SurfaceResizeSource::new(ViewportParams::both((320, 180).into())),
            samplers: TextureSamplerStore::new(&gpu.device),
            pipelines: PipelineStorage::new(gpu.device.clone(), wgpu::TextureFormat::Rgba8Unorm),
            dynamic: DynamicBuffer::new(gpu.device.clone(), BytesAddress::new(2 * 1024 * 1024)),
            frames: 0,
            channels: 0,
        }
    }
    pub(super) fn draw(
        &mut self,
        gpu: &Offscreen,
        backend: &mut BasicLoadBackend,
        plane: PlaneId,
    ) -> Vec<u8> {
        self.draw_internal(gpu, backend, Some(plane))
    }
    pub(super) fn draw_scene(
        &mut self,
        gpu: &Offscreen,
        backend: &mut BasicLoadBackend,
    ) -> Vec<u8> {
        self.draw_internal(gpu, backend, None)
    }
    fn draw_internal(
        &mut self,
        gpu: &Offscreen,
        backend: &mut BasicLoadBackend,
        plane: Option<PlaneId>,
    ) -> Vec<u8> {
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        let mut context = PreRenderContext {
            device: &gpu.device,
            queue: &gpu.queue,
            breakup_texture: None,
            resize_source: &self.resize,
            sampler_store: &self.samplers,
            depth_stencil: DepthStencilTarget { view: &self.depth },
            pipeline_storage: &mut self.pipelines,
            dynamic_buffer: &mut self.dynamic,
            encoder: &mut encoder,
        };
        if let Some(plane) = plane {
            backend.pre_render_plane(plane, &mut context, &TransformParams::default());
        } else {
            backend
                .pre_render_scene(&mut context, &TransformParams::default())
                .unwrap();
        }
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &self.view,
                },
                Some(context.depth_stencil),
                "Ger retained plane draw",
            );
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::BLACK),
                Some(0),
                Some(1.0),
            );
            for kind in [PassKind::Opaque, PassKind::Transparent] {
                if let Some(plane) = plane {
                    backend.render_plane(plane, &mut pass, &TransformParams::default(), 0, kind);
                } else {
                    backend
                        .render_scene(&mut pass, &TransformParams::default(), 0, kind)
                        .unwrap();
                }
            }
        }
        let pixels = crate::layer::user::readback_test_texture(&mut context, &self.target);
        self.frames += 1;
        self.channels += pixels.len();
        pixels
    }
}

#[test]
#[ignore = "requires supplied ROM/native replacement oracle,Metal and exact Loopback Audio"]
fn retained_replacements_native_clocks_properties_all_wipers_and_retirement() {
    let gpu = Offscreen::new();
    let assets = assets(&gpu);
    let scenario = Arc::new(Gerokasu2Scenario::new(bytes().into()).unwrap());
    let mut native = Native::new();
    let plane = PlaneId::new(0);
    for depth in 1..=3 {
        for keep in [0, 2, 10] {
            let mut backend = new_backend(&assets, &scenario);
            let mut layers = Gerokasu2Layers::default();
            complete(
                &mut load(&mut backend, &mut layers, command(1, 0, tile(0xff00))),
                &mut backend,
            );
            let bank = layers.target(LayerId::new(3)).unwrap().layerbank;
            backend
                .layer_mut(plane, bank)
                .unwrap()
                .properties_mut()
                .property_tweener_mut(LayerProperty::TranslateX)
                .fast_forward_to(137.0);
            for level in 0..depth {
                backend.set_transition(LoadTransition {
                    duration: [60, 30, 91][level],
                    flags: 2,
                    ..Default::default()
                });
                complete(
                    &mut load(
                        &mut backend,
                        &mut layers,
                        command(1, keep | 4, tile(0xf001 + level as i32)),
                    ),
                    &mut backend,
                );
            }
            let properties =
                backend.planes()[0].reference_slot_properties(bank, LayerProperty::TranslateX);
            assert_eq!(properties.len(), depth + 1);
            for (i, (identity, value)) in properties.iter().enumerate() {
                assert_eq!(*value, if i == 0 || keep & 2 != 0 { 137.0 } else { 0.0 });
                if i > 0 {
                    assert_eq!(*identity == properties[i - 1].0, keep == 10);
                }
            }
            let mut row = [0; 8];
            row[1] = depth as u32;
            for (i, duration) in [60.0f32, 30.0, 91.0][..depth].iter().enumerate() {
                row[2 + i] = duration.to_bits();
            }
            native.compare(row, &backend);
            let mut frame = FrameId::default();
            let mut frame_number = 0;
            for (delta, allowed, new_frame) in [
                (15.0, true, true),
                (31.0, false, true),
                (10.0, true, true),
                (10.0, true, false),
                (-8.0, true, true),
            ] {
                if new_frame {
                    frame.advance();
                    frame_number += 1;
                }
                advance(&mut backend, &gpu, &assets, frame, delta, allowed);
                native.compare(
                    [1, frame_number, allowed as u32, delta.to_bits(), 0, 0, 0, 0],
                    &backend,
                );
            }
            backend.fast_forward();
            native.compare([2, 0, 0, 0, 0, 0, 0, 0], &backend);
            frame.advance();
            frame_number += 1;
            advance(&mut backend, &gpu, &assets, frame, 0.0, false);
            native.compare([1, frame_number, 0, 0, 0, 0, 0, 0], &backend);
            assert!(
                !backend
                    .any_target_wiping(&[LoadTarget {
                        bank,
                        logical: LayerId::new(3),
                        keep_properties: false,
                        already_same: false,
                        load_counter: 0
                    }])
                    .unwrap()
            );
            frame.advance();
            frame_number += 1;
            advance(&mut backend, &gpu, &assets, frame, -60.0, true);
            native.compare(
                [1, frame_number, 1, (-60.0f32).to_bits(), 0, 0, 0, 0],
                &backend,
            );
        }
    }
    println!(
        "GEROKASU2_REPLACEMENT_NATIVE cases=9 snapshots={} words={} original_nested_update_ff_stencil_clock_source_retirement_on_actual_shared_groups=verified",
        native.snapshots, native.words
    );
    let mut frames = Frames::new(&gpu);
    for kind in 0..=9 {
        let mut backend = new_backend(&assets, &scenario);
        let mut layers = Gerokasu2Layers::default();
        let mut frame = FrameId::default();
        complete(
            &mut load(&mut backend, &mut layers, command(1, 0, tile(0xff00))),
            &mut backend,
        );
        backend.set_transition(LoadTransition {
            kind,
            duration: 60,
            flags: 0,
            params: match kind {
                1 => [0, 500, 0, 0, 0, 0, 0, 0],
                3 => [1000, 1000, 0, 0, 0, 0, 0, 0],
                6 => [2, 0, 0, 0, 0, 0, 0, 0],
                _ => [0; 8],
            },
        });
        let mut loading = load(&mut backend, &mut layers, command(1, 4, tile(0xf00f)));
        let deadline = Instant::now() + Duration::from_secs(10);
        while loading.stage() != crate::adv::gerokasu2::layer_load::LoadStage::Wipes {
            assert!(!loading.poll(&mut backend).unwrap());
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(backend.loaded_notifications().len(), 2);
        assert!(loading.poll(&mut backend).is_ok_and(|done| !done));
        for (delta, progress) in [(0.0, 0.0), (15.0, 0.25), (45.0, 1.0)] {
            frame.advance();
            advance(&mut backend, &gpu, &assets, frame, delta, true);
            let pixels = frames.draw(&gpu, &mut backend, plane);
            if kind == 0 {
                for pixel in pixels.chunks_exact(4) {
                    let expected = [255.0 * (1.0 - progress), 0.0, 255.0 * progress, 255.0];
                    for channel in 0..4 {
                        assert!(
                            (pixel[channel] as f32 - expected[channel]).abs() <= 1.0,
                            "retained Default {pixel:?}/{progress}"
                        );
                    }
                }
            }
            assert_eq!(
                loading.poll(&mut backend).unwrap(),
                progress == 1.0,
                "kind{kind}"
            );
            assert_eq!(
                backend.loaded_notifications().len(),
                2,
                "notification repeated"
            );
        }
        assert!(
            backend.prepared_wiper.is_none()
                && backend.prototype.is_none()
                && backend.prepared.is_none()
        );
    }
    // The registry owner belongs to retained source sides, including deferred
    // captures. A following preparation releases the retired frame's source.
    let mut backend = new_backend(&assets, &scenario);
    let mut layers = Gerokasu2Layers::default();
    complete(
        &mut load(&mut backend, &mut layers, command(3, 0, [0; 8])),
        &mut backend,
    );
    let bank = layers.target(LayerId::new(3)).unwrap().layerbank;
    let mut null = load(
        &mut backend,
        &mut layers,
        Gerokasu2RuntimeCommand::LayerLoad {
            layer_id: 4,
            layer_type: 0,
            flags: 0,
            params: (0, 0, 0, 0, 0, 0, 0, 0),
        },
    );
    complete(&mut null, &mut backend);
    let weak = {
        let view = backend.layer(plane, bank).unwrap();
        Arc::downgrade(view.retained_bustup().unwrap())
    };
    backend.set_transition(LoadTransition {
        duration: 60,
        flags: 2,
        ..Default::default()
    });
    complete(
        &mut load(&mut backend, &mut layers, command(1, 4, tile(0xf00f))),
        &mut backend,
    );
    assert!(weak.upgrade().is_some());
    frames.draw(&gpu, &mut backend, plane);
    backend.fast_forward();
    let mut frame = FrameId::default();
    frame.advance();
    advance(&mut backend, &gpu, &assets, frame, 0.0, true);
    assert!(
        weak.upgrade().is_some(),
        "prepared frame retains its actual source"
    );
    frames.draw(&gpu, &mut backend, plane);
    assert!(
        weak.upgrade().is_none(),
        "retired source leaked registry owner"
    );
    // Each plane owns its bank, and all selected targets receive distinct wipes.
    let mut selected = new_backend(&assets, &scenario);
    let mut state = Gerokasu2Layers::default();
    for p in 0..PLANES_COUNT {
        state.select_plane(p as i32).unwrap();
        complete(
            &mut load(&mut selected, &mut state, command(1, 0, tile(0xff00))),
            &mut selected,
        );
    }
    state.select_plane(0).unwrap();
    state.select(3, 5);
    selected.set_transition(LoadTransition {
        duration: 60,
        flags: 2,
        ..Default::default()
    });
    let mut selected_load = load(
        &mut selected,
        &mut state,
        Gerokasu2RuntimeCommand::LayerLoad {
            layer_id: -6,
            layer_type: 1,
            flags: 4,
            params: (0xf00f, -960, -540, 1920, 1080, 0, 0, 0),
        },
    );
    complete(&mut selected_load, &mut selected);
    assert_eq!(selected_load.plan().targets.len(), 3);
    assert_eq!(selected.planes()[0].reference_transition_words().len(), 3);
    assert!(
        selected
            .any_target_wiping(&selected_load.plan().targets)
            .unwrap()
    );
    selected.fast_forward();
    frame.advance();
    advance(&mut selected, &gpu, &assets, frame, 0.0, true);
    assert!(
        !selected
            .any_target_wiping(&selected_load.plan().targets)
            .unwrap()
    );
    println!(
        "GEROKASU2_REPLACEMENT_GPU kinds=10 frames={} channels={} retained_source_target_captures_keep_copy_share_pause_duplicate_frame_wait_done_notifications_once_registry_retirement_planes_selected_targets=verified audio=Loopback Audio",
        frames.frames, frames.channels
    );
}

#[test]
#[ignore = "requires supplied ROM,Metal and exact Loopback Audio"]
fn real_transset_and_autowipe_commands_wait_in_the_retained_runner() {
    let gpu = Offscreen::new();
    let assets = assets(&gpu);
    let source = Gerokasu2Scenario::new(bytes().into()).unwrap();
    let mut data = bytes();
    data.truncate(source.header.code_offset as usize);
    let number = |data: &mut Vec<u8>, value: i32| {
        let mut out = Cursor::new(Vec::new());
        NumberSpec::<i32>::constant(value)
            .write_le(&mut out)
            .unwrap();
        data.extend(out.into_inner());
    };
    let layer = |data: &mut Vec<u8>, flags: i32, color: i32| {
        data.push(0xc1);
        for v in [3, 1, flags] {
            number(data, v);
        }
        data.push(0x1f);
        for v in tile(color)[..5].iter().copied() {
            number(data, v);
        }
    };
    layer(&mut data, 0, 0xff00);
    data.push(0xc9);
    for v in [0, 0, 30] {
        number(&mut data, v);
    }
    data.push(0);
    layer(&mut data, 4, 0xf00f);
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
        updates: usize,
    }
    impl MessageNotifications for Services {
        fn observed_voice(&mut self, _: &str) {}
        fn completed(&mut self, _: u32) {}
    }
    impl RunnerServices for Services {
        fn layer_load_services(&mut self) -> Option<&mut (dyn LoadServices + '_)> {
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
            anyhow::bail!("runner must supply its presentation FrameId")
        }
        fn advance_layers_in_frame(
            &mut self,
            frame: FrameId,
            _: &Gerokasu2Layers,
            delta: Ticks,
            allowed: bool,
        ) -> Result<()> {
            self.loader.update(&AdvUpdateContext {
                frame_id: frame,
                delta_ticks: delta,
                asset_server: &self.assets,
                device: &self.device,
                queue: &self.queue,
                are_animations_allowed: allowed,
                voice_callback: None,
                movie_volume_settings: Default::default(),
            });
            self.updates += 1;
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
        updates: 0,
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
    let mut frame = FrameId::default();
    let mut frames = Frames::new(&gpu);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        services.started = Instant::now();
        let status = runner.pump(false, 100, 10000, &mut services).unwrap();
        if status == RunnerStatus::Exited {
            break;
        }
        assert_eq!(status, RunnerStatus::Waiting);
        assert!(Instant::now() < deadline);
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
        frames.draw(&gpu, &mut services.loader, PlaneId::new(0));
    }
    assert_eq!(runner.completed_commands(), 4);
    assert_eq!(services.loader.loaded_notifications().len(), 2);
    assert_eq!(services.loader.transition().duration, 30);
    assert!(services.updates >= 2);
    let target = frames.draw(&gpu, &mut services.loader, PlaneId::new(0));
    assert!(target.chunks_exact(4).all(|p| p == [0, 0, 255, 255]));
    println!(
        "GEROKASU2_REPLACEMENT_RUNNER commands=4 frames={} channels={} decoded_TRANSSET_LayerLoad_AUTOWIPE_owned_wait_root_FrameId_update_retirement_EXIT=verified audio=Loopback Audio",
        frames.frames, frames.channels
    );
}
