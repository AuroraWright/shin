use super::super::{
    frame::tests::gpu::render_to_texture,
    message_glyphs::tests::gpu::gpu_assets,
    message_ui::{HigurashiMessageTextures, tests::gpu::replay_native_owner},
    scene_textures::HigurashiSceneTextures,
};
use super::*;
use shin_core::{format::scenario::instruction_elements::Register, primitives::color::UnormColor};
use shin_render::{
    PassKind,
    shaders::types::texture::{TextureTarget, TextureTargetKind},
};
use std::{
    io::Write,
    process::{Command, Stdio},
};

pub(super) fn policy() -> Policy {
    Policy {
        animations_enabled: true,
        paused: false,
        fast_skip: false,
        suppress_transient_audio: false,
        read_flags: 0,
        text_speed: 50,
        outline_mode: 0,
    }
}
#[derive(Default)]
pub(super) struct Voice {
    requests: Vec<String>,
}
impl MessageVoice for Voice {
    fn is_playing(&self) -> bool {
        false
    }
    fn position_micros(&self) -> u64 {
        0
    }
    fn play(&mut self, path: &str, _: f32, _: bool, _: u32, _: u32) -> bool {
        self.requests.push(path.into());
        false
    }
    fn stop(&mut self) {}
}

impl VoiceCommands for Voice {
    fn play_command(&mut self, path: &str, _: f32, _: i32) {
        self.requests.push(path.into());
    }
    fn status_bits(&self) -> i32 {
        0
    }
}

#[test]
#[ignore = "executes updated native WAIT callbacks and original ADV clock fragment"]
fn higurashi_runner_wait_matches_native_clock_and_signed_setup() {
    let mut input = String::new();
    let mut expected = Vec::new();
    for duration in [
        0,
        1,
        -1,
        12,
        -12,
        120,
        -120,
        i32::MIN,
        i32::MAX,
        -2147483647,
    ] {
        for allow in [0, 1, 2, 255] {
            for skip_at in [0, 3, 19] {
                input.push_str(&format!("N {duration} {allow}\n"));
                let mut wait = Wait::new(duration, allow);
                let result = wait.poll(false);
                expected.push(vec![
                    u32::from(result),
                    u32::from(wait.allow_interrupt),
                    wait.remaining.to_bits(),
                ]);
                for step in 0..20 {
                    let delta = [0.0f32, 0.5, 1.0, 6.0, -2.5, 30.0][step % 6];
                    let animate = step % 3 != 0;
                    let paused = step % 4 == 1;
                    input.push_str(&format!(
                        "U {:x} {} {}\n",
                        delta.to_bits(),
                        u8::from(animate),
                        u8::from(paused)
                    ));
                    if !paused {
                        wait.advance(delta);
                    }
                    expected.push(vec![
                        0,
                        u32::from(wait.allow_interrupt),
                        wait.remaining.to_bits(),
                    ]);
                    input.push_str(&format!("P {}\n", u8::from(step == skip_at)));
                    let result = wait.poll(step == skip_at);
                    expected.push(vec![
                        u32::from(result),
                        u32::from(wait.allow_interrupt),
                        wait.remaining.to_bits(),
                    ]);
                }
            }
        }
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_WAIT").unwrap())
        .arg(std::env::var("SHIN_HIGURASHI_EXE").unwrap())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let native = String::from_utf8(output.stdout).unwrap();
    assert_eq!(native.lines().count(), expected.len());
    for (row, (line, expected)) in native.lines().zip(&expected).enumerate() {
        let words: Vec<u32> = line
            .split_whitespace()
            .skip(1)
            .map(|w| w.parse().unwrap())
            .collect();
        assert_eq!(&words, expected, "WAIT row{row}");
    }
    for allow in [0, 1, 2, 255] {
        let mut wait = Wait::new(120, allow);
        assert_eq!(wait.interrupt(), allow == 1);
        assert_eq!(wait.remaining == 0.0, allow == 1);
    }
    println!(
        "Higurashi WAIT:120 native sequences/4920 snapshots/14760 observations match; signed durations, first poll, skip, pause and animation-independent clock pass"
    );
}

fn synthetic(code: &[u8]) -> Arc<HigurashiScenario> {
    let mut data = vec![0; 0x50];
    data[..4].copy_from_slice(b"SNR ");
    data[12..16].copy_from_slice(&65u32.to_le_bytes());
    data[16..20].copy_from_slice(&133u32.to_le_bytes());
    for i in 0..11 {
        let offset = data.len() as u32;
        data[36 + i * 4..40 + i * 4].copy_from_slice(&offset.to_le_bytes());
        if !matches!(i, 7 | 8) {
            data.extend_from_slice(&4u32.to_le_bytes());
        }
        data.extend_from_slice(&0u32.to_le_bytes());
    }
    let entry = data.len() as u32;
    data[32..36].copy_from_slice(&entry.to_le_bytes());
    data.extend_from_slice(code);
    let size = data.len() as u32;
    data[4..8].copy_from_slice(&size.to_le_bytes());
    Arc::new(HigurashiScenario::new(data.into()).unwrap())
}

#[test]
#[ignore = "runs authored updated prologue through retained VM/frame continuation and offscreen rendering; controlled external services"]
fn higurashi_runner_drives_authored_prologue_and_retains_vm_continuations() {
    let (device, queue, assets) = gpu_assets();
    let assets = Arc::new(assets);
    let scenario = shin_tasks::block_on(assets.load::<HigurashiScenario, _>("/main.snr")).unwrap();
    let textures = shin_tasks::block_on(HigurashiMessageTextures::load(&assets)).unwrap();
    let mut baseline = HigurashiScripter::new(&scenario, 0, 42).unwrap();
    let mut response = CommandResult::None;
    let mut runner = shin_tasks::block_on(HigurashiRunner::load(
        assets.clone(),
        scenario.clone(),
        0,
        42,
        || [1, 2, 3, 4],
    ))
    .unwrap();
    let mut voice = Voice::default();
    let mut messages = 0;
    let mut wipes = 0;
    let mut externals = 0;
    let mut frames = 0;
    let captures =
        std::path::PathBuf::from(std::env::var("SHIN_HIGURASHI_RUNNER_CAPTURES").unwrap());
    std::fs::create_dir_all(&captures).unwrap();
    for index in 0..100 {
        let expected = baseline.run(response, 100000).unwrap();
        response = match &expected {
            Cmd::SGET { dest, slot: 2 } => CommandResult::WriteMemory(*dest, 0),
            _ => CommandResult::None,
        };
        let mut status =
            shin_tasks::block_on(runner.pump(policy(), 1, 100000, || [1, 2, 3, 4], &mut voice))
                .unwrap();
        assert_eq!(
            runner.position(),
            baseline.position(),
            "command{index} {expected:?}"
        );
        assert_eq!(runner.next_position(), baseline.next_position());
        assert_eq!(runner.random_state(), baseline.random_state());
        if let Some(command) = runner.pending_command() {
            assert_eq!(command, &expected);
        }
        if matches!(expected, Cmd::LAYERLOAD { .. }) && status == Status::Waiting {
            let retained = (
                runner.position(),
                runner.next_position(),
                runner.random_state(),
            );
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            while status == Status::Waiting {
                assert!(
                    std::time::Instant::now() < deadline,
                    "layer load timed out: {expected:?}"
                );
                let mut seed_calls = 0;
                status = shin_tasks::block_on(runner.pump(
                    policy(),
                    1,
                    100000,
                    || {
                        seed_calls += 1;
                        [1, 2, 3, 4]
                    },
                    &mut voice,
                ))
                .unwrap();
                assert_eq!(
                    retained,
                    (
                        runner.position(),
                        runner.next_position(),
                        runner.random_state()
                    )
                );
                if status == Status::Waiting {
                    // A ready poll may construct fresh property owners. Only
                    // still-pending polls must avoid consuming their entropy.
                    assert_eq!(seed_calls, 0, "unready load consumed fresh seeds");
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
            }
            assert_eq!(status, Status::CommandBudget);
        }
        match status {
            Status::External => {
                assert!(
                    matches!(
                        &expected,
                        Cmd::SGET { slot: 2, .. }
                            | Cmd::SAVEINFO { .. }
                            | Cmd::AUTOSAVE {}
                            | Cmd::CHART { .. }
                            | Cmd::SEPLAY { .. }
                            | Cmd::SESTOP { .. }
                    ),
                    "new external service {expected:?}"
                );
                let retained = (
                    runner.position(),
                    runner.next_position(),
                    runner.random_state(),
                    runner.completed_commands(),
                );
                assert_eq!(
                    shin_tasks::block_on(runner.pump(
                        policy(),
                        1,
                        100000,
                        || [1, 2, 3, 4],
                        &mut voice
                    ))
                    .unwrap(),
                    Status::External
                );
                assert_eq!(
                    retained,
                    (
                        runner.position(),
                        runner.next_position(),
                        runner.random_state(),
                        runner.completed_commands()
                    )
                );
                assert!(
                    runner
                        .complete_external(CommandResult::WriteMemory(
                            Register::from_regular_register(31),
                            0
                        ))
                        .is_err()
                );
                runner.complete_external(response).unwrap();
                assert!(runner.complete_external(response).is_err());
                externals += 1;
            }
            Status::Waiting => {
                let retained = (
                    runner.position(),
                    runner.next_position(),
                    runner.random_state(),
                );
                let mut paused = policy();
                paused.paused = true;
                runner.advance(Ticks::from_f32(1000.0), paused, &mut voice, None);
                assert_eq!(
                    shin_tasks::block_on(runner.pump(
                        paused,
                        1,
                        100000,
                        || [1, 2, 3, 4],
                        &mut voice
                    ))
                    .unwrap(),
                    Status::Waiting
                );
                assert_eq!(
                    retained,
                    (
                        runner.position(),
                        runner.next_position(),
                        runner.random_state()
                    )
                );
                if let Cmd::MSGSET { text, .. } = &expected {
                    messages += 1;
                    for _ in 0..4096 {
                        let p = &runner.frame.message.playback;
                        if p.wait.is_some() && p.progress().iter().all(|v| *v == 1.0) {
                            break;
                        }
                        runner.advance(Ticks::from_f32(1.0), policy(), &mut voice, None);
                    }
                    assert!(runner.frame.message.playback.wait.is_some());
                    assert!(
                        runner
                            .frame
                            .message
                            .playback
                            .progress()
                            .iter()
                            .all(|v| *v == 1.0)
                    );
                    for size in [(1280, 720), (1920, 1080)] {
                        let render = |reference: bool| {
                            render_to_texture(&device, &queue, size, |context, view| {
                                let mut screens = HigurashiSceneTextures::new(context);
                                screens.prepare_scene(context, &runner.frame.scene).unwrap();
                                let mut pass = context.begin_pass(
                                    TextureTarget {
                                        view,
                                        kind: TextureTargetKind::RenderTexture,
                                    },
                                    Some(context.depth_stencil),
                                    "Higurashi authored VM runner",
                                );
                                pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
                                if reference {
                                    // Actual default outer sort reserves screen1/message2.
                                    screens.render_screen(&mut pass, &runner.frame.scene, 1);
                                    replay_native_owner(
                                        &mut pass,
                                        &runner.frame.message,
                                        text,
                                        &textures,
                                        2,
                                    );
                                } else {
                                    screens.render_frame(&mut pass, &runner.frame).unwrap();
                                }
                            })
                        };
                        let actual = render(false);
                        assert_eq!(actual, render(true), "message{messages} {size:?}");
                        assert!(actual.chunks_exact(4).any(|p| p != [0, 0, 0, 255]));
                        std::fs::write(
                            captures.join(format!("message{messages}-{}x{}.rgba", size.0, size.1)),
                            actual,
                        )
                        .unwrap();
                        frames += 1;
                    }
                    assert!(runner.click_message(&mut voice));
                } else {
                    assert!(matches!(expected, Cmd::WIPE { .. }), "pending {expected:?}");
                    wipes += 1;
                    runner.advance(Ticks::from_f32(120.0), policy(), &mut voice, None);
                }
                assert_eq!(
                    shin_tasks::block_on(runner.pump(
                        policy(),
                        1,
                        100000,
                        || [1, 2, 3, 4],
                        &mut voice
                    ))
                    .unwrap(),
                    Status::CommandBudget
                );
                assert_eq!(
                    retained,
                    (
                        runner.position(),
                        runner.next_position(),
                        runner.random_state()
                    ),
                    "poll consumed a VM instruction"
                );
            }
            Status::CommandBudget => {}
            Status::Exited => panic!("unexpected exit"),
        }
        assert_eq!(runner.completed_commands(), index + 1);
    }
    assert_eq!((messages, wipes, externals, frames), (7, 3, 7, 14));
    assert_eq!(voice.requests.len(), 2);
    // WAIT uses the clock after command polling, independent of animation gate.
    let mut timer = shin_tasks::block_on(HigurashiRunner::load(
        assets.clone(),
        synthetic(&[0x83, 1, 12, 0, 0, 0]),
        0,
        42,
        || [1, 2, 3, 4],
    ))
    .unwrap();
    assert_eq!(
        shin_tasks::block_on(timer.pump(policy(), 8, 100, || [1, 2, 3, 4], &mut voice)).unwrap(),
        Status::Waiting
    );
    assert!(timer.complete_external(CommandResult::None).is_err());
    let mut no_animation = policy();
    no_animation.animations_enabled = false;
    timer.advance(Ticks::from_f32(12.0), no_animation, &mut voice, None);
    assert_eq!(
        shin_tasks::block_on(timer.pump(no_animation, 8, 100, || [1, 2, 3, 4], &mut voice))
            .unwrap(),
        Status::Exited
    );
    assert_eq!(timer.completed_commands(), 2);
    assert_eq!(
        shin_tasks::block_on(timer.pump(policy(), 8, 100, || [1, 2, 3, 4], &mut voice)).unwrap(),
        Status::Exited
    );
    // Native6408 lowers missing Mask metadata to Default and retains the
    // ordinary waiting command until its published transition retires.
    let fallback_scenario = synthetic(&[0x8e, 1, 0, 60, 0, 0, 0, 0]);
    assert!(matches!(
        HigurashiScripter::new(&fallback_scenario, 0, 42)
            .unwrap()
            .run(CommandResult::None, 100)
            .unwrap(),
        Cmd::WIPE {
            args: [1, 0, 60],
            ..
        }
    ));
    let mut fallback = shin_tasks::block_on(HigurashiRunner::load(
        assets.clone(),
        fallback_scenario,
        0,
        42,
        || [1, 2, 3, 4],
    ))
    .unwrap();
    assert_eq!(
        shin_tasks::block_on(fallback.pump(policy(), 8, 100, || [1, 2, 3, 4], &mut voice)).unwrap(),
        Status::Waiting
    );
    let retained = (
        fallback.position(),
        fallback.next_position(),
        fallback.random_state(),
    );
    assert!(matches!(fallback.pending_command(), Some(Cmd::WIPE { .. })));
    assert_eq!(
        shin_tasks::block_on(fallback.pump(policy(), 8, 100, || [1, 2, 3, 4], &mut voice)).unwrap(),
        Status::Waiting
    );
    assert_eq!(
        retained,
        (
            fallback.position(),
            fallback.next_position(),
            fallback.random_state()
        )
    );
    assert_eq!(fallback.completed_commands(), 0);
    fallback.advance(Ticks::from_f32(60.), policy(), &mut voice, None);
    assert!(fallback.frame().scene.transition().is_none());
    assert_eq!(
        shin_tasks::block_on(fallback.pump(policy(), 1, 100, || [1, 2, 3, 4], &mut voice)).unwrap(),
        Status::CommandBudget
    );
    assert_eq!(fallback.completed_commands(), 1);
    assert_eq!(
        shin_tasks::block_on(fallback.pump(policy(), 8, 100, || [1, 2, 3, 4], &mut voice)).unwrap(),
        Status::Exited
    );
    println!(
        "Higurashi runner:100 authored command/PC/PRNG continuations match;7 live message waits,3 authored wipes,7 explicit external services,2 controlled voice requests and14 whole native-packet-replayed frames pass at720p/1080p; WAIT/exit/missing-Mask fallback retention pass"
    );
}

#[test]
#[ignore = "runs explicit voice bytecode through retained runner with authored NXA, offscreen Metal and exact Loopback Audio"]
fn higurashi_runner_executes_voice_commands_and_retains_audio_waits() {
    use super::super::{voice::HigurashiVoicePlayer, voice_commands};
    use shin_audio::AudioManager;
    use std::time::{Duration, Instant};
    let (_, _, assets) = gpu_assets();
    let assets = Arc::new(assets);
    let authored = shin_tasks::block_on(assets.load::<HigurashiScenario, _>("/main.snr")).unwrap();
    let mut voice = HigurashiVoicePlayer::new(
        Arc::new(AudioManager::new_for_tests()),
        assets.clone(),
        &authored,
    );
    voice.set_preferences(100, u32::MAX, true);
    voice.set_paused(true);
    let mut code = Vec::new();
    for (name, waits) in [
        ("S01/11/VTF_oishi_0001", true),
        ("S01/11/VTF_oishi_0002", false),
    ] {
        code.extend([0x9c, name.len() as u8 + 1]);
        code.extend(name.as_bytes());
        code.push(0);
        code.extend([0x81, 0xf4, 3]); // volume500, raw flags3.
        if waits {
            code.extend([0x9e, 1, 0x9e, 2]);
        }
    }
    code.extend([0x9d, 0x9e, 0x7f, 0, 0, 0]); // STOP; WAIT(-1); EXIT(0,0).
    let mut runner = shin_tasks::block_on(HigurashiRunner::load(
        assets.clone(),
        synthetic(&code),
        0,
        42,
        || [1, 2, 3, 4],
    ))
    .unwrap();
    let pump = |runner: &mut HigurashiRunner, p: Policy, voice: &mut HigurashiVoicePlayer| {
        shin_tasks::block_on(runner.pump(p, 1, 100, || [1, 2, 3, 4], voice)).unwrap()
    };
    assert_eq!(
        pump(&mut runner, policy(), &mut voice),
        Status::CommandBudget
    );
    assert_eq!(voice.status_bits(), 3);
    assert_eq!(voice.position_micros(), 0);
    assert_eq!(pump(&mut runner, policy(), &mut voice), Status::Waiting);
    assert!(matches!(
        runner.pending_command(),
        Some(Cmd::VOICEWAIT { status: 1 })
    ));
    assert!(runner.complete_external(CommandResult::None).is_err());
    let retained = (
        runner.position(),
        runner.next_position(),
        runner.completed_commands(),
    );
    for _ in 0..3 {
        assert_eq!(pump(&mut runner, policy(), &mut voice), Status::Waiting);
        assert_eq!(
            (
                runner.position(),
                runner.next_position(),
                runner.completed_commands()
            ),
            retained
        );
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while voice.status_bits() & 1 != 0 {
        assert!(Instant::now() < deadline, "authored voice load timed out");
        voice.update(Ticks::ZERO);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(voice.status_bits(), 2);
    assert_eq!(voice.position_micros(), 0);
    let mappings = super::super::voice::mappings(&authored);
    let speakers = crate::audio::voice_speakers(
        &mappings,
        "S01/11/VTF_oishi_0001",
        crate::audio::VoicePlayFlags::ENABLE_CHARACTER_LIPSYNC,
    );
    assert!(!speakers.is_empty());
    let character = i32::from(speakers[0]);
    assert!(voice.character_lipsync(character, -0.5).is_some());
    assert_eq!(
        pump(&mut runner, policy(), &mut voice),
        Status::CommandBudget
    );
    assert_eq!(pump(&mut runner, policy(), &mut voice), Status::Waiting);
    assert!(matches!(
        runner.pending_command(),
        Some(Cmd::VOICEWAIT { status: 2 })
    ));
    let mut skip = policy();
    skip.fast_skip = true;
    skip.suppress_transient_audio = true;
    assert_eq!(pump(&mut runner, skip, &mut voice), Status::Waiting);
    let mut paused = policy();
    paused.paused = true;
    assert_eq!(pump(&mut runner, paused, &mut voice), Status::Waiting);
    assert_eq!(runner.completed_commands(), 2);
    voice.set_paused(false);
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut advanced = false;
    while voice.status_bits() & 2 != 0 {
        assert!(
            Instant::now() < deadline,
            "authored voice did not reach EOF"
        );
        advanced |= voice.position_micros() > 0;
        voice.update(Ticks::from_f32(1.0));
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(advanced);
    assert_eq!(
        pump(&mut runner, policy(), &mut voice),
        Status::CommandBudget
    );
    assert_eq!(
        pump(&mut runner, policy(), &mut voice),
        Status::CommandBudget
    );
    assert_eq!(voice.status_bits(), 3); // second PLAY remains loading.
    assert_eq!(
        pump(&mut runner, policy(), &mut voice),
        Status::CommandBudget
    ); // STOP cancels it.
    assert_eq!(voice.status_bits(), 0);
    voice.update(Ticks::from_f32(10.0));
    assert_eq!(
        pump(&mut runner, policy(), &mut voice),
        Status::CommandBudget
    );
    assert_eq!(pump(&mut runner, policy(), &mut voice), Status::Exited);
    assert_eq!(runner.completed_commands(), 7);
    // Muting and lipsync bits remain independent for explicit commands.
    voice.set_paused(true);
    voice.set_preferences(100, 0, true);
    let known = Cmd::VOICEPLAY {
        name: "S01/11/VTF_oishi_0001".into(),
        args: [1000, 2],
    };
    assert_eq!(
        voice_commands::dispatch(&known, false, &mut voice),
        Some(CommandStatus::Complete)
    );
    assert_eq!(voice.status_bits(), 0);
    voice.play_command("S01/11/VTF_oishi_0001", 1.0, 0);
    assert_eq!(voice.status_bits(), 3);
    let deadline = Instant::now() + Duration::from_secs(10);
    while voice.status_bits() & 1 != 0 {
        assert!(Instant::now() < deadline);
        voice.update(Ticks::ZERO);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(voice.status_bits(), 2);
    assert!(voice.character_lipsync(character, 0.5).is_none());
    voice.stop();
    voice.update(Ticks::from_f32(10.0));
    // A suppressed invalid filename completes without loading or replacing audio.
    let skipped = Cmd::VOICEPLAY {
        name: "../must-not-load".into(),
        args: [1000, 3],
    };
    assert_eq!(
        voice_commands::dispatch(&skipped, true, &mut voice),
        Some(CommandStatus::Complete)
    );
    assert_eq!(voice.status_bits(), 0);
    let missing = Cmd::VOICEPLAY {
        name: "missing-file".into(),
        args: [1000, 0],
    };
    assert_eq!(
        voice_commands::dispatch(&missing, false, &mut voice),
        Some(CommandStatus::Complete)
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while voice.status_bits() & 1 != 0 {
        assert!(Instant::now() < deadline);
        voice.update(Ticks::ZERO);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(voice_commands::wait(-1, &voice), CommandStatus::Complete);

    // Replay the first authored explicit PLAY/WAIT bytes unchanged in a
    // bounded scenario. This tests real flag1 (lipsync, no character muting),
    // unlike message voices which always request the muting bit.
    let mut pair = authored.raw()[0x61ff6..0x6200e].to_vec();
    pair.extend([0x9d, 0, 0, 0]); // STOP; EXIT after the authored wait.
    let mut authored_runner = shin_tasks::block_on(HigurashiRunner::load(
        assets.clone(),
        synthetic(&pair),
        0,
        42,
        || [1, 2, 3, 4],
    ))
    .unwrap();
    assert_eq!(
        pump(&mut authored_runner, policy(), &mut voice),
        Status::CommandBudget
    );
    assert_eq!(voice.status_bits(), 3); // mask0 must not mute authored flags1.
    assert_eq!(
        pump(&mut authored_runner, policy(), &mut voice),
        Status::Waiting
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while voice.status_bits() & 1 != 0 {
        assert!(
            Instant::now() < deadline,
            "first explicit authored voice load timed out"
        );
        voice.update(Ticks::ZERO);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(voice.status_bits(), 2);
    assert_eq!(voice.position_micros(), 0);
    assert_eq!(
        pump(&mut authored_runner, skip, &mut voice),
        Status::Waiting
    );
    // Controlled host stop releases the retained authored WAIT. Full route
    // navigation and the native platform loop are outside this fixture.
    voice.stop();
    assert_eq!(
        pump(&mut authored_runner, policy(), &mut voice),
        Status::CommandBudget
    );
    assert_eq!(
        pump(&mut authored_runner, policy(), &mut voice),
        Status::CommandBudget
    );
    assert_eq!(
        pump(&mut authored_runner, policy(), &mut voice),
        Status::Exited
    );

    // Suppression is resolved separately from fast_skip at the real runner.
    let mut suppressed = policy();
    suppressed.suppress_transient_audio = true;
    let mut suppressed_runner = shin_tasks::block_on(HigurashiRunner::load(
        assets,
        synthetic(&pair),
        0,
        42,
        || [1, 2, 3, 4],
    ))
    .unwrap();
    assert_eq!(
        pump(&mut suppressed_runner, suppressed, &mut voice),
        Status::CommandBudget
    );
    assert_eq!(voice.status_bits(), 0);
    assert_eq!(
        pump(&mut suppressed_runner, suppressed, &mut voice),
        Status::CommandBudget
    );
    assert_eq!(
        pump(&mut suppressed_runner, suppressed, &mut voice),
        Status::CommandBudget
    );
    assert_eq!(
        pump(&mut suppressed_runner, suppressed, &mut voice),
        Status::Exited
    );
    voice.update(Ticks::from_f32(10.0));
    println!(
        "Higurashi runner voice: exact Loopback Audio;7 real VM commands, authored0001 EOF/zero origin and0002 loading cancellation, retained LOADING/PLAYING waits, pause/skip ownership, mask/flags/lipsync, suppressed filename and failed-load release pass; unchanged first authored explicit PLAY/WAIT bytes at61ff6/6200c load S10/40/430100047 with flags1 and mask0, controlled stop releases WAIT, independent host suppression passes"
    );
}
