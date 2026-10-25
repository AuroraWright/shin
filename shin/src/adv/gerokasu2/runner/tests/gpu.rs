//! Real retained driver, resources, message/voice owners and offscreen frames.
use super::*;
use crate::{
    asset::{
        font::gerokasu2::draw::gpu_tests::Offscreen,
        system::{AssetLoadContext, locate_assets_for_game},
    },
    cli::Game,
};
use binrw::BinWrite;
use shin_core::{
    format::{
        rom::RomReader,
        scenario::instruction_elements::{NumberSpec, Register},
    },
    primitives::stateless_reader::StatelessFile,
};
use std::io::Cursor;

#[derive(Default)]
struct Services {
    installed: Vec<u32>,
    portraits: usize,
    completed: Vec<u32>,
    voices: Vec<String>,
    advance: usize,
    fast: usize,
    fail_install: bool,
    install_attempts: usize,
    skip: bool,
}
impl MessageNotifications for Services {
    fn observed_voice(&mut self, name: &str) {
        self.voices.push(name.into());
    }
    fn completed(&mut self, id: u32) {
        self.completed.push(id);
    }
}
impl RunnerServices for Services {
    fn command_context(&mut self, _: &Cmd, _: bool) -> Result<MessageCommandContext> {
        Ok(MessageCommandContext {
            was_read: false,
            event_mode: -1,
            skip: self.skip,
            message_speed: 50,
        })
    }
    fn message_installed(&mut self, id: u32, _: PortraitChange) -> Result<()> {
        self.install_attempts += 1;
        if self.fail_install {
            self.fail_install = false;
            anyhow::bail!("controlled installation notification failure");
        }
        self.installed.push(id);
        Ok(())
    }
    fn portrait_changed(&mut self, _: PortraitChange) -> Result<()> {
        self.portraits += 1;
        Ok(())
    }
    fn advance_layers(&mut self, _: &Gerokasu2Layers, _: Ticks, _: bool) -> Result<()> {
        self.advance += 1;
        Ok(())
    }
    fn fast_forward_layers(&mut self, _: &Gerokasu2Layers) -> Result<()> {
        self.fast += 1;
        Ok(())
    }
}
fn data() -> Vec<u8> {
    let mut rom = RomReader::new(
        StatelessFile::new(
            std::fs::File::open(std::env::var_os("SHIN_GEROKASU2_ROM").unwrap()).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let file = rom.find_file("/main.snr").unwrap();
    let mut data = Vec::new();
    rom.open_file(file).unwrap().read_to_end(&mut data).unwrap();
    data
}
fn scenario() -> Arc<Gerokasu2Scenario> {
    Arc::new(Gerokasu2Scenario::new(data().into()).unwrap())
}
fn number(code: &mut Vec<u8>, value: i32) {
    let mut out = Cursor::new(Vec::new());
    NumberSpec::<i32>::constant(value)
        .write_le(&mut out)
        .unwrap();
    code.extend(out.into_inner());
}
fn text(code: &mut Vec<u8>, value: &str) {
    code.extend(((value.len() + 1) as u16).to_le_bytes());
    code.extend(value.as_bytes());
    code.push(0);
}
fn synthetic() -> Arc<Gerokasu2Scenario> {
    let mut bytes = data();
    let offset = Gerokasu2Scenario::new(bytes.clone().into())
        .unwrap()
        .header
        .code_offset as usize;
    bytes.truncate(offset);
    bytes.push(0x81);
    bytes.extend(2u16.to_le_bytes());
    number(&mut bytes, 0);
    bytes.push(0x85);
    number(&mut bytes, 0);
    bytes.extend([0x83, 2]);
    number(&mut bytes, -12);
    bytes.extend([0x86, 1, 0, 0, 1]);
    number(&mut bytes, 0);
    text(&mut bytes, "@rAあ 🐈");
    bytes.extend([0x8a, 1]);
    bytes.push(0xc6);
    number(&mut bytes, 1);
    number(&mut bytes, 2);
    bytes.push(0x9c);
    text(&mut bytes, "hanp00010");
    number(&mut bytes, 700);
    number(&mut bytes, 3);
    bytes.push(0x9e);
    number(&mut bytes, 3);
    bytes.extend([0, 0]);
    number(&mut bytes, 0);
    let size = bytes.len() as u32;
    bytes[4..8].copy_from_slice(&size.to_le_bytes());
    Arc::new(Gerokasu2Scenario::new(bytes.into()).unwrap())
}
fn assets(gpu: &Offscreen) -> Arc<AssetServer> {
    let path = std::path::PathBuf::from(std::env::var_os("SHIN_GEROKASU2_ROM").unwrap());
    Arc::new(AssetServer::new(
        locate_assets_for_game(Game::Gerokasu2, path.parent().unwrap())
            .unwrap()
            .into(),
        AssetLoadContext {
            game: crate::cli::Game::Gerokasu2,
            wgpu_device: gpu.device.clone(),
            wgpu_queue: gpu.queue.clone(),
            bustup_cache: crate::asset::system::cache::AssetCache::new(),
            higurashi_bustup_cache: Default::default(),
            picture_cache: Default::default(),
            font_cache: Default::default(),
        },
    ))
}
fn initial() -> InitialMessage {
    InitialMessage {
        style: 0,
        asynchronous: false,
        face: -1,
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
#[ignore = "requires supplied ROM, Metal and the default audio output"]
fn retained_runner_waits_feedback_retry_message_voice_and_offscreen_frames() {
    let gpu = Offscreen::new();
    let assets = assets(&gpu);
    let manager = Arc::new(AudioManager::new_for_tests());
    let mut runner = shin_tasks::block_on(Gerokasu2Runner::load(
        assets.clone(),
        synthetic(),
        manager,
        0,
        42,
        InitialMessage {
            face: 0,
            ..initial()
        },
        Default::default(),
    ))
    .unwrap();
    let mut services = Services::default();
    assert_eq!(
        runner.pump(false, 32, 10000, &mut services).unwrap(),
        RunnerStatus::External
    );
    let snapshot = (
        runner.vm.position(),
        runner.vm.next_position(),
        runner.vm.random_state(),
    );
    assert!(runner.complete_external(CommandResult::None).is_err());
    assert_eq!(
        snapshot,
        (
            runner.vm.position(),
            runner.vm.next_position(),
            runner.vm.random_state()
        )
    );
    runner
        .complete_external(CommandResult::WriteMemory(
            Register::from_regular_register(2),
            7,
        ))
        .unwrap();
    assert!(runner.complete_external(CommandResult::None).is_err());
    assert_eq!(
        runner.pump(false, 1, 10000, &mut services).unwrap(),
        RunnerStatus::CommandBudget
    );
    assert_eq!(
        runner.vm.read_register(Register::from_regular_register(2)),
        7
    );
    let mut frame = FrameId::default();
    frame.advance();
    assert_eq!(
        runner
            .update(
                frame,
                Ticks::from_f32(6.0),
                policy(),
                32,
                10000,
                &mut services
            )
            .unwrap(),
        RunnerStatus::Waiting
    );
    assert_eq!(runner.wait.remaining(), 6.0);
    assert!(!runner.interrupt_wait());
    let pc = runner.vm.next_position();
    let random = runner.vm.random_state();
    frame.advance();
    runner
        .update(
            frame,
            Ticks::from_f32(6.0),
            FramePolicy {
                paused: true,
                ..policy()
            },
            32,
            10000,
            &mut services,
        )
        .unwrap();
    assert_eq!(runner.wait.remaining(), 6.0);
    assert_eq!(runner.vm.next_position(), pc);
    assert_eq!(runner.vm.random_state(), random);
    frame.advance();
    runner
        .update(
            frame,
            Ticks::from_f32(6.0),
            policy(),
            32,
            10000,
            &mut services,
        )
        .unwrap();
    assert_eq!(runner.wait.remaining(), 0.0);
    services.fail_install = true;
    assert!(runner.pump(false, 32, 10000, &mut services).is_err());
    assert_eq!(runner.message().portrait().generation(), 1);
    assert!(matches!(
        runner.pending_command(),
        Some(Cmd::MessageSet { .. })
    ));
    frame.advance();
    runner
        .advance(frame, Ticks::from_f32(6.0), policy(), &mut services)
        .unwrap();
    let slide = runner.message().message().unwrap().clock().slide;
    assert_eq!(
        runner.pump(false, 32, 10000, &mut services).unwrap(),
        RunnerStatus::Waiting
    );
    assert_eq!(services.install_attempts, 2);
    assert_eq!(
        runner.message().portrait().generation(),
        1,
        "Notification retry must not resubmit MSGFACE"
    );
    assert_eq!(services.installed, vec![0]);
    assert_eq!(
        runner.message().message().unwrap().clock().slide,
        slide,
        "Notification retry does not reinstall text/window"
    );
    let parent =
        shin_render::shin_orthographic_projection_matrix(-960.0, 960.0, 540.0, -540.0, -1.0, 1.0);
    let local = Mat4::from_translation(glam::vec3(-960.0, -540.0, 0.0));
    let mut frames = 0;
    let mut channels = 0;
    let mut render = |runner: &Gerokasu2Runner| {
        let pixels = super::super::super::message_ui::reference_frame(
            &gpu.device,
            &gpu.queue,
            (640, 360),
            false,
            |pass| {
                runner
                    .render_message(pass, parent, local, 7, PassKind::Transparent)
                    .unwrap()
            },
        );
        frames += 1;
        channels += pixels.len();
        pixels
    };
    render(&runner);
    frame.advance();
    runner
        .advance(frame, Ticks::from_f32(6.0), policy(), &mut services)
        .unwrap();
    assert!(
        render(&runner)
            .chunks_exact(4)
            .any(|pixel| pixel != [96, 136, 184, 255])
    );
    frame.advance();
    runner
        .update(
            frame,
            Ticks::from_f32(1.0),
            FramePolicy {
                fast_skip: true,
                ..policy()
            },
            32,
            10000,
            &mut services,
        )
        .unwrap();
    assert_eq!(services.completed, vec![0]);
    assert_eq!(services.fast, 1);
    assert_eq!(
        runner.pump(false, 32, 10000, &mut services).unwrap(),
        RunnerStatus::Waiting
    );
    assert!(matches!(
        runner.pending_command(),
        Some(Cmd::Shared(RuntimeCommand::MSGCLOSE(_)))
    ));
    frame.advance();
    runner
        .advance(frame, Ticks::from_f32(10.0), policy(), &mut services)
        .unwrap();
    assert!(
        render(&runner)
            .chunks_exact(4)
            .all(|pixel| pixel == [96, 136, 184, 255])
    );
    assert_eq!(
        runner.pump(false, 32, 10000, &mut services).unwrap(),
        RunnerStatus::External
    );
    assert_eq!(runner.layers.selected_range(), [1, 2]);
    assert!(
        runner
            .complete_external(CommandResult::WriteMemory(
                Register::from_regular_register(2),
                0
            ))
            .is_err()
    );
    runner.complete_external(CommandResult::None).unwrap();
    assert_eq!(
        runner.pump(false, 32, 10000, &mut services).unwrap(),
        RunnerStatus::Waiting
    );
    assert!(matches!(
        runner.pending_command(),
        Some(Cmd::VoiceWait { .. })
    ));
    assert_eq!(services.voices, vec!["hanp00010"]);
    runner.voice.stop();
    assert_eq!(
        runner.pump(false, 32, 10000, &mut services).unwrap(),
        RunnerStatus::Exited
    );
    assert_eq!(
        runner.pump(false, 32, 10000, &mut services).unwrap(),
        RunnerStatus::Exited
    );
    assert_eq!(runner.completed_commands(), 9);
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    println!(
        "GEROKASU2_RUNNER_GPU synthetic_commands=9 frames={frames} channels={channels} audio=default output feedback_once_wait_pause_notification_retry_message_voice_external_layers_exit=verified"
    );
}

#[test]
#[ignore = "requires supplied ROM, Metal and the default audio output"]
fn supplied_runner_prefix_retains_external_owners_and_real_message_continuations() {
    let gpu = Offscreen::new();
    let assets = assets(&gpu);
    let scenario = scenario();
    let manager = Arc::new(AudioManager::new_for_tests());
    let mut runner = shin_tasks::block_on(Gerokasu2Runner::load(
        assets,
        scenario.clone(),
        manager,
        0,
        42,
        initial(),
        Default::default(),
    ))
    .unwrap();
    let mut baseline = Gerokasu2Scripter::new(scenario, 0, 42);
    let mut response = CommandResult::None;
    let mut services = Services {
        skip: true,
        ..Default::default()
    };
    let mut frame = FrameId::default();
    let mut authored = 0;
    let mut external = 0;
    let mut messages = 0;
    for _ in 0..500 {
        let before = runner.completed_commands();
        let status = runner.pump(false, 1, 100000, &mut services).unwrap();
        if runner.completed_commands() > before || runner.pending_command().is_some() {
            let expected = baseline.run(response, 100000).unwrap();
            response = CommandResult::None;
            assert_eq!(runner.vm.position(), baseline.position());
            assert_eq!(runner.vm.next_position(), baseline.next_position());
            assert_eq!(runner.vm.random_state(), baseline.random_state());
            authored += 1;
            messages += usize::from(matches!(expected, Cmd::MessageSet { .. }));
            match status {
                RunnerStatus::External => {
                    assert_eq!(
                        format!("{:?}", runner.pending_command().unwrap()),
                        format!("{expected:?}")
                    );
                    response = baseline
                        .pending_destination()
                        .map_or(CommandResult::None, |dest| {
                            CommandResult::WriteMemory(dest, 0)
                        });
                    let feedback = baseline
                        .pending_destination()
                        .map_or(CommandResult::None, |dest| {
                            CommandResult::WriteMemory(dest, 0)
                        });
                    runner.complete_external(feedback).unwrap();
                    external += 1;
                }
                RunnerStatus::Waiting => {
                    frame.advance();
                    runner
                        .advance(
                            frame,
                            Ticks::from_f32(60.0),
                            FramePolicy {
                                fast_skip: true,
                                ..policy()
                            },
                            &mut services,
                        )
                        .unwrap();
                    for _ in 0..100 {
                        if runner.pending_command().is_none() {
                            break;
                        }
                        let result = runner.pump(false, 1, 100000, &mut services).unwrap();
                        if result == RunnerStatus::Waiting {
                            frame.advance();
                            runner
                                .advance(
                                    frame,
                                    Ticks::from_f32(60.0),
                                    FramePolicy {
                                        fast_skip: true,
                                        ..policy()
                                    },
                                    &mut services,
                                )
                                .unwrap();
                        } else {
                            break;
                        }
                    }
                    assert!(
                        runner.pending_command().is_none(),
                        "Authored fixture continuation remains waiting"
                    );
                }
                RunnerStatus::Exited => break,
                RunnerStatus::CommandBudget => {}
            }
            if authored >= 100 {
                break;
            }
        }
    }
    assert!(authored >= 100);
    assert!(external > 0);
    assert!(messages > 0);
    println!(
        "GEROKASU2_RUNNER_PREFIX commands={authored} external={external} messages={messages} real_VM_PC_seed_message_voice_resources_explicit_fixture_feedback=verified"
    );
}
