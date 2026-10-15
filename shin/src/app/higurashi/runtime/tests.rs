use super::*;
use crate::app::higurashi::{
    gameplay::title_slot::ActiveScene,
    load_game::{Paths, Source},
};
use shin_core::format::save::higurashi::SaveData;
use shin_input::inputs::MouseButton;
use shin_render::{
    dynamic_buffer::DynamicBuffer,
    pipelines::PipelineStorage,
    shaders::types::{
        buffer::BytesAddress,
        texture::{TextureSamplerStore, TextureTarget, TextureTargetKind},
    },
};
use std::{
    sync::{
        Condvar, Mutex as StdMutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};
use winit::keyboard::KeyCode;

mod journeys;
mod launch_input;
mod presentation;
mod touch_effects;

#[test]
fn higurashi_window_runtime_keeps_large_scene_storage_off_callback_stack() {
    let runtime = std::mem::size_of::<Runtime>();
    let slot = std::mem::size_of::<TitleSlot>();
    let app = std::mem::size_of::<super::super::window_app::HigurashiApp>();
    eprintln!(
        "Higurashi owner sizes:Runtime={runtime},TitleSlot={slot},ShinApp={app};scene Slot is heap-owned"
    );
    assert!(
        runtime < 256 * 1024 && app < 256 * 1024,
        "window callback owner must not embed all possible large scenes"
    );
}
#[test]
fn higurashi_pointer_viewport_normalizes_once_at_both_output_sizes() {
    for size in [Vec2::new(1280., 720.), Vec2::new(1920., 1080.)] {
        let viewport = InputViewport::new(Vec2::new(51., 37.), size).unwrap();
        assert_eq!(
            viewport.touch_position(viewport.offset + size * 0.5),
            Vec2::new(640., 360.)
        );
        assert!(!viewport.contains(viewport.offset - Vec2::X));
        assert!(!viewport.contains(viewport.offset + size));
        assert!(viewport.contains(viewport.offset + size - Vec2::ONE));
    }
    assert!(InputViewport::new(Vec2::ZERO, Vec2::ZERO).is_err());
    assert!(InputViewport::new(Vec2::ZERO, Vec2::new(f32::NAN, 720.)).is_err());
}

struct Graphics {
    device: wgpu::Device,
    queue: wgpu::Queue,
    resize: SurfaceResizeSource,
    samplers: TextureSamplerStore,
    pipelines: PipelineStorage,
    dynamic: DynamicBuffer,
    _depth: wgpu::Texture,
    depth_view: wgpu::TextureView,
}
impl Graphics {
    fn new(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        let depth = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Runtime test depth"),
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
        Self {
            samplers: TextureSamplerStore::new(&device),
            pipelines: PipelineStorage::new(device.clone(), wgpu::TextureFormat::Rgba8Unorm),
            dynamic: DynamicBuffer::new(device.clone(), BytesAddress::new(1024 * 1024)),
            resize: SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
                1920, 1080,
            ))),
            _depth: depth,
            depth_view,
            device,
            queue,
        }
    }
    fn context<'a>(
        &'a mut self,
        encoder: &'a mut wgpu::CommandEncoder,
    ) -> PreRenderContext<'a, 'a, 'a, 'a> {
        PreRenderContext {
            device: &self.device,
            queue: &self.queue,
            breakup_texture: None,
            resize_source: &self.resize,
            sampler_store: &self.samplers,
            depth_stencil: DepthStencilTarget {
                view: &self.depth_view,
            },
            pipeline_storage: &mut self.pipelines,
            dynamic_buffer: &mut self.dynamic,
            encoder,
        }
    }
    fn update(
        &mut self,
        runtime: &mut Runtime,
        raw: &RawInputState,
        viewport: InputViewport,
        elapsed: Duration,
    ) -> Frame {
        self.update_with_permission(runtime, raw, viewport, elapsed, true)
    }
    fn update_with_permission(
        &mut self,
        runtime: &mut Runtime,
        raw: &RawInputState,
        viewport: InputViewport,
        elapsed: Duration,
        permission: bool,
    ) -> Frame {
        self.dynamic.recall();
        runtime.raw_input(raw);
        let before = runtime.globals().unwrap().map(|g| g.clock.ticks());
        let frozen = runtime.freezes_scene();
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let frame = shin_tasks::block_on(runtime.update(
            &mut self.context(&mut encoder),
            elapsed,
            viewport,
            permission,
            Policy {
                animations_enabled: true,
                paused: false,
                fast_skip: false,
                suppress_transient_audio: false,
                read_flags: 0,
                text_speed: 50,
                outline_mode: 0,
            },
            AudioFramePolicy {
                focus_mode_active: true,
                voice_focus_enabled: true,
                effects_update_enabled: true,
                music_rate: 1.,
            },
        ))
        .unwrap();
        if let (Some(before), Some(after)) =
            (before, runtime.globals().unwrap().map(|g| g.clock.ticks()))
        {
            if matches!(frame, Frame::Advanced(_)) && !frozen {
                assert_eq!(
                    after,
                    before + Ticks::from_duration(elapsed).as_f32() as f64,
                    "one full elapsed clock tail per callback"
                );
            } else {
                assert_eq!(after, before, "close UI/writer cannot advance scene clock");
            }
        }
        let mut uploads = self.device.create_command_encoder(&Default::default());
        self.dynamic.finish(&mut uploads);
        self.queue.submit([uploads.finish(), encoder.finish()]);
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        frame
    }
    fn pixels(&mut self, runtime: &Runtime, extent: (u32, u32), capture: bool) -> Vec<u8> {
        self.dynamic.recall();
        let target = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Runtime presentation"),
            size: wgpu::Extent3d {
                width: extent.0,
                height: extent.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let depth = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Runtime presentation window stencil"),
            size: target.size(),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: shin_render::DEPTH_STENCIL_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let depth_view = depth.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let mut context = self.context(&mut encoder);
        let copy = if capture {
            runtime.capture_composed(&mut context)
        } else {
            None
        };
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    view: &view,
                    kind: TextureTargetKind::Screen,
                },
                Some(DepthStencilTarget { view: &depth_view }),
                "Runtime pure presentation",
            );
            pass.clear(Some(UnormColor::BLACK), None, None);
            if let Some(copy) = &copy {
                Runtime::render_texture(copy, &mut pass);
            } else {
                runtime.render(&mut pass);
            }
        }
        crate::layer::user::readback_test_texture(&mut context, &target)
    }
}
struct Store {
    paths: Paths,
    fail: AtomicBool,
    gate: (StdMutex<bool>, Condvar),
    writes: StdMutex<Vec<Vec<u8>>>,
}
impl Storage for Store {
    fn trial_token(&self) -> bool {
        false
    }
    fn read(&self, source: Source) -> Result<Option<Vec<u8>>> {
        self.paths.read(source)
    }
    fn trial_exists(&self) -> Result<bool> {
        Ok(false)
    }
    fn write(&self, bytes: &[u8]) -> Result<()> {
        self.writes.lock().unwrap().push(bytes.to_vec());
        if self.fail.load(Ordering::SeqCst) {
            anyhow::bail!("controlled desktop write failure");
        }
        let mut open = self.gate.0.lock().unwrap();
        while !*open {
            open = self.gate.1.wait(open).unwrap();
        }
        self.paths.write(bytes)
    }
}
struct Release(Arc<Store>);
impl Drop for Release {
    fn drop(&mut self) {
        *self.0.gate.0.lock().unwrap() = true;
        self.0.gate.1.notify_all();
    }
}
fn viewport(size: (u32, u32)) -> InputViewport {
    InputViewport::new(Vec2::new(40., 20.), Vec2::new(size.0 as f32, size.1 as f32)).unwrap()
}
fn key(key: KeyCode) -> RawInputState {
    let mut raw = RawInputState::new();
    raw.keyboard.insert(key);
    raw
}
fn mouse(position: Vec2, held: bool) -> RawInputState {
    let mut raw = RawInputState::new();
    raw.mouse.position = position;
    raw.mouse.buttons[MouseButton::Left] = held;
    raw
}
fn tick(
    gpu: &mut Graphics,
    runtime: &mut Runtime,
    raw: &RawInputState,
    viewport: InputViewport,
) -> Frame {
    gpu.update(runtime, raw, viewport, Duration::from_millis(50))
}
fn notice(runtime: &Runtime) -> Option<(i32, Vec2)> {
    runtime.bootstrap.as_ref()?.reference_notice()
}
fn ready_menu(runtime: &Runtime, kind: &str) -> bool {
    runtime
        .slot()
        .and_then(|s| s.title.as_ref())
        .and_then(|t| t.reference_menu_state())
        .is_some_and(|(k, p)| k == kind && p == 2)
}

#[test]
#[ignore = "normal Runtime boot/mandatory pointer notice/720p→1080p resize/TitleDemo/title/authored story/pure composed presentation and held close-writer failure/retry; offscreen Metal/exact Loopback Audio; temporary progress only"]
fn higurashi_runtime_runs_normal_boot_story_and_frozen_close_retry() {
    let (device, queue, server) = crate::adv::higurashi::reference_gpu_assets();
    let server = Arc::new(server);
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let sounds = Arc::new(Mutex::new(SystemSoundPlayer::new(manager.clone())));
    let dir = std::env::temp_dir().join(format!(
        "higurashi-normal-runtime-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let paths = Paths {
        current: Some(dir.join("progress.dat")),
        trial: None,
    };
    let mut saved = SaveData::default();
    saved.play_seconds = 17;
    saved.settings.common.v3_sysvol = 37;
    paths.write(&saved.encode().unwrap()).unwrap();
    let store = Arc::new(Store {
        paths,
        fail: AtomicBool::new(true),
        gate: (StdMutex::new(false), Condvar::new()),
        writes: StdMutex::new(Vec::new()),
    });
    let mut runtime = Runtime::new(
        server.clone(),
        manager.clone(),
        sounds.clone(),
        store.clone(),
    );
    let mut gpu = Graphics::new(device, queue);
    // This guard drops before Runtime, including assertions while IO is held.
    let _release = Release(store.clone());
    let deadline = Instant::now() + Duration::from_secs(90);
    let small = viewport((1280, 720));
    let large = viewport((1920, 1080));
    let blank = RawInputState::new();
    while !notice(&runtime).is_some_and(|(phase, _)| phase == 2) {
        tick(&mut gpu, &mut runtime, &blank, small);
        assert!(Instant::now() < deadline, "mandatory notice not ready");
    }
    let original_common = runtime.globals().unwrap().unwrap().common.clone();
    assert!(Arc::ptr_eq(
        runtime.globals().unwrap().unwrap().manager,
        &manager
    ));
    assert_eq!(sounds.lock().volume_percent, 37);
    let logical = notice(&runtime).unwrap().1;
    let at = |viewport: InputViewport| {
        viewport.offset + logical / Vec2::new(1920., 1080.) * viewport.size
    };
    tick(&mut gpu, &mut runtime, &mouse(at(small), true), small);
    tick(&mut gpu, &mut runtime, &mouse(at(large), true), large);
    tick(&mut gpu, &mut runtime, &mouse(at(large), false), large);
    assert_eq!(
        notice(&runtime).unwrap().0,
        2,
        "resize cancels held notice contact instead of answering"
    );
    tick(&mut gpu, &mut runtime, &mouse(at(large), true), large);
    tick(&mut gpu, &mut runtime, &mouse(at(large), false), large);
    assert_eq!(
        notice(&runtime).unwrap().0,
        3,
        "1080p physical pointer answers the actual native notice"
    );
    while !runtime
        .bootstrap
        .as_ref()
        .is_some_and(SceneHost::reference_demo_ready)
    {
        tick(&mut gpu, &mut runtime, &blank, large);
        assert!(Instant::now() < deadline, "actual opening movie not ready");
    }
    tick(&mut gpu, &mut runtime, &key(KeyCode::Enter), large);
    tick(&mut gpu, &mut runtime, &blank, large);
    while runtime.slot().is_none() {
        tick(&mut gpu, &mut runtime, &blank, large);
        assert!(
            Instant::now() < deadline,
            "owned title handoff not accepted"
        );
    }
    assert_eq!(
        runtime.slot().unwrap().active_scene().unwrap(),
        ActiveScene::Title
    );
    assert!(Arc::ptr_eq(
        runtime.globals().unwrap().unwrap().common,
        &original_common
    ));
    while !ready_menu(&runtime, "top") {
        tick(&mut gpu, &mut runtime, &blank, large);
        assert!(Instant::now() < deadline, "top menu not ready");
    }
    // Fifteen platform ticks still advance the scene by its three-tick clamp.
    // Graphics::update verifies the single full15-tick global tail here.
    gpu.update(&mut runtime, &blank, large, Duration::from_millis(250));
    for extent in [(1280, 720), (1920, 1080)] {
        let before = runtime.globals().unwrap().unwrap().clock.ticks();
        let frames = runtime.frames;
        let a = gpu.pixels(&runtime, extent, false);
        let b = gpu.pixels(&runtime, extent, true);
        let c = gpu.pixels(&runtime, extent, false);
        assert!(
            a.chunks_exact(4).any(|pixel| pixel[..3] != [0, 0, 0]),
            "ready title composition must contain its real artwork"
        );
        assert_eq!(a, b, "whole composed clone must be exact at{extent:?}");
        assert_eq!(a, c, "repeated render must be a getter");
        assert_eq!(runtime.frames, frames);
        assert_eq!(runtime.globals().unwrap().unwrap().clock.ticks(), before);
    }
    {
        let mut globals = runtime.slot.as_mut().unwrap().active_globals_mut().unwrap();
        let slots = globals.runtime_saves.as_mut().unwrap();
        slots.manual[99].valid = true;
        slots.manual[99].data.progress.extra_word = 0xdead_beef;
        slots.manual[99].data.progress.entries[1].response_data = vec![0xf1, 0xe2];
        slots.manual[99].data.progress.set_active_index(1).unwrap();
        slots.manual[99].data.trailing_values = vec![0xffff, 0x1234];
    }
    tick(&mut gpu, &mut runtime, &key(KeyCode::Enter), large);
    tick(&mut gpu, &mut runtime, &blank, large);
    while !ready_menu(&runtime, "cat") {
        tick(&mut gpu, &mut runtime, &blank, large);
        assert!(Instant::now() < deadline, "story category not ready");
    }
    tick(&mut gpu, &mut runtime, &key(KeyCode::Enter), large);
    tick(&mut gpu, &mut runtime, &blank, large);
    while !runtime.slot().is_some_and(|s| s.game.is_some()) {
        tick(&mut gpu, &mut runtime, &blank, large);
        assert!(
            Instant::now() < deadline,
            "actual authored story factory not accepted"
        );
    }
    while runtime
        .slot()
        .unwrap()
        .game
        .as_ref()
        .unwrap()
        .host()
        .unwrap()
        .runner
        .completed_commands()
        < 10
    {
        tick(&mut gpu, &mut runtime, &blank, large);
        assert!(
            Instant::now() < deadline,
            "authored story initialization stalled"
        );
    }
    while !matches!(
        runtime
            .slot()
            .unwrap()
            .game
            .as_ref()
            .unwrap()
            .host()
            .unwrap()
            .runner
            .pending_command(),
        Some(
            shin_core::vm::higurashi::command::HigurashiRuntimeCommand::MSGWAIT { .. }
                | shin_core::vm::higurashi::command::HigurashiRuntimeCommand::MSGSET {
                    auto_wait: 1,
                    ..
                }
        )
    ) {
        tick(&mut gpu, &mut runtime, &blank, large);
        assert!(
            Instant::now() < deadline,
            "authored first message stalled at {:?}",
            runtime
                .slot()
                .unwrap()
                .game
                .as_ref()
                .unwrap()
                .host()
                .unwrap()
                .runner
                .pending_command()
        );
    }
    for _ in 0..8 {
        tick(&mut gpu, &mut runtime, &blank, large);
    }
    let commands = runtime
        .slot()
        .unwrap()
        .game
        .as_ref()
        .unwrap()
        .host()
        .unwrap()
        .runner
        .completed_commands();
    assert!(Arc::ptr_eq(
        runtime.globals().unwrap().unwrap().common,
        &original_common
    ));
    for extent in [(1280, 720), (1920, 1080)] {
        let game = runtime.slot().unwrap().game.as_ref().unwrap();
        let before = game
            .host()
            .unwrap()
            .runner
            .frame()
            .cache
            .reference_snapshot();
        let a = gpu.pixels(&runtime, extent, false);
        let b = gpu.pixels(&runtime, extent, true);
        assert!(
            a.chunks_exact(4).any(|pixel| pixel[..3] != [0, 0, 0]),
            "first authored message must be visibly composed"
        );
        assert_eq!(a, b);
        assert_eq!(
            runtime
                .slot()
                .unwrap()
                .game
                .as_ref()
                .unwrap()
                .host()
                .unwrap()
                .runner
                .frame()
                .cache
                .reference_snapshot(),
            before,
            "outer getter cannot prepare ADV cache"
        );
    }
    runtime.request_close().unwrap();
    while runtime.shutdown.failure_phase() != Some(2) {
        tick(&mut gpu, &mut runtime, &blank, large);
        assert!(
            Instant::now() < deadline,
            "shutdown failure notice not ready"
        );
    }
    let frozen_clock = runtime.globals().unwrap().unwrap().clock.ticks();
    assert_eq!(store.writes.lock().unwrap().len(), 1);
    // A focused desktop retry notice obeys the same outer input gate as
    // bootstrap. Continue its UI/worker polling while denying both A and B.
    for (key_code, bit) in [(KeyCode::Enter, 24), (KeyCode::Escape, 25)] {
        gpu.update_with_permission(
            &mut runtime,
            &blank,
            large,
            Duration::from_millis(50),
            false,
        );
        gpu.update_with_permission(
            &mut runtime,
            &key(key_code),
            large,
            Duration::from_millis(50),
            false,
        );
        assert_ne!(runtime.input.buttons().pressed & (1 << bit), 0);
        assert_eq!(runtime.shutdown.failure_phase(), Some(2));
        assert_eq!(store.writes.lock().unwrap().len(), 1);
        assert_eq!(
            runtime.globals().unwrap().unwrap().clock.ticks(),
            frozen_clock
        );
        gpu.update_with_permission(
            &mut runtime,
            &key(key_code),
            large,
            Duration::from_millis(50),
            true,
        );
        assert_eq!(runtime.input.buttons().pressed & (1 << bit), 0);
        assert_eq!(runtime.shutdown.failure_phase(), Some(2));
    }
    tick(&mut gpu, &mut runtime, &blank, large);
    let first = store.writes.lock().unwrap()[0].clone();
    let data = SaveData::decode(&first).unwrap();
    assert_eq!(data.manual_save_slots[99].as_ref().unwrap().extra_word, 7);
    assert_eq!(
        runtime
            .globals()
            .unwrap()
            .unwrap()
            .runtime_saves
            .unwrap()
            .manual[99]
            .data
            .progress
            .extra_word,
        0xdead_beef
    );
    // A subsequent live mutation must never replace the writer's frozen retry.
    runtime
        .slot
        .as_mut()
        .unwrap()
        .active_globals_mut()
        .unwrap()
        .runtime_saves
        .as_mut()
        .unwrap()
        .manual[99]
        .data
        .progress
        .extra_word = 0xbbbb0005;
    store.fail.store(false, Ordering::SeqCst);
    tick(&mut gpu, &mut runtime, &key(KeyCode::Enter), large);
    tick(&mut gpu, &mut runtime, &blank, large);
    while store.writes.lock().unwrap().len() < 2 {
        tick(&mut gpu, &mut runtime, &blank, large);
        assert!(Instant::now() < deadline, "actual retry not started");
    }
    assert_eq!(
        store.writes.lock().unwrap()[1],
        first,
        "retry must use same frozen settings/full rows"
    );
    let retirement = runtime.shutdown.writer_retirement().unwrap();
    for _ in 0..12 {
        assert_eq!(tick(&mut gpu, &mut runtime, &blank, large), Frame::Closing);
        assert_eq!(
            runtime.globals().unwrap().unwrap().clock.ticks(),
            frozen_clock
        );
        assert!(!retirement.is_finished());
    }
    *store.gate.0.lock().unwrap() = true;
    store.gate.1.notify_all();
    while tick(&mut gpu, &mut runtime, &blank, large) != Frame::Exit {
        assert!(
            Instant::now() < deadline,
            "close did not wait for/accept successful writer"
        );
    }
    assert!(retirement.is_finished());
    assert!(runtime.slot.is_none() && runtime.bootstrap.is_none());
    assert_eq!(
        std::fs::read(store.paths.current.as_ref().unwrap()).unwrap(),
        first
    );
    std::fs::remove_dir_all(&dir).unwrap();
    eprintln!(
        "Higurashi normal Runtime:real boot/notice pointer resize/movie/title/first authored message({commands} commands),one full-clock tail including15-vs3 clamp,720p/1080p visible pure composed captures/ADV-cache getters,full rows,failure+same-snapshot retry,12 held IO callbacks and source retirement before exit PASS;{} composed callbacks",
        runtime.frames
    );
}
