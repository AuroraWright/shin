use super::super::{
    autosave::{AutosaveMemory, AutosaveServices, Execution, ProgressEntry, SaveProgress},
    choice::{ChoiceHistory, tests::Services},
    input::ReadState,
    progress::ProgressMemory,
    read_state::Checkpoint,
};
use super::*;
use shin_core::format::{
    save::{PersistData, index_set::IndexSet},
    scenario::instruction_elements::Register,
};
struct Snapshots<'a> {
    progress: &'a mut ProgressMemory,
    kinds: &'a mut Vec<Checkpoint>,
}
impl ReadServices for Snapshots<'_> {
    fn checkpoint(&mut self, kind: Checkpoint, _: ReadState) -> Result<()> {
        self.progress.commit_pending(true)?;
        self.kinds.push(kind);
        Ok(())
    }
    fn message_is_read(&self, _: u32) -> bool {
        false
    }
    fn message_completed(&mut self, _: u32) -> Result<()> {
        Ok(())
    }
}
impl AutosaveServices for Services {
    fn packed_time(&mut self) -> Result<u64> {
        super::super::choice::ChoiceServices::packed_time(self)
    }
    fn notify(&mut self, text: &str) -> Result<()> {
        assert_eq!(text, " ");
        Ok(())
    }
}
fn pump(
    r: &mut HigurashiRunner,
    p: &mut ProgressMemory,
    k: &mut Vec<Checkpoint>,
    v: &mut super::tests::Voice,
) -> Status {
    shin_tasks::block_on(r.pump_native(
        super::tests::policy(),
        ReadPolicy {
            modes: 0,
            forced: false,
            read_only: true,
        },
        &mut Snapshots {
            progress: p,
            kinds: k,
        },
        32,
        10000,
        || [1, 2, 3, 4],
        v,
    ))
    .unwrap()
}
fn choice(code: &mut Vec<u8>) {
    code.extend([
        0x8d, 1, 0, 1, 0, 31, 0, 7, 1, 0, 7, b'A', 0, b'B', 0, b'C', 0, 0,
    ]);
}
fn position(r: &HigurashiRunner) -> (CodeAddress, CodeAddress, u32, u64) {
    (
        r.position(),
        r.next_position(),
        r.random_state(),
        r.completed_commands(),
    )
}
#[test]
#[ignore = "offscreen Metal retained SELECT UI/replay/clock owner sharing actual variable/autosave progress stream; controlled voice, no OS audio"]
fn higurashi_runner_choices_share_responses_and_retain_replay_clock_handoffs() {
    let (_, _, assets) = super::super::message_glyphs::tests::gpu::gpu_assets();
    let assets = Arc::new(assets);
    let authored = shin_tasks::block_on(assets.load::<HigurashiScenario, _>("/main.snr")).unwrap();
    let mut code = vec![0x82, 2, 1, 0x81, 31, 0, 2];
    choice(&mut code);
    code.extend([0x81, 30, 0, 2]);
    choice(&mut code);
    choice(&mut code);
    code.extend([0xa1, 0x83, 1, 4, 0, 0, 0]);
    let mut raw = authored.raw().to_vec();
    let start = authored.header.code_offset as usize;
    raw[start..start + code.len()].copy_from_slice(&code);
    let scenario = Arc::new(HigurashiScenario::new(raw.into()).unwrap());
    let mut r = shin_tasks::block_on(HigurashiRunner::load(assets, scenario, 0, 42, || {
        [1, 2, 3, 4]
    }))
    .unwrap();
    let entry = ProgressEntry {
        scenario_id: 0,
        random_seed: 42,
        save_position: 100,
        replay_target: 100,
        response_data: Vec::new(),
    };
    let source = SaveProgress::new(0, 0, [entry.clone(), entry]).unwrap();
    let mut p = ProgressMemory::new(PersistData(Vec::new()), source.clone(), Vec::new());
    let mut memory = AutosaveMemory {
        valid: false,
        packed_time: 0,
        progress: source,
    };
    let mut history = ChoiceHistory::new(IndexSet::new(), Vec::new());
    let mut s = Services::default();
    let mut k = Vec::new();
    let mut v = super::tests::Voice::default();
    assert_eq!(pump(&mut r, &mut p, &mut k, &mut v), Status::External);
    assert_eq!(
        r.poll_choice_command(&mut p, &mut memory, &mut history, Execution::Live, &mut s)
            .unwrap(),
        None
    );
    assert_eq!(
        r.poll_progress_command(&mut p, &mut memory, Execution::Live, &mut s)
            .unwrap(),
        Some(true)
    );
    assert_eq!(pump(&mut r, &mut p, &mut k, &mut v), Status::External);
    assert_eq!(
        r.poll_progress_command(&mut p, &mut memory, Execution::Live, &mut s)
            .unwrap(),
        Some(true)
    );
    assert_eq!(p.responses().1, 2);
    assert_eq!(pump(&mut r, &mut p, &mut k, &mut v), Status::External);
    assert_eq!(p.pending_persistent(), []);
    assert_eq!(p.persistent().get(2), 1);
    assert_eq!(r.native_message_timeline().unwrap().boundary, 1);
    let cursor = position(&r);
    s.fail_clock = true;
    assert!(
        r.poll_choice_command(&mut p, &mut memory, &mut history, Execution::Live, &mut s)
            .is_err()
    );
    assert_eq!(position(&r), cursor);
    assert_eq!(s.opens, 1);
    s.fail_clock = false;
    assert_eq!(
        r.poll_choice_command(&mut p, &mut memory, &mut history, Execution::Live, &mut s)
            .unwrap(),
        Some(false)
    );
    let before_result = memory.clone();
    s.selected = Some(2);
    s.fail_clock = true;
    assert!(
        r.poll_choice_command(&mut p, &mut memory, &mut history, Execution::Live, &mut s)
            .is_err()
    );
    assert_eq!(r.vm.as_ref().unwrap().read_register(Register::from_regular_register(31)), 2);
    assert_eq!(p.responses().1, 6);
    assert_eq!(p.responses().0[0], 0xc8);
    assert_eq!(r.native_message_timeline().unwrap().boundary, 2);
    assert_eq!(history.last()[0], 3);
    assert!(
        r.poll_choice_command(&mut p, &mut memory, &mut history, Execution::Live, &mut s)
            .is_err()
    );
    assert_eq!(p.responses().1, 6);
    assert_eq!(position(&r), cursor);
    s.fail_clock = false;
    assert_eq!(
        r.poll_choice_command(&mut p, &mut memory, &mut history, Execution::Live, &mut s)
            .unwrap(),
        Some(true)
    );
    assert_eq!(s.clocks, 2);
    assert_eq!(before_result.progress.active_entry().save_position, 1);
    assert_eq!(memory.progress.active_entry().save_position, 2);
    assert_eq!(pump(&mut r, &mut p, &mut k, &mut v), Status::External);
    assert_eq!(
        r.poll_progress_command(&mut p, &mut memory, Execution::Live, &mut s)
            .unwrap(),
        Some(true)
    );
    assert_eq!(p.responses().1, 8);
    assert_eq!(p.responses().0[0], 0xcb);
    assert_eq!(pump(&mut r, &mut p, &mut k, &mut v), Status::External);
    let cursor = position(&r);
    assert_eq!(
        r.poll_choice_command(&mut p, &mut memory, &mut history, Execution::Replay, &mut s)
            .unwrap(),
        Some(false)
    );
    assert_eq!(r.choice_replay_stop(), Some(0));
    assert_eq!(position(&r), cursor);
    assert_eq!(p.responses().1, 8);
    s.selected = None;
    assert_eq!(
        r.poll_choice_command(&mut p, &mut memory, &mut history, Execution::Live, &mut s)
            .unwrap(),
        Some(false)
    );
    s.selected = Some(1);
    assert_eq!(
        r.poll_choice_command(&mut p, &mut memory, &mut history, Execution::Live, &mut s)
            .unwrap(),
        Some(true)
    );
    assert_eq!(p.responses().1, 12);
    assert_eq!(p.responses().0[1], 0x10);
    assert_eq!(s.opens, 2);
    assert_eq!(s.clocks, 4);
    p.restore_response_cursor(8).unwrap();
    let active = p.progress().active_index();
    p.progress_mut().entries[active].save_position = 6;
    assert_eq!(pump(&mut r, &mut p, &mut k, &mut v), Status::External);
    assert_eq!(r.native_message_timeline().unwrap().boundary, 5);
    assert_eq!(
        r.poll_choice_command(&mut p, &mut memory, &mut history, Execution::Replay, &mut s)
            .unwrap(),
        Some(false)
    );
    assert_eq!(r.choice_replay_stop(), Some(1));
    assert_eq!(r.vm.as_ref().unwrap().read_register(Register::from_regular_register(31)), 1);
    assert_eq!(r.native_message_timeline().unwrap().boundary, 6);
    assert_eq!(p.responses().1, 12);
    let frozen = memory.clone();
    assert_eq!(
        r.poll_choice_command(&mut p, &mut memory, &mut history, Execution::Live, &mut s)
            .unwrap(),
        Some(true)
    );
    assert_eq!(s.opens, 2);
    assert_eq!(s.clocks, 4);
    assert_eq!(memory, frozen);
    assert_eq!(pump(&mut r, &mut p, &mut k, &mut v), Status::External);
    assert_eq!(
        r.poll_progress_command(&mut p, &mut memory, Execution::Live, &mut s)
            .unwrap(),
        Some(true)
    );
    assert_eq!(memory.progress.active_entry().save_position, 7);
    assert_eq!(s.clocks, 5);
    assert_eq!(
        memory.progress.active_entry().response_data[0..2],
        [0xcb, 0x10]
    );
    assert_eq!(
        k,
        vec![
            Checkpoint::Interactive,
            Checkpoint::Interactive,
            Checkpoint::Interactive,
            Checkpoint::Autosave
        ]
    );
    assert_eq!(pump(&mut r, &mut p, &mut k, &mut v), Status::Waiting);
    assert!(r.interrupt_wait());
    assert_eq!(pump(&mut r, &mut p, &mut k, &mut v), Status::Exited);
    assert_eq!(r.completed_commands(), 9);
    println!(
        "Higurashi choice runner: 9 commands; single variable/choice/autosave stream, pre-capture commits, retained opening/final clock errors, immediate register publication, both replay stop paths, UI-free consumed-choice live handoff and WAIT continuation"
    );
}
