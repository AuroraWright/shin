#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "executes guarded original/1.03 VM arithmetic, expressions and jumps; Darwin trig imports"]
fn reference_native_vm_arithmetic_expressions_and_jumps() {
    use std::{fs::File, io::Cursor, process::{Command, Stdio}};
    use binrw::BinWrite;
    use num_traits::FromPrimitive;
    use crate::format::scenario::{instruction_elements::{NumberSpec, Register}, instructions::{
        BinaryOperationType as B, Expression, ExpressionTerm as E, JumpCond, JumpCondType,
        UnaryOperationType as U,
    }};

    #[derive(Clone, Copy, Debug)]
    struct Case { mode: char, op: u8, explicit: bool, seed: u32, a: i32, b: i32, c: i32, dest: u16 }
    let terms = [E::Add, E::Subtract, E::Multiply, E::Divide, E::Modulo, E::ShiftLeft,
        E::ShiftRight, E::BitwiseAnd, E::BitwiseOr, E::BitwiseXor, E::Negate, E::BitwiseNot,
        E::Abs, E::CmpEqual, E::CmpNotEqual, E::CmpGreaterOrEqual, E::CmpGreater,
        E::CmpLessOrEqual, E::CmpLess, E::CmpZero, E::CmpNotZero, E::LogicalAnd,
        E::LogicalOr, E::Select, E::MultiplyReal, E::DivideReal, E::Sin, E::Cos, E::Tan,
        E::Min, E::Max];
    let values = [i32::MIN, i32::MIN + 1, -1000000000, -1000000, -2001, -1001, -1000,
        -999, -251, -250, -32, -1, 0, 1, 31, 32, 63, 250, 251, 999, 1000, 1001,
        2001, 1000000, 1000000000, i32::MAX - 1, i32::MAX];
    let destinations = [0, 31, 2047, 0x1000, 0x100f];
    let base = Case { mode: 'U', op: 0, explicit: false, seed: 0x12345678,
        a: 0, b: 0, c: 0, dest: 31 };
    let mut cases = Vec::new();
    for op in 0..12 { for a in values { for dest in destinations { for explicit in [false, true] {
        cases.push(Case { op, a, dest, explicit, ..base });
    } } } }
    for op in 0..18 { for a in values { for b in values { for dest in [31, 0x1000] { for explicit in [false, true] {
        cases.push(Case { mode: 'B', op, a, b, dest, explicit, ..base });
    } } } } }
    for op in 1..32 {
        let arity = terms[op as usize - 1].argument_count();
        for a in values { for b in if arity >= 2 {values.to_vec()} else {vec![0]} {
            for c in if arity == 3 {vec![-1, 0, 1, i32::MIN]} else {vec![0]} {
                for dest in [31, 0x100f] { cases.push(Case { mode: 'E', op, a, b, c, dest, ..base }); }
            }
        } }
    }
    for op in 0..8 { for a in values { for b in values { for explicit in [false, true] {
        cases.push(Case { mode: 'J', op, a, b, explicit, ..base });
    } } } }
    for a in -2000..=2000 { for op in 4..7 {
        cases.push(Case { op, a, explicit: true, ..base });
        cases.push(Case { mode: 'E', op: op + 23, a, ..base });
    } }
    for a in -1100..=1100 { for op in 7..10 { cases.push(Case { op, a, explicit: true, ..base }); } }
    let mut random = 0x831520cbd394fe67u64;
    for i in 0..16384 {
        let mut next = || { random ^= random << 13; random ^= random >> 7;
            random ^= random << 17; random as u32 };
        let seed = next(); let a = next() as i32; let b = next() as i32; let c = next() as i32;
        let (mode, op) = match i % 5 {
            0 => ('U', (i / 5 % 12) as u8), 1 => ('B', (i / 5 % 18) as u8),
            2 => ('E', (1 + i / 5 % 31) as u8), 3 => ('J', (i / 5 % 8) as u8),
            _ => ('X', (i / 5 % 5) as u8),
        };
        cases.push(Case { mode, op, explicit: matches!(mode, 'U' | 'B' | 'J') && i % 2 != 0,
            seed, a, b, c, dest: destinations[i % destinations.len()] });
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let binary = std::env::temp_dir().join(format!("shin-vm-arithmetic-{}", std::process::id()));
    let input = binary.with_extension("requests"); let mutant = binary.with_extension("mutant");
    let compile = Command::new("/usr/bin/clang").args(["-O0", "-Wall", "-Wextra"])
        .arg(root.join("reverse-engineering/native-vm-arithmetic.c")).arg("-o").arg(&binary)
        .output().unwrap();
    assert!(compile.status.success(), "{}", String::from_utf8_lossy(&compile.stderr));
    std::fs::write(&input, cases.iter().map(|c| format!("{} {} {} {} {} {} {} {}\n",
        c.mode, c.seed, c.a, c.b, c.c, c.dest, c.op, u8::from(c.explicit))).collect::<String>()).unwrap();
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap(); let folder = std::path::Path::new(&rom).parent().unwrap();
    let source = Register::from_regular_register(1); let right = Register::from_regular_register(17);
    let third = Register::from_regular_register(18);
    let mut expected = Vec::with_capacity(cases.len());
    for case in &cases {
        let dest = if case.dest < 0x1000 { Register::from_regular_register(case.dest) }
            else { Register::from_argument(case.dest - 0x1000) };
        let mut code = Cursor::new(Vec::new());
        let instruction = match case.mode {
            'U' => Instruction::uo(UnaryOperation { ty: U::from_u8(case.op).unwrap(), destination: dest,
                source: NumberSpec::register(if case.explicit {source} else {dest}) }),
            'B' => Instruction::bo(BinaryOperation { ty: B::from_u8(case.op).unwrap(), destination: dest,
                left: NumberSpec::register(if case.explicit {source} else {dest}), right: NumberSpec::register(right) }),
            'E' => {
                let term = terms[case.op as usize - 1];
                let mut expr: Vec<_> = [source, right, third].into_iter().take(term.argument_count())
                    .map(|r| E::Push(NumberSpec::register(r))).collect();
                expr.push(term); Instruction::exp { dest, expr: Expression::new(expr).unwrap() }
            }
            'X' => {
                let a = E::Push(NumberSpec::register(source)); let b = E::Push(NumberSpec::register(right));
                let c = E::Push(NumberSpec::register(third));
                let expr = match case.op {
                    0 => vec![a,b,E::Add,c,a,E::Subtract,E::Multiply],
                    1 => vec![a,b,E::Min,a,b,E::Max,c,E::Select],
                    2 => vec![a,b,E::Divide,a,b,E::Modulo,E::Add],
                    3 => vec![a,b,E::BitwiseXor,c,E::ShiftLeft,a,E::Negate,E::Max],
                    4 => std::iter::repeat_n(a,32).chain(std::iter::repeat_n(E::Add,31)).collect(),
                    _ => unreachable!(),
                };
                Instruction::exp { dest, expr: Expression::new(expr).unwrap() }
            }
            'J' => Instruction::jc { cond: JumpCond { is_negated: case.explicit,
                condition: JumpCondType::from_u8(case.op).unwrap() }, left: NumberSpec::register(source),
                right: NumberSpec::register(right), target: CodeAddress(12) },
            _ => unreachable!(),
        };
        instruction.write_le(&mut code).unwrap();
        let exit = Instruction::Command(crate::vm::command::CompiletimeCommand::EXIT(
            crate::vm::command::compiletime::EXIT { arg1: 0, arg2: NumberSpec::constant(0) }));
        exit.write_le(&mut code).unwrap();
        if case.mode == 'J' { exit.write_le(&mut code).unwrap(); }
        let mut ctx = VmCtx::new(0, case.seed); ctx.push_data_stack_frame(&[0;16]);
        ctx.write_register(source, case.a); ctx.write_register(right, case.b); ctx.write_register(third, case.c);
        ctx.write_register(dest, case.a);
        let mut script = Scripter { initial_values: (0, case.seed), ctx,
            instruction_reader: InstructionReader::new(code.into_inner().into(), CodeAddress(0)),
            position: CodeAddress(0), breakpoints: CodeBreakpointSet::new() };
        let RuntimeCommand::EXIT(cmd) = script.run(CommandResult::None).unwrap()
            else { panic!("Expected EXIT after {case:?}") };
        assert_eq!((cmd.arg1,cmd.arg2),(0,0));
        let mut previous = VmCtx::new(0, case.seed); previous.update_prng();
        expected.push(vec![i64::from(script.ctx.read_register(dest)), i64::from(script.ctx.get_prng_state()),
            i64::from(script.instruction_reader.position().0), i64::from(previous.get_prng_state()),
            i64::from(script.position.0), 1]);
    }
    for (name, shift, got, table) in [("uncompressed_executable",0usize,0x225550usize,0x21fd00usize),
        ("uncompressed_executable_update",0x730,0x2275b0,0x221d80)] {
        let output = Command::new(&binary).arg(folder.join(name)).stdin(Stdio::from(File::open(&input).unwrap()))
            .output().unwrap();
        assert!(output.status.success(), "{name}: {}", String::from_utf8_lossy(&output.stderr));
        let text = String::from_utf8(output.stdout).unwrap(); assert_eq!(text.lines().count(),cases.len());
        let mut mismatches = Vec::new(); let mut counts = [0usize;5];
        for ((case, expected), line) in cases.iter().zip(&expected).zip(text.lines()) {
            let native: Vec<i64> = line.split_whitespace().map(|v| v.parse().unwrap()).collect();
            let kind = match case.mode {'U'=>0,'B'=>1,'E'=>2,'J'=>3,'X'=>4,_=>unreachable!()}; counts[kind]+=1;
            if native != *expected { mismatches.push(format!("{case:?}: native{native:?} port{expected:?}")); }
        }
        for mismatch in mismatches.iter().take(24) {eprintln!("{name}: {mismatch}");}
        eprintln!("{name}: {} arithmetic cases (U{},B{},E{},J{},X{}); {} mismatches",cases.len(),counts[0],counts[1],counts[2],counts[3],counts[4],mismatches.len());
        assert!(mismatches.is_empty(), "Native arithmetic/dispatch mismatch");
        let raw = std::fs::read(folder.join(name)).unwrap();
        let mut addresses: Vec<_> = [0xecc90,0xeed20,0xeb810,0xeb9f0,0xea200,0xea210,0xea220]
            .into_iter().map(|a|a+shift).collect();
        addresses.extend(if shift==0 {[0xeba70,0xebd50,0xec050,0xec910,0x119440,0x119480,
            0x11cb70,0x11d060,0x11d090,0x11d500,0x11d510,0x11d040,0x11cb90,
            0x173e58,0x173e5f,0x173e6a,0x173e7c,0x173ebc,0x187a04,0x159a58,0x159518,0x159578]}
            else {[0xec1a0,0xec480,0xec780,0xed040,0x119d00,0x119d40,
            0x11d430,0x11d8d0,0x11d900,0x11dd70,0x11dd80,0x11d8b0,0x11d450,
            0x174fa7,0x174fae,0x174fb9,0x174fcc,0x17500c,0x188b54,0x15aa38,0x15a4f8,0x15a558]});
        for at in [got,table,table+0x40*16,table+0x41*16,table+0x42*16,table+0x46*16] {
            let key: Vec<_> = (at as u64).to_le_bytes().into_iter().chain(0x403u64.to_le_bytes()).collect();
            addresses.push(raw[256..].windows(16).position(|v|v==key).unwrap()+16);
        }
        addresses.extend([table+8,table+0x40*16+8,table+0x41*16+8,table+0x42*16+8,table+0x46*16+8]);
        assert_eq!(addresses.len(),40);
        for at in addresses { let mut changed=raw.clone();changed[at+256]^=1;std::fs::write(&mutant,changed).unwrap();
            let rejected=Command::new(&binary).arg(&mutant).output().unwrap();
            assert_eq!(rejected.status.code(),Some(2),"{name}: guard at{at:x}");assert!(rejected.stdout.is_empty());
        }
        eprintln!("{name}: 40 arithmetic fixture mutants rejected before execution");
    }
    for path in [binary,input,mutant] {std::fs::remove_file(path).unwrap();}
}
