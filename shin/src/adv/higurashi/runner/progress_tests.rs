use super::super::{
    autosave::{self, AutosaveMemory, AutosaveServices, ProgressEntry, SaveProgress},
    progress::ProgressMemory,
    read_state::Checkpoint,
};
use super::*;
use shin_core::{format::save::PersistData, format::scenario::instruction_elements::Register};

struct Reads {
    fail: bool,
    captures: Vec<ProgressMemory>,
}
struct Services<'a> {
    reads: &'a mut Reads,
    progress: &'a mut ProgressMemory,
}
impl ReadServices for Services<'_> {
    fn checkpoint(&mut self, kind: Checkpoint, _: super::super::input::ReadState) -> Result<()> {
        ensure!(!self.reads.fail, "required full checkpoint unavailable");
        assert_eq!(kind, Checkpoint::Autosave);
        self.progress.commit_pending(true)?;
        self.reads.captures.push(self.progress.clone());
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
}
impl AutosaveServices for Platform {
    fn packed_time(&mut self) -> Result<u64> {
        self.clocks += 1;
        Ok(42 + self.clocks as u64)
    }
    fn notify(&mut self, text: &str) -> Result<()> {
        self.notices.push(text.into());
        Ok(())
    }
}
fn pump(
    runner: &mut HigurashiRunner,
    reads: &mut Reads,
    progress: &mut ProgressMemory,
    voice: &mut super::tests::Voice,
) -> Result<Status> {
    shin_tasks::block_on(runner.pump_native(
        super::tests::policy(),
        ReadPolicy {
            modes: 0,
            forced: false,
            read_only: true,
        },
        &mut Services { reads, progress },
        32,
        10000,
        || [1, 2, 3, 4],
        voice,
    ))
}
fn info(code: &mut Vec<u8>, level: u8, text: &str) {
    code.extend([0xa0, level, (text.len() + 1) as u8]);
    code.extend(text.bytes());
    code.push(0);
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
#[ignore = "actual native runner variable responses, checked checkpoint commits, cursor binding and autosave memory through one session on offscreen Metal"]
fn higurashi_runner_progress_session_shares_variable_responses_with_checkpoint_and_autosave_owners()
{
    let (_, _, assets) = super::super::message_glyphs::tests::gpu::gpu_assets();
    let assets = Arc::new(assets);
    let authored = shin_tasks::block_on(assets.load::<HigurashiScenario, _>("/main.snr")).unwrap();
    let mut code = vec![0x82, 1, 0xb0, 0x81, 31, 0, 1, 0xdb, 2, 0xb0, 0xda, 32, 0, 2];
    info(&mut code, 0, "Title");
    info(&mut code, 1, "Bound");
    code.extend([
        0xa1, 0x83, 1, 0x83, 0xe8, 0x81, 33, 0, 1, 0xda, 34, 0, 2, 0xa1, 0, 0, 0,
    ]);
    let mut raw = authored.raw().to_vec();
    let start = authored.header.code_offset as usize;
    raw[start..start + code.len()].copy_from_slice(&code);
    let scenario = Arc::new(HigurashiScenario::new(raw.into()).unwrap());
    let mut runner =
        shin_tasks::block_on(HigurashiRunner::load(assets, scenario, 65535, 42, || {
            [1, 2, 3, 4]
        }))
        .unwrap();
    let source = SaveProgress::new(
        1,
        123,
        [
            ProgressEntry {
                scenario_id: 3,
                random_seed: 56,
                save_position: 123,
                replay_target: 456,
                response_data: vec![42; 5],
            },
            ProgressEntry {
                scenario_id: 65535,
                random_seed: 42,
                save_position: 0,
                replay_target: 0,
                response_data: Vec::new(),
            },
        ],
    )
    .unwrap();
    let inactive = source.entries[0].clone();
    let mut saved = AutosaveMemory {
        valid: false,
        packed_time: 0,
        progress: source.clone(),
    };
    let mut progress = ProgressMemory::new(PersistData::new(), source, Vec::new());
    let mut reads = Reads {
        fail: true,
        captures: Vec::new(),
    };
    let mut platform = Platform {
        clocks: 0,
        notices: Vec::new(),
    };
    let mut voice = super::tests::Voice::default();
    for _ in 0..4 {
        assert_eq!(
            pump(&mut runner, &mut reads, &mut progress, &mut voice).unwrap(),
            Status::External
        );
        let before = cursor(&runner);
        assert_eq!(
            runner
                .poll_progress_command(
                    &mut progress,
                    &mut saved,
                    autosave::Execution::Live,
                    &mut platform
                )
                .unwrap(),
            Some(true)
        );
        assert_eq!(runner.position(), before.0);
        assert_eq!(runner.next_position(), before.1);
        assert_eq!(runner.random_state(), before.2);
        assert_eq!(runner.completed_commands(), before.3 + 1);
        assert!(
            runner
                .poll_progress_command(
                    &mut progress,
                    &mut saved,
                    autosave::Execution::Live,
                    &mut platform
                )
                .is_err()
        );
    }
    assert_eq!(progress.responses().1, 25);
    assert_eq!(progress.pending_persistent(), [(1, 65535)]);
    assert_eq!(progress.pending_bytes(), [(2, 65535)]);
    assert!(pump(&mut runner, &mut reads, &mut progress, &mut voice).is_err());
    let before = cursor(&runner);
    let retained = progress.clone();
    assert!(pump(&mut runner, &mut reads, &mut progress, &mut voice).is_err());
    assert_eq!(cursor(&runner), before);
    assert_eq!(progress.responses(), retained.responses());
    assert_eq!(progress.pending_persistent(), retained.pending_persistent());
    assert_eq!(progress.pending_bytes(), retained.pending_bytes());
    assert_eq!(runner.autosave_flags(), None);
    assert_eq!(runner.completed_commands(), 6);
    reads.fail = false;
    assert_eq!(
        pump(&mut runner, &mut reads, &mut progress, &mut voice).unwrap(),
        Status::External
    );
    assert_eq!(progress.persistent().get(1), -1);
    assert_eq!(progress.bytes()[2], 255);
    assert!(progress.pending_persistent().is_empty());
    assert!(progress.pending_bytes().is_empty());
    assert_eq!(reads.captures.len(), 1);
    assert_eq!(reads.captures[0].responses(), progress.responses());
    assert_eq!(
        runner
            .poll_progress_command(
                &mut progress,
                &mut saved,
                autosave::Execution::Live,
                &mut platform
            )
            .unwrap(),
        Some(true)
    );
    let first = saved.clone();
    assert_eq!(
        &first.progress.active_entry().response_data,
        progress.responses().0
    );
    assert_eq!(first.progress.entries[0], inactive);
    assert_eq!(platform.notices, ["Title Bound"]);
    assert_eq!(
        pump(&mut runner, &mut reads, &mut progress, &mut voice).unwrap(),
        Status::Waiting
    );
    assert!(
        runner
            .poll_progress_command(
                &mut progress,
                &mut saved,
                autosave::Execution::Live,
                &mut platform
            )
            .is_err()
    );
    assert!(runner.interrupt_wait());
    for _ in 0..2 {
        assert_eq!(
            pump(&mut runner, &mut reads, &mut progress, &mut voice).unwrap(),
            Status::External
        );
        assert_eq!(
            runner
                .poll_progress_command(
                    &mut progress,
                    &mut saved,
                    autosave::Execution::Live,
                    &mut platform
                )
                .unwrap(),
            Some(true)
        );
    }
    assert_eq!(progress.responses().1, 50);
    assert_eq!(saved, first);
    assert_eq!(progress.progress().entries[0], inactive);
    assert_eq!(
        pump(&mut runner, &mut reads, &mut progress, &mut voice).unwrap(),
        Status::External
    );
    assert_eq!(
        runner.vm.as_ref().unwrap().read_register(Register::from_regular_register(31)),
        65535
    );
    assert_eq!(
        runner.vm.as_ref().unwrap().read_register(Register::from_regular_register(32)),
        65535
    );
    assert_eq!(
        runner.vm.as_ref().unwrap().read_register(Register::from_regular_register(33)),
        -1
    );
    assert_eq!(
        runner.vm.as_ref().unwrap().read_register(Register::from_regular_register(34)),
        255
    );
    assert_eq!(reads.captures.len(), 2);
    assert_eq!(reads.captures[0].responses().1, 25);
    assert_eq!(reads.captures[1].responses().1, 50);
    assert_eq!(
        runner
            .poll_progress_command(
                &mut progress,
                &mut saved,
                autosave::Execution::Replay,
                &mut platform
            )
            .unwrap(),
        Some(false)
    );
    assert_eq!(saved, first);
    assert_eq!(platform.clocks, 1);
    assert_eq!(
        runner
            .poll_progress_command(
                &mut progress,
                &mut saved,
                autosave::Execution::Live,
                &mut platform
            )
            .unwrap(),
        Some(true)
    );
    assert_eq!(
        &saved.progress.active_entry().response_data,
        progress.responses().0
    );
    assert_eq!(saved.progress.active_entry().save_position, 2);
    assert_eq!(saved.progress.active_entry().replay_target, 2);
    assert_eq!(saved.progress.entries[0], inactive);
    assert_eq!(
        pump(&mut runner, &mut reads, &mut progress, &mut voice).unwrap(),
        Status::Exited
    );
    assert_eq!(runner.completed_commands(), 12);
    assert_eq!(platform.clocks, 2);
    assert_eq!(platform.notices, ["Title Bound", "Title Bound"]);
    println!(
        "Higurashi progress runner:12 actual commands, six variable continuations, two checked checkpoint commits and two autosave copies pass; pending full32 becomes signed16/unsigned8 storage,25/50-bit responses bind active entry directly, failure/WAIT/replay retention preserves ownership and earlier captured vectors stay independent; controlled voice, no OS audio"
    );
}
