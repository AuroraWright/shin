use super::*;
mod skip_wait;
use crate::adv::higurashi::{
    autosave::{ProgressEntry, SaveProgress},
    checkpoints::CheckpointMemory,
    progress::ProgressMemory,
    read_history::MessageReadHistory,
    read_state::{Checkpoint, ReadServices},
    runner::Status,
};
use shin_core::format::save::PersistData;
use std::{
    io::Write,
    process::{Command, Stdio},
};
#[test]
#[ignore = "original c3cc8 finite/infinite SE skip state decisions, volume/pan queue endpoints and four-tick fade"]
fn higurashi_effect_skip_operations_match_original_state_machine() {
    use crate::adv::higurashi::effects::{SkipOperation, skip_operation};
    let mut input = String::new();
    let mut expected = Vec::new();
    for count in [0, 1, 2, i32::MAX as u32, 0x80000000, u32::MAX] {
        for state in [0, 1, 2, 3] {
            input.push_str(&format!("{count} {state}\n"));
            expected.push(match skip_operation(count, state) {
                SkipOperation::DropLoad => vec![0, 1, 1, 0, 0, 0],
                SkipOperation::Stop => vec![1, 0, 0, 1, 1, 4.0f32.to_bits()],
                SkipOperation::FinishParameters => vec![1, 0, 0, 2, 0, 0],
                SkipOperation::Keep => vec![1, 0, 0, 0, 0, 0],
            });
        }
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_SKIP_EFFECTS").unwrap())
        .arg("game-data/higurashi/uncompressed_executable_update")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    let actual = text
        .lines()
        .map(|l| {
            l.strip_prefix("T ")
                .unwrap()
                .split_whitespace()
                .map(|v| v.parse::<u32>().unwrap())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);
    eprintln!(
        "Native SE skip:24 original c3cc8 cases/144 exact values,finite load retirement/playing stop/retained tails,zero/signed-negative loops,volume/pan endpoints and4-tick fade pass"
    );
}
struct Reads {
    memory: CheckpointMemory,
    progress: ProgressMemory,
    history: MessageReadHistory,
}
impl ReadServices for Reads {
    fn checkpoint_owner(&mut self) -> Option<(&mut CheckpointMemory, &mut ProgressMemory)> {
        Some((&mut self.memory, &mut self.progress))
    }
    fn checkpoint(&mut self, _: Checkpoint, _: input::ReadState) -> Result<()> {
        unreachable!()
    }
    fn message_is_read(&self, id: u32) -> bool {
        self.history.contains(id)
    }
    fn message_completed(&mut self, id: u32) -> Result<()> {
        self.history.record_completion(true, id);
        Ok(())
    }
}
#[test]
#[ignore = "real native host/controller/message before WAIT order, post-poll auto/read skip policy and protected UI clocks;offscreen Metal/exact Loopback Audio"]
fn higurashi_gameplay_controls_drive_owned_messages_and_post_poll_skip() {
    use crate::adv::higurashi::{audio::HigurashiAudio, reference_gpu_assets};
    let (_, _, assets) = reference_gpu_assets();
    let assets = Arc::new(assets);
    let authored = assets
        .load_sync::<shin_core::format::scenario::higurashi::HigurashiScenario>("/main.snr")
        .unwrap();
    let mut code = vec![0x86, 1, 0, 0, 1, 0x7f, 83, 0, b'@', b'r'];
    code.extend(std::iter::repeat_n(b'A', 80));
    code.push(0);
    code.extend([
        0x83, 1, 30, 0x86, 1, 0, 0, 1, 0x7f, 4, 0, b'@', b'r', b'B', 0, 0x83, 1, 12, 0, 0, 0,
    ]);
    let mut raw = authored.raw().to_vec();
    let start = authored.header.code_offset as usize;
    raw[start..start + code.len()].copy_from_slice(&code);
    let script = Arc::new(
        shin_core::format::scenario::higurashi::HigurashiScenario::new(raw.into()).unwrap(),
    );
    let runner = shin_tasks::block_on(HigurashiRunner::load(
        assets.clone(),
        script.clone(),
        0,
        42,
        || [1, 2, 3, 4],
    ))
    .unwrap();
    let mut host = HigurashiHost::new(
        runner,
        HigurashiAudio::new(
            Arc::new(shin_audio::AudioManager::new_for_tests()),
            assets,
            script,
        ),
    );
    let entry = ProgressEntry {
        scenario_id: 0,
        random_seed: 42,
        save_position: 0,
        replay_target: 0,
        response_data: Vec::new(),
    };
    let mut reads = Reads {
        memory: CheckpointMemory::new(Vec::new()),
        progress: ProgressMemory::new(
            PersistData::new(),
            SaveProgress::new(0, 0, [entry.clone(), entry]).unwrap(),
            Vec::new(),
        ),
        history: MessageReadHistory::new(),
    };
    let mut controls = Controls::default();
    let policy = super::super::tests::policy();
    let ap = super::super::tests::audio();
    let update = |host: &mut HigurashiHost,
                  controls: &mut Controls,
                  reads: &mut Reads,
                  buttons: Buttons,
                  read_only: bool| {
        let cues = controls
            .process(host, reads, true, buttons, policy, false)
            .unwrap();
        let rp = controls.read_policy(read_only);
        let status = shin_tasks::block_on(host.update_native_with_frame_services(
            Ticks::from_f32(3.0),
            policy,
            rp,
            reads,
            ap,
            32,
            10000,
            || [1, 2, 3, 4],
            |_, _, _| Ok(false),
            |runner, audio, ui, _| post_poll(runner, audio, ui, rp),
        ))
        .unwrap();
        (status, cues)
    };
    assert_eq!(
        update(
            &mut host,
            &mut controls,
            &mut reads,
            Buttons::default(),
            true
        )
        .0,
        Status::Waiting
    );
    controls.boundary.captured = true;
    assert_eq!(
        update(
            &mut host,
            &mut controls,
            &mut reads,
            Buttons {
                pressed: 1 << 10,
                ..Default::default()
            },
            true
        )
        .1,
        Vec::<Request>::new()
    );
    assert_eq!(controls.state.modes, 0);
    controls.boundary.captured = false;
    for _ in 0..5 {
        update(
            &mut host,
            &mut controls,
            &mut reads,
            Buttons::default(),
            true,
        );
    }
    // First A reveals;second A publishes completion before decoding the same
    // encoded message ID again. The native zero-based read bit is then present.
    for _ in 0..2 {
        update(
            &mut host,
            &mut controls,
            &mut reads,
            Buttons {
                pressed: 1 << 24,
                ..Default::default()
            },
            true,
        );
    }
    assert!(reads.history.contains(0));
    assert_eq!(host.runner.completed_commands(), 1); // Message complete,WAIT installed.
    let (status, cues) = update(
        &mut host,
        &mut controls,
        &mut reads,
        Buttons {
            pressed: 1 << 11,
            ..Default::default()
        },
        true,
    );
    assert_eq!(status, Status::Waiting);
    assert_eq!(cues, [Request::CueEnabled]);
    assert_eq!(controls.state.modes, 2);
    assert!(!input::suppress_transient_audio(
        host.runner.native_read_state(),
        controls.state.modes,
        false,
        true
    ));
    update(
        &mut host,
        &mut controls,
        &mut reads,
        Buttons::default(),
        false,
    );
    // Post-poll skip zeros WAIT after its failed poll. Next frame retires it,
    // constructs the visited message with its read flag and completes it later.
    assert_eq!(host.runner.completed_commands(), 1);
    update(
        &mut host,
        &mut controls,
        &mut reads,
        Buttons::default(),
        true,
    );
    assert!(host.runner.completed_commands() > 1);
    assert_eq!(host.runner.frame().message.playback.flags & 1, 1);
    let (_, cues) = update(
        &mut host,
        &mut controls,
        &mut reads,
        Buttons {
            pressed: 1 << 25,
            ..Default::default()
        },
        true,
    );
    assert_eq!(cues, [Request::CueDisabled]);
    assert_eq!(controls.state.modes, 0);
    eprintln!(
        "Native gameplay controls:captured input/toggle cues,message-first A,zero-based completion before revisited construction,owned modes/read-only policy and post-poll WAIT skip ordering pass"
    );
}

#[test]
fn temporary_skip_preserves_native_read_gates_and_latched_modes() {
    let mut controls = Controls::default();
    for latched in 0..=3 {
        controls.state.modes = latched;
        controls.desktop_held_skip = true;
        controls.allow_temporary_skip(true);
        let policy = controls.read_policy(true);
        assert_eq!(policy.modes, latched | 2);
        assert!(!policy.forced);
        for read in [0, 1] {
            for protected in [false, true] {
                for blocked in [0, 1] {
                    let result = input::message_requests(
                        Some(input::ReadState {
                            flags: if protected { 0 } else { 0x8000_0000 },
                            fast_forward_blocked: blocked,
                            is_read: read,
                        }),
                        policy.modes,
                        policy.forced,
                        policy.read_only,
                    );
                    assert_eq!(result.fast_forward, read != 0 && !protected && blocked == 0);
                }
            }
        }
        assert_eq!(controls.state.modes, latched);
        controls.desktop_held_skip = false;
        controls.allow_temporary_skip(true);
        assert_eq!(controls.read_policy(false).modes, latched);
    }
    controls.state.modes = 0;
    controls.desktop_held_skip = true;
    for gate in 0..4 {
        controls.boundary.captured = gate == 1;
        controls.boundary.disabled = gate == 2;
        if gate == 3 {
            controls.requests.push_back(Request::SystemMenu);
        }
        controls.allow_temporary_skip(gate != 0);
        assert_eq!(controls.read_policy(false).modes, 0);
    }
}
