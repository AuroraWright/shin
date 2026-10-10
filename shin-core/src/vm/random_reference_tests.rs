#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "executes guarded original/1.03 VM RND and real dispatch/NumberSpec readers"]
fn reference_native_vm_random_ranges_and_instruction_steps() {
    use std::{fs::File, process::{Command, Stdio}};
    use crate::format::scenario::instruction_elements::Register;

    #[derive(Clone, Copy, Debug)]
    struct Case { dispatch: bool, seed: u32, a: i32, b: i32, dest: u16, count: usize }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let binary = std::env::temp_dir().join(format!("shin-vm-random-{}", std::process::id()));
    let input = binary.with_extension("requests");
    let mutant = binary.with_extension("mutant");
    let compile = Command::new("/usr/bin/clang").args(["-O0", "-Wall", "-Wextra"])
        .arg(root.join("reverse-engineering/native-vm-random.c")).arg("-o").arg(&binary)
        .output().unwrap();
    assert!(compile.status.success(), "{}", String::from_utf8_lossy(&compile.stderr));
    let seeds = [0, 1, 255, 256, 257, 0xffff, 0xffffff, 0x1000000, 0x12345678,
        0x7fffffff, 0x80000000, 0xffffff00, u32::MAX];
    let values = [i32::MIN, i32::MIN + 1, -1000000000, -1000000, -1001, -1000, -1,
        0, 1, 999, 1000, 1001, 1000000, 1000000000, i32::MAX - 1, i32::MAX];
    let mut cases = Vec::new();
    for seed in seeds { for a in values { for b in values {
        cases.push(Case { dispatch: false, seed, a, b, dest: 31, count: 1 });
    } } }
    let destinations = [0, 31, 127, 2047, 0x1000, 0x100f];
    for seed in seeds { for (a, b) in [(0, 1000), (1000, 0), (-1000, 1000),
        (i32::MIN, i32::MAX), (i32::MAX, i32::MIN), (42, 42), (-1000000, 1000000)] {
        for dest in destinations { for count in [1, 2, 7, 64] {
            cases.push(Case { dispatch: true, seed, a, b, dest, count });
        } }
    } }
    let mut random = 0x831520cbd394fe67u64;
    for i in 0..8192 {
        let mut next = || { random ^= random << 13; random ^= random >> 7;
            random ^= random << 17; random as u32 };
        let seed = next(); let a = next() as i32; let b = next() as i32;
        cases.push(Case { dispatch: i % 2 != 0, seed, a, b,
            dest: destinations[i % destinations.len()], count: if i % 2 != 0 {1 + i % 64} else {1} });
    }
    let request: String = cases.iter().map(|c| format!("{} {} {} {} {} {}\n",
        if c.dispatch {'D'} else {'R'}, c.seed, c.a, c.b, c.dest, c.count)).collect();
    std::fs::write(&input, request).unwrap();
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let folder = std::path::Path::new(&rom).parent().unwrap();
    for (name, shift, number_table, got, table) in [
        ("uncompressed_executable", 0usize, 0x173e58usize, 0x225550usize, 0x21fd00usize),
        ("uncompressed_executable_update", 0x730, 0x174fa7, 0x2275b0, 0x221d80),
    ] {
        let executable = folder.join(name);
        let output = Command::new(&binary).arg(&executable)
            .stdin(Stdio::from(File::open(&input).unwrap())).output().unwrap();
        assert!(output.status.success(), "{name}: {}", String::from_utf8_lossy(&output.stderr));
        let text = String::from_utf8(output.stdout).unwrap();
        assert_eq!(text.lines().count(), cases.len());
        for (case, line) in cases.iter().zip(text.lines()) {
            let native: Vec<i64> = line.split_whitespace().map(|v| v.parse().unwrap()).collect();
            let dest = if case.dest < 0x1000 { Register::from_regular_register(case.dest) }
                else { Register::from_argument(case.dest - 0x1000) };
            let mut ctx = VmCtx::new(0, case.seed);
            ctx.push_data_stack_frame(&[0; 16]);
            let expected = if case.dispatch {
                let mut code = Vec::new();
                for _ in 0..case.count {
                    code.push(0x4c); code.extend_from_slice(&case.dest.to_le_bytes());
                    code.extend_from_slice(&[0xb1, 0xc0, 17]);
                }
                code.extend_from_slice(&[0, 0, 0]);
                ctx.write_register(Register::from_regular_register(1), case.a);
                ctx.write_register(Register::from_regular_register(17), case.b);
                let mut script = Scripter {
                    initial_values: (0, case.seed), ctx,
                    instruction_reader: InstructionReader::new(code.into(), CodeAddress(0)),
                    position: CodeAddress(0), breakpoints: CodeBreakpointSet::new(),
                };
                let RuntimeCommand::EXIT(exit) = script.run(CommandResult::None).unwrap()
                    else { panic!("Expected EXIT after bounded random instructions") };
                assert_eq!((exit.arg1, exit.arg2), (0, 0));
                let mut before_exit = VmCtx::new(0, case.seed);
                for _ in 0..case.count { before_exit.update_prng(); }
                vec![i64::from(script.ctx.read_register(dest)), i64::from(script.ctx.get_prng_state()),
                    i64::from(script.instruction_reader.position().0), i64::from(before_exit.get_prng_state()),
                    i64::from(script.position.0), 1]
            } else {
                vec![i64::from(ctx.run_prng(case.a, case.b)), i64::from(ctx.get_prng_state()), 5, 0, 0, 0]
            };
            assert_eq!(native, expected, "{name}: {case:?}");
            assert!((i64::from(case.a.min(case.b))..=i64::from(case.a.max(case.b))).contains(&native[0]));
        }
        let raw = std::fs::read(&executable).unwrap();
        let mut altered_offsets: Vec<_> = [0xecc90, 0xeed20, 0xeb810, 0xeb9f0, 0xea200, 0xea210, 0xea220]
            .into_iter().map(|at| 256 + at + shift).collect();
        altered_offsets.push(256 + number_table);
        for address in [got, table, table + 0x4c * 16] {
            let key: Vec<_> = (address as u64).to_le_bytes().into_iter().chain(0x403u64.to_le_bytes()).collect();
            let at = raw[256..].windows(16).position(|v| v == key).unwrap();
            altered_offsets.push(256 + at + 16);
        }
        altered_offsets.extend([256 + table + 8, 256 + table + 0x4c * 16 + 8]);
        for at in &altered_offsets {
            let mut changed = raw.clone(); changed[*at] ^= 1;
            std::fs::write(&mutant, changed).unwrap();
            let rejected = Command::new(&binary).arg(&mutant).output().unwrap();
            assert_eq!(rejected.status.code(), Some(2), "{name}: guard at{at:x}");
            assert!(rejected.stdout.is_empty(), "Guard rejection must precede execution");
        }
        let dispatched = cases.iter().filter(|c| c.dispatch).count();
        eprintln!("{name}: {} native VM random cases, {dispatched} real Scripter/dispatch sequences; values, seeds, PCs and EXIT agree; {} mutants rejected",
            cases.len(), altered_offsets.len());
    }
    for p in [binary, input, mutant] { std::fs::remove_file(p).unwrap(); }
}
