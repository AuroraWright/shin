use super::*;
mod backlog;
mod first_boot;
mod message_properties;
mod playback_modes;
mod restoration;
mod system_menu;
mod title_demo;
mod transitions;
use crate::{
    adv::gerokasu2::{layer_factory::GerDrawable, play_scene::SceneEvent},
    asset::{font::gerokasu2::draw::gpu_tests::Offscreen, system::LayeredAssetIo},
};
use binrw::BinWrite;
use clap::Parser;
use shin_core::{
    format::{
        rom::RomReader,
        scenario::instruction_elements::{NumberSpec, Register},
    },
    primitives::stateless_reader::StatelessFile,
    vm::command::types::LayerId,
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
    io::{Cursor, Read},
    time::Instant,
};

#[test]
fn ger_app_viewport_mapping_excludes_bars_edges_and_invalid_sizes() {
    let v = Viewport {
        offset: Vec2::new(120.0, 40.0),
        size: Vec2::new(960.0, 540.0),
    };
    assert_eq!(v.map(v.offset), Some(Vec2::ZERO));
    assert_eq!(
        v.map(Vec2::new(600.0, 310.0)),
        Some(Vec2::new(960.0, 540.0))
    );
    for p in [
        Vec2::new(119.0, 310.0),
        Vec2::new(600.0, 39.0),
        Vec2::new(1080.0, 310.0),
        Vec2::new(600.0, 580.0),
        Vec2::NAN,
    ] {
        assert_eq!(v.map(p), None);
    }
    for size in [Vec2::ZERO, Vec2::new(-1.0, 540.0), Vec2::INFINITY] {
        assert_eq!(Viewport { size, ..v }.map(v.offset), None);
    }
}
#[test]
fn ger_app_options_allow_native_save_import_and_reject_unimplemented_debug_entries() {
    assert_eq!(Cli::try_parse_from(["shin"]).unwrap().game, Game::Umineko);
    assert!(validate_options(&Cli::try_parse_from(["shin"]).unwrap()).is_err());
    assert!(
        validate_options(&Cli::try_parse_from(["shin", "--game", "gerokasu2"]).unwrap()).is_ok()
    );
    for option in ["--save-file", "--load-save"] {
        let cli = Cli::try_parse_from(["shin", "--game", "gerokasu2", option, "1"]).unwrap();
        assert!(validate_options(&cli).is_ok());
    }
    for option in ["--unsafe-entry-point", "--fast-forward-to"] {
        let cli = Cli::try_parse_from(["shin", "--game", "gerokasu2", option, "1"]).unwrap();
        assert!(validate_options(&cli).is_err());
    }
}
pub(super) fn bytes() -> Vec<u8> {
    let mut rom = RomReader::new(
        StatelessFile::new(
            std::fs::File::open(std::env::var_os("SHIN_GEROKASU2_ROM").unwrap()).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let entry = rom.find_file("/main.snr").unwrap();
    let mut data = Vec::new();
    rom.open_file(entry)
        .unwrap()
        .read_to_end(&mut data)
        .unwrap();
    data
}
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
            bustup_cache: AssetCache::new(),
            higurashi_bustup_cache: Default::default(),

            picture_cache: Default::default(),
            font_cache: Default::default(),
        },
    ))
}
fn host(gpu: &Offscreen, data: Vec<u8>) -> GerHost {
    shin_tasks::block_on(GerHost::load(
        assets(gpu),
        Arc::new(Gerokasu2Scenario::new(data.into()).unwrap()),
        Arc::new(AudioManager::new_for_tests()),
        42,
    ))
    .unwrap()
}
pub(super) fn num(d: &mut Vec<u8>, v: i32) {
    let mut c = Cursor::new(Vec::new());
    NumberSpec::<i32>::constant(v).write_le(&mut c).unwrap();
    d.extend(c.into_inner());
}
struct Frames {
    target: wgpu::Texture,
    view: wgpu::TextureView,
    depth: wgpu::TextureView,
    resize: SurfaceResizeSource,
    samplers: TextureSamplerStore,
    pipelines: PipelineStorage,
    dynamic: DynamicBuffer,
    frames: usize,
}
impl Frames {
    fn new(gpu: &Offscreen) -> Self {
        let descriptor = |format, usage| wgpu::TextureDescriptor {
            label: Some("Ger app frame"),
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
        }
    }
    fn draw(&mut self, gpu: &Offscreen, host: &mut GerHost) -> Vec<u8> {
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        let mut ctx = PreRenderContext {
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
        host.pre_render(&mut ctx).unwrap();
        {
            let mut pass = ctx.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &self.view,
                },
                Some(ctx.depth_stencil),
                "Ger app frame",
            );
            host.render(&mut pass).unwrap();
        }
        self.frames += 1;
        crate::layer::user::readback_test_texture(&mut ctx, &self.target)
    }
}
fn viewport() -> Viewport {
    Viewport {
        offset: Vec2::new(120.0, 40.0),
        size: Vec2::new(960.0, 540.0),
    }
}
fn tick(
    host: &mut GerHost,
    frame: &mut FrameId,
    input: &EnumMap<AppAction, ActionState>,
    point: Vec2,
    ticks: f32,
) -> RunnerStatus {
    frame.advance();
    host.update(
        *frame,
        Ticks::from_f32(ticks),
        input,
        point,
        viewport(),
        1.0 / 60.0,
    )
    .unwrap()
}
fn held(action: AppAction) -> EnumMap<AppAction, ActionState> {
    let mut input = EnumMap::default();
    input[action] = ActionState {
        is_held: true,
        is_clicked: true,
        is_clicked_or_repeated: true,
        is_clicked_or_rapid_repeated: true,
    };
    input
}
#[test]
#[ignore = "requires supplied ROM,Metal and exact Loopback Audio"]
fn ger_app_pointer_capture_focus_suspend_and_actual_quiz_result() {
    let gpu = Offscreen::new();
    let original = Gerokasu2Scenario::new(bytes().into()).unwrap();
    let mut d = bytes();
    d.truncate(original.header.code_offset as usize);
    // Interruptible WAIT, then actual two-card widget, QUIZ and EXIT.
    d.extend([0x83, 1]);
    num(&mut d, 600);
    d.push(0xc1);
    for v in [3, 9, 0] {
        num(&mut d, v);
    }
    d.push(2);
    num(&mut d, 5);
    d.extend([0xe0, 0, 0]);
    num(&mut d, 3);
    d.extend([0, 0]);
    num(&mut d, 0);
    let size = d.len() as u32;
    d[4..8].copy_from_slice(&size.to_le_bytes());
    let mut host = host(&gpu, d);
    let mut frame = FrameId::default();
    let idle = EnumMap::default();
    let mouse = held(AppAction::PointerClick);
    let center = Vec2::new(600.0, 310.0);
    tick(&mut host, &mut frame, &idle, center, 0.0);
    assert_eq!(host.scene.runner().completed_commands(), 0);
    tick(&mut host, &mut frame, &mouse, Vec2::ZERO, 0.0); // Press in bar.
    tick(&mut host, &mut frame, &idle, center, 0.0); // Release inside cannot advance.
    assert_eq!(host.scene.runner().completed_commands(), 0);
    tick(&mut host, &mut frame, &mouse, center, 0.0);
    host.cancel_input();
    tick(&mut host, &mut frame, &mouse, center, 0.0);
    tick(&mut host, &mut frame, &idle, center, 0.0);
    assert_eq!(host.scene.runner().completed_commands(), 0);
    host.suspended = true;
    tick(&mut host, &mut frame, &idle, center, 1000.0);
    assert_eq!(host.scene.runner().completed_commands(), 0);
    host.suspended = false;
    tick(&mut host, &mut frame, &mouse, center, 0.0);
    tick(&mut host, &mut frame, &idle, center, 0.0);
    let deadline = Instant::now() + Duration::from_secs(15);
    while !host.scene.card_ready() {
        tick(&mut host, &mut frame, &idle, center, 30.0);
        assert!(
            Instant::now() < deadline,
            "{:?}",
            host.scene.runner().pending_command()
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    let target = host
        .scene
        .runner()
        .layers()
        .target(LayerId::new(3))
        .unwrap();
    let guard = host
        .scene
        .backend()
        .layer(
            host.scene.runner().layers().current_plane(),
            target.layerbank,
        )
        .unwrap();
    let GerDrawable::Card(layer) = &guard.layer else {
        panic!("actual CardSelect");
    };
    let m = layer.inner_ref().model.lock();
    let card = Vec2::new(m.cards[1].tweens[2].value(), m.cards[1].tweens[3].value());
    drop(m);
    drop(guard);
    let p = viewport().offset + card * viewport().size / Vec2::new(1920.0, 1080.0);
    let mut frames = Frames::new(&gpu);
    let pixels = frames.draw(&gpu, &mut host);
    assert!(pixels.chunks_exact(4).any(|p| p[..3] != [0, 0, 0]));
    tick(&mut host, &mut frame, &mouse, p, 0.0);
    host.cancel_input();
    tick(&mut host, &mut frame, &idle, p, 0.0);
    assert!(host.scene.card_ready());
    // A held confirm inherited through focus loss must also wait for release.
    host.cancel_input();
    tick(&mut host, &mut frame, &held(AppAction::Enter), p, 0.0);
    assert!(host.scene.card_ready());
    tick(&mut host, &mut frame, &idle, p, 0.0);
    tick(&mut host, &mut frame, &mouse, p, 0.0);
    tick(&mut host, &mut frame, &idle, p, 0.0);
    while host.status != RunnerStatus::Exited {
        tick(&mut host, &mut frame, &idle, p, 30.0);
        frames.draw(&gpu, &mut host);
        assert!(Instant::now() < deadline);
    }
    assert_eq!(host.scene.runner().completed_commands(), 4);
    assert_eq!(
        host.scene
            .runner()
            .vm()
            .read_register(Register::from_regular_register(0)),
        2
    );
    println!(
        "GEROKASU2_APP_INPUT commands=4 bar_press_focus_cancel_suspend_inherited_confirm_real_pointer_QUIZ_EXIT=verified frames={} channels={} audio=Loopback Audio",
        frames.frames,
        frames.frames * 320 * 180 * 4
    );
}
#[test]
#[ignore = "requires supplied ROM,Metal and exact Loopback Audio"]
fn ger_app_authored_prologue_keyboard_and_root_render_without_external_ack() {
    let gpu = Offscreen::new();
    let mut host = host(&gpu, bytes());
    let mut frame = FrameId::default();
    let mut frames = Frames::new(&gpu);
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut messages = 0;
    let mut visible = false;
    let mut titles = Vec::new();
    let mut captures = 0;
    let mut pressed = false;
    while messages < 20 || host.scene.runner().completed_commands() < 100 {
        pressed = !pressed;
        let input = if pressed {
            held(AppAction::Enter)
        } else {
            EnumMap::default()
        };
        let status = tick(&mut host, &mut frame, &input, Vec2::new(600.0, 310.0), 60.0);
        assert_ne!(status, RunnerStatus::Exited);
        for event in host.scene.take_events() {
            match event {
                SceneEvent::MessageInstalled(_, _) => messages += 1,
                SceneEvent::AutosaveCaptured(_) => captures += 1,
                SceneEvent::SceneTitle(title) => titles.push(title),
                _ => {}
            }
        }
        if messages > 0 {
            let pixels = frames.draw(&gpu, &mut host);
            visible |= pixels.chunks_exact(4).any(|p| p[..3] != [0, 0, 0]);
        }
        assert!(
            Instant::now() < deadline,
            "{:?}",
            host.scene.runner().pending_command()
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(visible);
    assert_eq!(titles, ["プロローグ"]);
    assert!(captures >= messages + 1);
    println!(
        "GEROKASU2_APP_STORY commands={} messages={messages} captures={captures} title=1 fresh_story_keyboard_root_render_no_external_ack=verified frames={} channels={} audio=Loopback Audio",
        host.scene.runner().completed_commands(),
        frames.frames,
        frames.frames * 320 * 180 * 4
    );
}
pub(super) fn msg(d: &mut Vec<u8>, wire_id: u32, text: &str) {
    d.push(0x86);
    d.extend(&wire_id.to_le_bytes()[..3]);
    d.push(1); // auto-wait; actual message owner retains the command.
    num(d, 0);
    d.extend(((text.len() + 1) as u16).to_le_bytes());
    d.extend(text.as_bytes());
    d.push(0);
}
fn sys(d: &mut Vec<u8>, id: i32, argument: i32) {
    d.push(0xa6);
    num(d, id);
    num(d, argument);
}
fn retained(host: &GerHost) -> String {
    host.scene
        .runner()
        .message()
        .retained_text()
        .unwrap_or("")
        .to_owned()
}
#[test]
#[ignore = "requires supplied ROM,Metal and exact Loopback Audio"]
fn ger_app_native_read_skip_autoplay_sys_gates_and_new_message_recheck() {
    let gpu = Offscreen::new();
    let original = Gerokasu2Scenario::new(bytes().into()).unwrap();
    let mut d = bytes();
    d.truncate(original.header.code_offset as usize);
    sys(&mut d, 0, 1);
    d.push(0x85);
    num(&mut d, 0);
    msg(&mut d, 1, "@r最初の未読");
    msg(&mut d, 1, "@r同じIDの既読");
    msg(&mut d, 2, "@r次の未読");
    sys(&mut d, 0, 0);
    d.extend([0x83, 1]);
    num(&mut d, 600);
    sys(&mut d, 0, 1);
    msg(&mut d, 3, "@rオートで進む");
    d.extend([0, 0]);
    num(&mut d, 0);
    let size = d.len() as u32;
    d[4..8].copy_from_slice(&size.to_le_bytes());
    let mut host = host(&gpu, d);
    let mut frame = FrameId::default();
    let idle = EnumMap::default();
    let point = Vec2::new(600.0, 310.0);
    let mut frames = Frames::new(&gpu);
    let deadline = Instant::now() + Duration::from_secs(15);
    tick(&mut host, &mut frame, &idle, point, 60.0);
    assert_eq!(retained(&host), "@r最初の未読");
    assert!(!host.scene.playback().was_read);
    tick(
        &mut host,
        &mut frame,
        &held(AppAction::ToggleSkip),
        point,
        60.0,
    );
    for _ in 0..20 {
        tick(&mut host, &mut frame, &idle, point, 30.0);
    }
    assert_eq!(retained(&host), "@r最初の未読");
    assert!(!host.scene.playback().seen_messages.contains(0));
    assert!(host.scene.playback().modes.skip);
    assert!(!host.scene.playback().skip(-1, false));
    frames.draw(&gpu, &mut host);
    // Manual confirmation marks read at final completion, then the repeated
    // wire ID is classified from the real global bitset on installation.
    let mut press = true;
    while retained(&host) == "@r最初の未読" {
        let input = if press {
            held(AppAction::Enter)
        } else {
            EnumMap::default()
        };
        tick(&mut host, &mut frame, &input, point, 30.0);
        press = !press;
        assert!(Instant::now() < deadline);
    }
    assert_eq!(retained(&host), "@r同じIDの既読");
    assert!(host.scene.playback().was_read && host.scene.playback().seen_messages.contains(0));
    assert_eq!(
        host.scene
            .runner()
            .message()
            .message()
            .unwrap()
            .clock()
            .params
            .flags
            & 1,
        1
    );
    // Native 11650 excludes A from its clear mask: confirming kept skip
    // latched, so the read message now skips on its own.
    assert!(host.scene.playback().modes.skip);
    tick(&mut host, &mut frame, &idle, point, 30.0);
    while retained(&host) != "@r次の未読" {
        tick(&mut host, &mut frame, &idle, point, 30.0);
        assert!(Instant::now() < deadline);
    }
    assert!(host.scene.playback().modes.skip);
    assert!(!host.scene.playback().was_read);
    assert!(!host.scene.playback().seen_messages.contains(1));
    for _ in 0..20 {
        tick(&mut host, &mut frame, &idle, point, 30.0);
    }
    assert_eq!(
        retained(&host),
        "@r次の未読",
        "Post-pump policy must stop at new unread text"
    );
    frames.draw(&gpu, &mut host);
    let mut both = held(AppAction::ToggleAuto);
    both[AppAction::ToggleSkip] = held(AppAction::ToggleSkip)[AppAction::ToggleSkip];
    tick(&mut host, &mut frame, &both, point, 30.0);
    assert!(host.scene.playback().modes.autoplay && host.scene.playback().modes.skip);
    while host.scene.playback().system_controls {
        tick(&mut host, &mut frame, &idle, point, 30.0);
        assert!(Instant::now() < deadline);
    }
    assert!(host.scene.playback().seen_messages.contains(1));
    assert!(!host.scene.playback().modes.autoplay && !host.scene.playback().modes.skip);
    tick(&mut host, &mut frame, &both, point, 0.0);
    assert!(!host.scene.playback().modes.autoplay && !host.scene.playback().modes.skip);
    tick(
        &mut host,
        &mut frame,
        &held(AppAction::HoldSkip),
        point,
        0.0,
    );
    assert!(!host.scene.playback().skip(-1, false));
    assert!(!host.scene.playback().system_controls);
    tick(&mut host, &mut frame, &idle, point, 0.0);
    tick(&mut host, &mut frame, &held(AppAction::Enter), point, 30.0);
    assert!(host.scene.playback().system_controls);
    assert_eq!(retained(&host), "@rオートで進む");
    tick(&mut host, &mut frame, &idle, point, 30.0);
    tick(
        &mut host,
        &mut frame,
        &held(AppAction::ToggleAuto),
        point,
        30.0,
    );
    while host.status != RunnerStatus::Exited {
        tick(&mut host, &mut frame, &idle, point, 30.0);
        assert!(Instant::now() < deadline);
    }
    assert!(host.scene.playback().seen_messages.contains(2));
    assert_eq!(host.scene.runner().completed_commands(), 10);
    frames.draw(&gpu, &mut host);
    println!(
        "GEROKASU2_PLAYBACK_SCENE commands=10 message_installs=4 seen_ids=3 unread_stop_post_pump_recheck_autoplay_SYS_modes_CTRL_EXIT=verified frames={} channels={} audio=Loopback Audio",
        frames.frames,
        frames.frames * 320 * 180 * 4
    );
}
fn choice(d: &mut Vec<u8>, base: u16, index: u16, dest: u16, mask: i32) {
    d.push(0x8d);
    d.extend(base.to_le_bytes());
    d.extend(index.to_le_bytes());
    d.extend(dest.to_le_bytes());
    num(d, mask);
    let title = "タイトル";
    d.extend(((title.len() + 1) as u16).to_le_bytes());
    d.extend(title.as_bytes());
    d.push(0);
    let labels = ["日本語の選択", "隠される選択", "半角ｶﾅと絵文字😀"].join("\0");
    d.extend(((labels.len() + 2) as u16).to_le_bytes());
    d.extend(labels.as_bytes());
    d.extend([0, 0]);
}
#[test]
#[ignore = "requires supplied ROM,Metal and exact Loopback Audio"]
fn ger_app_select_overlay_keyboard_pointer_focus_history_save_feedback_and_exit() {
    let gpu = Offscreen::new();
    let original = Gerokasu2Scenario::new(bytes().into()).unwrap();
    let mut d = bytes();
    d.truncate(original.header.code_offset as usize);
    choice(&mut d, 8, 2, 0, 5);
    choice(&mut d, 0, 0, 1, 0x8005);
    d.extend([0, 0]);
    num(&mut d, 0);
    let size = d.len() as u32;
    d[4..8].copy_from_slice(&size.to_le_bytes());
    let mut host = host(&gpu, d);
    let mut frame = FrameId::default();
    let idle = EnumMap::default();
    let point = Vec2::new(600.0, 310.0);
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut frames = Frames::new(&gpu);
    let mut captures = 0;
    while !host.scene.selection_ready() {
        tick(&mut host, &mut frame, &idle, point, 6.0);
        assert!(Instant::now() < deadline);
    }
    let owner = host.scene.selection().unwrap();
    assert_eq!(owner.lock().model.indices, [0, 2]);
    assert!(!owner.lock().blocks_playback());
    assert!(
        host.scene
            .reference_audio()
            .system_player
            .bank
            .as_ref()
            .unwrap()
            .sounds
            .contains_key("select")
    );
    let pixels = frames.draw(&gpu, &mut host);
    assert!(pixels.chunks_exact(4).any(|p| p[..3] != [0, 0, 0]));
    tick(&mut host, &mut frame, &held(AppAction::Up), point, 1.0);
    assert_eq!(owner.lock().model.selected, Some(1));
    tick(&mut host, &mut frame, &idle, point, 1.0);
    tick(&mut host, &mut frame, &held(AppAction::Enter), point, 1.0);
    assert!(owner.lock().blocks_playback());
    assert_eq!(
        host.scene
            .runner()
            .vm()
            .read_register(Register::from_regular_register(0)),
        0,
        "Completion waits for closing animation"
    );
    while host.scene.runner().completed_commands() < 1 || !host.scene.selection_ready() {
        tick(&mut host, &mut frame, &idle, point, 6.0);
        frames.draw(&gpu, &mut host);
        assert!(Instant::now() < deadline);
    }
    assert_eq!(
        host.scene
            .runner()
            .vm()
            .read_register(Register::from_regular_register(0)),
        2
    );
    assert!(host.scene.selection_history().seen.contains(9));
    assert_eq!(host.scene.selection_history().last[1], 3);
    let second = host.scene.selection().unwrap();
    assert!(!Arc::ptr_eq(&owner, &second));
    let m = second.lock();
    let virtual_point = Vec2::new(
        960.0,
        m.model.top() + m.model.heights[0] + 32.0 + m.model.heights[1] * 0.5,
    );
    drop(m);
    let p = viewport().offset + virtual_point * viewport().size / Vec2::new(1920.0, 1080.0);
    let mouse = held(AppAction::PointerClick);
    tick(&mut host, &mut frame, &mouse, p, 0.0);
    host.cancel_input();
    tick(&mut host, &mut frame, &idle, p, 0.0);
    assert!(host.scene.selection_ready());
    tick(&mut host, &mut frame, &mouse, p, 0.0);
    tick(&mut host, &mut frame, &idle, p, 0.0);
    while host.status != RunnerStatus::Exited {
        tick(&mut host, &mut frame, &idle, p, 6.0);
        frames.draw(&gpu, &mut host);
        assert!(Instant::now() < deadline);
    }
    for e in host.scene.take_events() {
        if matches!(e, SceneEvent::AutosaveCaptured(_)) {
            captures += 1;
        }
    }
    assert_eq!(host.scene.runner().completed_commands(), 3);
    assert_eq!(
        host.scene
            .runner()
            .vm()
            .read_register(Register::from_regular_register(1)),
        2
    );
    assert!(host.scene.selection().is_none());
    let save = host.scene.save_session();
    assert_eq!(save.timeline.boundary, 4);
    assert_eq!(save.response_bit_position, 8);
    assert_eq!(save.progress.active_entry().responses[0], 0x22);
    assert_eq!(captures, 4);
    println!(
        "GEROKASU2_SELECTION_APP commands=3 choices=2 results=2,2 route_bits=8 captures=4 genuine_keyboard_pointer_focus_history_filtered_index_EXIT=verified frames={} channels={} audio=Loopback Audio",
        frames.frames,
        frames.frames * 320 * 180 * 4
    );
}
