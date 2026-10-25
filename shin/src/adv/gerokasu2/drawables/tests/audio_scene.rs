use super::transitions::{Frames, assets};
use super::*;
use crate::adv::gerokasu2::{
    play_scene::{Gerokasu2PlayScene, SceneEvent},
    runner::FramePolicy,
};
use shin_core::time::Tween;
use shin_core::{
    primitives::update::FrameId,
    vm::{
        command::types::{AudioWaitStatus, Pan, Volume},
        gerokasu2::audio::Gerokasu2AudioCommand as Audio,
    },
};
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
#[ignore = "requires supplied ROM,Metal and the default audio output"]
fn decoded_ger_audio_bgm_se_once_real_transport_waits_and_exit() {
    let gpu = Offscreen::new();
    let assets = assets(&gpu);
    let parsed = Gerokasu2Scenario::new(bytes().into()).unwrap();
    let mut data = bytes();
    data.truncate(parsed.header.code_offset as usize);
    fn number(data: &mut Vec<u8>, v: i32) {
        let mut out = Cursor::new(Vec::new());
        NumberSpec::<i32>::constant(v).write_le(&mut out).unwrap();
        data.extend(out.into_inner());
    }
    fn op(data: &mut Vec<u8>, code: u8, args: &[i32]) {
        data.push(code);
        for &v in args {
            number(data, v);
        }
    }
    op(&mut data, 0x90, &[0, 0, 2, 1500]);
    op(&mut data, 0x93, &[1]);
    op(&mut data, 0x94, &[0]);
    op(&mut data, 0x92, &[700, 0]);
    op(&mut data, 0x95, &[0, 0, 0, 0, 1500, -500, 1000]);
    op(&mut data, 0x9a, &[0, 1]);
    op(&mut data, 0x98, &[0, 500, 0]);
    op(&mut data, 0x99, &[0, 500, 0]);
    op(&mut data, 0x96, &[0, 0]);
    op(&mut data, 0x9a, &[0, 3]);
    op(&mut data, 0x9b, &[0, 0, 1000, 0, 9999]);
    op(&mut data, 0x97, &[0]);
    op(&mut data, 0x9a, &[-1, 3]);
    op(&mut data, 0x91, &[0]);
    op(&mut data, 0x93, &[3]);
    data.extend([0, 0]);
    number(&mut data, 0);
    let size = data.len() as u32;
    data[4..8].copy_from_slice(&size.to_le_bytes());
    let scenario = Arc::new(Gerokasu2Scenario::new(data.into()).unwrap());
    let mut scene = shin_tasks::block_on(Gerokasu2PlayScene::load(
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
        context(),
    ))
    .unwrap();
    let mut frames = Frames::new(&gpu);
    let mut frame = FrameId::default();
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut bgm_events = 0;
    let mut waits = 0;
    loop {
        frame.advance();
        let status = scene
            .step(frame, Ticks::from_f32(1.0), policy(), context())
            .unwrap();
        assert_ne!(
            status,
            RunnerStatus::External,
            "unowned command {:?}",
            scene.runner().pending_command()
        );
        for event in scene.take_events() {
            if matches!(event, SceneEvent::BgmChanged(0)) {
                bgm_events += 1;
            }
        }
        if status == RunnerStatus::Waiting {
            waits += 1;
        }
        if let Some(Gerokasu2RuntimeCommand::Audio(Audio::BgmWait { mask: 1 })) =
            scene.runner().pending_command()
        {
            if let Some(count) = scene
                .reference_audio()
                .bgm_player
                .reference_loading_loop_count()
            {
                assert_eq!(count, 2);
            }
        }
        if status == RunnerStatus::Exited {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(scene.runner().completed_commands(), 16);
    assert_eq!(bgm_events, 1);
    assert!(waits > 0);
    assert!(
        scene
            .reference_audio()
            .bgm_player
            .get_wait_status()
            .is_empty()
    );
    assert!(
        scene
            .reference_audio()
            .se_player
            .get_wait_status(-1)
            .is_empty()
    );
    assert!(
        frames
            .draw_scene(&gpu, scene.backend_mut())
            .chunks_exact(4)
            .all(|p| p == [0, 0, 0, 255])
    );
    println!(
        "GEROKASU2_AUDIO_SCENE commands=16 bgm_events=1 real_NXA_BGM_SE_once_raw_loop2_waits_sync_stop_EXIT=verified frames={} channels={} audio=default output",
        frames.frames, frames.channels
    );
}

#[test]
#[ignore = "requires supplied ROM,Metal and the default audio output"]
fn ger_audio_negative_active_slots_volume_pan_stop_and_legacy_bounds() {
    let gpu = Offscreen::new();
    let assets = assets(&gpu);
    let scenario = Arc::new(Gerokasu2Scenario::new(bytes().into()).unwrap());
    let mut scene = shin_tasks::block_on(Gerokasu2PlayScene::load(
        assets.clone(),
        scenario.clone(),
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
    let mut frames = Frames::new(&gpu);
    let mut audio = scene.reference_audio_mut();
    let player = &mut audio.se_player;
    let path = format!("/se/{}.nxa", scenario.se_info[0].name.as_str());
    let slot = player.request_once(
        assets.clone(),
        path,
        <shin_core::format::scenario::info::SeId as shin_core::format::scenario::instruction_elements::FromNumber>::from_number(0),
        Volume(1.0),
        Pan(0.0),
    );
    assert_eq!(slot, -1);
    let deadline = Instant::now() + Duration::from_secs(10);
    while player
        .get_once_wait_status(slot)
        .contains(AudioWaitStatus::LOADING)
    {
        player.update();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(
        player
            .get_once_wait_status(slot)
            .contains(AudioWaitStatus::PLAYING)
    );
    player.set_volume(slot, Volume(0.5), Tween::IMMEDIATE);
    player.set_panning(slot, Pan(-0.5), Tween::IMMEDIATE);
    player.stop(slot, Tween::IMMEDIATE);
    assert!(player.get_once_wait_status(slot).is_empty());
    player.stop(-999, Tween::IMMEDIATE);
    player.set_volume(-999, Volume(1.0), Tween::IMMEDIATE);
    player.set_panning(-999, Pan(0.0), Tween::IMMEDIATE);
    drop(audio);
    assert!(
        frames
            .draw_scene(&gpu, scene.backend_mut())
            .chunks_exact(4)
            .all(|p| p == [0, 0, 0, 255])
    );
    println!(
        "GEROKASU2_AUDIO_NEGATIVE active_once_volume_pan_stop_missing_negative_slot_safe=verified frames={} channels={} audio=default output",
        frames.frames, frames.channels
    );
}
