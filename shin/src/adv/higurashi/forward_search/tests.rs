use super::super::replay::{
    Status as ReplayStatus,
    tests::{scenario, session, snapshot},
};
use super::*;
use shin_core::vm::higurashi::command::HigurashiRuntimeCommand as Cmd;
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    })
}
fn messages() -> Vec<u8> {
    vec![
        0x86, 1, 0, 0, 1, 0x7f, 2, 0, b'A', 0, 0x86, 2, 0, 0, 1, 0x7f, 2, 0, b'B', 0, 0x86, 3, 0,
        0, 1, 0x7f, 2, 0, b'C', 0,
    ]
}
#[test]
#[ignore = "original1155c/11664 searches, both poll paths, clone/prune and reconstructed result; headless"]
fn higurashi_forward_memory_search_matches_native_read_policy_and_event_end() {
    let msg = messages();
    let choice = vec![
        0x8d, 0, 0, 0, 0, 31, 0, 7, 1, 0, 7, b'A', 0, b'B', 0, b'C', 0, 0,
    ];
    let mut variables = msg[..10].to_vec();
    variables.extend([0x82, 2, 1, 0xdb, 1, 2, 0x81, 31, 0, 2, 0xda, 31, 0, 1]);
    variables.extend(&msg[10..]);
    variables.extend([0, 0, 0]);
    let mut chapter = msg[..20].to_vec();
    chapter.extend([0xa1]);
    chapter.extend(&msg[20..]);
    chapter.extend([0xa1, 0, 0, 0]);
    let mut selection = msg[..10].to_vec();
    selection.extend(&choice);
    selection.extend(&msg[10..]);
    selection.extend([0, 0, 0]);
    let mut resumed = choice.clone();
    resumed.extend(&msg);
    resumed.extend([0, 0, 0]);
    let mut event = vec![0xa2, 0];
    event.extend(&msg[..20]);
    event.extend([0xa3]);
    event.extend(&msg[20..]);
    event.extend([0, 0, 0]);
    let mut nested = vec![0xa2, 0];
    nested.extend(&msg[..10]);
    nested.extend([0xa2, 7]);
    nested.extend(&msg[10..20]);
    nested.extend([0xa3]);
    nested.extend(&msg[20..]);
    nested.extend([0, 0, 0]);
    let mut event_choice = vec![0xa2, 0];
    event_choice.extend(&msg[..10]);
    event_choice.extend(&choice);
    event_choice.extend([0xa3]);
    event_choice.extend(&msg[20..]);
    event_choice.extend([0, 0, 0]);
    let mut plain = msg.clone();
    plain.extend([0, 0, 0]);
    let mut sparse = Vec::new();
    for id in 1..=202u32 {
        sparse.extend([
            0x86,
            id as u8,
            (id >> 8) as u8,
            (id >> 16) as u8,
            1,
            0x7f,
            2,
            0,
            b'A',
            0,
        ]);
    }
    sparse.extend([0, 0, 0]);
    let programs = [
        vec![0, 0, 0],
        plain,
        variables,
        chapter,
        selection,
        resumed,
        event,
        nested,
        event_choice,
        sparse,
    ];
    compare_native(programs);
}

#[test]
#[ignore = "original1155c/11664 KAKERA ef94 barrier, preserved last text and reconstructed result;headless"]
fn higurashi_forward_search_stops_at_kakera_without_consuming_responses_or_queueing_scene() {
    let msg = messages();
    let mut plain = msg[..10].to_vec();
    plain.extend([0xd5]);
    plain.extend(&msg[10..]);
    plain.extend([0, 0, 0]);
    let mut event = vec![0xa2, 0];
    event.extend(&msg[..10]);
    event.extend([0xd5]);
    event.extend(&msg[10..20]);
    event.extend([0xa3, 0, 0, 0]);
    compare_native([plain, event]);
}

fn compare_native(programs: impl IntoIterator<Item = Vec<u8>>) {
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut profiles = 0;
    for code in programs {
        let scenario = scenario(&code);
        let hex: String = code.iter().map(|b| format!("{b:02x}")).collect();
        for saved in [0, 1, 2, 3, 4, 99, 199, 200, 201, 400, u32::MAX] {
            for seed in [0, 42, u32::MAX] {
                for read_only in [false, true] {
                    for mask in [0u32, 1, 3, 7] {
                        let mut history = MessageReadHistory::new();
                        for bit in 0..8 {
                            if mask & (1 << bit) != 0 {
                                history.record_completion(true, bit);
                            }
                        }
                        let mut source = session(scenario.clone(), saved, saved, seed);
                        assert_eq!(
                            source.pump(&history, 10000, 10000, || false).unwrap(),
                            ReplayStatus::Ready
                        );
                        let before = snapshot(&source);
                        let before_progress = format!("{:?}", source.progress);
                        for kind in [Kind::ReadAhead, Kind::EventEnd] {
                            profiles += 1;
                            input.push_str(&format!(
                                "{} {saved} {seed} {} {mask} {hex}\n",
                                u8::from(kind == Kind::EventEnd),
                                u8::from(read_only)
                            ));
                            let mut result = None;
                            let mut decoded = 0;
                            if let Some(mut search) = ForwardSearch::begin(
                                scenario.clone(),
                                &source.memory,
                                &source.progress,
                                source.read.timeline().boundary,
                                source.read.state().flags,
                                kind,
                                read_only,
                            )
                            .unwrap()
                            {
                                loop {
                                    if search.pump(&history, 1, 10000, || false).unwrap()
                                        == Status::Complete
                                    {
                                        break;
                                    }
                                    assert!(search.temporary.completed_commands < 10000);
                                }
                                decoded = search.temporary.completed_commands
                                    + u64::from(search.temporary.pending.is_some());
                                assert!(!search.temporary.memory.commits_enabled);
                                result = search.take_result().unwrap();
                            }
                            assert_eq!(snapshot(&source), before);
                            assert_eq!(format!("{:?}", source.progress), before_progress);
                            expected.push(format!(
                                "S {} {} {decoded} {}",
                                source.read.timeline().boundary,
                                u8::from(result.is_some()),
                                if result.is_some() { 2 } else { 1 }
                            ));
                            if let Some(mut result) = result {
                                assert!(result.memory.commits_enabled);
                                expected.push(snapshot(&result));
                                expected.push(format!(
                                    "V {:016x}",
                                    hash(result.progress.responses().0)
                                ));
                                assert_eq!(
                                    result.pump(&history, 10000, 10000, || false).unwrap(),
                                    ReplayStatus::Ready
                                );
                                expected.push(snapshot(&result));
                                expected.push(format!(
                                    "V {:016x}",
                                    hash(result.progress.responses().0)
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_FORWARD_SEARCH").unwrap())
        .arg(std::env::var("SHIN_HIGURASHI_EXE").unwrap())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let actual = String::from_utf8(output.stdout).unwrap();
    let actual: Vec<_> = actual.lines().collect();
    assert_eq!(actual.len(), expected.len());
    for (i, (a, e)) in actual.iter().zip(&expected).enumerate() {
        assert_eq!(*a, e, "forward search output row {i}");
    }
    eprintln!(
        "Verified {profiles} native forward searches and their reconstructed results with source/commit/response ownership"
    );
}

#[test]
#[ignore = "headless cancellation/fault ownership and required forward-search budgets with actual scenario metadata"]
fn higurashi_forward_search_cancellation_and_fault_cannot_publish_temporary_memory() {
    let scenario = scenario(&[0xd1, 0, 0, 31, 0, 7, 0, 0, 0]);
    let source = session(scenario.clone(), 0, 0, 42);
    let begin = || {
        ForwardSearch::begin(
            scenario.clone(),
            &source.memory,
            &source.progress,
            0,
            u32::MAX,
            Kind::ReadAhead,
            true,
        )
        .unwrap()
        .unwrap()
    };
    let mut cancelled = begin();
    assert_eq!(
        cancelled
            .pump(&MessageReadHistory::new(), 1, 100, || true)
            .unwrap(),
        Status::Cancelled
    );
    assert!(cancelled.take_result().is_err());
    let mut faulted = begin();
    assert!(
        faulted
            .pump(&MessageReadHistory::new(), 0, 100, || false)
            .is_err()
    );
    assert!(faulted.fault().is_none());
    assert!(
        faulted
            .pump(&MessageReadHistory::new(), 1, 100, || false)
            .is_err()
    );
    let fault = faulted.fault().unwrap().to_owned();
    assert!(
        faulted
            .pump(&MessageReadHistory::new(), 1, 100, || false)
            .is_err()
    );
    assert_eq!(faulted.fault(), Some(fault.as_str()));
    assert!(faulted.take_result().is_err());
    assert!(
        ForwardSearch::begin(
            scenario,
            &source.memory,
            &source.progress,
            0,
            u32::MAX,
            Kind::EventEnd,
            true
        )
        .unwrap()
        .is_none()
    );
    assert_eq!(source.progress.persistent().get(2), 0);
    assert!(matches!(source.pending_command(), None::<&Cmd>));
}
