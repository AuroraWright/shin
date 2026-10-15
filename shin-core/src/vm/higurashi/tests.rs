use super::*;
use crate::format::scenario::{
    higurashi::{HigurashiVariant, bytecode::HigurashiCommand},
    instruction_elements::{BitmaskNumberArray, NumberSpec},
};
use binrw::BinRead;
use bytes::Bytes;
use std::io::Cursor;

mod checkpoint_tests;

fn vm(code: Vec<u8>, seed: u32, values: [i32; 4]) -> HigurashiScripter {
    let mut ctx = VmCtx::new(values[0], seed);
    for (reg, value) in [1, 17, 18].into_iter().zip(values[1..].iter().copied()) {
        ctx.write_register(Register::from_regular_register(reg), value);
    }
    HigurashiScripter {
        ctx,
        reader: HigurashiInstructionReader::new(
            Bytes::from(code),
            HigurashiVariant::Updated,
            0,
            CodeAddress(0),
        )
        .unwrap(),
        position: CodeAddress(0),
        previous_random_state: seed,
        pending_destination: None,
        halted: false,
    }
}

#[test]
fn older_not_and_random_semantics_preserve_umineko_rules() {
    let mut script = vm(
        vec![
            0x40, 0x81, 31, 0, 0xb1, 0x4c, 32, 0, 0xb1, 0xc0, 17, 0, 0, 0,
        ],
        42,
        [0, -1, i32::MAX, 0],
    );
    assert!(matches!(
        script.run(CommandResult::None, 100).unwrap(),
        HigurashiRuntimeCommand::EXIT { arg1: 0, .. }
    ));
    assert_eq!(
        script.read_register(Register::from_regular_register(31)),
        -65536
    );
    // Width is 0x80000001 and the multiply wraps before the 16-bit shift.
    let sample = (script.previous_random_state() >> 8) & 0xffff;
    assert_eq!(
        script.read_register(Register::from_regular_register(32)),
        (-1i32).wrapping_add((sample.wrapping_mul(0x80000001) >> 16) as i32)
    );
    assert_eq!(
        VmCtx::new(0, 0).evaluate_unary_operation(UnaryOperationType::Not16, -1),
        0
    );
    assert!(script.run(CommandResult::None, 1).is_err());
}

#[test]
fn command_feedback_is_explicit_and_rejects_wrong_destinations() {
    let mut script = vm(
        vec![0x81, 31, 0, 7, 0x85, 0xc0, 31, 1, 0x7f, 0, 0, 0],
        0,
        [0; 4],
    );
    let dest = Register::from_regular_register(31);
    assert_eq!(
        script.run(CommandResult::None, 100).unwrap(),
        HigurashiRuntimeCommand::SGET { dest, slot: 7 }
    );
    let state = (script.random_state(), script.next_position());
    assert!(script.run(CommandResult::None, 100).is_err());
    assert!(
        script
            .run(
                CommandResult::WriteMemory(Register::from_regular_register(30), 8),
                100
            )
            .is_err()
    );
    assert_eq!(state, (script.random_state(), script.next_position()));
    assert_eq!(
        script
            .run(CommandResult::WriteMemory(dest, 8), 100)
            .unwrap(),
        HigurashiRuntimeCommand::MSGINIT {
            style: 8,
            updated_options: Some([1, -1])
        }
    );
    assert!(matches!(
        script.run(CommandResult::None, 100).unwrap(),
        HigurashiRuntimeCommand::EXIT { .. }
    ));
}

#[test]
fn lowering_preserves_sparse_arrays_lists_and_message_operands() {
    let mut ctx = VmCtx::new(77, 0);
    ctx.write_register(Register::from_regular_register(1), -123);
    let params = BitmaskNumberArray::read_le(&mut Cursor::new([0x85, 0xb0, 0xb1, 0x7f])).unwrap();
    assert_eq!(
        HigurashiCommand::LAYERLOAD {
            args: [NumberSpec::constant(0); 3],
            params
        }
        .into_runtime_form(&ctx),
        HigurashiRuntimeCommand::LAYERLOAD {
            args: [0; 3],
            params: (77, 0, -123, 0, 0, 0, 0, -1)
        }
    );
    let command = HigurashiCommand::read_le_args(
        &mut Cursor::new([0x86, 1, 0, 0, 1, 0xb1, 3, 0, b'A', b'B', 0]),
        (HigurashiVariant::Updated,),
    )
    .unwrap();
    assert_eq!(
        command.into_runtime_form(&ctx),
        HigurashiRuntimeCommand::MSGSET {
            msg_id: crate::format::scenario::instruction_elements::MessageId(1),
            auto_wait: 1,
            argument: -123,
            text: "AB".into()
        }
    );
    let command = HigurashiCommand::TIPSGET {
        ids: [
            NumberSpec::register(Register::from_regular_register(0)),
            NumberSpec::constant(-1),
        ]
        .into_iter()
        .collect(),
    };
    assert_eq!(
        command.into_runtime_form(&ctx),
        HigurashiRuntimeCommand::TIPSGET {
            ids: [77, -1].into_iter().collect()
        }
    );
}

#[test]
fn bounded_execution_and_stack_errors_are_reported() {
    let mut script = vm(vec![0x47, 0, 0, 0, 0], 0, [0; 4]);
    assert!(
        script
            .run(CommandResult::None, 4)
            .unwrap_err()
            .to_string()
            .contains("budget exhausted")
    );
    assert_eq!(
        script.random_state(),
        (0..4).fold(0u32, |v, _| v.wrapping_mul(0x343fd).wrapping_add(0x269ec3))
    );
    for code in [vec![0x49], vec![0x50], vec![0x4e, 1, 31, 0]] {
        let mut script = vm(code, 0, [0; 4]);
        assert!(
            format!("{:#}", script.run(CommandResult::None, 10).unwrap_err()).contains("underflow")
        );
    }
    let mut script = vm(vec![0x48, 0, 0, 0, 0], 0, [0; 4]);
    assert!(
        format!("{:#}", script.run(CommandResult::None, 100).unwrap_err()).contains("overflow")
    );
}

// Programs shared verbatim with the native dispatcher oracle. No dummy engine
// commands: EXIT 1 is a checkpoint and EXIT 0 terminates the host program.
struct Case {
    code: Vec<u8>,
    seed: u32,
    values: [i32; 4],
    stops: usize,
    label: String,
}
fn exit(code: &mut Vec<u8>, checkpoint: bool) {
    code.extend([0, u8::from(checkpoint), 0]);
}
fn patch(code: &mut [u8], at: usize, target: usize) {
    code[at..at + 4].copy_from_slice(&(target as u32).to_le_bytes());
}
fn mov(code: &mut Vec<u8>, dest: u8, source: u8) {
    code.extend([0x41, 0, dest, 0, source]);
}
fn branch(code: &mut Vec<u8>, opcode: u8) -> usize {
    code.push(opcode);
    let at = code.len();
    code.extend([0; 4]);
    at
}
fn native_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    let values = [
        i32::MIN,
        i32::MAX,
        -65536,
        -1001,
        -1000,
        -999,
        -250,
        -33,
        -1,
        0,
        1,
        31,
        32,
        250,
        999,
        1000,
        1001,
        65535,
    ];
    for a in values {
        for ty in 0..12 {
            for explicit in [false, true] {
                let mut code = Vec::new();
                if !explicit {
                    mov(&mut code, 31, 0xb1);
                }
                code.extend([0x40, ty | if explicit { 0x80 } else { 0 }, 31, 0]);
                if explicit {
                    code.push(0xb1);
                }
                exit(&mut code, false);
                cases.push(Case {
                    code,
                    seed: 42,
                    values: [0, a, 0, 0],
                    stops: 1,
                    label: format!("uo {ty} {a} {explicit}"),
                });
            }
        }
        for b in values {
            for ty in 0..18 {
                for explicit in [false, true] {
                    let mut code = Vec::new();
                    if !explicit {
                        mov(&mut code, 31, 0xb1);
                    }
                    code.extend([0x41, ty | if explicit { 0x80 } else { 0 }, 31, 0]);
                    if explicit {
                        code.push(0xb1);
                    }
                    code.extend([0xc0, 17]);
                    exit(&mut code, false);
                    cases.push(Case {
                        code,
                        seed: 0xffff_ffff,
                        values: [0, a, b, 0],
                        stops: 1,
                        label: format!("bo {ty} {a} {b} {explicit}"),
                    });
                }
            }
            for ty in 1..32 {
                let mut code = vec![0x42, 31, 0, 0, 0xb1];
                if !matches!(ty, 11..=13 | 20..=21 | 27..=29) {
                    code.extend([0, 0xc0, 17]);
                }
                if ty == 24 {
                    code.extend([0, 0xc0, 18]);
                }
                code.extend([ty, 0x20, 0x80]);
                exit(&mut code, false);
                cases.push(Case {
                    code,
                    seed: 100,
                    values: [0, a, b, 1],
                    stops: 1,
                    label: format!("rpn {ty} {a} {b}"),
                });
            }
            for seed in [0, 42, 0xffff_ffff, 0x00ff_ff00, 0x8000_0000] {
                let mut code = vec![0x4c, 31, 0, 0xb1, 0xc0, 17];
                exit(&mut code, false);
                cases.push(Case {
                    code,
                    seed,
                    values: [0, a, b, 0],
                    stops: 1,
                    label: format!("rnd {seed} {a} {b}"),
                });
            }
            for cond in (0..8).chain(0x80..0x88) {
                let mut code = vec![0x46, cond, 0xb1, 0xc0, 17];
                let target = code.len();
                code.extend([0; 4]);
                exit(&mut code, false);
                let to = code.len();
                patch(&mut code, target, to);
                mov(&mut code, 31, 1);
                exit(&mut code, false);
                cases.push(Case {
                    code,
                    seed: 99,
                    values: [0, a, b, 0],
                    stops: 1,
                    label: format!("jc {cond} {a} {b}"),
                });
            }
        }
    }
    // Invalid table indexes fall through; CALLTABLE pushes a continuation only
    // for a valid entry. Tables use byte counts in this older dialect.
    for opcode in [0x4a, 0x4b] {
        for count in [0, 1, 3, 255] {
            for index in [-1, 0, 1, 2, 254, 255] {
                let mut code = vec![opcode, 0xb1, count];
                let table = code.len();
                code.resize(table + count as usize * 4, 0);
                exit(&mut code, false);
                let to = code.len();
                for i in 0..count as usize {
                    patch(&mut code, table + i * 4, to);
                }
                mov(&mut code, 31, 2);
                if opcode == 0x4b {
                    exit(&mut code, true);
                    code.push(0x49);
                } else {
                    exit(&mut code, false);
                }
                cases.push(Case {
                    code,
                    seed: 42,
                    values: [0, index, 0, 0],
                    stops: if opcode == 0x4b && index >= 0 && index < (count as i32) {
                        2
                    } else {
                        1
                    },
                    label: format!("table {opcode} {count} {index}"),
                });
            }
        }
    }
    for depth in [1, 2, 16, 64] {
        let mut code = Vec::new();
        let mut call = branch(&mut code, 0x48);
        exit(&mut code, false);
        for d in 0..depth {
            let to = code.len();
            patch(&mut code, call, to);
            if d + 1 < depth {
                call = branch(&mut code, 0x48);
            } else {
                exit(&mut code, true);
            }
            code.extend([0x41, 2, 31, 0, 1, 0x49]);
        }
        cases.push(Case {
            code,
            seed: 42,
            values: [0; 4],
            stops: 2,
            label: format!("gosub depth {depth}"),
        });
    }
    for (depth, args) in [(1, 0), (4, 0), (1, 1), (2, 16), (4, 63), (1, 255)] {
        let mut code = Vec::new();
        let mut call = branch(&mut code, 0x4f);
        code.push(args);
        for i in 0..args {
            code.push(if i % 2 == 0 { 0xb1 } else { 0xc0 });
            if i % 2 != 0 {
                code.push(17);
            }
        }
        exit(&mut code, false);
        for d in 0..depth {
            let to = code.len();
            patch(&mut code, call, to);
            exit(&mut code, true);
            if d + 1 < depth {
                call = branch(&mut code, 0x4f);
                code.push(args);
                for i in 0..args {
                    code.push(0xd0 | (i % 16));
                }
            }
            if args > 0 {
                mov(&mut code, 31, 0xd0);
                code.extend([0x4d, 2, 0x7f, 0xb1, 0x4e, 2, 32, 0, 33, 0]);
            }
            exit(&mut code, true);
            code.push(0x50);
        }
        cases.push(Case {
            code,
            seed: 42,
            values: [0, i32::MIN, i32::MAX, 0],
            stops: depth * 2 + 1,
            label: format!("call depth {depth} args {args}"),
        });
    }
    cases
}

#[test]
#[ignore = "Requires reviewed updated executable and AArch64 native VM fixture"]
fn updated_native_vm_reference() {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let fixture = std::env::var("SHIN_HIGURASHI_NATIVE_VM").expect("SHIN_HIGURASHI_NATIVE_VM");
    let executable = std::env::var("SHIN_HIGURASHI_EXECUTABLE").expect("SHIN_HIGURASHI_EXECUTABLE");
    let cases = native_cases();
    // Temp files avoid filling a pipe while the child emits full snapshots.
    let input = std::env::temp_dir().join(format!("higu-vm-{}-input.txt", std::process::id()));
    let output = std::env::temp_dir().join(format!("higu-vm-{}-output.txt", std::process::id()));
    let mut stream = std::fs::File::create(&input).unwrap();
    let mut expected = Vec::new();
    for (index, case) in cases.iter().enumerate() {
        write!(
            stream,
            "{} {} {} {} {} {} ",
            case.seed, case.values[0], case.values[1], case.values[2], case.values[3], case.stops
        )
        .unwrap();
        for byte in &case.code {
            write!(stream, "{byte:02x}").unwrap();
        }
        writeln!(stream).unwrap();
        let mut script = vm(case.code.clone(), case.seed, case.values);
        for _ in 0..case.stops {
            let command = script
                .run(CommandResult::None, 10000)
                .unwrap_or_else(|e| panic!("{}: {e:#}", case.label));
            assert!(
                matches!(command, HigurashiRuntimeCommand::EXIT { .. }),
                "{}",
                case.label
            );
            let mut state = vec![
                i64::from(script.random_state()),
                i64::from(script.next_position().0),
                i64::from(script.previous_random_state()),
                i64::from(script.position().0),
                1,
            ];
            state.extend(
                (0..64)
                    .map(|i| i64::from(script.read_register(Register::from_regular_register(i)))),
            );
            state.extend(script.ctx.reference_stack_snapshot());
            expected.push((index, state));
        }
    }
    drop(stream);
    let result = Command::new(fixture)
        .arg(executable)
        .stdin(Stdio::from(std::fs::File::open(&input).unwrap()))
        .stdout(Stdio::from(std::fs::File::create(&output).unwrap()))
        .status()
        .unwrap();
    assert!(result.success(), "native fixture failed: {result}");
    let native = std::fs::read_to_string(&output).unwrap();
    let actual = native
        .lines()
        .map(|l| {
            l.split_whitespace()
                .map(|n| n.parse::<i64>().unwrap())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(actual.len(), expected.len());
    for ((case, expected), actual) in expected.iter().zip(&actual) {
        assert_eq!(actual, expected, "{}", cases[*case].label);
    }
    println!(
        "Higurashi native VM: {} programs, {} state snapshots agree",
        cases.len(),
        actual.len()
    );
}

#[test]
#[ignore = "Requires reviewed updated executable, SNR and AArch64 native VM fixture"]
fn updated_authored_startup_reference() {
    use std::process::Command;
    let fixture = std::env::var("SHIN_HIGURASHI_NATIVE_VM").expect("SHIN_HIGURASHI_NATIVE_VM");
    let executable = std::env::var("SHIN_HIGURASHI_EXECUTABLE").expect("SHIN_HIGURASHI_EXECUTABLE");
    let snr = std::env::var("SHIN_HIGURASHI_SCENARIO").expect("SHIN_HIGURASHI_SCENARIO");
    let native = Command::new(fixture)
        .args([executable, snr.clone(), "prologue".into()])
        .output()
        .unwrap();
    assert!(
        native.status.success(),
        "native authored prologue failed: {}",
        native.status
    );
    let states = String::from_utf8(native.stdout).unwrap();
    let states = states
        .lines()
        .map(|l| {
            l.split_whitespace()
                .map(|n| n.parse::<i64>().unwrap())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(states.len(), 100);
    let scenario = HigurashiScenario::new(Bytes::from(std::fs::read(snr).unwrap())).unwrap();
    let mut script = HigurashiScripter::new(&scenario, 0, 42).unwrap();
    let mut result = CommandResult::None;
    let mut messages = 0;
    for (index, actual) in states.iter().enumerate() {
        let command = script.run(result, 100000).unwrap();
        if index == 0 {
            assert_eq!(
                command,
                HigurashiRuntimeCommand::MSGINIT {
                    style: 0,
                    updated_options: Some([0, 0])
                }
            );
        }
        if let HigurashiRuntimeCommand::MSGSET { .. } = command {
            messages += 1;
        }
        result = if let Some(dest) = command.destination() {
            assert_eq!(index, 1);
            assert_eq!(
                command,
                HigurashiRuntimeCommand::SGET {
                    dest: Register::from_regular_register(1),
                    slot: 2
                }
            );
            // Host test condition only, not inferred native persistent data.
            CommandResult::WriteMemory(dest, 0)
        } else {
            CommandResult::None
        };
        let mut expected = vec![
            i64::from(script.random_state()),
            i64::from(script.next_position().0),
            i64::from(script.previous_random_state()),
            i64::from(script.position().0),
            1,
        ];
        expected.extend(
            (0..64).map(|i| i64::from(script.read_register(Register::from_regular_register(i)))),
        );
        expected.extend(script.ctx.reference_stack_snapshot());
        assert_eq!(
            actual, &expected,
            "authored prologue command {index}: {command:?}"
        );
    }
    assert_eq!(messages, 7);
    println!(
        "Higurashi updated authored prologue: 100 command/VM state snapshots agree, including 7 messages; explicit persistent slot 2 response = 0, engine effects omitted"
    );
}
