use super::transitions::{Frames, assets};
use super::*;
use crate::adv::gerokasu2::{
    play_scene::{Gerokasu2PlayScene, SceneEvent},
    runner::FramePolicy,
};
use shin_core::{format::scenario::instruction_elements::Register, primitives::update::FrameId};
fn num(d: &mut Vec<u8>, value: i32) {
    let mut c = Cursor::new(Vec::new());
    NumberSpec::<i32>::constant(value).write_le(&mut c).unwrap();
    d.extend(c.into_inner());
}
fn text(d: &mut Vec<u8>, v: &str) {
    d.extend(((v.len() + 1) as u16).to_le_bytes());
    d.extend(v.as_bytes());
    d.push(0);
}
fn context() -> MessageCommandContext {
    MessageCommandContext {
        event_mode: -1,
        was_read: false,
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
#[ignore = "requires supplied ROM,Metal and exact Loopback Audio"]
fn ger_event_variables_scene_live_pending_commit_message_gate_route_bookmark_and_exit() {
    let gpu = Offscreen::new();
    let original = Gerokasu2Scenario::new(bytes().into()).unwrap();
    let mut d = bytes();
    d.truncate(original.header.code_offset as usize);
    d.push(0x82);
    num(&mut d, 0);
    num(&mut d, 65537);
    d.extend([0x81, 0, 0]);
    num(&mut d, 0);
    d.push(0xa2);
    num(&mut d, 42);
    d.extend([0x81, 1, 0]);
    num(&mut d, 0);
    d.push(0x82);
    num(&mut d, 0);
    num(&mut d, -32768);
    d.extend([0x86, 1, 0, 0, 0]);
    num(&mut d, 0);
    text(&mut d, "@rイベントのテキスト");
    d.extend([0x83, 0]);
    num(&mut d, 60);
    d.push(0xa3);
    d.extend([0x81, 2, 0]);
    num(&mut d, 0);
    d.extend([0xa4, 0xa1, 0, 0]);
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
    let mut frame = FrameId::default();
    let mut frames = Frames::new(&gpu);
    let mut captures = Vec::new();
    let mut event_wait = false;
    let deadline = Instant::now() + Duration::from_secs(10);
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
        for event in scene.take_events() {
            if let SceneEvent::AutosaveCaptured(memory) = event {
                captures.push(memory);
            }
        }
        if matches!(
            scene.runner().pending_command(),
            Some(Gerokasu2RuntimeCommand::Wait { .. })
        ) {
            event_wait = true;
            let save = scene.save_session();
            assert_eq!(save.timeline.event, 42);
            assert_eq!(save.timeline.boundary, 1);
            assert_eq!(save.timeline.normal_messages, 0);
            assert_eq!(save.variables.get(0), -32768);
            assert_eq!(save.variables.committed().get(0), 1);
            assert!(!scene.click_message(false).unwrap());
        }
        frames.draw_scene(&gpu, scene.backend_mut());
        if status == RunnerStatus::Exited {
            break;
        }
        assert!(Instant::now() < deadline);
    }
    assert!(event_wait);
    let vm = scene.runner().vm();
    for (dest, value) in [(0, 65537), (1, 1), (2, -32768)] {
        assert_eq!(
            vm.read_register(Register::from_regular_register(dest)),
            value
        );
    }
    assert_eq!(scene.runner().completed_commands(), 12);
    let save = scene.save_session();
    assert_eq!(save.timeline.event, -1);
    assert_eq!(save.timeline.boundary, 4);
    assert_eq!(save.timeline.resume_position, 3);
    assert_eq!(save.response_bit_position, 36);
    assert_eq!(scene.persistent().get(0), -32768);
    assert!(save.variables.pending().is_empty());
    assert_eq!(captures.len(), 2);
    assert_eq!(captures[0].progress.active_entry().save_position, 1);
    assert_eq!(captures[1].progress.active_entry().save_position, 4);
    assert_eq!(
        save.autosave.progress.active_entry().responses,
        save.progress.active_entry().responses
    );
    println!(
        "GEROKASU2_EVENT_VARIABLES_SCENE commands=12 message_input_gated=1 full32_before_commit=65537 signed16_after_commit=-32768 boundaries=4 bookmark=3 route_bits=36 captures=2 external_acknowledgements=0 frames={} channels={} audio=Loopback Audio",
        frames.frames, frames.channels
    );
}
