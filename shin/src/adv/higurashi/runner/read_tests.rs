use super::super::{
    input::ReadState,
    read_state::{Checkpoint, ReadPolicy, ReadServices},
};
use super::*;
use std::cell::RefCell;

struct Services {
    checkpoints: Vec<Checkpoint>,
    queries: RefCell<Vec<u32>>,
    fail: bool,
    fail_at: Option<Checkpoint>,
}
impl ReadServices for Services {
    fn checkpoint(&mut self, kind: Checkpoint, _: ReadState) -> Result<()> {
        if self.fail || self.fail_at == Some(kind) {
            anyhow::bail!("Snapshot storage unavailable");
        }
        self.checkpoints.push(kind);
        Ok(())
    }
    fn message_is_read(&self, id: u32) -> bool {
        self.queries.borrow_mut().push(id);
        id == 0
    }
    fn message_completed(&mut self, _: u32) -> Result<()> {
        Ok(()) // This older fixture isolates command-current fields, not history.
    }
}
#[derive(Default)]
struct Voice {
    plays: Vec<String>,
}
impl MessageVoice for Voice {
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
impl VoiceCommands for Voice {
    fn play_command(&mut self, name: &str, _: f32, _: i32) {
        self.plays.push(name.to_owned());
    }
    fn status_bits(&self) -> i32 {
        0
    }
}
fn message(code: &mut Vec<u8>, id: u32) {
    code.push(0x86);
    code.extend_from_slice(&id.to_le_bytes());
    code.extend([0, 2, 0, b'A', 0]);
}
fn voice(code: &mut Vec<u8>, name: &str) {
    code.extend([0x9c, (name.len() + 1) as u8]);
    code.extend_from_slice(name.as_bytes());
    code.extend([0, 0x83, 0xe8, 0]);
}

#[test]
#[ignore = "real retained runner/message resources on offscreen Metal; voice transport is a checked service"]
fn higurashi_runner_native_read_state_resolves_each_callback_and_retains_failed_checkpoint() {
    let (_, _, assets) = super::super::message_glyphs::tests::gpu::gpu_assets();
    let assets = Arc::new(assets);
    let authored = shin_tasks::block_on(assets.load::<HigurashiScenario, _>("/main.snr")).unwrap();
    let mut code = Vec::new();
    message(&mut code, 1);
    voice(&mut code, "read-suppressed");
    code.extend([0xa2, 0]);
    message(&mut code, 2);
    voice(&mut code, "outside-event");
    code.push(0xa3);
    voice(&mut code, "unread-after-end");
    message(&mut code, 1);
    voice(&mut code, "read-after-end");
    code.extend([0, 0, 0]);
    let mut raw = authored.raw().to_vec();
    let start = authored.header.code_offset as usize;
    raw[start..start + code.len()].copy_from_slice(&code);
    let scenario = Arc::new(HigurashiScenario::new(raw.into()).unwrap());
    let mut runner = shin_tasks::block_on(HigurashiRunner::load(assets, scenario, 0, 42, || {
        [1, 2, 3, 4]
    }))
    .unwrap();
    let mut services = Services {
        checkpoints: Vec::new(),
        queries: RefCell::default(),
        fail: false,
        fail_at: Some(Checkpoint::EventEnd),
    };
    let mut voice = Voice::default();
    let policy = super::tests::policy();
    let native = ReadPolicy {
        modes: 2,
        forced: false,
        read_only: true,
    };
    let pump = |r: &mut HigurashiRunner, s: &mut Services, v: &mut Voice, n: ReadPolicy| {
        max_pump(r, s, v, n, policy)
    };
    let error = pump(&mut runner, &mut services, &mut voice, native).unwrap_err();
    assert!(error.to_string().contains("Snapshot storage unavailable"));
    assert!(matches!(runner.pending_command(), Some(Cmd::EVEND {})));
    assert_eq!(voice.plays, vec!["outside-event"]);
    assert_eq!(runner.native_read_state().unwrap().flags, 0);
    assert_eq!(runner.frame.message.playback.flags, 2);
    assert_eq!(runner.frame.message.playback.message_id, 1);
    assert_eq!(runner.completed_commands(), 5);
    assert_eq!(
        services.checkpoints,
        vec![Checkpoint::Message, Checkpoint::EventBegin]
    );
    let retained = (
        runner.position(),
        runner.next_position(),
        runner.random_state(),
        runner.completed_commands(),
    );
    assert!(pump(&mut runner, &mut services, &mut voice, native).is_err());
    assert_eq!(
        retained,
        (
            runner.position(),
            runner.next_position(),
            runner.random_state(),
            runner.completed_commands()
        )
    );
    services.fail_at = None;
    assert_eq!(
        pump(&mut runner, &mut services, &mut voice, native).unwrap(),
        Status::Exited
    );
    assert_eq!(voice.plays, vec!["outside-event", "unread-after-end"]);
    assert_eq!(*services.queries.borrow(), vec![0, 1, 0]);
    assert_eq!(
        services.checkpoints,
        vec![
            Checkpoint::Message,
            Checkpoint::EventBegin,
            Checkpoint::EventEnd,
            Checkpoint::Message
        ]
    );
    assert_eq!(runner.completed_commands(), 10);
    assert_eq!(runner.native_read_state().unwrap().fast_forward_blocked, 1);
    assert!(
        shin_tasks::block_on(runner.pump(policy, 8, 1000, || [1, 2, 3, 4], &mut voice)).is_err()
    );
    println!(
        "Higurashi native-read runner:10 commands resolve changing read/suppression within each pump, native message IDs/flags, retained external events and checkpoint failure/retry without PC/PRNG/snapshot duplication; explicit-policy bypass rejected"
    );
}
fn max_pump(
    r: &mut HigurashiRunner,
    s: &mut Services,
    v: &mut Voice,
    n: ReadPolicy,
    p: Policy,
) -> Result<Status> {
    shin_tasks::block_on(r.pump_native(p, n, s, 32, 10000, || [1, 2, 3, 4], v))
}

#[test]
#[ignore = "native-policy host frame/input with offscreen Metal and the default audio output"]
fn higurashi_host_native_read_frame_and_input_use_owned_vm_state() {
    use super::super::{
        audio::{AudioFramePolicy, HigurashiAudio},
        host::HigurashiHost,
        input::{Buttons, Context, InputState},
    };
    let (_, _, assets) = super::super::message_glyphs::tests::gpu::gpu_assets();
    let assets = Arc::new(assets);
    let authored = shin_tasks::block_on(assets.load::<HigurashiScenario, _>("/main.snr")).unwrap();
    let mut code = Vec::new();
    message(&mut code, 1);
    code.extend([0x83, 1, 12, 0xa3, 0, 0, 0]);
    let mut raw = authored.raw().to_vec();
    let start = authored.header.code_offset as usize;
    raw[start..start + code.len()].copy_from_slice(&code);
    let scenario = Arc::new(HigurashiScenario::new(raw.into()).unwrap());
    let runner = shin_tasks::block_on(HigurashiRunner::load(
        assets.clone(),
        scenario.clone(),
        0,
        42,
        || [1, 2, 3, 4],
    ))
    .unwrap();
    let mut audio = HigurashiAudio::new(
        Arc::new(shin_audio::AudioManager::new_for_tests()),
        assets,
        scenario,
    );
    audio.set_paused(true);
    let mut host = HigurashiHost::new(runner, audio);
    let policy = super::tests::policy();
    let native = ReadPolicy {
        modes: 0,
        forced: false,
        read_only: true,
    };
    let ap = AudioFramePolicy {
        focus_mode_active: true,
        voice_focus_enabled: true,
        effects_update_enabled: true,
        music_rate: 1.0,
    };
    let mut services = Services {
        checkpoints: Vec::new(),
        queries: RefCell::default(),
        fail: false,
        fail_at: None,
    };
    let update = |host: &mut HigurashiHost, s: &mut Services| {
        shin_tasks::block_on(host.update_native(
            Ticks::from_f32(120.0),
            policy,
            native,
            s,
            ap,
            32,
            10000,
            || [1, 2, 3, 4],
        ))
        .unwrap()
    };
    assert_eq!(update(&mut host, &mut services), Status::Waiting);
    assert_eq!(host.runner.completed_commands(), 1);
    assert_eq!(host.runner.frame.message.playback.message_id, 0);
    assert_eq!(host.runner.frame.message.playback.flags, 1);
    let remaining = |h: &HigurashiHost| match h.runner.pending.as_ref().map(|p| &p.owner) {
        Some(Owner::Clock(w)) => w.remaining,
        _ => panic!("Expected WAIT"),
    };
    assert_eq!(remaining(&host), 9.0);
    let context = Context {
        permission: true,
        captured: false,
        disabled: false,
        system_locked: false,
        vm: Some(ReadState {
            flags: 0,
            fast_forward_blocked: 1,
            is_read: 0,
        }),
        paused: false,
        continuation_blocked: false,
        overlay_transition_busy: false,
        overlay_transition_value: 0.0,
        notification_busy: false,
        animations_enabled: true,
        overlay: None,
        hint_busy: false,
        scene_jump_available: false,
        wait_allow_interrupt: 2,
    };
    let mut state = InputState {
        modes: 0,
        temporary_indicator: 9,
    };
    let buttons = Buttons {
        pressed: 1 << 6,
        ..Default::default()
    };
    host.runner.frame.message.playback.natural_slide = 1.0;
    host.runner.frame.message.playback.wait = Some(super::super::message_playback::WaitKind::Last);
    // Actual glyph reveal, final-message click, then WAIT interruption. The
    // deliberately conflicting VM flags in Context must not bypass read mode.
    for click in 0..3 {
        let outcome = host.process_input(&mut state, context, buttons);
        assert!(!outcome.overlay_input_enabled && outcome.requests.is_empty());
        assert_eq!(remaining(&host), if click < 2 { 9.0 } else { 0.0 });
    }
    assert_eq!(
        host.runner.frame.message.playback.completed_messages,
        vec![0]
    );
    assert_eq!(update(&mut host, &mut services), Status::Exited);
    assert_eq!(host.runner.completed_commands(), 4);
    assert_eq!(
        services.checkpoints,
        vec![Checkpoint::Message, Checkpoint::EventEnd]
    );
    println!(
        "Higurashi native-read host:4 real VM/message/WAIT/event commands, three-tick clamp, owned read flags override conflicting input context, glyph/message-first confirm then exact WAIT interrupt, native zero-based completion ID; audio/offscreen Metal pass"
    );
}
