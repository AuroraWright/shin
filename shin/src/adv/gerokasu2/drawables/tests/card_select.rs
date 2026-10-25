use super::transitions::{Frames, assets};
use super::*;
use crate::adv::gerokasu2::{
    layer_factory::GerDrawable,
    play_scene::{Gerokasu2PlayScene, SceneEvent},
    runner::FramePolicy,
};
use shin_core::{format::scenario::instruction_elements::Register, primitives::update::FrameId};
fn num(d: &mut Vec<u8>, v: i32) {
    let mut c = Cursor::new(Vec::new());
    NumberSpec::<i32>::constant(v).write_le(&mut c).unwrap();
    d.extend(c.into_inner());
}
fn context() -> MessageCommandContext {
    MessageCommandContext {
        was_read: false,
        event_mode: -1,
        skip: false,
        message_speed: 50,
    }
}
fn policy() -> FramePolicy {
    FramePolicy {
        paused: false,
        scene_changing: false,
        animations_allowed: true,
        autoplay: false,
        fast_skip: false,
    }
}
#[test]
#[ignore = "requires original card atlas,Metal and the default audio output"]
fn ger_card_scene_real_resource_pointer_quiz_feedback_save_route_and_exit() {
    let gpu = Offscreen::new();
    let original = Gerokasu2Scenario::new(bytes().into()).unwrap();
    let mut d = bytes();
    d.truncate(original.header.code_offset as usize);
    d.push(0xc1);
    for v in [3, 9, 0] {
        num(&mut d, v);
    }
    d.push(2);
    num(&mut d, 5);
    d.extend([0xe0, 0, 0]);
    num(&mut d, 3);
    d.extend([0xe0, 1, 0]);
    num(&mut d, 7); // Actual missing layer returns zero.
    d.extend([0, 0]);
    num(&mut d, 0);
    let size = d.len() as u32;
    d[4..8].copy_from_slice(&size.to_le_bytes());
    let mut scene = shin_tasks::block_on(Gerokasu2PlayScene::load(
        assets(&gpu),
        Arc::new(Gerokasu2Scenario::new(d.into()).unwrap()),
        Arc::new(AudioManager::new_for_tests()),
        0,
        42,
        InitialMessage {
            style: 0,
            asynchronous: false,
            face: -1,
        },
        Default::default(),
        context(),
    ))
    .unwrap();
    assert!(!scene.confirm_card());
    let mut frame = FrameId::default();
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut frames = Frames::new(&gpu);
    let mut captures = Vec::new();
    let mut submitted = false;
    let mut visible = false;
    loop {
        frame.advance();
        let status = scene
            .step(frame, Ticks::from_f32(30.0), policy(), context())
            .unwrap();
        assert_ne!(
            status,
            RunnerStatus::External,
            "{:?}",
            scene.runner().pending_command()
        );
        for e in scene.take_events() {
            if let SceneEvent::AutosaveCaptured(m) = e {
                captures.push(m);
            }
        }
        if scene
            .runner()
            .layers()
            .target(LayerId::try_new(3).unwrap())
            .is_some()
        {
            let pixels = frames.draw_scene(&gpu, scene.backend_mut());
            visible |= pixels.chunks_exact(4).any(|p| p[..3] != [0, 0, 0]);
        }
        if scene.card_ready() && !submitted {
            assert_eq!(status, RunnerStatus::Waiting);
            assert_eq!(
                scene
                    .runner()
                    .vm()
                    .read_register(Register::from_regular_register(0)),
                0
            );
            let target = scene
                .runner()
                .layers()
                .target(LayerId::try_new(3).unwrap())
                .unwrap();
            let guard = scene
                .backend()
                .layer(scene.runner().layers().current_plane(), target.layerbank)
                .unwrap();
            let GerDrawable::Card(layer) = &guard.layer else {
                panic!("actual CardSelect drawable");
            };
            let m = layer.inner_ref().model.lock();
            assert_eq!(m.cards.iter().map(|c| c.face).collect::<Vec<_>>(), [0, 2]);
            let point = glam::Vec2::new(m.cards[1].tweens[2].value(), m.cards[1].tweens[3].value());
            drop(m);
            drop(guard);
            assert!(scene.card_pointer_move(point));
            assert!(scene.card_pointer_press());
            assert!(scene.card_pointer_release());
            assert!(!scene.confirm_card());
            submitted = true;
        }
        if status == RunnerStatus::Exited {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "{:?}",
            scene.runner().pending_command()
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(submitted, "real input was not submitted");
    assert!(visible, "card frame stayed black;{} frames", frames.frames);
    assert_eq!(scene.runner().completed_commands(), 4);
    assert_eq!(
        scene
            .runner()
            .vm()
            .read_register(Register::from_regular_register(0)),
        2
    );
    assert_eq!(
        scene
            .runner()
            .vm()
            .read_register(Register::from_regular_register(1)),
        0
    );
    let save = scene.save_session();
    assert_eq!(save.timeline.boundary, 4);
    assert_eq!(save.timeline.current_interaction, 3);
    assert_eq!(save.response_bit_position, 12);
    assert_eq!(save.progress.active_entry().responses.len(), 1024);
    assert_eq!(save.progress.active_entry().responses[0], 8);
    assert_eq!(captures.len(), 4);
    assert_eq!(captures[0].progress.active_entry().save_position, 1);
    assert_eq!(captures[1].progress.active_entry().save_position, 2);
    assert_eq!(save.autosave.progress.active_entry().responses[0], 8);
    let target = scene
        .runner()
        .layers()
        .target(LayerId::try_new(3).unwrap())
        .unwrap();
    assert!(
        scene
            .runner()
            .layers()
            .record(target.layerbank)
            .interaction_completed()
    );
    assert_eq!(
        scene
            .runner()
            .layers()
            .record(target.layerbank)
            .interaction_result(),
        2
    );
    println!(
        "GEROKASU2_CARD_SCENE commands=4 pointer_result=2 missing_result=0 boundaries=4 captures=4 route_bits=12 external_acknowledgements=0 frames={} channels={} audio=default output",
        frames.frames, frames.channels
    );
}
