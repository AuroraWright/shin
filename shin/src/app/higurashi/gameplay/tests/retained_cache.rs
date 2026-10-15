//! Real Gameplay/Host transport for ADV98's retained composed cache. Native
//! cache admission/resource/packet fixtures are separate. These assertions
//! compare retained snapshots and actual supplied message/Choice resources;
//! they do not claim original glyph pixels from the bounded text endpoint.
use super::{character::common, *};
use crate::adv::higurashi::{
    adv_cache::{NATIVE_EXTENT, Snapshot},
    audio::HigurashiAudio,
    input::Request,
    reference_gpu_assets, reference_render_to_texture,
    scene_textures::HigurashiSceneTextures,
    temporary_wrapper::Shared,
    touch::Overlay as InputOverlay,
};
use glam::{Vec2, Vec4};
use shin_core::primitives::color::{FloatColor4, UnormColor};
use shin_render::{
    ColorBlendType, DrawPrimitive, LayerFragmentShader, RenderProgramWithArguments,
    RenderRequestBuilder,
    render_texture::RenderTexture,
    shaders::types::{
        buffer::VertexSource,
        texture::{TextureTarget, TextureTargetKind},
        vertices::PosTexVertex,
    },
};

const NOW: SaveDate = SaveDate {
    year: 2026,
    month: 10,
    day: 13,
    hour: 12,
    minute: 34,
    second: 56,
};
const TIPS_SCRIPT: &[u8] = &[
    // Immediate message, a bounded WAIT, real TIPSGET/direct Choice, long WAIT.
    0x86, 1, 0, 0, 0, 0x7f, 4, 0, b'@', b'r', b'A', 0, 0x83, 0, 63, 0xd0, 3, 2, 2, 3, 0x83, 0xe8, 0,
    0, 0,
];

struct Fixture {
    game: Gameplay,
    number: u32,
}
impl Fixture {
    fn tick(&mut self, buttons: Buttons, policy: Policy) {
        self.number += 1;
        shin_tasks::block_on(self.game.update(
            Ticks::from_f32(3.),
            true,
            buttons,
            &PointerFrame {
                number: self.number,
                time_seconds: self.number as f64 / 60.,
                events: &[],
            },
            policy,
            audio(),
        ))
        .unwrap();
    }
    fn idle(&mut self) {
        self.tick(Buttons::default(), policy());
    }
    fn until(&mut self, mut ready: impl FnMut(&Gameplay) -> bool, description: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !ready(&self.game) {
            self.idle();
            assert!(Instant::now() < deadline, "{description}");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
fn fixture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    server: &Arc<AssetServer>,
    manager: &Arc<shin_audio::AudioManager>,
    code: &[u8],
    output: (u32, u32),
    saved_position: u32,
) -> Fixture {
    let (authored, common) = common(server, code);
    let mut saved = SaveData::default();
    saved.settings.common.v14_showroutenavi = false;
    saved.settings.common.v11_disallowskipunread = false;
    saved.save_vectors.extra_words_0 = vec![0; 256];
    let mut data = fresh();
    data.entry.random_seed = 42;
    data.entry.save_position = saved_position;
    data.entry.replay_target = saved_position;
    let globals = Globals {
        storage: None,
        runtime_saves: None,
        common,
        data: saved,
        clock: PlayClock::from_saved_seconds(0),
        platform_flag: true,
        last_title_mode: 0,
        audio: HigurashiAudio::new(manager.clone(), server.clone(), authored),
        server: server.clone(),
        manager: manager.clone(),
        sounds: Arc::new(Mutex::new(SystemSoundPlayer::new(manager.clone()))),
    };
    let mut game = None;
    reference_render_to_texture(device, queue, output, |context, _| {
        let mut source = context.new_render_texture("ADV cache initial platform capture".into());
        {
            let mut pass =
                context.begin_pass(source.as_texture_target(), None, "Source platform frame");
            // Distinct from the new ADV cache's own black getter.
            pass.clear(Some(UnormColor::from_rgba(23, 67, 101, 255)), None, None);
        }
        game = Some(Gameplay::new(globals, data, 0, source, || [1, 2, 3, 4]).unwrap());
    });
    let mut game = game.unwrap();
    game.set_clock_source(|| Ok(NOW));
    Fixture { game, number: 0 }
}
fn state(game: &Gameplay) -> Snapshot {
    game.host()
        .unwrap()
        .runner
        .frame()
        .cache
        .reference_snapshot()
}
fn epoch(game: &Gameplay) -> u64 {
    game.host().unwrap().runner.frame().visual_epoch()
}
fn wrapper(game: &Gameplay) -> Option<Shared> {
    game.tips_get
        .job
        .as_ref()?
        .ui
        .as_ref()
        .map(|ui| ui.wrapper.clone())
}
fn widget_phase(game: &Gameplay) -> Option<u32> {
    let ui = game.tips_get.job.as_ref()?.ui.as_ref()?;
    let owner = game
        .host()?
        .runner
        .frame()
        .scene
        .layers()
        .layer(ui.target)?;
    Some(owner.temporary_owner()?.reference_native_words()[0])
}
fn copy_to_output(
    context: &mut PreRenderContext,
    target: &wgpu::TextureView,
    source: &RenderTexture,
) {
    let vertices = [(0., 0.), (1., 0.), (0., 1.), (1., 1.)].map(|(x, y)| PosTexVertex {
        position: Vec2::new(x, y),
        texture_position: Vec2::new(x, y),
    });
    let mut pass = context.begin_pass(
        TextureTarget {
            view: target,
            kind: TextureTargetKind::RenderTexture,
        },
        None,
        "Read cached ADV snapshot",
    );
    pass.clear(Some(UnormColor::BLACK), None, None);
    pass.run(
        RenderRequestBuilder::new()
            .color_blend_type(ColorBlendType::Opaque)
            .build(
                RenderProgramWithArguments::HigurashiPostprocess {
                    vertices: VertexSource::VertexData {
                        vertices: &vertices,
                    },
                    texture: source.as_texture_source(),
                    transform: crate::render::normalized_projection_matrix(),
                    color: FloatColor4::WHITE,
                    operation: LayerFragmentShader::Default,
                    param: Vec4::ZERO,
                },
                DrawPrimitive::TrianglesStrip,
            ),
    );
}
fn capture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    output: (u32, u32),
    game: &Gameplay,
) -> Vec<u8> {
    reference_render_to_texture(device, queue, output, |context, target| {
        let mut screens = HigurashiSceneTextures::new(context);
        let snapshot = screens
            .capture_frame(context, &game.host().unwrap().runner)
            .unwrap();
        assert_eq!(snapshot.dimensions(), (NATIVE_EXTENT[0], NATIVE_EXTENT[1]));
        copy_to_output(context, target, &snapshot);
    })
}
fn render_host(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    output: (u32, u32),
    game: &mut Gameplay,
) -> Vec<u8> {
    reference_render_to_texture(device, queue, output, |context, target| {
        game.prepare(context).unwrap();
        let mut pass = context.begin_pass(
            TextureTarget {
                view: target,
                kind: TextureTargetKind::RenderTexture,
            },
            Some(context.depth_stencil),
            "Display retained ADV and separate55548",
        );
        pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.));
        game.host()
            .unwrap()
            .render_adv(&mut pass, game.screens.as_ref().unwrap())
            .unwrap();
    })
}
fn same(a: &[u8], b: &[u8], description: &str) {
    assert_eq!(a.len(), b.len());
    let channels = a.iter().zip(b).filter(|(a, b)| a != b).count();
    let max = a
        .iter()
        .zip(b)
        .map(|(&a, &b)| a.abs_diff(b))
        .max()
        .unwrap_or(0);
    assert_eq!(
        channels, 0,
        "{description}: {channels} channels differ, max {max}"
    );
}
fn changed(a: &[u8], b: &[u8], description: &str) {
    assert_eq!(a.len(), b.len());
    let channels = a.iter().zip(b).filter(|(a, b)| a != b).count();
    assert!(channels > 0, "{description}: no channels changed");
}

#[test]
#[ignore = "actual Gameplay message/TIPS child0/cache/admission/menu/55548 at720p/1080p; offscreen Metal/exact Loopback Audio; no windows/saves"]
fn higurashi_gameplay_adv_cache_reuses_composed_message_and_child() {
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    for output in [(1280, 720), (1920, 1080)] {
        let mut f = fixture(&device, &queue, &server, &manager, TIPS_SCRIPT, output, 0);
        let initial = state(&f.game);
        assert_eq!(initial.extent, [1920, 1080]);
        assert_eq!(
            (initial.color_id, initial.depth_id, initial.prepares),
            (None, None, 0)
        );
        assert_eq!(
            f.game
                .host()
                .unwrap()
                .runner
                .frame()
                .scene
                .history()
                .reference_snapshot()
                .0,
            [1920, 1080]
        );
        let black = capture(&device, &queue, output, &f.game);
        assert!(black.chunks_exact(4).all(|pixel| pixel == [0, 0, 0, 255]));
        let allocated = state(&f.game);
        assert_eq!(
            (allocated.color_id, allocated.depth_id, allocated.prepares),
            (Some(1), None, 0)
        );
        // Repeated getters preserve color-only allocation and never prepare.
        same(
            &capture(&device, &queue, output, &f.game),
            &black,
            "Repeated initial getter",
        );
        assert_eq!(state(&f.game), allocated);

        f.until(
            |game| widget_phase(game) == Some(2),
            "TIPS widget did not reach ready",
        );
        f.tick(
            Buttons {
                pressed: 1 << 24,
                ..Default::default()
            },
            policy(),
        );
        let mut ui = wrapper(&f.game).unwrap();
        f.until(
            |_| ui.reference_child().is_some_and(|child| child.0 == 2),
            "TIPS direct child did not reach ready",
        );
        assert_eq!(
            f.game.host().unwrap().reference_touch_overlay_identity(),
            Some(ui.identity())
        );
        assert!(!f.game.host().unwrap().has_modes());
        let visible = capture(&device, &queue, output, &f.game);
        assert!(
            visible
                .chunks_exact(4)
                .any(|p| p[0] != 0 || p[1] != 0 || p[2] != 0)
        );
        assert_eq!(
            (state(&f.game).color_id, state(&f.game).depth_id),
            (Some(1), Some(2))
        );
        // Display preparation and cached outer draws do not rerender children.
        let before = state(&f.game);
        let displayed = render_host(&device, &queue, output, &mut f.game);
        assert_eq!(state(&f.game), before);
        same(&displayed, &visible, "Cached outer display");
        same(
            &render_host(&device, &queue, output, &mut f.game),
            &displayed,
            "Repeated display",
        );
        assert_eq!(state(&f.game), before);

        // Two permitted visual callbacks with no display redraw each eagerly
        // prepare, retaining one color/depth pair rather than coalescing ticks.
        let before_epoch = epoch(&f.game);
        f.idle();
        f.idle();
        let after = state(&f.game);
        assert_eq!(after.prepares, before.prepares + 2);
        assert_eq!(epoch(&f.game), before_epoch + 2);
        assert_eq!(
            (after.color_id, after.depth_id),
            (before.color_id, before.depth_id)
        );

        // Real message state changed outside an admitted callback cannot alter
        // a capture or outer draw. Subsequent callbacks include its new state.
        let with_message = capture(&device, &queue, output, &f.game);
        f.game
            .host_mut()
            .unwrap()
            .runner
            .reference_frame_mut()
            .message
            .playback
            .hidden = true;
        let frozen = state(&f.game);
        same(
            &capture(&device, &queue, output, &f.game),
            &with_message,
            "Capture retained before message update",
        );
        same(
            &render_host(&device, &queue, output, &mut f.game),
            &with_message,
            "Display retained before message update",
        );
        assert_eq!(state(&f.game), frozen);
        for _ in 0..12 {
            f.idle();
        }
        assert_eq!(
            f.game
                .host()
                .unwrap()
                .runner
                .frame()
                .message
                .playback
                .modal_slide,
            0.
        );
        let without_message = capture(&device, &queue, output, &f.game);
        changed(
            &with_message,
            &without_message,
            "Prepared cache omitted the real message change",
        );

        // The nested Choice is child0 of the composed ADV cache; the widget
        // itself remains a Screen child. Retained UI is independently hidden.
        ui.set_hidden(true);
        same(
            &capture(&device, &queue, output, &f.game),
            &without_message,
            "Capture retained before child update",
        );
        for _ in 0..12 {
            f.idle();
        }
        let without_child = capture(&device, &queue, output, &f.game);
        changed(
            &without_message,
            &without_child,
            "Prepared cache omitted child0 Choice",
        );
        assert!(ui.reference_child().unwrap().3);

        let before_pause = state(&f.game);
        let before_epoch = epoch(&f.game);
        let before_child = ui.reference_child().unwrap();
        for disabled in [false, true] {
            let mut p = policy();
            p.paused = !disabled;
            p.animations_enabled = !disabled;
            f.tick(Buttons::default(), p);
            f.tick(Buttons::default(), p);
            assert_eq!(state(&f.game), before_pause);
            assert_eq!(epoch(&f.game), before_epoch);
            let child = ui.reference_child().unwrap();
            assert_eq!(
                (
                    child.2.to_bits(),
                    child.4.map(f32::to_bits),
                    child.6.clone()
                ),
                (
                    before_child.2.to_bits(),
                    before_child.4.map(f32::to_bits),
                    before_child.6.clone()
                )
            );
            same(
                &render_host(&device, &queue, output, &mut f.game),
                &without_child,
                "Gated display",
            );
            assert_eq!(state(&f.game), before_pause);
        }

        // A55548 reconstruction overlay is a separate outer owner. Supply a
        // conspicuous actual capture to its production constructor, without
        // polling its worker/publication or touching the retained ADV cache.
        let replay = ReplaySession::new(
            f.game.globals.common.scenario.clone(),
            f.game
                .bindings
                .memory
                .fork_for_restore(0, 0, &f.game.bindings.progress)
                .unwrap(),
        )
        .unwrap();
        let history = f.game.bindings.history.clone();
        reference_render_to_texture(&device, &queue, (1920, 1080), |context, _| {
            let mut overlay = context.new_render_texture("Separate55548 proof".into());
            {
                let mut pass =
                    context.begin_pass(overlay.as_texture_target(), None, "Separate55548 color");
                pass.clear(Some(UnormColor::from_rgba(211, 37, 159, 255)), None, None);
            }
            f.game
                .host_mut()
                .unwrap()
                .runner
                .begin_reconstruction_mode(replay, 1, overlay, history, || [5, 6, 7, 8])
                .unwrap();
        });
        same(
            &capture(&device, &queue, output, &f.game),
            &without_child,
            "Separate owner omitted from retained cache",
        );
        let overlay_display = render_host(&device, &queue, output, &mut f.game);
        assert!(
            overlay_display
                .chunks_exact(4)
                .all(|p| p == [211, 37, 159, 255])
        );
        assert_eq!(state(&f.game), before_pause);
        f.game
            .host_mut()
            .unwrap()
            .runner
            .cancel_reconstruction_mode()
            .unwrap();

        // Enter the actual SystemMenu from its prepared Screen capture. Neither
        // menu preparation nor paused ADV callbacks may replace the ADV cache.
        // Rebinding the desktop adapter must use the existing Screen without
        // preparing it or changing its fixed native constructor extent.
        f.game.screens = None;
        f.game.controls.requests.push_back(Request::SystemMenu);
        render_host(&device, &queue, output, &mut f.game);
        assert!(f.game.host().unwrap().has_modes());
        assert_eq!(f.game.host().unwrap().modal_pause_depth(), 1);
        let menu_state = state(&f.game);
        let menu_epoch = epoch(&f.game);
        for _ in 0..4 {
            f.idle();
            render_host(&device, &queue, output, &mut f.game);
        }
        assert_eq!(state(&f.game), menu_state);
        assert_eq!(epoch(&f.game), menu_epoch);
        same(
            &capture(&device, &queue, output, &f.game),
            &without_child,
            "Separate owner omitted from retained cache",
        );
        let weak = f.game.host().unwrap().runner.frame().cache.reference_weak();
        drop(f);
        assert!(
            weak.upgrade().is_none(),
            "Host graphics or retained UI leaked the ADV cache owner"
        );
    }
}

#[test]
#[ignore = "real Gameplay previous-message request/reconstruction capture gates; offscreen Metal/exact Loopback Audio; no windows/saves"]
fn higurashi_gameplay_navigation_captures_last_cache_without_preparing() {
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let mut code = Vec::new();
    for (n, c) in [b'A', b'B', b'C'].into_iter().enumerate() {
        code.push(0x86);
        code.extend(((n + 1) as u32).to_le_bytes()[..3].iter());
        code.extend([1, 0x7f, 4, 0, b'@', b'r', c, 0]);
    }
    code.extend([0, 0, 0]);
    let output = (1280, 720);
    let mut f = fixture(&device, &queue, &server, &manager, &code, output, 3);
    f.until(
        |game| {
            game.host().is_some_and(|host| {
                !host.has_modes()
                    && host.runner.reconstruction_phase().is_none()
                    && host.runner.native_vm_attached()
                    && host.runner.frame().message.playback.message_id == 2
            })
        },
        "Saved message replay did not finish",
    );
    for _ in 0..18 {
        f.idle();
    }
    let source = capture(&device, &queue, output, &f.game);
    assert!(
        source
            .chunks_exact(4)
            .any(|p| p[0] != 0 || p[1] != 0 || p[2] != 0)
    );
    let before = state(&f.game);
    let before_epoch = epoch(&f.game);
    // Uniform keyboard H/controller L resolves to the native previous-message
    // request through controls; navigation owns a real forked replay.
    f.tick(
        Buttons {
            repeat: 1 << 4,
            ..Default::default()
        },
        policy(),
    );
    assert!(f.game.navigation.pending());
    assert_eq!(
        f.game.pending_input_request(),
        Some(Request::PreviousMessage)
    );
    assert_eq!(state(&f.game), before);
    assert_eq!(epoch(&f.game), before_epoch);
    same(
        &capture(&device, &queue, output, &f.game),
        &source,
        "Navigation cached getter",
    );
    let display = render_host(&device, &queue, output, &mut f.game);
    same(&display, &source, "Navigation capture overlay");
    assert!(!f.game.navigation.pending());
    assert_eq!(f.game.pending_input_request(), None);
    assert!(f.game.host().unwrap().has_modes());
    assert_eq!(state(&f.game), before);
    assert_eq!(epoch(&f.game), before_epoch);
    same(
        &capture(&device, &queue, output, &f.game),
        &source,
        "Navigation cached getter",
    );
    // Preparing the new mode repeatedly is still a cached getter, even if its
    // asynchronous replay has already become ready. No mode poll occurs here.
    for _ in 0..2 {
        same(
            &render_host(&device, &queue, output, &mut f.game),
            &display,
            "Navigation preparation without poll",
        );
        assert_eq!(state(&f.game), before);
    }
    f.game
        .host_mut()
        .unwrap()
        .runner
        .cancel_reconstruction_mode()
        .unwrap();
}

#[test]
#[ignore = "actual generic Character11 setting changes before eager cache preparation; Metal/exact Loopback Audio"]
fn higurashi_gameplay_adv_cache_uses_current_generic_character_route_setting() {
    use crate::layer::user::higurashi::HigurashiLayerKind;
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    // Generic LAYERLOAD11, rather than CHARSELECT's independently polled job.
    let code = [0xc1, 1, 11, 0, 1, 18, 0x83, 0xe8, 0, 0, 0];
    let mut f = fixture(&device, &queue, &server, &manager, &code, (1280, 720), 0);
    let route = |game: &Gameplay| {
        let layers = game.host()?.runner.frame().scene.layers();
        let target = layers.state.target(1)?;
        let owner = layers.layer(target)?;
        assert_eq!(owner.layer_type(), HigurashiLayerKind::CharacterSelection);
        Some(owner.character_view()?.route_navigation)
    };
    f.until(
        |game| route(game).is_some(),
        "Generic Character11 did not load",
    );
    assert_eq!(route(&f.game), Some(false));
    for value in [true, false, true] {
        let before = state(&f.game);
        f.game.globals.data.settings.common.v14_showroutenavi = value;
        // No Gameplay.prepare/display call occurs between the setting change
        // and the admitted callback's composition.
        f.idle();
        assert_eq!(route(&f.game), Some(value));
        assert_eq!(state(&f.game).prepares, before.prepares + 1);
        assert_eq!(state(&f.game).extent, NATIVE_EXTENT);
    }
}
