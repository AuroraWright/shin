use super::*;
use crate::adv::higurashi::autosave::{ProgressEntry, SaveProgress};
use shin_core::format::{save::higurashi::SaveDate, scenario::instruction_elements::Register};
use std::{
    fmt::Write as _,
    io::Write as _,
    process::{Command, Stdio},
};

mod effects_integration;
mod factories;
mod framebuffer;
mod pixels;
mod temporary_integration;

fn scenario(code: &[u8]) -> Arc<HigurashiScenario> {
    let mut bytes = std::fs::read(std::env::var("SHIN_HIGURASHI_SCENARIO").unwrap()).unwrap();
    let start = u32::from_le_bytes(bytes[32..36].try_into().unwrap()) as usize;
    bytes[start..start + code.len()].copy_from_slice(code);
    Arc::new(HigurashiScenario::new(bytes.into()).unwrap())
}
fn data(active: usize, saved: u32, target: u32, seed: u32) -> RuntimeGameData {
    RuntimeGameData::from_progress(
        SaveProgress::new(
            active,
            0xfedcba98,
            std::array::from_fn(|i| ProgressEntry {
                scenario_id: (i * 11) as i32,
                random_seed: seed.wrapping_add((i * 17) as u32),
                save_position: if i == active { saved } else { 0x11223344 },
                replay_target: if i == active { target } else { 0x55667788 },
                response_data: vec![0x23, 0x04, 0x56],
            }),
        )
        .unwrap(),
        SaveDate {
            year: 65535,
            month: 255,
            day: 17,
            hour: 25,
            minute: 61,
            second: 73,
        },
        vec![65535, 513, 7],
        vec![3, 4, 255],
    )
}
fn programs() -> Vec<Vec<u8>> {
    let text = vec![0x86, 1, 0, 0, 1, 0x7f, 2, 0, b'A', 0];
    let choice = vec![
        0x8d, 0, 0, 0, 0, 31, 0, 7, 1, 0, 7, b'A', 0, b'B', 0, b'C', 0, 0,
    ];
    let prefix = vec![0xc1, 7, 1, 0, 0, 0xc3, 7, 0, 1, 12];
    let mut after_text = text.clone();
    after_text.extend(choice.clone());
    after_text.extend([0xc3, 7, 0, 1, 0xc0, 31]);
    let mut choices = choice.clone();
    choices.extend(choice);
    choices.extend([0xc3, 7, 0, 1, 0xc0, 31]);
    let mut response = vec![0x81, 31, 0, 7, 0x83, 0, 4, 0xda, 30, 0, 0, 0xdb, 0, 7];
    response.extend([0xc3, 7, 0, 1, 0xc0, 31, 0xc3, 7, 1, 1, 0xc0, 30]);
    let mut result = vec![
        vec![],
        vec![0, 1, 7],
        vec![0xa0, 0, 2, b'A', 0, 0xa0, 1, 2, b'B', 0, 0xa1],
        vec![0xa0, 0, 2, b'A', 0, 0xa1, 0xa0, 0, 2, b'B', 0, 0xa1],
        vec![0xa2, 0, 0x86, 1, 0, 0, 1, 0x7f, 2, 0, b'A', 0, 0xa3, 0xa1],
        after_text,
        choices,
        response,
        vec![0xc5, 7, 0xc3, 7, 0, 1, 22, 0x8e, 0, 0, 0, 0, 0xa1],
        vec![0xc5, 0x7e, 0xc3, 0x7e, 0, 1, 22, 0x8e, 0, 0, 0, 0, 0xa1],
        vec![
            0xc3, 7, 28, 1, 33, 0xc3, 7, 31, 1, 44, 0xc3, 7, 29, 1, 55, 0xa1,
        ],
        vec![
            0xc9, 7, 2, 3, 4, 5, 0xc5, 7, 0xc9, 7, 6, 7, 8, 9, 0x8e, 0, 0, 0, 0, 0xa1,
        ],
        vec![0xc1, 9, 2, 0, 1, 0, 0xc6, 7, 9, 0xa1],
        vec![0xc7, 9, 7, 0xc3, 0x7c, 5, 1, 33, 0xa1],
        vec![0xd1, 18, 0, 31, 0, 7, 0xd1, 33, 0, 30, 0, 5, 0xa1],
        vec![0xd0, 3, 2, 3, 4, 0xa1],
        vec![0xd4, 0, 0xd5, 0xa1],
        vec![0xd7, 31, 0, 0, 0, 0, 0xc3, 7, 0, 1, 0xc0, 31, 0xa1],
    ];
    for program in &mut result {
        let mut final_code = prefix.clone();
        final_code.append(program);
        final_code.extend([0, 0, 0]);
        *program = final_code;
    }
    result
}
fn words(saved: &Hydrated) -> Vec<u32> {
    saved
        .records
        .iter()
        .flatten()
        .chain(&saved.root)
        .chain(&saved.screen)
        .map(|&v| v as u32)
        .collect()
}

#[test]
#[ignore = "actual updated d2d74/d2ef4/c049c/d3200; bounded scenario provider; no graphics/audio/windows/saves"]
fn higurashi_saved_preview_vm_matches_native_suppression_boundaries_and_metadata() {
    let mut input = String::new();
    let mut expected = Vec::new();
    for (program_id, code) in programs().iter().enumerate() {
        let scenario = scenario(code);
        for active in 0..2 {
            for saved in 0..7 {
                for target in [0, 999] {
                    let data = data(active, saved, target, 42);
                    let before = data.clone();
                    let snapshot = Hydrated::replay(&scenario, &data).unwrap_or_else(|error|
                        panic!("program{program_id} saved{saved} active{active} target{target}: {error:#}"));
                    assert_eq!(data, before, "source mutation");
                    let code = code.iter().map(|b| format!("{b:02x}")).collect::<String>();
                    writeln!(input, "R {active} 42 {saved} {target} 0 230456 {code}").unwrap();
                    expected.push((program_id, saved, snapshot));
                }
            }
        }
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_SAVE_PREVIEW").unwrap())
        .arg(std::env::var("SHIN_HIGURASHI_EXE").unwrap())
        .arg(std::env::var("SHIN_HIGURASHI_FONT").unwrap())
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
    let mut checked = 0;
    for (case, (line, (program, saved, snapshot))) in text.lines().zip(&expected).enumerate() {
        let mut tokens = line.split_whitespace();
        assert_eq!(tokens.next(), Some("R"));
        for (lane, word) in words(snapshot).into_iter().enumerate() {
            let native: u32 = tokens.next().unwrap().parse().unwrap();
            // d55b8 unconditionally copies six stack lanes after its length.
            // Unused TIPS params have no initialized meaning or factory consumer.
            let unused = snapshot.records.get(lane / 79).is_some_and(|row| {
                row[0] == 12 && (8 + row[7] as usize..14).contains(&(lane % 79))
            });
            if !unused {
                assert_eq!(
                    word, native,
                    "case{case} program{program} saved{saved} word{lane}"
                );
                checked += 1;
            }
        }
        for label in &snapshot.labels {
            let n: usize = tokens.next().unwrap().parse().unwrap();
            let bytes = tokens
                .by_ref()
                .take(n)
                .map(|s| s.parse::<u8>().unwrap())
                .collect::<Vec<_>>();
            assert_eq!(label.as_bytes(), bytes, "case{case} saved SAVEINFO");
        }
        assert!(tokens.next().is_none());
    }
    eprintln!(
        "Saved preview:{} original VM/snapshot owners,{checked} meaningful exact words; saved-position vs replay-target, event/text/selection/quiz stops, variable/byte suppression, metadata, layers/back/wipe/swap/voice/temp rows; complete source owners unchanged",
        expected.len()
    );
}

#[test]
#[ignore = "requires reviewed updated scenario bytes; headless memory replay"]
fn higurashi_saved_preview_does_not_read_live_variables_or_replay_target() {
    let scenario = scenario(&[
        0x81, 31, 0, 7, 0xc1, 7, 1, 0, 0, 0xc3, 7, 0, 1, 0xc0, 31, 0xa1, 0, 0, 0,
    ]);
    let data = data(0, 1, 0, 42);
    let a = Hydrated::replay(&scenario, &data).unwrap();
    let mut altered = data.clone();
    altered.progress.entries[0].replay_target = u32::MAX;
    altered.byte_variables.fill(0xff);
    let b = Hydrated::replay(&scenario, &altered).unwrap();
    assert_eq!(words(&a), words(&b));
    assert_eq!(a.labels, b.labels);
    assert_eq!(
        a.records[0][14], 17928,
        "SGET response uses route0x2304's signed16 payload"
    );
    let mut script = HigurashiScripter::new(&scenario, 0, 42).unwrap();
    let cmd = script.run(CommandResult::None, 100).unwrap();
    assert!(matches!(cmd,Cmd::SGET { dest, .. } if dest==Register::from_regular_register(31)));
}
