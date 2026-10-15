use super::super::{
    autosave::{self, AutosaveMemory, AutosaveServices, ProgressEntry, SaveProgress},
    read_state::Checkpoint,
};
use super::*;

struct ReadOwner {
    captures: usize,
    fail: bool,
}
impl ReadServices for ReadOwner {
    fn checkpoint(&mut self, kind: Checkpoint, _: super::super::input::ReadState) -> Result<()> {
        ensure!(!self.fail, "checkpoint unavailable");
        assert_eq!(kind, Checkpoint::Autosave);
        self.captures += 1;
        Ok(())
    }
    fn message_is_read(&self, _: u32) -> bool {
        false
    }
    fn message_completed(&mut self, _: u32) -> Result<()> {
        Ok(())
    }
}
struct Platform {
    clocks: usize,
    notices: Vec<String>,
    fail_clock: bool,
    fail_notice: bool,
}
impl AutosaveServices for Platform {
    fn packed_time(&mut self) -> Result<u64> {
        ensure!(!self.fail_clock, "clock unavailable");
        self.clocks += 1;
        Ok(42)
    }
    fn notify(&mut self, text: &str) -> Result<()> {
        ensure!(!self.fail_notice, "notification unavailable");
        self.notices.push(text.into());
        Ok(())
    }
}
fn info(code: &mut Vec<u8>, level: u8, text: &str) {
    code.extend([0xa0, level, (text.len() + 1) as u8]);
    code.extend(text.bytes());
    code.push(0);
}
fn pump(
    runner: &mut HigurashiRunner,
    reads: &mut ReadOwner,
    voice: &mut super::tests::Voice,
) -> Result<Status> {
    shin_tasks::block_on(runner.pump_native(
        super::tests::policy(),
        ReadPolicy {
            modes: 0,
            forced: false,
            read_only: true,
        },
        reads,
        32,
        10000,
        || [1, 2, 3, 4],
        voice,
    ))
}
fn cursor(runner: &HigurashiRunner) -> (CodeAddress, CodeAddress, u32, u64) {
    (
        runner.position(),
        runner.next_position(),
        runner.random_state(),
        runner.completed_commands(),
    )
}
#[test]
#[ignore = "actual native runner AUTOSAVE memory owner, replay target retention, failure recovery and single feedback on offscreen Metal"]
fn higurashi_runner_autosave_owner_retains_replay_target_and_retries_only_unacknowledged_notification()
 {
    let (_, _, assets) = super::super::message_glyphs::tests::gpu::gpu_assets();
    let assets = Arc::new(assets);
    let authored = shin_tasks::block_on(assets.load::<HigurashiScenario, _>("/main.snr")).unwrap();
    let mut code = Vec::new();
    info(&mut code, 0, "Title");
    info(&mut code, 1, "Chapter A");
    code.push(0xa1);
    info(&mut code, 1, "Chapter B");
    code.extend([0xa1, 0x83, 1, 0x83, 0xe8, 0xa1, 0, 0, 0]);
    let mut raw = authored.raw().to_vec();
    let start = authored.header.code_offset as usize;
    raw[start..start + code.len()].copy_from_slice(&code);
    let scenario = Arc::new(HigurashiScenario::new(raw.into()).unwrap());
    let mut runner = shin_tasks::block_on(HigurashiRunner::load(assets, scenario, 0, 42, || {
        [1, 2, 3, 4]
    }))
    .unwrap();
    let mut reads = ReadOwner {
        captures: 0,
        fail: true,
    };
    let mut voice = super::tests::Voice::default();
    let mut progress = SaveProgress::new(
        1,
        123,
        std::array::from_fn(|i| ProgressEntry {
            scenario_id: i as i32,
            random_seed: 42 + i as u32,
            save_position: 0,
            replay_target: 99,
            response_data: vec![i as u8; 17 + i],
        }),
    )
    .unwrap();
    let original = progress.clone();
    let mut memory = AutosaveMemory {
        valid: false,
        packed_time: 0,
        progress: progress.clone(),
    };
    let mut platform = Platform {
        clocks: 0,
        notices: Vec::new(),
        fail_clock: true,
        fail_notice: false,
    };
    assert!(pump(&mut runner, &mut reads, &mut voice).is_err());
    assert_eq!(runner.autosave_flags(), None);
    let before = cursor(&runner);
    assert!(
        runner
            .poll_autosave_command(
                &mut progress,
                &mut memory,
                autosave::Execution::Live,
                &mut platform
            )
            .is_err()
    );
    assert_eq!(progress, original);
    assert_eq!(platform.clocks, 0);
    reads.fail = false;
    assert_eq!(
        pump(&mut runner, &mut reads, &mut voice).unwrap(),
        Status::External
    );
    assert_eq!(runner.autosave_flags(), Some([1, 0]));
    assert_eq!(
        runner
            .poll_autosave_command(
                &mut progress,
                &mut memory,
                autosave::Execution::Replay,
                &mut platform
            )
            .unwrap(),
        Some(false)
    );
    assert_eq!(runner.autosave_flags(), Some([0, 1]));
    assert_eq!(cursor(&runner), before);
    assert!(!memory.valid);
    assert!(
        runner
            .poll_autosave_command(
                &mut progress,
                &mut memory,
                autosave::Execution::Live,
                &mut platform
            )
            .is_err()
    );
    assert_eq!(progress.active_entry().save_position, 1);
    assert_eq!(progress.active_entry().replay_target, 1);
    assert_eq!(memory.progress, original);
    assert!(memory.valid);
    platform.fail_clock = false;
    platform.fail_notice = true;
    assert!(
        runner
            .poll_autosave_command(
                &mut progress,
                &mut memory,
                autosave::Execution::Live,
                &mut platform
            )
            .is_err()
    );
    assert_eq!(cursor(&runner), before);
    assert_eq!(platform.clocks, 1);
    assert_eq!(memory.progress.active_entry().save_position, 1);
    assert_eq!(memory.progress.active_entry().replay_target, 1);
    let first = memory.clone();
    progress.entries[1].response_data.push(0xff);
    reads.fail = true;
    assert_eq!(
        pump(&mut runner, &mut reads, &mut voice).unwrap(),
        Status::External
    );
    assert_eq!(reads.captures, 1);
    platform.fail_notice = false;
    assert_eq!(
        runner
            .poll_autosave_command(
                &mut progress,
                &mut memory,
                autosave::Execution::Live,
                &mut platform
            )
            .unwrap(),
        Some(true)
    );
    assert_eq!(platform.clocks, 1);
    assert_eq!(platform.notices, ["Title Chapter A"]);
    assert_eq!(memory, first);
    assert_eq!(runner.completed_commands(), 3);
    assert_eq!(runner.autosave_flags(), None);
    reads.fail = false;
    assert_eq!(
        pump(&mut runner, &mut reads, &mut voice).unwrap(),
        Status::External
    );
    assert_eq!(runner.native_message_timeline().unwrap().boundary, 2);
    // An autosave before the active saved boundary completes replay immediately,
    // without a clock/settings copy/notification or overwriting either entry.
    progress.entries[1].save_position = 3;
    let prior = progress.clone();
    assert_eq!(
        runner
            .poll_autosave_command(
                &mut progress,
                &mut memory,
                autosave::Execution::Replay,
                &mut platform
            )
            .unwrap(),
        Some(true)
    );
    assert_eq!(progress, prior);
    assert_eq!(memory, first);
    assert_eq!(platform.clocks, 1);
    assert_eq!(
        pump(&mut runner, &mut reads, &mut voice).unwrap(),
        Status::Waiting
    );
    assert!(
        runner
            .poll_autosave_command(
                &mut progress,
                &mut memory,
                autosave::Execution::Live,
                &mut platform
            )
            .is_err()
    );
    assert!(runner.interrupt_wait());
    assert_eq!(
        pump(&mut runner, &mut reads, &mut voice).unwrap(),
        Status::External
    );
    assert_eq!(
        runner
            .poll_autosave_command(
                &mut progress,
                &mut memory,
                autosave::Execution::Live,
                &mut platform
            )
            .unwrap(),
        Some(true)
    );
    assert_eq!(memory.progress.active_entry().save_position, 3);
    assert_eq!(platform.notices, ["Title Chapter A", "Title Chapter B"]);
    assert_eq!(platform.clocks, 2);
    assert_eq!(
        pump(&mut runner, &mut reads, &mut voice).unwrap(),
        Status::Exited
    );
    assert_eq!(runner.completed_commands(), 8);
    assert_eq!(reads.captures, 3);
    assert!(
        runner
            .poll_autosave_command(
                &mut progress,
                &mut memory,
                autosave::Execution::Live,
                &mut platform
            )
            .is_err()
    );
    println!(
        "Higurashi autosave runner:8 actual commands, three pre-constructor checkpoints, two acknowledged live notifications and one replay completion pass; replay target/clock/notification failures retain PC/PRNG/count, accepted copies survive retry and WAIT ownership stays intact; controlled voice, no OS audio"
    );
}
