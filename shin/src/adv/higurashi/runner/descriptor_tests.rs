use super::super::{input::ReadState, music_commands::MusicCommands, read_state::Checkpoint};
use super::*;
use shin_core::format::scenario::instruction_elements::Register;

struct Services {
    checkpoints: Vec<Checkpoint>,
    fail: bool,
}
impl ReadServices for Services {
    fn checkpoint(&mut self, kind: Checkpoint, _: ReadState) -> Result<()> {
        ensure!(!self.fail, "snapshot unavailable");
        self.checkpoints.push(kind);
        Ok(())
    }
    fn message_is_read(&self, _: u32) -> bool {
        true
    }
    fn message_completed(&mut self, _: u32) -> Result<()> {
        Ok(())
    }
}
#[derive(Default)]
struct Audio {
    requests: Vec<(i32, u32, f32)>,
    fail_volume: bool,
    volume_calls: u32,
}
impl MessageVoice for Audio {
    fn is_playing(&self) -> bool {
        false
    }
    fn position_micros(&self) -> u64 {
        0
    }
    fn play(&mut self, _: &str, _: f32, _: bool, _: u32, _: u32) -> bool {
        false
    }
    fn stop(&mut self) {}
}
impl VoiceCommands for Audio {
    fn play_command(&mut self, _: &str, _: f32, _: i32) {
        unreachable!()
    }
    fn status_bits(&self) -> i32 {
        0
    }
    fn music(&mut self) -> Option<&mut dyn MusicCommands> {
        Some(self)
    }
}
impl MusicCommands for Audio {
    fn play_bgm(&mut self, id: i32, _: i32, count: u32, volume: f32) -> Result<()> {
        self.requests.push((id, count, volume));
        Ok(())
    }
    fn stop_bgm(&mut self, _: i32) {}
    fn volume_bgm(&mut self, _: f32, _: i32) -> Result<()> {
        self.volume_calls += 1;
        ensure!(!self.fail_volume, "volume unavailable");
        Ok(())
    }
    fn bgm_status(&self) -> i32 {
        0
    }
    fn bgm_clock(&self) -> u64 {
        0
    }
}
fn choice(code: &mut Vec<u8>) {
    code.extend([0x8d, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0]);
}
fn pump(r: &mut HigurashiRunner, s: &mut Services, a: &mut Audio, fast: bool) -> Result<Status> {
    let mut policy = super::tests::policy();
    policy.fast_skip = fast;
    shin_tasks::block_on(r.pump_native(
        policy,
        ReadPolicy {
            modes: 2,
            forced: false,
            read_only: true,
        },
        s,
        32,
        10000,
        || [1, 2, 3, 4],
        a,
    ))
}
fn cursor(r: &HigurashiRunner) -> (CodeAddress, CodeAddress, u32, u64) {
    (
        r.position(),
        r.next_position(),
        r.random_state(),
        r.completed_commands(),
    )
}
fn acknowledge_choice(r: &mut HigurashiRunner) {
    r.complete_external(CommandResult::WriteMemory(
        Register::from_regular_register(0),
        0,
    ))
    .unwrap();
}

#[test]
#[ignore = "offscreen Metal runner with real message resources; controlled audio and explicit choice feedback, no OS audio"]
fn higurashi_runner_retains_playback_descriptors_and_conditional_choice_constructor() {
    let (_, _, assets) = super::super::message_glyphs::tests::gpu::gpu_assets();
    let assets = Arc::new(assets);
    let authored = shin_tasks::block_on(assets.load::<HigurashiScenario, _>("/main.snr")).unwrap();
    let mut code = Vec::new();
    choice(&mut code);
    code.extend([0x90, 7, 0, 0, 0x83, 0xe8, 0x92, 0x81, 0xf4, 0]); // repeat BGM 1000, volume 500
    code.extend([0x95, 31, 9, 0, 1, 0x83, 0xe8, 0, 0x83, 0xe8]); // finite SE retains id -1
    code.push(0x86);
    code.extend(0u32.to_le_bytes());
    code.extend([0, 2, 0, b'A', 0]);
    choice(&mut code);
    code.extend([0x87, 0x7f, 0x8a, 0]);
    choice(&mut code);
    code.extend([0x83, 1, 4, 0, 0, 0]); // WAIT, EXIT
    let mut raw = authored.raw().to_vec();
    let start = authored.header.code_offset as usize;
    raw[start..start + code.len()].copy_from_slice(&code);
    let scenario = Arc::new(HigurashiScenario::new(raw.into()).unwrap());
    let mut r = shin_tasks::block_on(HigurashiRunner::load(assets, scenario, 0, 42, || {
        [1, 2, 3, 4]
    }))
    .unwrap();
    let mut s = Services {
        checkpoints: Vec::new(),
        fail: true,
    };
    let mut a = Audio {
        fail_volume: true,
        ..Default::default()
    };
    assert!(pump(&mut r, &mut s, &mut a, false).is_err());
    let retained = cursor(&r);
    assert_eq!(r.native_message_timeline().unwrap().boundary, 0);
    assert!(pump(&mut r, &mut s, &mut a, false).is_err());
    assert_eq!(cursor(&r), retained);
    s.fail = false;
    assert_eq!(
        pump(&mut r, &mut s, &mut a, false).unwrap(),
        Status::External
    );
    assert_eq!(s.checkpoints, vec![Checkpoint::Interactive]);
    assert_eq!(r.native_message_timeline().unwrap().resume_position, 1);
    s.fail = true;
    assert_eq!(
        pump(&mut r, &mut s, &mut a, false).unwrap(),
        Status::External
    );
    assert_eq!(s.checkpoints.len(), 1);
    acknowledge_choice(&mut r);
    s.fail = false;
    assert!(pump(&mut r, &mut s, &mut a, false).is_err());
    let retained = cursor(&r);
    assert_eq!(a.requests, vec![(7, 0, 1.0)]);
    let frozen = r.native_playback_descriptors().unwrap().clone();
    assert_eq!(frozen.music.id, 7);
    assert_eq!(frozen.music.volume, 0.5);
    assert!(pump(&mut r, &mut s, &mut a, false).is_err());
    assert_eq!(cursor(&r), retained);
    assert_eq!(r.native_playback_descriptors().unwrap(), &frozen);
    assert_eq!(a.requests.len(), 1);
    a.fail_volume = false;
    assert_eq!(
        pump(&mut r, &mut s, &mut a, false).unwrap(),
        Status::External
    );
    assert_eq!(a.volume_calls, 3);
    assert!(matches!(r.pending_command(), Some(Cmd::SEPLAY { .. })));
    let effect = r.native_playback_descriptors().unwrap().effects[31];
    assert_eq!(
        (effect.id, effect.volume, effect.pan, effect.speed),
        (-1, 1.0, 0.0, 1.0)
    );
    r.complete_external(CommandResult::None).unwrap();
    assert_eq!(
        pump(&mut r, &mut s, &mut a, true).unwrap(),
        Status::External
    );
    assert!(matches!(r.pending_command(), Some(Cmd::SELECT { .. })));
    assert_eq!(
        r.native_playback_descriptors()
            .unwrap()
            .message
            .text
            .as_deref(),
        Some("A")
    );
    assert_eq!(
        s.checkpoints,
        vec![Checkpoint::Interactive, Checkpoint::Message]
    );
    assert_eq!(r.native_message_timeline().unwrap().boundary, 2);
    acknowledge_choice(&mut r);
    assert_eq!(pump(&mut r, &mut s, &mut a, true).unwrap(), Status::Waiting);
    assert!(matches!(
        r.pending_command(),
        Some(Cmd::MSGWAIT { signal: -1 })
    ));
    assert_eq!(
        r.native_playback_descriptors().unwrap().message.wait_flag,
        0
    );
    assert_eq!(
        r.native_playback_descriptors()
            .unwrap()
            .message
            .text
            .as_deref(),
        Some("A")
    );
    assert_eq!(s.checkpoints.len(), 2);
    r.fast_forward_visuals(&mut a);
    assert_eq!(
        pump(&mut r, &mut s, &mut a, true).unwrap(),
        Status::External
    );
    assert_eq!(r.native_playback_descriptors().unwrap().message.text, None);
    assert_eq!(
        s.checkpoints,
        vec![
            Checkpoint::Interactive,
            Checkpoint::Message,
            Checkpoint::Interactive
        ]
    );
    assert_eq!(r.native_message_timeline().unwrap().resume_position, 3);
    acknowledge_choice(&mut r);
    assert_eq!(
        pump(&mut r, &mut s, &mut a, false).unwrap(),
        Status::Waiting
    );
    assert!(r.interrupt_wait());
    assert_eq!(pump(&mut r, &mut s, &mut a, false).unwrap(), Status::Exited);
    assert_eq!(r.completed_commands(), 11);
    assert_eq!(frozen.message.text, None);
    println!(
        "Higurashi descriptor runner: 11 commands; conditional choice captures, checkpoint retry, retained audio service error, pre-callback raw targets, finite SE, message pointer lifetime, WAIT continuation; controlled audio and choice feedback"
    );
}
