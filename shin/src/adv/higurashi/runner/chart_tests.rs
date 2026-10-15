use super::*;
use crate::adv::higurashi::{
    autosave::{AutosaveMemory, AutosaveServices, Execution, ProgressEntry, SaveProgress},
    chart_progress::tests::chart,
    checkpoints::{CheckpointMemory, CheckpointServices},
    progress::ProgressMemory,
    read_history::MessageReadHistory,
    replay::ReplaySession,
};
use shin_core::format::save::PersistData;
struct Platform;
impl AutosaveServices for Platform {
    fn packed_time(&mut self) -> Result<u64> {
        Ok(42)
    }
    fn notify(&mut self, _: &str) -> Result<()> {
        Ok(())
    }
}
fn pump(
    runner: &mut HigurashiRunner,
    memory: &mut CheckpointMemory,
    progress: &mut ProgressMemory,
    history: &mut MessageReadHistory,
    voice: &mut super::tests::Voice,
    max: usize,
) -> Result<Status> {
    shin_tasks::block_on(runner.pump_native(
        super::tests::policy(),
        ReadPolicy {
            modes: 0,
            forced: false,
            read_only: false,
        },
        &mut CheckpointServices {
            memory,
            progress,
            history,
        },
        max,
        10000,
        || [1, 2, 3, 4],
        voice,
    ))
}
#[test]
#[ignore = "actual native runner constructor ownership/retry/retained AUTOSAVE/WAIT and shared-chart checkpoint rollback on offscreen Metal"]
fn higurashi_runner_chart_constructs_once_and_restores_nodes_without_relocking_progress() {
    let (_, _, assets) = crate::adv::higurashi::reference_gpu_assets();
    let assets = Arc::new(assets);
    let source = assets
        .load_sync::<shin_core::format::scenario::higurashi::HigurashiScenario>("/main.snr")
        .unwrap();
    let mut code = Vec::new();
    chart(&mut code, 0, &[]);
    chart(&mut code, 0, &[0, 0, 32]);
    code.extend([0xa1, 0x83, 1, 6]);
    chart(&mut code, 0, &[7]);
    chart(&mut code, 1, &[8192]);
    code.extend([0, 0, 0]);
    let mut raw = source.raw().to_vec();
    let start = source.header.code_offset as usize;
    raw[start..start + code.len()].copy_from_slice(&code);
    let scenario = Arc::new(
        shin_core::format::scenario::higurashi::HigurashiScenario::new(raw.into()).unwrap(),
    );
    let mut runner = shin_tasks::block_on(HigurashiRunner::load(
        assets,
        scenario.clone(),
        0,
        42,
        || [1, 2, 3, 4],
    ))
    .unwrap();
    let mut voice = super::tests::Voice::default();
    assert_eq!(
        shin_tasks::block_on(runner.pump_native(
            super::tests::policy(),
            ReadPolicy {
                modes: 0,
                forced: false,
                read_only: false
            },
            &mut NoReadServices,
            1,
            10000,
            || [1, 2, 3, 4],
            &mut voice
        ))
        .unwrap(),
        Status::CommandBudget
    );
    // A missing owner retains the command and its VM position before applying
    // any constructor effects. Supplying the owner retries that same command.
    for _ in 0..2 {
        assert!(
            shin_tasks::block_on(runner.pump_native(
                super::tests::policy(),
                ReadPolicy {
                    modes: 0,
                    forced: false,
                    read_only: false
                },
                &mut NoReadServices,
                1,
                10000,
                || [1, 2, 3, 4],
                &mut voice
            ))
            .is_err()
        );
        assert_eq!(runner.completed_commands(), 1);
        assert!(matches!(runner.pending_command(), Some(Cmd::CHART { .. })));
    }
    let entry = ProgressEntry {
        scenario_id: 0,
        random_seed: 42,
        save_position: 0,
        replay_target: 0,
        response_data: Vec::new(),
    };
    let pair = SaveProgress::new(0, 0, [entry.clone(), entry]).unwrap();
    let mut progress = ProgressMemory::new(PersistData::new(), pair.clone(), Vec::new());
    let shared = progress.chart_unlocks.clone();
    let mut memory = CheckpointMemory::new(vec![]);
    let mut history = MessageReadHistory::new();
    assert_eq!(
        pump(
            &mut runner,
            &mut memory,
            &mut progress,
            &mut history,
            &mut voice,
            1
        )
        .unwrap(),
        Status::CommandBudget
    );
    assert_eq!(memory.extra_words, [0, 0, 32]);
    assert_eq!(shared.saved_words()[0], 1);
    for _ in 0..2 {
        assert_eq!(
            pump(
                &mut runner,
                &mut memory,
                &mut progress,
                &mut history,
                &mut voice,
                8
            )
            .unwrap(),
            Status::External
        );
        assert_eq!(memory.extra_words, [0, 0, 32]);
        assert_eq!(memory.entries().len(), 1);
        assert_eq!(memory.entries()[0].state.extra_words, [0, 0, 32]);
    }
    let mut autosave = AutosaveMemory {
        valid: false,
        packed_time: 0,
        progress: pair,
    };
    assert_eq!(
        runner
            .poll_progress_command(&mut progress, &mut autosave, Execution::Live, &mut Platform)
            .unwrap(),
        Some(true)
    );
    assert_eq!(
        pump(
            &mut runner,
            &mut memory,
            &mut progress,
            &mut history,
            &mut voice,
            8
        )
        .unwrap(),
        Status::Waiting
    );
    for _ in 0..2 {
        assert_eq!(
            pump(
                &mut runner,
                &mut memory,
                &mut progress,
                &mut history,
                &mut voice,
                8
            )
            .unwrap(),
            Status::Waiting
        );
        assert_eq!(memory.extra_words, [0, 0, 32]);
    }
    runner.advance(
        Ticks::from_f32(6.0),
        super::tests::policy(),
        &mut voice,
        None,
    );
    assert_eq!(
        pump(
            &mut runner,
            &mut memory,
            &mut progress,
            &mut history,
            &mut voice,
            8
        )
        .unwrap(),
        Status::Exited
    );
    assert_eq!(runner.completed_commands(), 7);
    assert_eq!(memory.extra_words, [0, 0, 32, 7]);
    assert_eq!(memory.entries()[0].state.extra_words, [0, 0, 32]);
    let words = shared.saved_words();
    assert_eq!(words.len(), 512);
    assert_eq!(words[0], 1 | (1 << 7));
    assert_eq!(words[1], 1);
    assert_eq!(words[256], 1);
    let mut replay =
        ReplaySession::new(scenario, memory.fork_for_restore(1, 1, &progress).unwrap()).unwrap();
    assert_eq!(replay.memory.extra_words, [0, 0, 32]);
    assert_eq!(replay.progress.chart_unlocks.saved_words(), words);
    assert_eq!(
        replay.pump(&history, 8, 10000, || false).unwrap(),
        crate::adv::higurashi::replay::Status::Ready
    );
    assert_eq!(replay.memory.extra_words, [0, 0, 32]);
    assert_eq!(shared.saved_words(), words);
    eprintln!(
        "CHART live runner:7 actual commands;empty list needs no owner,missing-owner retry,one constructor per command,immediate completion,retained AUTOSAVE/WAIT without repeated nodes,mode1 unlock only,immutable checkpoint nodes and shared persistent unlocks after real replay rollback pass;controlled audio,no OS audio or disk writes"
    );
}
