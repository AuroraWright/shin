use super::*;
use crate::format::{
    scenario::{
        instruction_elements::RegisterRepr,
        types::{U8SmallList, U8SmallNumberList},
    },
    text::utf8::U16Utf8String,
};
use binrw::BinWrite;
use std::io::{Cursor, Read, Write};

fn reg(index: u16) -> Register {
    Register::from_regular_register(index)
}

fn prefix() -> Vec<u8> {
    let mut data = b"SNR ".to_vec();
    for word in [0u32, 0, 1, 1, 0, 0, 0, 0] {
        data.extend(word.to_le_bytes());
    }
    let mut tables = Vec::new();
    for index in 0..10 {
        data.extend((0x4c + tables.len() as u32).to_le_bytes());
        if index < 7 {
            tables.extend(4u32.to_le_bytes());
        }
        tables.extend(0u32.to_le_bytes());
    }
    data.extend(tables);
    let start = data.len() as u32;
    data[32..36].copy_from_slice(&start.to_le_bytes());
    data
}

fn finish(mut data: Vec<u8>) -> Arc<Gerokasu2Scenario> {
    let size = data.len() as u32;
    data[4..8].copy_from_slice(&size.to_le_bytes());
    Arc::new(Gerokasu2Scenario::new(data.into()).unwrap())
}

fn emit<T>(data: &mut Vec<u8>, value: T)
where
    for<'a> T: BinWrite<Args<'a> = ()>,
{
    let mut out = Cursor::new(Vec::new());
    value.write_le(&mut out).unwrap();
    data.extend(out.into_inner());
}

fn commands() -> Arc<Gerokasu2Scenario> {
    let mut data = prefix();
    data.extend([0x81, 31, 0, 7]);
    data.extend([0x85, 0]);
    data.extend([0x86, 1, 0, 0, 1]);
    emit(&mut data, NumberSpec::<i32>::register(reg(31)));
    emit(&mut data, U16Utf8String::new("日本@rかわいい😀"));
    data.extend([0x83, 255]);
    emit(&mut data, NumberSpec::<i32>::register(reg(1)));
    data.push(0x8b);
    emit(&mut data, NumberSpec::<i32>::register(reg(31)));
    data.extend([0, 0, 0]);
    finish(data)
}

fn gosubs(depth: usize) -> Arc<Gerokasu2Scenario> {
    let mut data = prefix();
    let mut patch = data.len() + 1;
    data.extend([0x48, 0, 0, 0, 0]);
    data.extend([0, 0, 0]);
    for index in 0..depth {
        let target = data.len() as u32;
        data[patch..patch + 4].copy_from_slice(&target.to_le_bytes());
        if index + 1 < depth {
            patch = data.len() + 1;
            data.extend([0x48, 0, 0, 0, 0]);
        } else {
            data.extend([0x81, 31, 0, 7]);
        }
        data.push(0x49);
    }
    finish(data)
}

fn calls(depth: usize) -> Arc<Gerokasu2Scenario> {
    let mut data = prefix();
    let mut patch = data.len() + 1;
    data.extend([0x4f, 0, 0, 0, 0, 15]);
    for value in 0..15 {
        emit(&mut data, NumberSpec::<i32>::constant(value));
    }
    data.extend([0, 0, 0]);
    for index in 0..depth {
        let target = data.len() as u32;
        data[patch..patch + 4].copy_from_slice(&target.to_le_bytes());
        if index + 1 < depth {
            patch = data.len() + 1;
            data.extend([0x4f, 0, 0, 0, 0, 15]);
            for argument in 0..15 {
                emit(
                    &mut data,
                    NumberSpec::<i32>::register(Register::from_argument(argument)),
                );
            }
        } else {
            data.extend([0x81, 0, 0x10, 7]);
        }
        data.push(0x50);
    }
    finish(data)
}

fn pushed() -> Arc<Gerokasu2Scenario> {
    let mut data = prefix();
    emit(
        &mut data,
        Instruction::push {
            values: U8SmallNumberList::from_contents([
                NumberSpec::register(reg(31)),
                NumberSpec::register(reg(63)),
                NumberSpec::register(reg(1023)),
            ]),
        },
    );
    data.extend([0x81, 31, 0, 7]);
    emit(
        &mut data,
        Instruction::pop {
            dest: U8SmallList::from_contents([reg(1023), reg(2047), reg(63)]),
        },
    );
    data.extend([0, 0, 0]);
    finish(data)
}

fn prepared(scenario: Arc<Gerokasu2Scenario>, seed: u32) -> Gerokasu2Scripter {
    let mut vm = Gerokasu2Scripter::new(scenario, i32::MIN, seed);
    for (index, value) in initial() {
        vm.ctx.write_register(reg(index), value);
    }
    vm
}

fn initial() -> [(u16, i32); 5] {
    [
        (1, -3000),
        (31, i32::MIN),
        (63, i32::MAX),
        (1023, 91),
        (2047, -73),
    ]
}

fn numeric_snapshot(vm: &Gerokasu2Scripter) -> Vec<u32> {
    let stack = vm.ctx.reference_stack_snapshot();
    let depth = stack[0] as usize;
    let top = stack[1 + depth] as u32;
    let mut words = vec![
        u32::from(vm.scenario.raw()[vm.position().0 as usize]),
        vm.next_position().0,
        vm.random_state(),
        vm.position().0,
        vm.previous_random_state(),
        depth as u32,
        top,
    ];
    words.extend((0..2048).map(|index| vm.read_register(reg(index)) as u32));
    words.extend(stack[1..1 + depth].iter().map(|&word| word as u32));
    words.extend(stack[2 + depth..].iter().map(|&word| word as u32));
    words
}

fn feedback(vm: &Gerokasu2Scripter) -> CommandResult {
    vm.pending_destination()
        .map(|dest| CommandResult::WriteMemory(dest, 99))
        .unwrap_or(CommandResult::None)
}

#[test]
fn ger_replay_checkpoint_validity_feedback_and_immutable_snapshot() {
    let scenario = commands();
    let mut vm = prepared(scenario.clone(), 42);
    assert!(vm.checkpoint_for_replay().is_err());
    let command = vm.run(CommandResult::None, 1000).unwrap();
    let before = numeric_snapshot(&vm);
    let saved = vm.checkpoint_for_replay().unwrap();
    assert!(saved.matches_scenario(&scenario));
    assert!(!saved.matches_scenario(&gosubs(1)));
    assert_eq!(saved.position(), vm.position());
    assert_eq!(saved.random_state(), vm.previous_random_state());
    assert_eq!(numeric_snapshot(&vm), before);
    let mut restored = saved.scripter();
    assert_eq!(restored.next_position(), saved.position());
    assert_eq!(restored.pending_destination(), None);
    assert!(!restored.is_halted());
    assert!(restored.checkpoint_for_replay().is_err());
    assert_eq!(
        format!("{:?}", restored.run(CommandResult::None, 1000).unwrap()),
        format!("{command:?}")
    );
    assert_eq!(numeric_snapshot(&restored), before);
    assert!(vm.write_pending_result(reg(30), 73).is_err());
    assert_eq!(numeric_snapshot(&vm), before);
    vm.write_pending_result(reg(31), 99).unwrap();
    assert_eq!(vm.pending_destination(), Some(reg(31)));
    assert_eq!(vm.read_register(reg(31)), 99);
    assert!(vm.run(CommandResult::None, 1000).is_err());
    assert!(vm.checkpoint_for_replay().is_ok());
    let left = vm.run(feedback(&vm), 1000).unwrap();
    let right = restored.run(feedback(&restored), 1000).unwrap();
    assert_eq!(format!("{left:?}"), format!("{right:?}"));
    assert_eq!(numeric_snapshot(&vm), numeric_snapshot(&restored));
    let mut again = saved.scripter();
    again.run(CommandResult::None, 1000).unwrap();
    assert_eq!(numeric_snapshot(&again), before);

    let mut data = prefix();
    let entry = data.len() as u32;
    emit(
        &mut data,
        Instruction::j {
            target: CodeAddress(entry),
        },
    );
    let mut looping = prepared(finish(data), 42);
    assert!(looping.run(CommandResult::None, 3).is_err());
    assert!(looping.checkpoint_for_replay().is_err());
    let mut malformed = prefix();
    malformed.extend([0x81]);
    let mut malformed = prepared(finish(malformed), 42);
    assert!(malformed.run(CommandResult::None, 3).is_err());
    assert!(malformed.checkpoint_for_replay().is_err());
}

#[test]
fn ger_replay_checkpoint_preserves_active_stacks_registers_and_replays_exit() {
    for scenario in [
        commands(),
        gosubs(1),
        gosubs(8),
        gosubs(64),
        calls(1),
        calls(3),
        calls(16),
        pushed(),
    ] {
        for seed in [0, 42, 0xdead_beef, u32::MAX] {
            let mut vm = prepared(scenario.clone(), seed);
            let mut result = CommandResult::None;
            for _ in 0..10 {
                let command = vm.run(result, 1000).unwrap();
                let before = numeric_snapshot(&vm);
                let saved = Arc::new(vm.checkpoint_for_replay().unwrap());
                let mut restored = saved.scripter();
                assert_eq!(
                    format!("{:?}", restored.run(CommandResult::None, 1000).unwrap()),
                    format!("{command:?}")
                );
                assert_eq!(numeric_snapshot(&restored), before);
                assert_eq!(restored.is_halted(), vm.is_halted());
                assert_eq!(restored.pending_destination(), vm.pending_destination());
                if vm.is_halted() {
                    break;
                }
                result = feedback(&vm);
            }
            assert!(vm.is_halted());
        }
    }
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "original full EEC0 numeric capture and interpreter re-decoding; CPU only"]
fn ger_native_replay_checkpoints_registers_stacks_pc_seed_and_command_redecode() {
    use std::{
        fs,
        process::{Command, Stdio},
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let folder = root.join("gerokasu2/.local/checkpoint-vm");
    fs::create_dir_all(&folder).unwrap();
    let binary = folder.join("native-checkpoint-vm");
    let compile = Command::new("clang")
        .args(["-std=c11", "-O2"])
        .arg(root.join("gerokasu2/native-checkpoint-vm.c"))
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let mut child = Command::new(binary)
        .arg(root.join("gerokasu2/uncompressed_executable"))
        .arg(root.join("gerokasu2/.local/newrodin-medium.fnt"))
        .arg(root.join("gerokasu2/.local/system.fnt"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = child.stdout.take().unwrap();
    fn words(input: &mut impl Write, row: &[u32]) {
        for value in row {
            input.write_all(&value.to_le_bytes()).unwrap();
        }
        input.flush().unwrap();
    }
    fn compare(output: &mut impl Read, vm: &Gerokasu2Scripter) {
        let mut header = [0; 32];
        output.read_exact(&mut header).unwrap();
        assert_eq!(&header[..4], b"GCP1");
        let depth = u32::from_le_bytes(header[24..28].try_into().unwrap()) as usize;
        let top = u32::from_le_bytes(header[28..32].try_into().unwrap()) as usize;
        assert!(depth <= 64 && top <= 256);
        let mut all = vec![0; (2048 + depth + 256 - top) * 4];
        output.read_exact(&mut all).unwrap();
        let actual: Vec<u32> = header[4..]
            .chunks_exact(4)
            .chain(all.chunks_exact(4))
            .map(|value| u32::from_le_bytes(value.try_into().unwrap()))
            .collect();
        assert_eq!(actual, numeric_snapshot(vm));
    }
    let mut snapshots = 0;
    let mut programs = 0;
    for scenario in [
        commands(),
        gosubs(1),
        gosubs(8),
        gosubs(64),
        calls(1),
        calls(3),
        calls(16),
        pushed(),
    ] {
        for seed in [0, 42, 0xdead_beef, u32::MAX] {
            let mut vm = prepared(scenario.clone(), seed);
            words(
                &mut input,
                &[
                    0,
                    scenario.raw().len() as u32,
                    scenario.header.code_offset,
                    seed,
                    i32::MIN as u32,
                    initial().len() as u32,
                ],
            );
            input.write_all(scenario.raw()).unwrap();
            for (reg, value) in initial() {
                input.write_all(&reg.to_le_bytes()).unwrap();
                input.write_all(&value.to_le_bytes()).unwrap();
            }
            input.flush().unwrap();
            let mut ack = [0; 4];
            output.read_exact(&mut ack).unwrap();
            assert_eq!(&ack, b"ACK1");
            let mut result = CommandResult::None;
            for _ in 0..10 {
                let native_feedback = match result {
                    CommandResult::None => [0, 0, 0],
                    CommandResult::WriteMemory(dest, value) => {
                        let raw = match dest.repr() {
                            RegisterRepr::Regular(index) => index,
                            RegisterRepr::Argument(index) => 0x1000 + index,
                        };
                        [1, u32::from(raw), value as u32]
                    }
                };
                words(
                    &mut input,
                    &[
                        1,
                        native_feedback[0],
                        native_feedback[1],
                        native_feedback[2],
                    ],
                );
                let command = vm.run(result, 1000).unwrap();
                compare(&mut output, &vm);
                snapshots += 1;
                let saved = vm.checkpoint_for_replay().unwrap();
                let mut restored = saved.scripter();
                words(&mut input, &[2]);
                assert_eq!(
                    format!("{:?}", restored.run(CommandResult::None, 1000).unwrap()),
                    format!("{command:?}")
                );
                compare(&mut output, &restored);
                snapshots += 1;
                assert_eq!(numeric_snapshot(&vm), numeric_snapshot(&restored));
                if vm.is_halted() {
                    break;
                }
                result = feedback(&vm);
            }
            assert!(vm.is_halted());
            programs += 1;
        }
    }
    drop(input);
    assert!(child.wait().unwrap().success());
    println!(
        "GEROKASU2_CHECKPOINT_VM programs={programs} snapshots={snapshots} registers=2048 call_stack=64 argument_bank=256 source_immutable_PC_preseed_and_EXIT_redecode=verified native_numeric_restore=host_copy"
    );
}
