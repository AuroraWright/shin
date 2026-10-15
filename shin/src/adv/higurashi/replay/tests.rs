use super::super::autosave::{ProgressEntry, SaveProgress};
use super::*;
use shin_core::format::{save::PersistData, scenario::instruction_elements::Register};
use std::{
    io::Write,
    process::{Command, Stdio},
};

pub(in crate::adv::higurashi) fn scenario(code: &[u8]) -> Arc<HigurashiScenario> {
    let path = std::env::var("SHIN_HIGURASHI_SCENARIO").unwrap();
    let mut raw = std::fs::read(path).unwrap();
    let start = u32::from_le_bytes(raw[32..36].try_into().unwrap()) as usize;
    raw[start..start + code.len()].copy_from_slice(code);
    Arc::new(HigurashiScenario::new(raw.into()).unwrap())
}
pub(in crate::adv::higurashi) fn session(
    scenario: Arc<HigurashiScenario>,
    saved: u32,
    target: u32,
    seed: u32,
) -> ReplaySession {
    let mut response_data = vec![0; 1024];
    response_data[0] = 0x23;
    let entry = ProgressEntry {
        scenario_id: 0,
        random_seed: seed,
        save_position: saved,
        replay_target: target,
        response_data,
    };
    let progress = ProgressMemory::new(
        PersistData(vec![0; 64]),
        SaveProgress::new(0, 0, [entry.clone(), entry]).unwrap(),
        vec![],
    );
    ReplaySession::new(
        scenario,
        CheckpointMemory::new(vec![])
            .fork_for_restore(saved, target, &progress)
            .unwrap(),
    )
    .unwrap()
}
fn poll_address(command: Option<&Cmd>) -> u64 {
    match command {
        None => 0,
        Some(Cmd::MSGSET { .. }) => 0x4ea8,
        Some(Cmd::AUTOSAVE { .. }) => 0x7f14,
        Some(Cmd::EVBEGIN { .. }) => 0x80d8,
        Some(Cmd::EVEND { .. }) => 0x810c,
        Some(Cmd::SELECT { .. }) => 0x5784,
        Some(Cmd::CHARSELECT { .. }) => 0xd810,
        Some(Cmd::KAKERA {}) => 0xef40,
        Some(Cmd::SNRSEL { .. }) => 0xee10,
        Some(Cmd::EXIT { arg1: 0, .. }) => 0x447c,
        Some(Cmd::EXIT { .. }) => 0x4460,
        _ => unreachable!(),
    }
}
pub(in crate::adv::higurashi) fn snapshot(r: &ReplaySession) -> String {
    let vm = r.vm.as_ref().unwrap();
    let start = r.scenario.header.code_offset;
    let decoded = r.completed_commands + r.exit_forks + u64::from(r.pending.is_some());
    let t = r.read.timeline();
    let state = r.read.state();
    let active = r.progress.progress().active_entry();
    let mut out = format!(
        "R {} {decoded} {} {} {} {} {} {:x}",
        u8::from(r.status == Status::Ready),
        vm.next_position().0 - start,
        vm.random_state(),
        vm.position().0 - start,
        vm.previous_random_state(),
        r.progress.responses().1,
        poll_address(r.pending_command())
    );
    for word in [
        t.boundary,
        t.previous_message,
        t.current_message,
        t.previous_chapter,
        t.current_chapter,
        t.resume_position,
        t.replay_target,
        t.normal_messages,
        t.quick_return_messages,
    ] {
        out.push_str(&format!(" {word}"));
    }
    out.push_str(&format!(
        " {} {} {} {} {} {} {}",
        state.fast_forward_blocked,
        state.is_read,
        state.flags,
        vm.read_register(Register::from_regular_register(31)) as u32,
        active.save_position,
        active.replay_target,
        r.memory.entries().len()
    ));
    for checkpoint in r.memory.entries() {
        out.push_str(&format!(" {}", checkpoint.boundary));
    }
    out.push_str(&format!(
        " {:016x} {}",
        super::super::layers::replay_snapshot(&r.layers),
        r.read.descriptors().music.id
    ));
    for effect in r.read.descriptors().effects {
        out.push_str(&format!(" {}", effect.id));
    }
    out.push_str(&format!(
        " {} {}",
        r.progress.persistent().get(2),
        r.progress.bytes().get(1).copied().unwrap_or(0)
    ));
    for queue in [r.progress.pending_persistent(), r.progress.pending_bytes()] {
        out.push_str(&format!(" {}", queue.len()));
        for (slot, value) in queue {
            out.push_str(&format!(" {slot} {value}"));
        }
    }
    out
}

#[test]
#[ignore = "updated native18db0/10fc4 replay, real constructors, checkpoints, responses and EXIT forks; headless"]
fn higurashi_memory_replay_matches_native_worker_and_retained_commands() {
    let messages = vec![
        0x86, 1, 0, 0, 1, 0x7f, 2, 0, b'A', 0, 0x86, 2, 0, 0, 1, 0x7f, 2, 0, b'B', 0, 0, 0, 0,
    ];
    let choice = vec![
        0x8d, 0, 0, 0, 0, 31, 0, 7, 1, 0, 7, b'A', 0, b'B', 0, b'C', 0, 0,
    ];
    let mut choice_after_text = messages[..10].to_vec();
    choice_after_text.extend(&choice);
    choice_after_text.extend([0xa1, 0, 0, 0]);
    let mut two_choices = choice.clone();
    two_choices.extend(&choice);
    two_choices.extend([0xa1, 0, 0, 0]);
    let mut character_after_text = messages[..10].to_vec();
    character_after_text.extend([0xd1, 18, 0, 31, 0, 7, 0x83, 1, 4, 0, 0, 0]);
    let mut two_characters = vec![0xd1, 18, 0, 31, 0, 7];
    two_characters.extend([0xd1, 33, 0, 31, 0, 5, 0x83, 1, 4, 0, 0, 0]);
    let programs = [
        vec![0, 0, 0],
        vec![0xa2, 0, 0xa3, 0xa1, 0, 0, 0],
        vec![0xa2, 0x7f, 0xa3, 0xa1, 0xa1, 0, 0, 0],
        messages,
        choice_after_text,
        two_choices,
        character_after_text,
        two_characters,
        vec![0xd5, 0xd5, 0x83, 0, 4, 0, 0, 0],
        vec![
            0x86, 1, 0, 0, 1, 0x7f, 2, 0, b'A', 0, 0xd5, 0x83, 0, 4, 0, 0, 0,
        ],
        // Tile/bank targets and WIPE have constructor effects but replay polls
        // complete without waiting for a drawable, animation or OS audio.
        vec![
            0xc1, 7, 1, 0, 0, 0xc5, 7, 0xc3, 7, 0, 1, 12, 0x8e, 0, 0, 0, 0, 0xa1, 0, 0, 0,
        ],
        vec![
            0x85, 2, 0x7f, 2, 0x90, 0, 12, 0, 0x83, 0xe8, 0x95, 2, 0x80, 48, 12, 0, 0x82, 0xbc, 0,
            0x83, 0xe8, 0xa1, 0, 0, 0,
        ],
        vec![0x81, 31, 0, 0, 0xa1, 0, 0, 0],
        vec![0xda, 31, 0, 0, 0xa1, 0, 0, 0],
        vec![
            0x82, 2, 1, 0xdb, 1, 2, 0xa1, 0x81, 31, 0, 2, 0xda, 31, 0, 1, 0xa1, 0, 0, 0,
        ],
    ];
    let mut input = String::new();
    let mut expected = Vec::new();
    for code in programs {
        let scenario = scenario(&code);
        let hex: String = code.iter().map(|b| format!("{b:02x}")).collect();
        for saved in [0, 1, 2, 3, 4, 7, 99, u32::MAX] {
            for target in [0, saved] {
                for seed in [0, 42, u32::MAX] {
                    for cancel in [0, 1, 2, 7] {
                        let mut r = session(scenario.clone(), saved, target, seed);
                        let mut calls = 0;
                        loop {
                            let decoded = r.completed_commands
                                + r.exit_forks
                                + u64::from(
                                    r.status == Status::CommandBudget || r.pending.is_some(),
                                );
                            let result = r
                            .pump(&MessageReadHistory::new(), 1, 10000, || {
                                cancel != 0 && decoded >= cancel
                            })
                            .unwrap_or_else(|e| {
                                panic!(
                                    "code={hex} saved={saved} seed={seed} cancel={cancel}: {e:#}"
                                )
                            });
                            calls += 1;
                            if result != Status::CommandBudget {
                                break;
                            }
                            assert!(calls < 10000);
                        }
                        expected.push(snapshot(&r));
                        input.push_str(&format!("{saved} {target} {seed} {cancel} {hex}\n"));
                    }
                }
            }
        }
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_REPLAY").unwrap())
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
        assert_eq!(*a, e, "native replay profile {i}");
    }
    eprintln!(
        "Verified {} native replay profiles including retained commands, cancellation, choice responses and EXIT forks",
        expected.len()
    );
}

#[test]
#[ignore = "headless authored replay constructor fault and cancellation ownership with real scenario metadata"]
fn higurashi_replay_faults_keep_partial_state_and_require_a_new_fork() {
    let authored = scenario(&[0xc9, 0, 0, 0, 0, 0, 0, 0, 0]); // FEELICON is not replay-supported yet.
    let mut r = session(authored, 9, 9, 42);
    assert!(
        r.pump(&MessageReadHistory::new(), 1, 100, || false)
            .is_err()
    );
    let fault = r.fault().unwrap().to_owned();
    let next = r.vm.as_ref().unwrap().next_position();
    assert!(
        r.pump(&MessageReadHistory::new(), 1, 100, || false)
            .is_err()
    );
    assert_eq!(r.fault(), Some(fault.as_str()));
    assert_eq!(r.vm.as_ref().unwrap().next_position(), next);
    assert!(r.check_publication(&r.scenario).is_err());
    let mut cancelled = session(scenario(&[0x83, 1, 4, 0xa1, 0, 0, 0]), 9, 9, 42);
    assert_eq!(
        cancelled
            .pump(&MessageReadHistory::new(), 1, 100, || true)
            .unwrap(),
        Status::Cancelled
    );
    assert!(cancelled.check_publication(&r.scenario).is_err());
}

#[test]
#[ignore = "authored SNRSEL replay through real scenario metadata;CPU only,no GPU/audio/window/disk"]
fn higurashi_scenario_select_replay_retains_stop_without_live_factory() {
    struct NoFactory;
    impl crate::adv::higurashi::scenario_select::ScenarioSelectServices for NoFactory {
        fn packed_time(&mut self) -> Result<u64> {
            panic!("A retained replay stop must not capture live time");
        }
        fn queue_scene(&mut self, _: i32, _: &ProgressMemory) -> Result<()> {
            panic!("A retained replay stop must not reopen ScenarioSelect");
        }
    }
    for kind in 0..6u8 {
        for saved in [1, 2] {
            let authored = scenario(&[0xd4, kind, 0xd4, kind, 0, 0, 0]);
            let mut r = session(authored, saved, saved, 42);
            let before = r.progress.progress().clone();
            for _ in 0..=saved {
                if r.pump(&MessageReadHistory::new(), 1, 100, || false).unwrap() == Status::Ready {
                    break;
                }
            }
            assert_eq!(r.status(), Status::Ready);
            assert!(r.fault().is_none());
            assert_eq!(r.read.timeline().boundary, saved);
            assert_eq!(r.completed_commands, u64::from(saved - 1));
            assert_eq!(r.progress.progress(), &before);
            assert_eq!(r.progress.responses().1, 0);
            assert!(!r.memory.entries().is_empty());
            assert_eq!(r.pending_command(), Some(&Cmd::SNRSEL {argument: i32::from(kind)}));
            let command = r.pending.as_mut().unwrap().scenario_select.as_mut().unwrap();
            assert_eq!(command.kind(), i32::from(kind));
            assert_eq!(command.replay_stop(), 0);
            assert!(!command.forward_complete());
            let mut autosave = super::super::autosave::AutosaveMemory {
                valid: false, packed_time: 0, progress: before.clone(),
            };
            assert!(command.poll_live(&mut r.progress, &mut autosave, saved, &mut NoFactory).unwrap());
            assert!(!autosave.valid);
            assert_eq!(r.progress.progress(), &before);
            assert_eq!(r.progress.responses().1, 0);
        }
    }
}

#[test]
#[ignore = "native18db0/10fc4 reconstruction with nonzero EXIT as NOPE;checks forward polls and zero-mode barrier without UI"]
fn higurashi_nonzero_exit_replay_matches_native_nope_and_forward_completion() {
    let mut input = String::new();
    let mut expected = Vec::new();
    for mode in [1, 2, 255] {
        let mut code = vec![];
        for id in 1..=3 {
            code.extend([0, mode, 0x7f, 0x82, 2, 7]);
            code.extend([0x86, id, 0, 0, 1, 0x7f, 2, 0, b'A', 0]);
        }
        code.extend([0, 0, 0]);
        let hex: String = code.iter().map(|b| format!("{b:02x}")).collect();
        for saved in 1..=3 {
            for seed in [0, 42, u32::MAX] {
                for cancel in [0, 1, 4, 7] {
                    let mut r = session(scenario(&code), saved, saved, seed);
                    let mut queries = 0;
                    loop {
                        let result = r
                            .pump(&MessageReadHistory::new(), 1, 10000, || {
                                queries += 1;
                                cancel != 0 && queries >= cancel
                            })
                            .unwrap();
                        if result != Status::CommandBudget {
                            break;
                        }
                    }
                    expected.push(snapshot(&r));
                    input += &format!("{saved} {saved} {seed} {cancel} {hex}\n");
                }
            }
        }
        for read_only in [false, true] {
            let mut r = session(scenario(&code), 3, 3, 42);
            r.prepare_command(&MessageReadHistory::new(), 10000)
                .unwrap();
            assert!(matches!(r.pending_command(), Some(Cmd::EXIT { arg1, .. }) if *arg1 == mode));
            assert!(r.poll_command(Some(read_only)).unwrap());
            assert_eq!(r.read.state().fast_forward_blocked, 0);
            r.retire_command();
            assert_eq!(r.exit_forks, 0);
        }
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_REPLAY").unwrap())
        .arg(std::env::var("SHIN_HIGURASHI_EXE").unwrap())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert_eq!(text.lines().count(), expected.len());
    for (i, (a, e)) in text.lines().zip(&expected).enumerate() {
        assert_eq!(a, e, "nonzero EXIT native replay profile{i}");
    }
    println!(
        "Nonzero EXIT:108 complete native replay profiles and6 direct forward polls match NOPE completion;zero EXIT barrier and response/checkpoint/memory state remain distinct"
    );
}
