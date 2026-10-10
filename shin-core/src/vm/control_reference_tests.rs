#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "executes guarded original/1.03 VM tables, calls and stacks"]
fn reference_native_vm_tables_calls_and_stacks() {
    use crate::format::scenario::{
        instruction_elements::{NumberSpec, Register},
        instructions::{BinaryOperationType, UnaryOperationType},
        types::{Pad4, U16SmallList},
    };
    use binrw::BinWrite;
    use std::{
        fs::File,
        io::Cursor,
        process::{Command, Stdio},
    };

    #[derive(Clone, Copy, Debug)]
    struct Case {
        mode: char,
        seed: u32,
        a: i32,
        b: i32,
        c: i32,
        n: usize,
        p: usize,
        flag: bool,
    }
    let base = Case {
        mode: 'G',
        seed: 0x12345678,
        a: 0,
        b: -7,
        c: 123456789,
        n: 0,
        p: 0,
        flag: false,
    };
    let boundaries = [
        i32::MIN,
        -65,
        -64,
        -1,
        0,
        1,
        15,
        16,
        31,
        32,
        33,
        63,
        64,
        i32::MAX,
    ];
    let mut cases = Vec::new();
    for n in 0..=255 {
        if (0xc8..=0xcf).contains(&n) {
            continue;
        }
        for p in [
            0, 1, 15, 16, 127, 128, 255, 256, 32767, 32768, 65535, 65536, 0x7fffff, 0x800000,
            0xffffff,
        ] {
            cases.push(Case {
                mode: 'N',
                n,
                p,
                a: i32::MIN,
                b: i32::MAX,
                c: -1,
                ..base
            });
        }
    }
    for mode in ['G', 'T'] {
        for n in 0..=32 {
            for a in boundaries {
                for flag in [false, true] {
                    cases.push(Case {
                        mode,
                        n,
                        a,
                        flag,
                        ..base
                    });
                }
            }
        }
    }
    for n in 0..=64 {
        for p in 0..=n {
            for flag in [false, true] {
                cases.push(Case {
                    mode: 'S',
                    n,
                    p,
                    flag,
                    a: i32::MIN,
                    b: i32::MAX,
                    c: -1,
                    ..base
                });
            }
        }
    }
    for n in 0..=64 {
        for seed in [0, 1, 0x80000000, u32::MAX] {
            cases.push(Case {
                mode: 'L',
                n,
                seed,
                ..base
            });
        }
    }
    for p in 0..=255 {
        for n in 1..=64 {
            for flag in [false, true] {
                if n * (p + 1) <= 256 && (!flag || n + p.min(16) + 1 <= 64) {
                    cases.push(Case {
                        mode: 'C',
                        n,
                        p,
                        flag,
                        a: i32::MIN,
                        b: i32::MAX,
                        c: -1,
                        ..base
                    });
                }
            }
        }
    }
    let mut random = 0xf432ca98175b631du64;
    for i in 0..4096 {
        let mut next = || {
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            random as u32
        };
        let seed = next();
        let a = next() as i32;
        let b = next() as i32;
        let c = next() as i32;
        let mode = ['G', 'T', 'S', 'L', 'C'][i % 5];
        let mut n = next() as usize % 33;
        let mut p = 0;
        let flag = mode != 'L' && i % 2 != 0;
        if mode == 'S' {
            p = next() as usize % (n + 1);
        }
        if mode == 'L' {
            n = next() as usize % 65;
        }
        if mode == 'C' {
            p = next() as usize % 256;
            n = 1 + next() as usize % (256 / (p + 1)).min(31);
        }
        cases.push(Case {
            mode,
            seed,
            a,
            b,
            c,
            n,
            p,
            flag,
        });
    }

    fn reg(index: usize) -> Register {
        if index >= 0x1000 {
            Register::from_argument((index - 0x1000) as u16)
        } else {
            Register::from_regular_register(index as u16)
        }
    }
    fn input(index: usize) -> NumberSpec {
        NumberSpec::register(reg([1, 17, 18][index % 3]))
    }
    fn value(index: usize, flag: bool) -> NumberSpec {
        if flag && index % 4 == 3 {
            NumberSpec::constant(-(index as i32) - 1)
        } else {
            input(index)
        }
    }
    fn exit() -> Instruction {
        Instruction::Command(crate::vm::command::CompiletimeCommand::EXIT(
            crate::vm::command::compiletime::EXIT {
                arg1: 0,
                arg2: NumberSpec::constant(0),
            },
        ))
    }
    fn write(code: &mut Cursor<Vec<u8>>, instruction: Instruction) {
        instruction.write_le(code).unwrap();
    }
    fn target(code: &mut Cursor<Vec<u8>>, op: u8, args: Vec<NumberSpec>) -> usize {
        let at = code.position() as usize + 1;
        write(
            code,
            match op {
                0x47 => Instruction::j {
                    target: CodeAddress(0),
                },
                0x48 => Instruction::gosub {
                    target: CodeAddress(0),
                },
                0x4f => Instruction::call {
                    target: CodeAddress(0),
                    args: U8SmallNumberList::from_contents(args),
                },
                _ => unreachable!(),
            },
        );
        at
    }
    fn patch(code: &mut Cursor<Vec<u8>>, at: usize, value: usize) {
        code.get_mut()[at..at + 4].copy_from_slice(&(value as u32).to_le_bytes());
    }
    fn add(code: &mut Cursor<Vec<u8>>, index: usize) {
        write(
            code,
            Instruction::bo(BinaryOperation {
                ty: BinaryOperationType::Add,
                destination: reg(index),
                left: NumberSpec::register(reg(index)),
                right: NumberSpec::constant(1),
            }),
        );
    }
    fn prepare(case: Case) -> (Vec<u8>, usize) {
        let mut code = Cursor::new(Vec::new());
        let n = case.n;
        let p = case.p;
        match case.mode {
            'N' => {
                let kind = (n >> 4) & 7;
                let length = if n < 128 {
                    1
                } else {
                    match kind {
                        0 | 4 => 2,
                        1 => 3,
                        2 => 4,
                        _ => 1,
                    }
                };
                code.get_mut().extend([0x41, 0, 31, 0, n as u8]);
                for i in (1..length).rev() {
                    code.get_mut().push((p >> (8 * (i - 1))) as u8);
                }
                code.set_position(code.get_ref().len() as u64);
                write(&mut code, exit());
            }
            'G' => {
                write(
                    &mut code,
                    Instruction::gt {
                        dest: reg(31),
                        index: input(0),
                        table: U16SmallList::from_contents(
                            (0..n).map(|i| Pad4(value(i, case.flag))),
                        ),
                    },
                );
                write(&mut code, exit());
            }
            'T' => {
                // Jump-table targets occupy fixed four-byte slots after the encoded index/count.
                write(
                    &mut code,
                    Instruction::jt {
                        index: input(0),
                        table: U16SmallList::from_contents(std::iter::repeat_n(CodeAddress(0), n)),
                    },
                );
                write(&mut code, exit());
                for i in 0..n {
                    let at = code.position() as usize;
                    patch(&mut code, 4 + i * 4, at);
                    write(
                        &mut code,
                        Instruction::uo(UnaryOperation {
                            ty: UnaryOperationType::Abs,
                            destination: reg(31),
                            source: NumberSpec::constant(i as i32 + 1),
                        }),
                    );
                    write(&mut code, exit());
                }
            }
            'S' => {
                write(
                    &mut code,
                    Instruction::push {
                        values: U8SmallNumberList::from_contents(
                            (0..n).map(|i| value(i, case.flag)),
                        ),
                    },
                );
                write(
                    &mut code,
                    Instruction::pop {
                        dest: U8SmallList::from_contents(
                            (0..p).map(|i| reg(if case.flag { 1 } else { 32 + i })),
                        ),
                    },
                );
                write(&mut code, exit());
            }
            'L' => {
                let first = target(&mut code, if n == 0 { 0x47 } else { 0x48 }, vec![]);
                write(&mut code, exit());
                if n == 0 {
                    let at = code.position() as usize;
                    patch(&mut code, first, at);
                    write(&mut code, exit());
                } else {
                    let mut calls = vec![first];
                    for d in 0..n {
                        let at = code.position() as usize;
                        patch(&mut code, calls[d], at);
                        if d + 1 < n {
                            calls.push(target(&mut code, 0x48, vec![]));
                        }
                        add(&mut code, 31);
                        write(&mut code, Instruction::retsub {});
                    }
                }
            }
            'C' => {
                let mut calls = vec![target(&mut code, 0x4f, (0..p).map(input).collect())];
                let mut helper_calls = Vec::new();
                write(&mut code, exit());
                for d in 0..n {
                    let at = code.position() as usize;
                    patch(&mut code, calls[d], at);
                    write(&mut code, exit());
                    if case.flag {
                        let saved = p.min(16);
                        write(
                            &mut code,
                            Instruction::push {
                                values: U8SmallNumberList::from_contents(
                                    (0..saved).map(|i| NumberSpec::register(reg(0x1000 + i))),
                                ),
                            },
                        );
                        if p != 0 {
                            write(
                                &mut code,
                                Instruction::uo(UnaryOperation {
                                    ty: UnaryOperationType::Negate,
                                    destination: reg(0x1000),
                                    source: NumberSpec::register(reg(0x1000)),
                                }),
                            );
                        }
                        write(&mut code, exit());
                        helper_calls.push(target(&mut code, 0x48, vec![]));
                        write(
                            &mut code,
                            Instruction::pop {
                                dest: U8SmallList::from_contents(
                                    (0..saved).map(|i| reg(0x1000 + saved - i - 1)),
                                ),
                            },
                        );
                    }
                    if d + 1 < n {
                        calls.push(target(
                            &mut code,
                            0x4f,
                            (0..p)
                                .map(|i| {
                                    if i % 2 == 0 {
                                        input(i)
                                    } else {
                                        NumberSpec::register(reg(0x1000 + (p - i - 1) % p.min(16)))
                                    }
                                })
                                .collect(),
                        ));
                    }
                    add(&mut code, 31);
                    write(&mut code, exit());
                    write(&mut code, Instruction::r#return {});
                }
                if !helper_calls.is_empty() {
                    let at = code.position() as usize;
                    for offset in helper_calls {
                        patch(&mut code, offset, at);
                    }
                    add(&mut code, 30);
                    write(&mut code, Instruction::retsub {});
                }
            }
            _ => unreachable!(),
        }
        (
            code.into_inner(),
            if case.mode == 'C' {
                (if case.flag { 3 } else { 2 }) * n + 1
            } else {
                1
            },
        )
    }

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let binary = std::env::temp_dir().join(format!("shin-vm-control-{}", std::process::id()));
    let input_path = binary.with_extension("requests");
    let mutant = binary.with_extension("mutant");
    let compile = Command::new("/usr/bin/clang")
        .args(["-O0", "-Wall", "-Wextra"])
        .arg(root.join("reverse-engineering/native-vm-control.c"))
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    std::fs::write(
        &input_path,
        cases
            .iter()
            .map(|c| {
                format!(
                    "{} {} {} {} {} {} {} {}\n",
                    c.mode,
                    c.seed,
                    c.a,
                    c.b,
                    c.c,
                    c.n,
                    c.p,
                    u8::from(c.flag)
                )
            })
            .collect::<String>(),
    )
    .unwrap();
    let mut expected = Vec::new();
    let mut counts = [0; 6];
    for case in &cases {
        let (code, stops) = prepare(*case);
        let mut ctx = VmCtx::new(0, case.seed);
        ctx.write_register(reg(1), case.a);
        ctx.write_register(reg(17), case.b);
        ctx.write_register(reg(18), case.c);
        if case.mode == 'N' {
            ctx.push_data_stack_frame(
                &(0..16)
                    .map(|i| [case.a, case.b, case.c][i % 3])
                    .collect::<Vec<_>>(),
            );
        }
        let mut script = Scripter {
            initial_values: (0, case.seed),
            ctx,
            instruction_reader: InstructionReader::new(code.into(), CodeAddress(0)),
            position: CodeAddress(0),
            breakpoints: CodeBreakpointSet::new(),
        };
        counts[match case.mode {
            'G' => 0,
            'T' => 1,
            'S' => 2,
            'L' => 3,
            'C' => 4,
            'N' => 5,
            _ => unreachable!(),
        }] += 1;
        for stop in 0..stops {
            let RuntimeCommand::EXIT(cmd) = script.run(CommandResult::None).unwrap() else {
                panic!("{case:?}: expected EXIT")
            };
            assert_eq!((cmd.arg1, cmd.arg2), (0, 0));
            let seed = script.ctx.get_prng_state();
            let previous = seed.wrapping_sub(0x269ec3).wrapping_mul(0xb9b33155);
            let mut words = vec![
                i64::from(seed),
                i64::from(script.instruction_reader.position().0),
                i64::from(previous),
                i64::from(script.position.0),
                1,
            ];
            words.extend((0..96).map(|i| i64::from(script.ctx.read_register(reg(i)))));
            words.extend(script.ctx.reference_stack_snapshot());
            expected.push((*case, stop, words));
        }
    }
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let folder = std::path::Path::new(&rom).parent().unwrap();
    for (name, shift, got, table) in [
        (
            "uncompressed_executable",
            0usize,
            0x225550usize,
            0x21fd00usize,
        ),
        ("uncompressed_executable_update", 0x730, 0x2275b0, 0x221d80),
    ] {
        let output = Command::new(&binary)
            .arg(folder.join(name))
            .stdin(Stdio::from(File::open(&input_path).unwrap()))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stdout).unwrap();
        assert_eq!(text.lines().count(), expected.len());
        let mut mismatches = Vec::new();
        for ((case, stop, expected), line) in expected.iter().zip(text.lines()) {
            let native: Vec<i64> = line
                .split_whitespace()
                .map(|v| v.parse().unwrap())
                .collect();
            if native != *expected {
                mismatches.push(format!(
                    "{case:?} stop{stop}: native{native:?} port{expected:?}"
                ));
            }
        }
        for mismatch in mismatches.iter().take(8) {
            eprintln!("{name}: {mismatch}");
        }
        eprintln!(
            "{name}: {} control programs (G{},T{},S{},L{},C{},N{}), {} yield snapshots; {} mismatches",
            cases.len(),
            counts[0],
            counts[1],
            counts[2],
            counts[3],
            counts[4],
            counts[5],
            expected.len(),
            mismatches.len()
        );
        assert!(mismatches.is_empty(), "Native control/stack mismatch");
        let raw = std::fs::read(folder.join(name)).unwrap();
        let mut addresses: Vec<_> = [
            0xecc90, 0xeed20, 0xeb810, 0xeb9f0, 0xea200, 0xea210, 0xea220, 0xec750, 0xeca90,
            0xecad0, 0xecb30, 0xecb60, 0xecd80, 0xece00, 0xecee0, 0xecfc0, 0xeba70, 0xebd50,
            0xeb6f0,
        ]
        .into_iter()
        .map(|at| at + shift)
        .collect();
        addresses.extend(if shift == 0 {
            [0x173e58, 0x173e5f, 0x173e6a]
        } else {
            [0x174fa7, 0x174fae, 0x174fb9]
        });
        let opcodes = [
            0, 0x40, 0x41, 0x44, 0x47, 0x48, 0x49, 0x4a, 0x4d, 0x4e, 0x4f, 0x50,
        ];
        for at in std::iter::once(got).chain(opcodes.map(|op| table + op * 16)) {
            let key: Vec<_> = (at as u64)
                .to_le_bytes()
                .into_iter()
                .chain(0x403u64.to_le_bytes())
                .collect();
            addresses.push(raw[256..].windows(16).position(|v| v == key).unwrap() + 16);
        }
        addresses.extend(opcodes.map(|op| table + op * 16 + 8));
        assert_eq!(addresses.len(), 47);
        for at in addresses {
            let mut changed = raw.clone();
            changed[at + 256] ^= 1;
            std::fs::write(&mutant, changed).unwrap();
            let rejected = Command::new(&binary).arg(&mutant).output().unwrap();
            assert_eq!(rejected.status.code(), Some(2), "{name}: guard at{at:x}");
            assert!(rejected.stdout.is_empty());
        }
        eprintln!("{name}: 47 control fixture mutants rejected before execution");
    }
    for path in [binary, input_path, mutant] {
        std::fs::remove_file(path).unwrap();
    }
}
