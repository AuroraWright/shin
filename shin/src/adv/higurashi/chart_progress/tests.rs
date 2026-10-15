use super::*;
use crate::adv::higurashi::{
    autosave::{ProgressEntry, SaveProgress},
    checkpoints::{CheckpointMemory, CheckpointServices},
    progress::ProgressMemory,
    read_history::MessageReadHistory,
    read_state::CommandReadState,
};
use shin_core::{
    format::save::PersistData, vm::higurashi::command::HigurashiRuntimeCommand as Cmd,
};
use std::{
    io::Write,
    process::{Command, Stdio},
};
fn progress() -> ProgressMemory {
    let entry = ProgressEntry {
        scenario_id: 0,
        random_seed: 42,
        save_position: 0,
        replay_target: 0,
        response_data: Vec::new(),
    };
    ProgressMemory::new(
        PersistData::new(),
        SaveProgress::new(0, 0, [entry.clone(), entry]).unwrap(),
        Vec::new(),
    )
}
fn snapshot(result: u32, memory: &CheckpointMemory, progress: &ProgressMemory) -> Vec<u32> {
    let words = progress.chart_unlocks.saved_words();
    let mut out = vec![result, memory.extra_words.len() as u32];
    out.extend(&memory.extra_words);
    out.push(words.len() as u32);
    out.extend(words);
    out
}
fn native(tool: &str, font: bool, input: String) -> String {
    let mut command = Command::new(std::env::var(tool).unwrap());
    command.arg("higurashi/uncompressed_executable_update");
    if font {
        command.arg("/tmp/higurashi-newrodin.fnt");
    }
    let mut child = command
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
    eprintln!("{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap()
}
#[test]
#[ignore = "original ec54 CHART ctor/edf4/edfc/ee04 polls,physical bit-word vectors and ordered node copies"]
fn higurashi_chart_constructor_matches_native_nodes_unlocks_and_constant_polls() {
    let full = (0..32)
        .map(|i| [0, 31, 32, 8191, 8192, 16383, 16384, 65535][i % 8])
        .collect::<Vec<i32>>();
    let cases: Vec<(u8, Vec<i32>)> = vec![
        (0, vec![]),
        (1, vec![]),
        (0, vec![0]),
        (0, vec![0, 31, 32, 8191, 8192, 8192]),
        (1, vec![65535, 8192]),
        (2, vec![1, 1, 16384]),
        (255, full.clone()),
        (0, full),
    ];
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut owners = 0;
    for length in [0, 1, 3, 255, 256, 257, 511, 512, 513] {
        for nodes in [vec![], vec![u32::MAX, 3, 3, 8192, 0]] {
            for seed in [0u32, u32::MAX] {
                owners += 1;
                let words = (0..length)
                    .map(|i| seed.wrapping_add(i as u32 * 17))
                    .collect::<Vec<_>>();
                input.push_str(&format!("N {} {length}", nodes.len()));
                for v in nodes.iter().chain(&words) {
                    input.push_str(&format!(" {v}"));
                }
                input.push('\n');
                let mut memory = CheckpointMemory::new(nodes.clone());
                let mut progress =
                    progress().with_chart_unlocks(ChartUnlocks::from_saved_words(words));
                let mut history = MessageReadHistory::new();
                let mut read = CommandReadState::new();
                expected.push(snapshot(0, &memory, &progress));
                for (mode, ids) in &cases {
                    input.push_str(&format!("C {mode} {}", ids.len()));
                    for id in ids {
                        input.push_str(&format!(" {id}"));
                    }
                    input.push('\n');
                    read.prepare(
                        &Cmd::CHART {
                            mode: *mode,
                            ids: ids.iter().copied().collect(),
                        },
                        &mut CheckpointServices {
                            memory: &mut memory,
                            progress: &mut progress,
                            history: &mut history,
                        },
                    )
                    .unwrap();
                    assert_eq!(read.timeline().boundary, 0);
                    assert!(memory.entries().is_empty());
                    expected.push(snapshot(0, &memory, &progress));
                    for op in ['R', 'L', 'G'] {
                        input.push_str(&format!("{op}\n"));
                        expected.push(snapshot(1, &memory, &progress));
                    }
                }
                input.push_str("D\n");
            }
        }
    }
    let output = native("SHIN_HIGURASHI_NATIVE_CHART_PROGRESS", true, input);
    let actual = output
        .lines()
        .map(|l| {
            l.strip_prefix("T ")
                .unwrap()
                .split_whitespace()
                .map(|v| v.parse::<u32>().unwrap())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(actual.len(), expected.len());
    let mut count = 0;
    for (i, (a, e)) in actual.iter().zip(&expected).enumerate() {
        assert_eq!(a, e, "native CHART state{i}");
        count += e.len();
    }
    eprintln!(
        "CHART:{owners} native owners/{} constructors/{} observations/{count} exact words;all4 modes,empty/duplicate/32-ID lists,loaded physical vector tails,8192-bit block boundaries,node order/no deduplication/unsigned existing values and constant replay/live polls pass",
        owners * cases.len(),
        expected.len()
    );
}
pub(in crate::adv::higurashi) fn chart(code: &mut Vec<u8>, mode: u8, ids: &[i32]) {
    use binrw::BinWrite;
    use shin_core::format::scenario::instruction_elements::UntypedNumberSpec;
    code.extend([0xd3, mode, ids.len() as u8]);
    for &id in ids {
        let mut output = std::io::Cursor::new(Vec::new());
        UntypedNumberSpec::Constant(id)
            .write_le(&mut output)
            .unwrap();
        code.extend(output.into_inner());
    }
}
#[test]
#[ignore = "full original replay loop/constructor/cancellation/checkpoint/EXIT reforks plus node/global unlock vectors"]
fn higurashi_chart_replay_preserves_native_constructor_checkpoints_and_global_unlocks() {
    use crate::adv::higurashi::replay::{
        Status,
        tests::{scenario, session, snapshot},
    };
    let mut first = Vec::new();
    chart(&mut first, 0, &[0, 0, 31, 32, 8192]);
    chart(&mut first, 1, &[1, 8192, 65535]);
    first.push(0xa1);
    chart(&mut first, 0, &[8192]);
    first.extend([0xa1, 0, 0, 0]);
    let mut event = Vec::new();
    chart(&mut event, 0, &[]);
    chart(&mut event, 2, &[32]);
    event.extend([0xa2, 0]);
    chart(&mut event, 0, &[1, 1]);
    event.extend([0xa3, 0xa1, 0, 0, 0]);
    let mut checkpoint = Vec::new();
    chart(&mut checkpoint, 0, &[3]);
    checkpoint.extend(std::iter::repeat_n(0xa1, 200));
    chart(&mut checkpoint, 0, &[32, 32]);
    checkpoint.push(0xa1);
    chart(&mut checkpoint, 1, &[65535]);
    checkpoint.extend([0xa1, 0, 0, 0]);
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut nodes_count = 0;
    let mut persistent_count = 0;
    for code in [first, event, checkpoint] {
        let script = scenario(&code);
        let hex = code.iter().map(|b| format!("{b:02x}")).collect::<String>();
        for saved in [0, 1, 2, 3, 200, 201, 203, u32::MAX] {
            for target in [0, saved] {
                for seed in [0, 42, u32::MAX] {
                    for cancel in [0, 1, 2, 7, 204] {
                        let mut replay = session(script.clone(), saved, target, seed);
                        // An external reference observes native global constructor
                        // effects even if a worker is subsequently cancelled.
                        let shared = replay.progress.chart_unlocks.clone();
                        loop {
                            let decoded = replay.completed_commands
                                + replay.exit_forks
                                + u64::from(
                                    replay.status() == Status::CommandBudget
                                        || replay.pending_command().is_some(),
                                );
                            let status = replay
                                .pump(&MessageReadHistory::new(), 1, 10000, || {
                                    cancel != 0 && decoded >= cancel
                                })
                                .unwrap();
                            if status != Status::CommandBudget {
                                break;
                            }
                            assert!(replay.completed_commands < 10000);
                        }
                        let words = replay.progress.chart_unlocks.saved_words();
                        assert_eq!(shared.saved_words(), words);
                        let mut out = snapshot(&replay);
                        out.push_str(" X");
                        let mut vectors = vec![replay.memory.extra_words.clone(), words.clone()];
                        for v in &vectors {
                            out.push_str(&format!(" {}", v.len()));
                            for w in v {
                                out.push_str(&format!(" {w}"));
                            }
                        }
                        out.push_str(&format!(" {}", replay.memory.entries().len()));
                        vectors.clear();
                        for cp in replay.memory.entries() {
                            let v = &cp.state.extra_words;
                            out.push_str(&format!(" {}", v.len()));
                            for w in v {
                                out.push_str(&format!(" {w}"));
                            }
                        }
                        nodes_count += replay.memory.extra_words.len();
                        persistent_count += words.len();
                        expected.push(out);
                        input.push_str(&format!("{saved} {target} {seed} {cancel} {hex}\n"));
                    }
                }
            }
        }
    }
    let output = native("SHIN_HIGURASHI_NATIVE_CHART_REPLAY", false, input);
    assert_eq!(output.lines().count(), expected.len());
    let mut count = 0;
    for (i, (a, e)) in output.lines().zip(&expected).enumerate() {
        assert_eq!(a, e, "native chart replay profile{i}");
        count += e.split_whitespace().count();
    }
    eprintln!(
        "CHART replay:{} original profiles/{count} exact words/{nodes_count} final node IDs/{persistent_count} physical unlock words;constructor-before-poll,source-shared unlocks,checkpointed nodes,200-boundary captures/EXIT reforks,cancellation and retained AUTOSAVE/event stops pass",
        expected.len()
    );
}
