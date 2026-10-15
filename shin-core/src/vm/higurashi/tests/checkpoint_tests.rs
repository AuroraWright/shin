use super::*;
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn state(label: char, script: &HigurashiScripter) -> String {
    let mut words = vec![
        i64::from(script.random_state()),
        i64::from(script.next_position().0),
        i64::from(script.previous_random_state()),
        i64::from(script.position().0),
    ];
    words.extend(
        (0..2048).map(|i| i64::from(script.read_register(Register::from_regular_register(i)))),
    );
    words.extend(script.ctx.reference_stack_snapshot());
    format!(
        "{label} {}",
        words
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" ")
    )
}

#[test]
fn replay_checkpoint_keeps_feedback_explicit_and_capture_immutable() {
    let mut script = vm(
        vec![0x81, 31, 0, 7, 0x85, 0xc0, 31, 1, 0x7f, 0, 0, 0],
        42,
        [0; 4],
    );
    assert!(script.checkpoint_for_replay().is_err());
    let command = script.run(CommandResult::None, 100).unwrap();
    let before = state('D', &script);
    let saved = script.checkpoint_for_replay().unwrap();
    assert_eq!(saved.position(), script.position());
    assert_eq!(saved.random_state(), script.previous_random_state());
    assert_eq!(state('D', &script), before);
    let mut restored = saved.scripter();
    assert_eq!(restored.run(CommandResult::None, 100).unwrap(), command);
    assert_eq!(state('D', &restored), before);
    assert!(restored.run(CommandResult::None, 100).is_err());
    let dest = Register::from_regular_register(31);
    let a = script
        .run(CommandResult::WriteMemory(dest, 99), 100)
        .unwrap();
    let b = restored
        .run(CommandResult::WriteMemory(dest, 99), 100)
        .unwrap();
    assert_eq!(a, b);
    assert_eq!(state('D', &script), state('D', &restored));
    // A saved snapshot remains unchanged after the source continues.
    let mut again = saved.scripter();
    assert_eq!(again.run(CommandResult::None, 100).unwrap(), command);
    assert_eq!(state('D', &again), before);
    script.run(CommandResult::None, 100).unwrap();
    let ended = script.checkpoint_for_replay().unwrap();
    assert!(matches!(
        ended.scripter().run(CommandResult::None, 100).unwrap(),
        HigurashiRuntimeCommand::EXIT { arg1: 0, .. }
    ));
    let mut control = vm(vec![0x47, 0, 0, 0, 0], 42, [0; 4]);
    assert!(control.run(CommandResult::None, 3).is_err());
    assert!(control.checkpoint_for_replay().is_err());
}

#[test]
#[ignore = "executes native1185c arithmetic capture/11400 restore and actualc049c replay"]
fn higurashi_arithmetic_checkpoints_replay_native_registers_stacks_pc_and_prng() {
    let mut cases = native_cases()
        .into_iter()
        .enumerate()
        .filter(|(i, c)| {
            c.label.starts_with("gosub")
                || c.label.starts_with("call")
                || c.label.starts_with("table")
                || i % 503 == 0
        })
        .map(|(_, c)| c)
        .collect::<Vec<_>>();
    for value in [i32::MIN, i32::MAX, 0x13579bdf] {
        let mut code = Vec::new();
        for register in [63u16, 127, 255, 511, 1023, 1535, 2047] {
            code.extend([0x41, 0]);
            code.extend(register.to_le_bytes());
            code.push(0xb1);
        }
        exit(&mut code, true);
        code.extend([0x4c, 31, 0, 0x7f, 1]);
        exit(&mut code, false);
        cases.push(Case {
            code,
            seed: value as u32,
            values: [0, value, 0, 0],
            stops: 2,
            label: format!("mutated upper registers {value}"),
        });
    }
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut snapshots = 0;
    for case in &cases {
        let hex = case
            .code
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let [init, a, b, c] = case.values;
        input.push_str(&format!(
            "D {} {init} {a} {b} {c} {} {hex}\n",
            case.seed, case.stops
        ));
        let mut script = vm(case.code.clone(), case.seed, case.values);
        for _ in 0..case.stops {
            let command = script.run(CommandResult::None, 10000).unwrap();
            let before = state('D', &script);
            let saved = script.checkpoint_for_replay().unwrap();
            assert_eq!(state('D', &script), before);
            let mut restored = saved.scripter();
            assert_eq!(
                restored.run(CommandResult::None, 10000).unwrap(),
                command,
                "{}",
                case.label
            );
            assert_eq!(state('D', &restored), before, "{}", case.label);
            expected.push(before);
            expected.push(state('R', &restored));
            snapshots += 1;
        }
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_CHECKPOINT").unwrap())
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
    let output = String::from_utf8(output.stdout).unwrap();
    assert_eq!(output.lines().count(), expected.len());
    for (row, (actual, expected)) in output.lines().zip(&expected).enumerate() {
        assert_eq!(actual, expected, "checkpoint/replay row{row}");
    }
    println!(
        "Higurashi arithmetic replay:{} native programs/{snapshots} checkpoints/{} full active-state snapshots match2048 registers, PC/pre-command PRNG, call64/data256 stacks and command replay; source capture stays immutable",
        cases.len(),
        expected.len()
    );
}
