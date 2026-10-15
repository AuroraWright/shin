use super::transitions::{Frames, assets};
use super::*;
use crate::adv::gerokasu2::{
    play_scene::{Gerokasu2PlayScene, SceneEvent},
    runner::FramePolicy,
};
use shin_core::primitives::update::FrameId;
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
fn scene(gpu: &Offscreen, scenario: Arc<Gerokasu2Scenario>) -> Gerokasu2PlayScene {
    shin_tasks::block_on(Gerokasu2PlayScene::load(
        assets(gpu),
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
    .unwrap()
}
fn num(data: &mut Vec<u8>, v: i32) {
    let mut out = Cursor::new(Vec::new());
    NumberSpec::<i32>::constant(v).write_le(&mut out).unwrap();
    data.extend(out.into_inner());
}
fn text(data: &mut Vec<u8>, text: &str) {
    data.extend(((text.len() + 1) as u16).to_le_bytes());
    data.extend(text.as_bytes());
    data.push(0);
}
#[test]
#[ignore = "requires supplied ROM,Metal and exact Loopback Audio"]
fn ger_save_scene_decoded_metadata_autosave_real_message_capture_and_exit() {
    let gpu = Offscreen::new();
    let original = Gerokasu2Scenario::new(bytes().into()).unwrap();
    let mut data = bytes();
    data.truncate(original.header.code_offset as usize);
    data.push(0xa0);
    num(&mut data, 0);
    text(&mut data, "半角ｶﾅ😀");
    data.push(0xa0);
    num(&mut data, 1);
    text(&mut data, "第一章😀");
    data.push(0xa1);
    data.push(0xa0);
    num(&mut data, 1);
    text(&mut data, "次の章");
    data.push(0x85);
    num(&mut data, 0);
    data.extend([0x86, 1, 0, 0, 0]);
    num(&mut data, 0);
    text(&mut data, "@r保存中のメッセージ😀");
    data.push(0x87);
    num(&mut data, -1);
    data.extend([0, 0]);
    num(&mut data, 0);
    let size = data.len() as u32;
    data[4..8].copy_from_slice(&size.to_le_bytes());
    let mut scene = scene(&gpu, Arc::new(Gerokasu2Scenario::new(data.into()).unwrap()));
    let mut frame = FrameId::default();
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut captures = Vec::new();
    let mut titles = Vec::new();
    loop {
        frame.advance();
        let status = scene
            .step(frame, Ticks::from_f32(60.0), policy(), context())
            .unwrap();
        assert_ne!(
            status,
            RunnerStatus::External,
            "{:?}",
            scene.runner().pending_command()
        );
        for event in scene.take_events() {
            match event {
                SceneEvent::AutosaveCaptured(m) => captures.push(m),
                SceneEvent::SceneTitle(t) => titles.push(t),
                _ => {}
            }
        }
        if status == RunnerStatus::Exited {
            break;
        }
        scene.click_message(false).unwrap();
        assert!(Instant::now() < deadline);
    }
    assert_eq!(scene.runner().completed_commands(), 8);
    assert_eq!(titles, ["第一章😀"]);
    assert_eq!(captures.len(), 2);
    assert_eq!(captures[0].progress.active_entry().save_position, 1);
    assert_eq!(captures[1].progress.active_entry().save_position, 2);
    let save = scene.save_session();
    assert_eq!(save.metadata.current()[1].as_ref(), "次の章");
    assert_eq!(save.metadata.saved()[1].as_ref(), "第一章😀");
    assert_eq!(save.timeline.boundary, 2);
    assert_eq!(save.timeline.current_chapter, 1);
    assert_eq!(save.timeline.current_message, 2);
    assert_eq!(save.progress.active_entry().random_seed, 42);
    assert_eq!(save.progress.active_entry().replay_target, 2);
    assert!(save.autosave.valid && save.autosave.packed_time != 0);
    assert_eq!(save.checkpoints.len(), 1);
    assert_eq!(save.checkpoints[0].timeline.boundary, 0);
    let mut frames = Frames::new(&gpu);
    let pixels = frames.draw_scene(&gpu, scene.backend_mut());
    assert!(pixels.chunks_exact(4).any(|p| p[..3] != [0, 0, 0]));
    println!(
        "GEROKASU2_SAVE_SCENE commands=8 captures=2 title=1 boundary=2 real_UTF8_message_autosave_checkpoint_EXIT=verified frames={} channels={} audio=Loopback Audio",
        frames.frames, frames.channels
    );
}
#[test]
#[ignore = "requires supplied ROM,Metal and exact Loopback Audio"]
fn ger_save_scene_supplied_prologue_owns_save_and_message_progress_without_external_ack() {
    let gpu = Offscreen::new();
    let mut scene = scene(
        &gpu,
        Arc::new(Gerokasu2Scenario::new(bytes().into()).unwrap()),
    );
    let mut frame = FrameId::default();
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut messages = 0;
    let mut captures = 0;
    let mut titles = 0;
    let mut frames = Frames::new(&gpu);
    while scene.runner().completed_commands() < 100 || messages < 20 {
        frame.advance();
        let status = scene
            .step(frame, Ticks::from_f32(60.0), policy(), context())
            .unwrap();
        assert_ne!(
            status,
            RunnerStatus::External,
            "unowned {:?}",
            scene.runner().pending_command()
        );
        assert_ne!(status, RunnerStatus::Exited);
        for event in scene.take_events() {
            match event {
                SceneEvent::MessageInstalled(_, _) => messages += 1,
                SceneEvent::AutosaveCaptured(_) => captures += 1,
                SceneEvent::SceneTitle(t) => {
                    assert_eq!(t, "プロローグ");
                    titles += 1
                }
                _ => {}
            }
        }
        if status == RunnerStatus::Waiting {
            scene.click_message(false).unwrap();
        }
        assert!(
            Instant::now() < deadline,
            "{:?}",
            scene.runner().pending_command()
        );
    }
    assert!(captures >= messages + 1);
    assert_eq!(titles, 1);
    let save = scene.save_session();
    assert_eq!(save.metadata.saved()[1].as_ref(), "プロローグ");
    assert!(save.timeline.normal_messages >= 20);
    assert_eq!(
        save.timeline.boundary,
        save.progress.active_entry().save_position
    );
    assert_eq!(
        save.timeline.boundary,
        save.autosave.progress.active_entry().save_position
    );
    assert_eq!(save.autosave.progress.active_entry().random_seed, 42);
    assert!(!save.checkpoints.is_empty());
    frames.draw_scene(&gpu, scene.backend_mut());
    println!(
        "GEROKASU2_SAVE_PROLOGUE commands={} messages={} captures={} titles={} external_acknowledgements=0 owned_SAVEINFO_AUTOSAVE_real_PIC_UTF8_text_voice_progress=verified frames={} channels={} audio=Loopback Audio",
        scene.runner().completed_commands(),
        messages,
        captures,
        titles,
        frames.frames,
        frames.channels
    );
}

#[test]
#[ignore = "supplied authored-path diagnostic;requires Metal and exact Loopback Audio"]
fn ger_save_scene_authored_path_reaches_next_owner_boundary() {
    let gpu = Offscreen::new();
    let mut scene = scene(
        &gpu,
        Arc::new(Gerokasu2Scenario::new(bytes().into()).unwrap()),
    );
    let mut frame = FrameId::default();
    let limit = std::env::var("SHIN_GEROKASU2_AUTHORED_COMMAND_LIMIT")
        .map(|v| v.parse::<u64>().unwrap())
        .unwrap_or(16384);
    let timeout = std::env::var("SHIN_GEROKASU2_AUTHORED_TIMEOUT_SECONDS")
        .map(|v| v.parse::<u64>().unwrap())
        .unwrap_or(120);
    let deadline = Instant::now() + Duration::from_secs(timeout);
    let mut messages = 0;
    let mut captures = 0;
    let mut choices = 0;
    let mut movie_logged = false;
    while scene.runner().completed_commands() < limit {
        frame.advance();
        let status = match scene.step(frame, Ticks::from_f32(60.0), policy(), context()) {
            Ok(status) => status,
            Err(e)
                if e.to_string()
                    .starts_with("Gerokasu2 resource constructor is not connected for ") =>
            {
                assert!(matches!(
                    scene.runner().pending_command(),
                    Some(Gerokasu2RuntimeCommand::LayerLoad { .. })
                ));
                assert!(scene.runner().completed_commands() >= 100);
                println!(
                    "GEROKASU2_AUTHORED_MISSING_RESOURCE commands={} messages={} captures={} PC={:x} next={:?} error={e:#} external_acknowledgements=0",
                    scene.runner().completed_commands(),
                    messages,
                    captures,
                    scene.runner().vm().position().0,
                    scene.runner().pending_command()
                );
                return;
            }
            Err(e) => panic!(
                "authored command{} {:?}: {e:#}",
                scene.runner().completed_commands(),
                scene.runner().pending_command()
            ),
        };
        for event in scene.take_events() {
            match event {
                SceneEvent::MessageInstalled(_, _) => messages += 1,
                SceneEvent::AutosaveCaptured(_) => captures += 1,
                _ => {}
            }
        }
        if status == RunnerStatus::External {
            assert!(scene.runner().completed_commands() >= 100);
            println!(
                "GEROKASU2_AUTHORED_NEXT commands={} messages={} captures={} PC={:x} next={:?} external_acknowledgements=0",
                scene.runner().completed_commands(),
                messages,
                captures,
                scene.runner().vm().position().0,
                scene.runner().pending_command()
            );
            return;
        }
        if status == RunnerStatus::Exited {
            println!(
                "GEROKASU2_AUTHORED_EXIT commands={} messages={} captures={} choices={} PC={:x} external_acknowledgements=0",
                scene.runner().completed_commands(),
                messages,
                captures,
                choices,
                scene.runner().vm().position().0
            );
            return;
        }
        if status == RunnerStatus::Waiting {
            if matches!(scene.runner().pending_command(),Some(Gerokasu2RuntimeCommand::MovieWait{..}))&&!movie_logged {
                println!("GEROKASU2_AUTHORED_MOVIE_WAIT commands={} PC={:x} next={:?}",scene.runner().completed_commands(),scene.runner().vm().position().0,scene.runner().pending_command());movie_logged=true;
            }
            if scene.card_ready() {
                // Declared test input: select the first displayed card. No VM
                // feedback is supplied directly; the real layer completes.
                scene.select_card(0); // Already-selected card is a native no-op.
                assert!(scene.confirm_card());
                choices += 1;
                println!(
                    "GEROKASU2_AUTHORED_CARD_INPUT choice={choices} commands={}",
                    scene.runner().completed_commands()
                );
            }
            scene.click_message(false).unwrap();
        }
        assert!(
            Instant::now() < deadline,
            "authored command{} {:?}",
            scene.runner().completed_commands(),
            scene.runner().pending_command()
        );
    }
    println!(
        "GEROKASU2_AUTHORED_BUDGET commands={} messages={} captures={} external_acknowledgements=0",
        scene.runner().completed_commands(),
        messages,
        captures
    );
}
