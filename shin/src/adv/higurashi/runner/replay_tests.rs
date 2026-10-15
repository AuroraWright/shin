use super::super::{
    autosave::{AutosaveMemory, Execution},
    choice::{ChoiceHistory, tests::Services as ChoiceServices},
    live_reconstruction::tests::Audio,
    read_history::MessageReadHistory,
    replay::{
        Status as ReplayStatus,
        tests::{scenario, session},
    },
};
use super::*;
use shin_core::format::save::index_set::IndexSet;
struct Reads;
impl ReadServices for Reads {
    fn checkpoint(
        &mut self,
        _: super::super::read_state::Checkpoint,
        _: super::super::input::ReadState,
    ) -> Result<()> {
        Ok(())
    }
    fn message_is_read(&self, _: u32) -> bool {
        false
    }
    fn message_completed(&mut self, _: u32) -> Result<()> {
        Ok(())
    }
}
#[test]
#[ignore = "real loaded runner and retained native read state;nonzero EXIT must complete NOPE and reach WAIT on offscreen Metal,controlled voice,no window/saves"]
fn higurashi_runner_nonzero_exit_completes_nope_before_wait() {
    let (_, _, assets) = super::super::message_glyphs::tests::gpu::gpu_assets();
    let assets = Arc::new(assets);
    for mode in [1, 2, 255] {
        let scenario = scenario(&[0, mode, 0x7f, 0x83, 1, 32, 0, 0, 0]);
        let mut runner = shin_tasks::block_on(HigurashiRunner::load(
            assets.clone(),
            scenario,
            0,
            42,
            || [1; 4],
        ))
        .unwrap();
        let mut audio = Audio::default();
        assert_eq!(pump(&mut runner, &mut audio), Status::Waiting);
        assert_eq!(runner.completed_commands(), 1);
        assert!(matches!(
            runner.pending_command(),
            Some(Cmd::WAIT { amount: 32, .. })
        ));
        assert_eq!(runner.native_read_state().unwrap().fast_forward_blocked, 0);
        assert!(!runner.exited);
        assert_eq!(runner.native_message_timeline().unwrap().boundary, 0);
    }
    println!(
        "Live nonzero EXIT:3 real runner profiles complete NOPE and advance to WAIT without exit/constructor UI effects or timeline/fast-forward changes"
    );
}
fn pump(r: &mut HigurashiRunner, audio: &mut Audio) -> Status {
    shin_tasks::block_on(r.pump_native(
        super::tests::policy(),
        ReadPolicy {
            modes: 0,
            forced: false,
            read_only: true,
        },
        &mut Reads,
        32,
        10000,
        || [1; 4],
        audio,
    ))
    .unwrap()
}
#[test]
#[ignore = "offscreen Metal retained replay SELECT/message publication, real resources and controlled audio"]
fn higurashi_runner_hands_replay_commands_to_live_without_constructor_or_response_duplication() {
    let (_, _, assets) = super::super::message_glyphs::tests::gpu::gpu_assets();
    let assets = Arc::new(assets);
    let choice = [
        0x8d, 0, 0, 0, 0, 31, 0, 7, 1, 0, 7, b'A', 0, b'B', 0, b'C', 0, 0,
    ];
    for starts_resume in [false, true] {
        let mut code = vec![];
        if !starts_resume {
            code.extend([0x86, 1, 0, 0, 1, 0x7f, 2, 0, b'A', 0]);
        }
        code.extend(choice);
        code.extend([0x83, 1, 0x80, 0x40, 0, 0, 0]);
        let scenario = scenario(&code);
        let saved = if starts_resume { 1 } else { 2 };
        let mut replay = session(scenario.clone(), saved, saved, 42);
        assert_eq!(
            replay
                .pump(&MessageReadHistory::new(), 32, 10000, || false)
                .unwrap(),
            ReplayStatus::Ready
        );
        assert!(matches!(replay.pending_command(), Some(Cmd::SELECT { .. })));
        assert_eq!(
            replay.progress.responses().1,
            if starts_resume { 0 } else { 4 }
        );
        let cursor = replay.progress.responses().1;
        let next = replay.vm.as_ref().unwrap().next_position();
        let prng = replay.vm.as_ref().unwrap().random_state();
        let mut runner = shin_tasks::block_on(HigurashiRunner::load(
            assets.clone(),
            scenario,
            0,
            42,
            || [1; 4],
        ))
        .unwrap();
        let mut audio = Audio::default();
        audio.fail_slot = Some(5);
        assert!(
            shin_tasks::block_on(runner.reconstruct_replay(&mut replay, || [2; 4], &mut audio))
                .is_err()
        );
        assert_eq!(replay.status(), ReplayStatus::Ready);
        assert_eq!(replay.vm.as_ref().unwrap().next_position(), next);
        assert_eq!(replay.progress.responses().1, cursor);
        assert!(runner.vm_fault.is_some());
        audio.fail_slot = None;
        shin_tasks::block_on(runner.reconstruct_replay(&mut replay, || [2; 4], &mut audio))
            .unwrap();
        assert_eq!(replay.status(), ReplayStatus::Published);
        assert_eq!(runner.next_position(), next);
        assert_eq!(runner.random_state(), prng);
        assert_eq!(
            runner.choice_replay_stop(),
            Some(if starts_resume { 0 } else { 1 })
        );
        assert_eq!(pump(&mut runner, &mut audio), Status::External);
        assert_eq!(
            runner.read_state.as_ref().unwrap().timeline().boundary,
            saved
        );
        let mut memory = AutosaveMemory {
            valid: false,
            packed_time: 0,
            progress: replay.progress.progress().clone(),
        };
        let mut history = ChoiceHistory::new(IndexSet::new(), vec![]);
        let mut services = ChoiceServices::default();
        services.selected = Some(2);
        services.fail_open = !starts_resume;
        assert_eq!(
            runner
                .poll_choice_command(
                    &mut replay.progress,
                    &mut memory,
                    &mut history,
                    Execution::Live,
                    &mut services
                )
                .unwrap(),
            Some(true)
        );
        assert_eq!(services.opens, u32::from(starts_resume));
        assert_eq!(services.clocks, if starts_resume { 2 } else { 0 });
        assert_eq!(replay.progress.responses().1, 4);
        assert_eq!(
            runner.vm.as_ref().unwrap().read_register(
                shin_core::format::scenario::instruction_elements::Register::from_regular_register(
                    31
                )
            ),
            2
        );
        assert_eq!(runner.read_state.as_ref().unwrap().timeline().boundary, 2);
        assert_eq!(pump(&mut runner, &mut audio), Status::Waiting);
    }
    let scenario = scenario(&[
        0x86, 1, 0, 0, 1, 0x7f, 2, 0, b'A', 0, 0x86, 2, 0, 0, 1, 0x7f, 2, 0, b'B', 0, 0, 0, 0,
    ]);
    let mut replay = session(scenario.clone(), 2, 2, 42);
    assert_eq!(
        replay
            .pump(&MessageReadHistory::new(), 32, 10000, || false)
            .unwrap(),
        ReplayStatus::Ready
    );
    let next = replay.vm.as_ref().unwrap().next_position();
    let prng = replay.vm.as_ref().unwrap().random_state();
    let mut runner =
        shin_tasks::block_on(HigurashiRunner::load(assets, scenario, 0, 42, || [1; 4])).unwrap();
    let mut audio = Audio::default();
    shin_tasks::block_on(runner.reconstruct_replay(&mut replay, || [2; 4], &mut audio)).unwrap();
    assert!(matches!(runner.pending_command(),Some(Cmd::MSGSET {msg_id, ..}) if msg_id.0==2));
    assert_eq!(pump(&mut runner, &mut audio), Status::Waiting);
    assert_eq!(runner.read_state.as_ref().unwrap().timeline().boundary, 2);
    assert_eq!(runner.next_position(), next);
    assert_eq!(runner.random_state(), prng);
    assert!(runner.frame.message.glyphs().is_some());
    assert_eq!(replay.memory.entries().len(), 1);
    eprintln!(
        "Verified both retained SELECT states, publication failure/retry and retained MSGSET with no duplicate constructor/response"
    );
}
