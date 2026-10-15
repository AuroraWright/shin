use super::*;
use crate::adv::{
    higurashi::{
        audio::HigurashiAudio,
        input::{OverlayInput, Request},
        reference_gpu_assets, reference_render_to_texture,
        touch::Overlay,
    },
    input_area::{PointerEvent, PointerKind},
};
use glam::Vec2;
use shin_core::{format::scenario::higurashi::HigurashiScenario, primitives::color::UnormColor};
use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};

struct Frames {
    number: u32,
    time: f64,
}
impl Frames {
    fn step(
        &mut self,
        game: &mut Gameplay,
        permission: bool,
        pressed: u32,
        events: &[PointerEvent],
        seconds: f64,
    ) {
        self.number += 1;
        self.time += seconds;
        shin_tasks::block_on(game.update(
            Ticks::from_f32(3.0),
            permission,
            Buttons {
                pressed,
                ..Default::default()
            },
            &PointerFrame {
                number: self.number,
                time_seconds: self.time,
                events,
            },
            policy(),
            audio(),
        ))
        .unwrap();
    }
    fn idle(&mut self, game: &mut Gameplay) {
        self.step(game, true, 0, &[], 1.0 / 60.0);
    }
    fn swipe(&mut self, game: &mut Gameplay, x: i32, y: i32) {
        self.step(game, true, 0, &[point(PointerKind::Down, 400, 400)], 0.01);
        self.step(
            game,
            true,
            0,
            &[point(PointerKind::Move, 400 + x, 400 + y)],
            0.01,
        );
        self.step(game, true, 0, &[point(PointerKind::Up, -1000, -1000)], 0.01);
    }
    fn tap(&mut self, game: &mut Gameplay, x: i32, y: i32, pressed: u32) {
        self.step(game, true, 0, &[point(PointerKind::Down, x, y)], 0.01);
        self.step(game, true, pressed, &[point(PointerKind::Up, 0, 0)], 0.01);
    }
}
fn point(kind: PointerKind, x: i32, y: i32) -> PointerEvent {
    PointerEvent {
        kind,
        id: 7,
        position: glam::ivec2(x, y),
    }
}
struct TouchOwner {
    id: u8,
    events: Arc<Mutex<Vec<(u8, char, Vec2)>>>,
}
impl Overlay for TouchOwner {
    fn input_permissions(&self) -> OverlayInput {
        OverlayInput {
            menus: false,
            hide_window: false,
        }
    }
    fn probe(&mut self, p: Vec2) -> bool {
        self.events.lock().push((self.id, 'p', p));
        true
    }
    fn press(&mut self, p: Vec2) {
        self.events.lock().push((self.id, 'd', p));
    }
    fn release(&mut self, p: Vec2) {
        self.events.lock().push((self.id, 'u', p));
    }
    fn cancel(&mut self) {
        self.events.lock().push((self.id, 'x', Vec2::ZERO));
    }
}

#[test]
#[ignore = "actual Gameplay root contact/skip/hide/SystemMenu/overlay retirement at720p/1080p;offscreen Metal/exact Loopback Audio"]
fn higurashi_gameplay_touch_drives_owned_messages_and_menus() {
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    let authored: Arc<HigurashiScenario> = server.load_sync("/main.snr").unwrap();
    let mut raw = authored.raw().to_vec();
    let start = authored.header.code_offset as usize;
    let mut code = vec![
        0xa0, 0, 8, b'C', b'h', b'a', b'p', b't', b'e', b'r', 0, 0xa0, 1, 6, b'S', b'c', b'e',
        b'n', b'e', 0, 0xa1,
    ];
    code.extend([
        0x86, 1, 0, 0, 1, 0x7f, 5, 0, b'@', b'r', b'A', b'B', 0, 0x83, 1, 30, 0x86, 2, 0, 0, 1,
        0x7f, 4, 0, b'@', b'r', b'C', 0, 0xa2, 0, 0x83, 1, 30, 0xa3, 0x86, 3, 0, 0, 1, 0x7f, 4, 0,
        b'@', b'r', b'D', 0, 0, 0, 0,
    ]);
    raw[start..start + code.len()].copy_from_slice(&code);
    let scenario = Arc::new(HigurashiScenario::new(raw.into()).unwrap());
    let common = Arc::new(CommonAssets {
        scenario,
        font: server.load_sync("/newrodin.fnt").unwrap(),
        system: server.load_sync("/systex.txa").unwrap(),
        message: server.load_sync("/msgtex.txa").unwrap(),
        effects: server.load_sync("/effects.txa").unwrap(),
        sysse: server.load_sync("/sysse.bin").unwrap(),
    });
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let globals = Globals {
        storage: None,
        runtime_saves: None,
        common,
        data: SaveData::default(),
        clock: PlayClock::from_saved_seconds(0),
        platform_flag: true,
        last_title_mode: 0,
        audio: HigurashiAudio::new(manager.clone(), server.clone(), authored),
        server,
        manager: manager.clone(),
        sounds: Arc::new(Mutex::new(SystemSoundPlayer::new(manager))),
    };
    let mut game = None;
    reference_render_to_texture(&device, &queue, (1280, 720), |context, _| {
        let mut snapshot = context.new_render_texture("Root touch initial".into());
        {
            let mut pass = context.begin_pass(
                snapshot.as_texture_target(),
                Some(context.depth_stencil),
                "Initial touch capture",
            );
            pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
        }
        let mut data = fresh();
        data.entry.random_seed = 42;
        game = Some(Gameplay::new(globals, data, 0, snapshot, || [1, 2, 3, 4]).unwrap());
    });
    let mut game = game.unwrap();
    let mut frames = Frames {
        number: 0,
        time: 0.0,
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        frames.idle(&mut game);
        if game.host().unwrap().runner.reconstruction_phase().is_none()
            && game.host().unwrap().runner.native_vm_attached()
            && game
                .host()
                .unwrap()
                .runner
                .frame()
                .message
                .playback
                .message_id
                == 0
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "phase {:?},attached {},message {},commands {}",
            game.host().unwrap().runner.reconstruction_phase(),
            game.host().unwrap().runner.native_vm_attached(),
            game.host()
                .unwrap()
                .runner
                .frame()
                .message
                .playback
                .message_id,
            game.host().unwrap().runner.completed_commands()
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    for _ in 0..30 {
        frames.idle(&mut game);
    }
    let render = |game: &mut Gameplay, size| {
        game.screens = None;
        reference_render_to_texture(&device, &queue, size, |context, target| {
            game.prepare(context).unwrap();
            let mut pass = context.begin_pass(
                TextureTarget {
                    view: target,
                    kind: TextureTargetKind::RenderTexture,
                },
                Some(context.depth_stencil),
                "Root touch owner",
            );
            game.render(&mut pass).unwrap();
        })
    };
    let before = game.host().unwrap().runner.completed_commands();
    frames.swipe(&mut game, -100, 0);
    assert_eq!(game.controls.state.temporary_indicator, 1);
    for _ in 0..4 {
        frames.idle(&mut game);
    }
    assert_eq!(game.host().unwrap().touch_state().opacity, 1.0);
    // Compare the actual composed Gameplay output with the original1382c UI
    // packets, preserving its real message/background/modal resources.
    let verify_ui = |game: &mut Gameplay, pressed: i32, modes: u32| {
        use crate::adv::higurashi::root_ui::{self, tests as original};
        let host = game.host().unwrap();
        let state = root_ui::State {
            vm: host.runner.native_read_state(),
            modes,
            pressed,
            navi_slide: 1.0,
            read_only: game.globals.data.settings.common.v11_disallowskipunread,
            elapsed_seconds: host.ui_elapsed_seconds(),
            ..Default::default()
        };
        let packets = original::native(&[(true, state)]).pop().unwrap();
        for size in [(1280, 720), (1920, 1080)] {
            let actual = render(game, size);
            game.screens = None;
            let expected = reference_render_to_texture(&device, &queue, size, |context, target| {
                game.prepare(context).unwrap();
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: target,
                        kind: TextureTargetKind::RenderTexture,
                    },
                    Some(context.depth_stencil),
                    "Native Gameplay root UI",
                );
                pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
                let host = game.host().unwrap();
                host.render_adv(&mut pass, game.screens.as_ref().unwrap())
                    .unwrap();
                game.notification.render(
                    &mut pass,
                    &host.ui.indicator,
                    game.globals.data.settings.common.v17_showscenetitle,
                );
                original::replay(&packets, game.atlas.as_ref().unwrap(), &mut pass);
                game.host_mut().unwrap().render_modes(&mut pass);
            });
            assert_eq!(
                actual, expected,
                "actual Gameplay root UI at{size:?},pressed{pressed},modes{modes}"
            );
        }
    };
    verify_ui(&mut game, -1, 0);
    frames.step(
        &mut game,
        true,
        0,
        &[point(PointerKind::Down, 952, 120)],
        0.01,
    );
    frames.step(&mut game, true, 0, &[], 0.2);
    assert_eq!(game.host().unwrap().touch_state().pressed, 0);
    verify_ui(&mut game, 0, 0);
    frames.step(
        &mut game,
        true,
        0,
        &[point(PointerKind::Cancel, 0, 0)],
        0.01,
    );
    let cues = game.controls.reference_cues.len();
    // Native touch coords1280x720 stay the same for both output resolutions.
    for size in [(1280, 720), (1920, 1080)] {
        render(&mut game, size);
        frames.tap(&mut game, 1080, 120, 1 << 24);
        assert_eq!(game.controls.state.modes, 2);
        verify_ui(&mut game, -1, 2);
        assert_eq!(game.host().unwrap().runner.completed_commands(), before);
        frames.tap(&mut game, 1080, 120, 1 << 24);
        assert_eq!(game.controls.state.modes, 0);
        assert_eq!(game.host().unwrap().runner.completed_commands(), before);
    }
    // Real audio handles retire while native/framebuffer comparisons run;
    // the active-handle count is not a cumulative cue counter.
    assert_eq!(
        &game.controls.reference_cues[cues..],
        &[
            Request::CueEnabled,
            Request::CueDisabled,
            Request::CueEnabled,
            Request::CueDisabled,
        ]
    );
    frames.swipe(&mut game, 100, 0);
    assert_eq!(game.controls.state.temporary_indicator, 0);
    // Dynamic overlay lookup: press/release/cancel use the current child.
    let events = Arc::new(Mutex::new(Vec::new()));
    game.host_mut()
        .unwrap()
        .set_touch_overlay(Some(Box::new(TouchOwner {
            id: 1,
            events: events.clone(),
        })));
    frames.step(
        &mut game,
        true,
        1 << 10,
        &[point(PointerKind::Down, 100, 100)],
        0.01,
    );
    frames.step(&mut game, true, 0, &[], 0.2);
    game.host_mut()
        .unwrap()
        .set_touch_overlay(Some(Box::new(TouchOwner {
            id: 2,
            events: events.clone(),
        })));
    frames.step(
        &mut game,
        true,
        1 << 24,
        &[point(PointerKind::Up, 0, 0)],
        0.01,
    );
    assert_eq!(
        *events.lock(),
        [
            (1, 'p', Vec2::splat(150.0)),
            (1, 'd', Vec2::splat(150.0)),
            (2, 'u', Vec2::splat(150.0))
        ]
    );
    frames.step(
        &mut game,
        true,
        0,
        &[point(PointerKind::Down, 100, 100)],
        0.01,
    );
    frames.step(&mut game, false, 1 << 24, &[], 0.01);
    assert_eq!(events.lock().last(), Some(&(2, 'x', Vec2::ZERO)));
    assert!(game.take_input_flush());
    frames.step(&mut game, true, 0, &[point(PointerKind::Up, 0, 0)], 0.01);
    assert_eq!(game.host().unwrap().runner.completed_commands(), before);
    game.host_mut().unwrap().set_touch_overlay(None);
    frames.swipe(&mut game, 0, 100);
    assert!(game.host().unwrap().has_modes());
    for _ in 0..4 {
        frames.idle(&mut game);
    }
    for size in [(1280, 720), (1920, 1080)] {
        let pixels = render(&mut game, size);
        assert!(pixels.chunks_exact(4).all(|p| p == [0, 0, 0, 255]));
    }
    frames.tap(&mut game, 100, 100, 0);
    frames.idle(&mut game);
    assert!(!game.host().unwrap().has_modes());
    for _ in 0..4 {
        frames.idle(&mut game);
    }
    for size in [(1280, 720), (1920, 1080)] {
        let pixels = render(&mut game, size);
        assert!(
            pixels
                .chunks_exact(4)
                .any(|p| p[0] != 0 || p[1] != 0 || p[2] != 0)
        );
    }
    frames.swipe(&mut game, 100, 0);
    assert_eq!(game.pending_input_request(), Some(Request::SystemMenu));
    render(&mut game, (1280, 720));
    assert_eq!(game.pending_input_request(), None);
    assert_eq!(game.host().unwrap().modal_pause_depth(), 1);
    for _ in 0..60 {
        frames.idle(&mut game);
    }
    for size in [(1280, 720), (1920, 1080)] {
        let pixels = render(&mut game, size);
        assert!(
            pixels
                .chunks_exact(4)
                .any(|p| p[0] != 0 || p[1] != 0 || p[2] != 0)
        );
    }
    frames.step(&mut game, true, 1 << 25, &[], 0.01);
    for _ in 0..80 {
        frames.idle(&mut game);
    }
    assert_eq!(game.host().unwrap().modal_pause_depth(), 0);
    assert_eq!(game.host().unwrap().runner.completed_commands(), before);
    // Ordinary tap then publishes completion; WAIT permits a second tap to zero it.
    for _ in 0..2 {
        frames.tap(&mut game, 100, 100, 0);
    }
    assert!(game.bindings.history.contains(0));
    frames.tap(&mut game, 100, 100, 0);
    frames.idle(&mut game);
    assert!(game.host().unwrap().runner.completed_commands() > before);
    assert_eq!(
        game.host()
            .unwrap()
            .runner
            .frame()
            .message
            .playback
            .message_id,
        1
    );
    game.controls.state.temporary_indicator = 7;
    for _ in 0..3 {
        frames.tap(&mut game, 100, 100, 0);
    }
    assert_eq!(
        game.host()
            .unwrap()
            .runner
            .native_read_state()
            .unwrap()
            .flags
            & 0x8000_0000,
        0
    );
    assert_eq!(game.controls.state.temporary_indicator, 0);
    frames.swipe(&mut game, 0, 100);
    assert_eq!(game.pending_input_request(), Some(Request::SceneJump));
    assert!(game.navigation.pending());
    render(&mut game, (1280, 720));
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        frames.idle(&mut game);
        if game.host().unwrap().runner.reconstruction_phase().is_none()
            && game.host().unwrap().runner.native_vm_attached()
            && game
                .host()
                .unwrap()
                .runner
                .frame()
                .message
                .playback
                .message_id
                == 2
        {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    for size in [(1280, 720), (1920, 1080)] {
        render(&mut game, size);
    }
    eprintln!(
        "Gameplay root touch:actual skip cues/contact priority,owner replacement/cancel/flush,hidden/restored pixels,SystemMenu capture/B return at720p/1080p,message completion,WAIT interruption,event indicator reset/downward scene jump and selected message resume pass"
    );
}
