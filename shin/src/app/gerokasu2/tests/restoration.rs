use super::*;
use shin_core::format::save::gerokasu2::{GameData, Progress, SaveData, SaveDate};

#[test]
#[ignore = "actual RESUME mode3 worker/publication, Metal and the default audio output"]
fn ger_app_resume_reconstructs_bookmark_and_keeps_constructor_boundary_once() {
    let gpu = Offscreen::new();
    let original = Gerokasu2Scenario::new(bytes().into()).unwrap();
    let mut code = bytes();
    code.truncate(original.header.code_offset as usize);
    code.push(0xc1);
    for word in [0, 1, 0] {
        num(&mut code, word);
    }
    code.push(0xff);
    for word in [0xffff, -960, -540, 1920, 1080, 0, 0, 0] {
        num(&mut code, word);
    }
    code.push(0xa4); // RESUMESET constructor owns boundary1.
    code.extend([0x83, 1]);
    num(&mut code, 1);
    code.push(0xa5);
    code.extend([0, 0, 0]);
    let size = code.len() as u32;
    code[4..8].copy_from_slice(&size.to_le_bytes());
    let mut host = host(&gpu, code);
    let mut frame = FrameId::default();
    tick(&mut host, &mut frame, &EnumMap::default(), Vec2::ZERO, 1.0);
    assert_eq!(host.scene.save_session().timeline.resume_position, 1);
    tick(&mut host, &mut frame, &EnumMap::default(), Vec2::ZERO, 0.0);
    assert_eq!(
        host.scene.reconstruction_phase(),
        Some(crate::adv::gerokasu2::reconstruction_mode::Phase::Preparing)
    );
    assert!(!host.scene.backend().animations_allowed());
    let mut frames = Frames::new(&gpu);
    frames.draw(&gpu, &mut host);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while host.scene.reconstruction_phase().is_some() {
        tick(&mut host, &mut frame, &EnumMap::default(), Vec2::ZERO, 0.0);
        assert!(
            std::time::Instant::now() < deadline,
            "Ger RESUME worker did not retire"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(
        host.scene.save_session().timeline.boundary,
        1,
        "retained RESUMESET constructor ran twice"
    );
    assert_eq!(
        host.scene
            .save_session()
            .progress
            .active_entry()
            .save_position,
        1
    );
    assert_eq!(
        host.scene
            .save_session()
            .progress
            .active_entry()
            .replay_target,
        1
    );
    assert!(
        matches!(
            host.scene.runner().pending_command(),
            Some(shin_core::vm::gerokasu2::Gerokasu2RuntimeCommand::Wait { .. })
        ),
        "immediate mode3 must pump rebuilt VM in publication frame"
    );
    assert_eq!(host.scene.runner().wait_clock().remaining(), 1.0);
    assert!(host.scene.backend().animations_allowed());
    let pixels = frames.draw(&gpu, &mut host);
    assert!(pixels.chunks_exact(4).any(|p| p[..3] != [0, 0, 0]));
    println!(
        "GEROKASU2_RESUME_APP mode3_native_bookmark_targets_tile_hydration_retained_constructor_once_same_frame_WAIT=verified frames={} audio=default output",
        frames.frames
    );
}

#[test]
#[ignore = "actual native-codec save import, restored tile/message handoff, Metal and the default audio output"]
fn ger_app_native_saved_progress_hydrates_and_resumes_without_recapturing_constructor() {
    let gpu = Offscreen::new();
    let original = Gerokasu2Scenario::new(bytes().into()).unwrap();
    let mut code = bytes();
    code.truncate(original.header.code_offset as usize);
    code.push(0xc1);
    for word in [0, 1, 0] {
        num(&mut code, word);
    }
    code.push(0xff);
    for word in [0xffff, -960, -540, 1920, 1080, 0, 0, 0] {
        num(&mut code, word);
    }
    msg(&mut code, 1, "@r復元したメッセージ");
    code.extend([0, 0, 0]);
    let size = code.len() as u32;
    code[4..8].copy_from_slice(&size.to_le_bytes());
    let scenario = Arc::new(Gerokasu2Scenario::new(code.clone().into()).unwrap());
    let mut host = host(&gpu, code);
    let mut progress = Progress::fresh(0, 42);
    progress.active_entry_mut().save_position = 1;
    progress.active_entry_mut().replay_target = 1;
    let mut data = SaveData::default();
    data.save_vectors.seen_messages = vec![1, 0];
    data.save_vectors.select_seen_choices = vec![0x80000000, 0];
    data.save_vectors.select_last_choices = vec![3, 0, 0];
    data.persist_data.0 = vec![-731, 0];
    data.auto_save_slot = Some(GameData {
        date: SaveDate::now(),
        progress,
    });
    // Exercise the actual Ger game key and inclusive native entry decoder.
    let data = SaveData::decode(&data.encode().unwrap()).unwrap();
    shin_tasks::block_on(host.restore_saved(scenario, &data, 0)).unwrap();
    assert_eq!(host.scene.save_session().timeline.boundary, 1);
    assert_eq!(host.scene.save_session().checkpoints.len(), 1);
    assert!(matches!(
        host.scene.runner().pending_command(),
        Some(shin_core::vm::gerokasu2::Gerokasu2RuntimeCommand::MessageSet { .. })
    ));
    assert!(host.scene.runner().message().message().is_none());
    let mut exported = SaveData::default();
    host.scene.export_save_globals(&mut exported);
    assert_eq!(
        exported.save_vectors.seen_messages,
        data.save_vectors.seen_messages
    );
    assert_eq!(
        exported.save_vectors.select_seen_choices,
        data.save_vectors.select_seen_choices
    );
    assert_eq!(
        exported.save_vectors.select_last_choices,
        data.save_vectors.select_last_choices
    );
    assert_eq!(exported.persist_data.0, data.persist_data.0);
    let mut frame = FrameId::default();
    tick(&mut host, &mut frame, &EnumMap::default(), Vec2::ZERO, 1.0);
    assert_eq!(
        host.scene.save_session().timeline.boundary,
        1,
        "retained MSGSET constructor must not run twice"
    );
    assert!(host.scene.runner().message().message().is_some());
    let mut frames = Frames::new(&gpu);
    let pixels = frames.draw(&gpu, &mut host);
    assert!(pixels.chunks_exact(4).any(|pixel| pixel[..3] != [0, 0, 0]));
    println!(
        "GEROKASU2_SAVE_RESTORATION_APP native_codec_slot0_tile_message_VM_feedback_globals_raw_tails_constructor_once=verified frames={} audio=default output",
        frames.frames
    );
}
