use super::super::{
    autosave::{Execution, ProgressEntry, SaveProgress},
    checkpoints::{CheckpointMemory, CheckpointServices},
    progress::ProgressMemory,
    read_history::MessageReadHistory,
};
use super::*;
use shin_core::format::{
    save::PersistData,
    scenario::instruction_elements::{Register, UntypedNumberArray},
};
fn pump(
    r: &mut HigurashiRunner,
    memory: &mut CheckpointMemory,
    progress: &mut ProgressMemory,
    history: &mut MessageReadHistory,
    voice: &mut super::tests::Voice,
) -> Status {
    shin_tasks::block_on(r.pump_native(
        super::tests::policy(),
        ReadPolicy {
            modes: 0,
            forced: false,
            read_only: true,
        },
        &mut CheckpointServices {
            memory,
            progress,
            history,
        },
        32,
        10000,
        || [1, 2, 3, 4],
        voice,
    ))
    .unwrap()
}
#[test]
#[ignore = "offscreen Metal concrete checkpoint owner with real resources and controlled voice; no OS audio"]
fn higurashi_runner_captures_complete_memory_before_constructors_and_forks_current_progress() {
    let (_, _, assets) = super::super::message_glyphs::tests::gpu::gpu_assets();
    let assets = Arc::new(assets);
    let authored = shin_tasks::block_on(assets.load::<HigurashiScenario, _>("/main.snr")).unwrap();
    // SSET, SAVEINFO, EVBEGIN, EVEND, AUTOSAVE, WAIT, EXIT. Rewind the
    // event command itself; metadata and layer targets are pre-constructor.
    let code = vec![
        0x82, 2, 1, 0xa0, 0, 2, b'A', 0, 0xa2, 0, 0xa3, 0xa1, 0x83, 1, 4, 0, 0, 0,
    ];
    let mut raw = authored.raw().to_vec();
    let start = authored.header.code_offset as usize;
    raw[start..start + code.len()].copy_from_slice(&code);
    let scenario = Arc::new(HigurashiScenario::new(raw.into()).unwrap());
    let mut r = shin_tasks::block_on(HigurashiRunner::load(assets, scenario, 0, 42, || {
        [1, 2, 3, 4]
    }))
    .unwrap();
    // Two banks and stale target words must survive independently of live owners.
    let layers = &mut r.frame.scene.layers_mut().unwrap().state;
    layers.load(7, 0, 0, UntypedNumberArray::default()).unwrap();
    layers.control(7, 0, 123, 0).unwrap();
    layers.start_back(7).unwrap();
    layers.control(7, 0, 456, 0).unwrap();
    let layer_before = format!("{layers:?}");
    let entry = ProgressEntry {
        scenario_id: 0,
        random_seed: 42,
        save_position: 99,
        replay_target: 88,
        response_data: vec![0xac; 1024],
    };
    let source = SaveProgress::new(1, 0x13579bdf, [entry.clone(), entry]).unwrap();
    let mut p = ProgressMemory::new(PersistData(vec![0; 4]), source, vec![9, 8, 7]);
    p.restore_response_cursor(13).unwrap();
    let mut memory = CheckpointMemory::new(vec![1, 0x12345678, u32::MAX]);
    assert!(
        memory
            .capture(
                r.vm.as_ref().unwrap(),
                &CommandReadState::new(),
                &r.frame.scene.layers().state,
                &mut p
            )
            .is_err()
    );
    assert!(memory.entries().is_empty());
    let mut history = MessageReadHistory::new();
    history.record_completion(true, 42);
    let mut voice = super::tests::Voice::default();
    assert_eq!(
        pump(&mut r, &mut memory, &mut p, &mut history, &mut voice),
        Status::External
    );
    assert!(matches!(r.pending_command(), Some(Cmd::SSET { .. })));
    p.dispatch_variables(r.pending_command().unwrap(), Execution::Live, 0)
        .unwrap();
    r.complete_external(CommandResult::None).unwrap();
    assert_eq!(p.pending_persistent(), [(2, 1)]);
    assert_eq!(
        pump(&mut r, &mut memory, &mut p, &mut history, &mut voice),
        Status::External
    );
    assert!(matches!(
        r.pending_command(),
        Some(Cmd::EVBEGIN { argument: 0 })
    ));
    assert_eq!(p.persistent().get(2), 1);
    assert!(p.pending_persistent().is_empty());
    assert_eq!(memory.entries().len(), 1);
    let saved = memory.entries()[0].state.clone();
    assert_eq!(saved.read.timeline().boundary, 0);
    assert_eq!(saved.read.state().flags, u32::MAX);
    assert_eq!(&*saved.read.metadata().current()[0], "A");
    assert_eq!(&*saved.read.metadata().saved()[0], "");
    assert_eq!(saved.response_cursor, 13); // Setters queue writes; getters record responses.
    assert_eq!(saved.extra_words, [1, 0x12345678, u32::MAX]);
    assert_eq!(format!("{:?}", saved.layers), layer_before);
    assert_eq!(saved.vm.position(), r.position());
    let mut replay = saved.vm.scripter();
    assert_eq!(
        replay.run(CommandResult::None, 100).unwrap(),
        r.pending_command().unwrap().clone()
    );
    assert_eq!(r.native_read_state().unwrap().flags, 0);
    assert_eq!(r.native_message_timeline().unwrap().boundary, 1);
    // Native commits happen even when sparse capture skips the current bucket.
    p.dispatch_variables(&Cmd::SSET { args: [3, 7] }, Execution::Live, 1)
        .unwrap();
    r.complete_external(CommandResult::None).unwrap();
    assert_eq!(
        pump(&mut r, &mut memory, &mut p, &mut history, &mut voice),
        Status::External
    );
    assert!(matches!(r.pending_command(), Some(Cmd::EVEND {})));
    assert_eq!(p.persistent().get(3), 7);
    assert!(p.pending_persistent().is_empty());
    assert_eq!(memory.entries().len(), 1);
    memory.extra_words[0] = 99;
    r.frame
        .scene
        .layers_mut()
        .unwrap()
        .state
        .control(7, 0, 999, 0)
        .unwrap();
    p.dispatch_variables(&Cmd::SSET { args: [1, 9] }, Execution::Live, 2)
        .unwrap();
    let full_responses = p.responses().0.to_vec();
    let fork = memory.fork_for_restore(37, 91, &p).unwrap();
    assert_eq!(memory.entries().len(), 1);
    assert!(fork.memory.entries().is_empty());
    assert!(fork.memory.commits_enabled);
    assert_eq!(fork.memory.extra_words, saved.extra_words);
    assert!(Arc::ptr_eq(fork.restored.as_ref().unwrap(), &saved));
    assert_eq!(fork.progress.responses(), (full_responses.as_slice(), 13));
    assert_eq!(fork.progress.progress().active_entry().save_position, 37);
    assert_eq!(fork.progress.progress().active_entry().replay_target, 91);
    assert_eq!(fork.progress.progress().entries[0], p.progress().entries[0]);
    assert_eq!(fork.progress.persistent().get(3), 7);
    assert_eq!(fork.progress.persistent().get(1), 0); // queued writes aren't constructor input.
    assert!(fork.progress.pending_persistent().is_empty());
    assert_eq!(p.pending_persistent(), [(1, 9)]);
    let mut short = p.progress().clone();
    short.entries[1].response_data.clear();
    let short = ProgressMemory::new(p.persistent().clone(), short, p.bytes().to_vec());
    let mut short_fork = memory.fork_for_restore(37, 91, &short).unwrap();
    assert_eq!(short_fork.progress.responses(), (&[][..], 13));
    short_fork
        .progress
        .dispatch_variables(
            &Cmd::SGET {
                dest: Register::from_regular_register(31),
                slot: 2,
            },
            Execution::Replay,
            0,
        )
        .unwrap();
    assert_eq!(short_fork.progress.responses().0.len(), 1024);
    assert_eq!(short_fork.progress.responses().1, 30);
    assert_eq!(fork.progress.bytes(), [9, 8, 7]);
    assert_eq!(format!("{:?}", saved.layers), layer_before);
    assert!(history.contains(42));
    // Original parent continues without changing the payload or fork archive.
    r.complete_external(CommandResult::None).unwrap();
    assert_eq!(
        pump(&mut r, &mut memory, &mut p, &mut history, &mut voice),
        Status::External
    );
    assert!(matches!(r.pending_command(), Some(Cmd::AUTOSAVE {})));
    assert_eq!(p.persistent().get(1), 9);
    r.complete_external(CommandResult::None).unwrap();
    assert_eq!(
        pump(&mut r, &mut memory, &mut p, &mut history, &mut voice),
        Status::Waiting
    );
    assert!(r.interrupt_wait());
    assert_eq!(
        pump(&mut r, &mut memory, &mut p, &mut history, &mut voice),
        Status::Exited
    );
    assert_eq!(r.completed_commands(), 7);
    assert_eq!(saved.read.timeline().boundary, 0);
    println!(
        "Higurashi concrete checkpoint runner:7 commands; pre-constructor VM/timeline/read/metadata/both layer banks/extra vector/response cursor capture, skipped-bucket commits, arithmetic command replay, current globals/full progress vectors, empty fork queues and independent archive/source ownership pass; live reconstruction remains pending"
    );
}
