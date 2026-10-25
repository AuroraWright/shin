use super::*;
use crate::adv::higurashi::{
    audio::HigurashiAudio, reference_gpu_assets, reference_render_to_texture,
};
use crate::app::higurashi::gameplay::tests::{audio, fresh, policy};
use shin_core::{format::scenario::higurashi::HigurashiScenario, primitives::color::UnormColor};
use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
use std::time::{Duration, Instant};

fn tick(game: &mut Gameplay, number: &mut u32, pressed: u32, repeat: u32) {
    *number += 1;
    shin_tasks::block_on(game.update(
        Ticks::from_f32(3.0),
        true,
        Buttons {
            pressed,
            repeat,
            ..Default::default()
        },
        &PointerFrame {
            events: &[],
            number: *number,
            time_seconds: *number as f64 / 60.0,
        },
        policy(),
        audio(),
    ))
    .unwrap();
}
fn wait_message(game: &mut Gameplay, number: &mut u32, id: u32) {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        tick(game, number, 0, 0);
        let host = game.host().unwrap();
        if host.runner.reconstruction_phase().is_none()
            && host.runner.native_vm_attached()
            && host.runner.frame().message.playback.message_id == id
        {
            break;
        }
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(1));
    }
    for _ in 0..18 {
        tick(game, number, 0, 0);
    }
}
fn wait_question(game: &mut Gameplay, number: &mut u32) {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        tick(game, number, 0, 0);
        if game
            .host()
            .unwrap()
            .reference_chapter_question()
            .is_some_and(|(phase, _)| phase == 2)
        {
            break;
        }
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(1));
    }
}
#[test]
#[ignore = "concrete Gameplay previous-message and chapter question/No/B/Yes/native searches/reconstruction at720p/1080p;offscreen Metal/the default audio output"]
fn higurashi_gameplay_navigation_rewinds_messages_and_confirms_chapters() {
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    let authored: Arc<HigurashiScenario> = server.load_sync("/main.snr").unwrap();
    let mut code = vec![
        0xa0, 0, 8, b'C', b'h', b'a', b'p', b't', b'e', b'r', 0, 0xa0, 1, 6, b'S', b'c', b'e',
        b'n', b'e', 0,
    ];
    for (n, c) in [b'A', b'B', b'C', b'D', b'E'].into_iter().enumerate() {
        if n % 2 == 0 {
            code.push(0xa1);
        }
        code.push(0x86);
        code.extend(((n + 1) as u32).to_le_bytes()[..3].iter());
        code.extend([1, 0x7f, 4, 0, b'@', b'r', c, 0]);
    }
    code.extend([0, 0, 0]);
    let mut raw = authored.raw().to_vec();
    let start = authored.header.code_offset as usize;
    raw[start..start + code.len()].copy_from_slice(&code);
    let scenario = Arc::new(HigurashiScenario::new(raw.into()).unwrap());
    let common = Arc::new(CommonAssets {
        scenario: scenario.clone(),
        font: server.load_sync("/newrodin.fnt").unwrap(),
        system: server.load_sync("/systex.txa").unwrap(),
        message: server.load_sync("/msgtex.txa").unwrap(),
        effects: server.load_sync("/effects.txa").unwrap(),
        sysse: server.load_sync("/sysse.bin").unwrap(),
    });
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let sounds = Arc::new(Mutex::new(SystemSoundPlayer::new(manager.clone())));
    let mut data = fresh();
    data.entry.random_seed = 42;
    data.entry.save_position = 3;
    data.entry.replay_target = 3;
    data.trailing_values = vec![0x1234, 0xfffe];
    data.byte_variables = vec![7; 64];
    data.entry.response_data = vec![0x23; 16];
    let mut saved = SaveData::default();
    saved.settings.common.v11_disallowskipunread = false;
    saved.manual_save_slots[2] = Some(data.clone());
    saved.save_vectors.extra_words_1 = vec![0x100];
    let globals = Globals {
        storage: None,
        runtime_saves: None,
        common: common.clone(),
        data: saved,
        clock: PlayClock::from_saved_seconds(123),
        platform_flag: true,
        last_title_mode: 7,
        audio: HigurashiAudio::new(manager.clone(), server.clone(), authored),
        server,
        manager,
        sounds,
    };
    let mut game = None;
    reference_render_to_texture(&device, &queue, (1280, 720), |context, _| {
        let mut snapshot = context.new_render_texture("Navigation initial capture".into());
        {
            let mut pass = context.begin_pass(
                snapshot.as_texture_target(),
                Some(context.depth_stencil),
                "Navigation capture",
            );
            pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
        }
        game = Some(Gameplay::new(globals, data.clone(), 0, snapshot, || [1, 2, 3, 4]).unwrap());
    });
    let mut game = game.unwrap();
    let mut number = 0;
    wait_message(&mut game, &mut number, 1);
    let mut renders = 0;
    let render = |game: &mut Gameplay, size| {
        game.screens = None;
        let pixels = reference_render_to_texture(&device, &queue, size, |context, target| {
            game.prepare(context).unwrap();
            let mut pass = context.begin_pass(
                TextureTarget {
                    view: target,
                    kind: TextureTargetKind::RenderTexture,
                },
                Some(context.depth_stencil),
                "Navigation owner",
            );
            game.render(&mut pass).unwrap();
        });
        assert!(pixels.chunks_exact(4).all(|p| p[3] == 255));
        assert!(
            pixels
                .chunks_exact(4)
                .any(|p| p[0] != 0 || p[1] != 0 || p[2] != 0)
        );
    };
    tick(&mut game, &mut number, 0, 1 << 4);
    assert!(game.navigation.pending());
    assert_eq!(game.pending_input_request(), Some(Request::PreviousMessage));
    assert_eq!(game.navigation.pending.as_ref().unwrap().mode, 1);
    render(&mut game, (1280, 720));
    renders += 1;
    assert_eq!(game.pending_input_request(), None);
    assert_eq!(game.host().unwrap().modal_pause_depth(), 0);
    wait_message(&mut game, &mut number, 0);
    assert_eq!(
        game.host()
            .unwrap()
            .runner
            .native_message_timeline()
            .unwrap()
            .boundary,
        2
    );
    // First message has no predecessor: reject with a cue without retaining a replay.
    tick(&mut game, &mut number, 0, 1 << 4);
    assert!(!game.navigation.pending());
    assert_eq!(game.pending_input_request(), None);
    // Opening resets modes, but not the temporary indicator. No and B return
    // from the real question without a restore; the ADV stays unpaused.
    for cancel in [false, true] {
        game.controls.state.modes = 3;
        game.controls.state.temporary_indicator = 7;
        tick(&mut game, &mut number, 0, 1 << 8);
        assert_eq!(game.controls.state.modes, 0);
        assert_eq!(game.controls.state.temporary_indicator, 7);
        assert_eq!(game.host().unwrap().modal_pause_depth(), 0);
        assert!(!game.input_permission);
        wait_question(&mut game, &mut number);
        assert_eq!(
            game.host().unwrap().reference_chapter_question(),
            Some((2, 1))
        );
        for size in [(1280, 720), (1920, 1080)] {
            render(&mut game, size);
            renders += 1;
        }
        tick(&mut game, &mut number, 1 << if cancel { 25 } else { 24 }, 0);
        for _ in 0..50 {
            tick(&mut game, &mut number, 0, 0);
            if !game.host().unwrap().has_modes() {
                break;
            }
        }
        assert!(!game.host().unwrap().has_modes());
        assert!(!game.navigation.pending());
        assert_eq!(
            game.host()
                .unwrap()
                .runner
                .native_message_timeline()
                .unwrap()
                .boundary,
            2
        );
        assert_eq!(game.controls.state.temporary_indicator, 7);
    }
    // Next searches only for preflight on opening, then searches again after Yes.
    tick(&mut game, &mut number, 1 << 9, 0);
    wait_question(&mut game, &mut number);
    tick(&mut game, &mut number, 0, 1 << 4);
    assert_eq!(
        game.host().unwrap().reference_chapter_question(),
        Some((2, 0))
    );
    tick(&mut game, &mut number, 1 << 24, 0);
    for _ in 0..70 {
        tick(&mut game, &mut number, 0, 0);
        if game.navigation.pending() {
            break;
        }
    }
    assert!(game.navigation.pending());
    assert_eq!(game.navigation.pending.as_ref().unwrap().mode, 0);
    assert_eq!(game.controls.state.temporary_indicator, 0);
    render(&mut game, (1280, 720));
    renders += 1;
    wait_message(&mut game, &mut number, 2);
    assert_eq!(
        game.host()
            .unwrap()
            .runner
            .native_message_timeline()
            .unwrap()
            .current_chapter,
        4
    );
    // At the first message of this chapter, previous goes to chapter1. With
    // two messages it goes to the current chapter; the native matrix covers both.
    tick(&mut game, &mut number, 0, 1 << 8);
    wait_question(&mut game, &mut number);
    tick(&mut game, &mut number, 0, 1 << 4);
    tick(&mut game, &mut number, 1 << 24, 0);
    for _ in 0..70 {
        tick(&mut game, &mut number, 0, 0);
        if game.navigation.pending() {
            break;
        }
    }
    assert!(game.navigation.pending());
    render(&mut game, (1280, 720));
    renders += 1;
    wait_message(&mut game, &mut number, 0);
    assert_eq!(
        game.host()
            .unwrap()
            .runner
            .native_message_timeline()
            .unwrap()
            .current_chapter,
        1
    );
    assert_eq!(game.globals.data.manual_save_slots[2], Some(data));
    assert_eq!(
        game.bindings.progress.chart_unlocks.saved_words(),
        vec![0x100]
    );
    assert!(Arc::ptr_eq(&game.globals.common, &common));
    assert!(game.globals.clock.ticks() > 123.0 * 60.0);
    // B inside an event computes11664 after native input gates. Its result is
    // owned until preparation captures the source; no supplied availability
    // flag or fake completed event is needed.
    let mut globals = game.into_globals();
    let mut raw = common.scenario.raw().to_vec();
    let event = [
        0xa2, 0, 0x86, 1, 0, 0, 0, 0x7f, 4, 0, b'@', b'r', b'A', 0, 0x83, 1, 0x83, 0xe8, 0xa3,
        0x86, 2, 0, 0, 1, 0x7f, 4, 0, b'@', b'r', b'B', 0, 0, 0, 0,
    ];
    raw[start..start + event.len()].copy_from_slice(&event);
    let event = Arc::new(HigurashiScenario::new(raw.into()).unwrap());
    globals.common = Arc::new(CommonAssets {
        scenario: event,
        font: common.font.clone(),
        system: common.system.clone(),
        message: common.message.clone(),
        effects: common.effects.clone(),
        sysse: common.sysse.clone(),
    });
    let mut event_game = None;
    reference_render_to_texture(&device, &queue, (1280, 720), |context, _| {
        let mut snapshot = context.new_render_texture("Event navigation initial capture".into());
        {
            let mut pass = context.begin_pass(
                snapshot.as_texture_target(),
                Some(context.depth_stencil),
                "Event capture",
            );
            pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
        }
        event_game = Some(Gameplay::new(globals, fresh(), 0, snapshot, || [1, 2, 3, 4]).unwrap());
    });
    let mut game = event_game.unwrap();
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        tick(&mut game, &mut number, 0, 0);
        if matches!(
            game.host().unwrap().runner.pending_command(),
            Some(shin_core::vm::higurashi::command::HigurashiRuntimeCommand::WAIT { .. })
        ) {
            break;
        }
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
        game.host()
            .unwrap()
            .runner
            .native_read_state()
            .unwrap()
            .flags
            & 0x80000000,
        0
    );
    for boundary in [
        controls::Boundary {
            captured: true,
            ..Default::default()
        },
        controls::Boundary {
            disabled: true,
            ..Default::default()
        },
    ] {
        game.set_input_boundary(boundary);
        tick(&mut game, &mut number, 1 << 25, 0);
        assert!(!game.navigation.pending());
        assert!(game.navigation.event_candidate.is_none());
    }
    game.set_input_boundary(Default::default());
    tick(&mut game, &mut number, 1 << 25, 0);
    assert!(game.navigation.pending());
    assert_eq!(game.pending_input_request(), Some(Request::SceneJump));
    assert_eq!(game.navigation.pending.as_ref().unwrap().mode, 0);
    render(&mut game, (1280, 720));
    renders += 1;
    wait_message(&mut game, &mut number, 1);
    assert_eq!(
        game.host()
            .unwrap()
            .runner
            .native_message_timeline()
            .unwrap()
            .boundary,
        3
    );
    eprintln!(
        "Gameplay navigation:{renders}720p/1080p owner frames;previous-message mode1/unavailable,chapter unpaused prompts/mode reset/No/B/late Yes/second forward search,mode0 masked return/current-previous chapter and loaded manual/chart/common/clock retention pass"
    );
}
