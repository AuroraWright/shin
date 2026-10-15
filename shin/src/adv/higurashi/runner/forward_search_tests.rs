use super::super::{
    checkpoints::CheckpointServices,
    forward_search::{Kind, Status as SearchStatus},
    live_reconstruction::tests::Audio,
    read_history::MessageReadHistory,
    replay::{
        Status as ReplayStatus,
        tests::{scenario, session},
    },
};
use super::*;

#[test]
#[ignore = "offscreen Metal forward-search result to live runner, glyph retention and isolated temporary writes"]
fn higurashi_runner_publishes_forward_search_result_and_keeps_source_until_publication() {
    let (_, _, assets) = super::super::message_glyphs::tests::gpu::gpu_assets();
    let scenario = scenario(&[
        0x86, 1, 0, 0, 1, 0x7f, 2, 0, b'A', 0, 0x82, 2, 1, 0x86, 2, 0, 0, 1, 0x7f, 2, 0, b'B', 0,
        0, 0, 0,
    ]);
    let mut initial = session(scenario.clone(), 0, 0, 42);
    let mut runner = shin_tasks::block_on(HigurashiRunner::load(
        Arc::new(assets),
        scenario,
        0,
        42,
        || [1; 4],
    ))
    .unwrap();
    let mut history = MessageReadHistory::new();
    history.record_completion(true, 0);
    let mut audio = Audio::default();
    let policy = ReadPolicy {
        modes: 0,
        forced: false,
        read_only: true,
    };
    assert_eq!(
        shin_tasks::block_on(runner.pump_native(
            super::tests::policy(),
            policy,
            &mut CheckpointServices {
                memory: &mut initial.memory,
                progress: &mut initial.progress,
                history: &mut history
            },
            32,
            10000,
            || [1; 4],
            &mut audio
        ))
        .unwrap(),
        Status::Waiting
    );
    let before = (
        runner.position(),
        runner.next_position(),
        runner.random_state(),
    );
    let glyphs = runner.frame.message.glyphs().unwrap() as *const _;
    let progress_before = format!("{:?}", initial.progress);
    let mut search = runner
        .begin_forward_search(&initial.memory, &initial.progress, Kind::ReadAhead, true)
        .unwrap()
        .unwrap();
    while search.pump(&history, 1, 10000, || false).unwrap() != SearchStatus::Complete {}
    assert_eq!(
        (
            runner.position(),
            runner.next_position(),
            runner.random_state()
        ),
        before
    );
    assert_eq!(runner.frame.message.glyphs().unwrap() as *const _, glyphs);
    assert_eq!(format!("{:?}", initial.progress), progress_before);
    let mut result = search.take_result().unwrap().unwrap();
    assert_eq!(result.progress.persistent().get(2), 0);
    assert!(result.progress.pending_persistent().is_empty());
    assert_eq!(result.progress.progress().active_entry().save_position, 2);
    assert_eq!(result.progress.progress().active_entry().replay_target, 1);
    assert!(
        shin_tasks::block_on(runner.reconstruct_replay(&mut result, || [2; 4], &mut audio))
            .is_err(),
        "the fork still needs replay"
    );
    assert_eq!(
        result.pump(&history, 32, 10000, || false).unwrap(),
        ReplayStatus::Ready
    );
    assert_eq!(result.progress.persistent().get(2), 1);
    assert_eq!(initial.progress.persistent().get(2), 0);
    shin_tasks::block_on(runner.reconstruct_replay(&mut result, || [2; 4], &mut audio)).unwrap();
    assert_eq!(runner.frame.message.glyphs().unwrap() as *const _, glyphs);
    assert!(runner.frame.message.playback.input_blocked);
    let vm_before = (runner.next_position(), runner.random_state());
    assert_eq!(
        shin_tasks::block_on(runner.pump_native(
            super::tests::policy(),
            policy,
            &mut CheckpointServices {
                memory: &mut result.memory,
                progress: &mut result.progress,
                history: &mut history
            },
            32,
            10000,
            || [1; 4],
            &mut audio
        ))
        .unwrap(),
        Status::Waiting
    );
    assert!(matches!(runner.pending_command(),Some(Cmd::MSGSET {msg_id,..}) if msg_id.0==2));
    assert_eq!(runner.read_state.as_ref().unwrap().timeline().boundary, 2);
    assert_eq!((runner.next_position(), runner.random_state()), vm_before);
    assert!(
        runner
            .begin_forward_search(&result.memory, &result.progress, Kind::EventEnd, true)
            .unwrap()
            .is_none()
    );
    eprintln!(
        "Verified real message resource forward-search/replay/publication pipeline, source retention and temporary variable commit isolation"
    );
}
