//! SELECT generic layout oracle, independently executing original90FC0.
use super::*;
use crate::{
    format::{
        font::gerokasu2::Gerokasu2Font,
        rom::RomReader,
        scenario::{
            gerokasu2::{Gerokasu2Scenario, bytecode::Instruction},
            instruction_elements::CodeAddress,
            instructions::Instruction as SharedInstruction,
        },
    },
    layout::{
        LineInfo,
        commands::{CharFontType, Command},
    },
    primitives::stateless_reader::StatelessFile,
    vm::command::CompiletimeCommand,
};
use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Command as Process, Stdio},
};
fn read_assets() -> (Gerokasu2Font, Gerokasu2Font, Gerokasu2Scenario) {
    let mut rom = RomReader::new(
        StatelessFile::new(
            std::fs::File::open(std::env::var_os("SHIN_GEROKASU2_ROM").unwrap()).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let mut read = |path| {
        let mut bytes = Vec::new();
        let f = rom.find_file(path).unwrap();
        rom.open_file(f).unwrap().read_to_end(&mut bytes).unwrap();
        bytes
    };
    (
        Gerokasu2Font::new(read("/newrodin-medium.fnt").into()).unwrap(),
        Gerokasu2Font::new(read("/system.fnt").into()).unwrap(),
        Gerokasu2Scenario::new(read("/main.snr").into()).unwrap(),
    )
}

fn words(commands: &[Command], lines: &[LineInfo], size: [f32; 2]) -> Vec<u32> {
    let mut result = vec![
        commands.len() as u32,
        lines.len() as u32,
        size[0].to_bits(),
        size[1].to_bits(),
    ];
    for command in commands {
        let (kind, line) = match command {
            Command::Wait(v) => (0, v.line_index),
            Command::Char(v) => (1, v.line_index),
            Command::Section(v) => (2, v.line_index),
            Command::Sync(v) => (3, v.line_index),
            Command::Voice(v) => (4, v.line_index),
            Command::VoiceSync(v) => (5, v.line_index),
            Command::VoiceWait(v) => (6, v.line_index),
        };
        result.extend([kind, command.time().to_bits(), line as u32]);
        match command {
            Command::Wait(v) => {
                result.extend([u32::from(v.is_last_wait), u32::from(v.is_auto_click)])
            }
            Command::Char(c) => {
                result.extend([
                    match c.font {
                        CharFontType::Regular => 0,
                        CharFontType::Bold => 0,
                    },
                    u32::from(c.codepoint),
                    u32::from(c.is_rubi),
                    u32::from(c.cant_be_at_line_start),
                    u32::from(c.cant_be_at_line_end),
                    u32::from(c.has_rubi),
                ]);
                result.extend(
                    [
                        c.width,
                        c.height,
                        c.position.x,
                        c.position.y,
                        c.horizontal_scale,
                        c.scale,
                    ]
                    .map(f32::to_bits),
                );
                let rgba = c.color.0;
                result.push((rgba & 0xff00ff00) | ((rgba & 255) << 16) | ((rgba >> 16) & 255));
                result.push(c.fade.to_bits());
            }
            Command::Section(v) => result.push(v.index),
            Command::Sync(v) => result.push(v.index),
            Command::Voice(v) => {
                let bytes = v.filename.as_bytes();
                result.push(bytes.len() as u32);
                result.extend(bytes.iter().map(|v| u32::from(*v)));
                result.extend([
                    v.volume.to_bits(),
                    u32::from(v.lipsync_enabled),
                    v.segment_duration,
                ]);
            }
            Command::VoiceSync(v) => result.extend([v.segment_start, v.segment_duration]),
            Command::VoiceWait(_) => {}
        }
    }
    for l in lines {
        result.extend(
            [
                l.width,
                l.y_position,
                l.line_height,
                l.baseline_ascent,
                l.rubi_height,
            ]
            .map(f32::to_bits),
        );
    }
    result
}

#[test]
#[ignore = "requires supplied ROM, exact executable and native SELECT text oracle"]
fn ger_native_select_generic_layout_defaults_controls_and_all_authored_variants() {
    let (font, _, scenario) = read_assets();
    let mut child = Process::new(
        std::env::var_os("SHIN_GEROKASU2_SELECTION_TEXT_ORACLE").expect("SELECT text oracle"),
    )
    .args([
        std::env::var_os("SHIN_GEROKASU2_EXECUTABLE").unwrap(),
        std::env::var_os("SHIN_GEROKASU2_NORMAL_FONT").unwrap(),
        std::env::var_os("SHIN_GEROKASU2_BOLD_FONT").unwrap(),
    ])
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::inherit())
    .spawn()
    .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut cases = 0;
    let mut command_count = 0;
    let mut line_count = 0;
    let mut word_count = 0;
    let mut compare = |text: &str| {
        // Oracle captures and uses the original SELECT TextLayer caller's
        // parameters. These placeholders satisfy its common input format.
        let row = format!(
            "0 0 0 0 0 0 0 0 0 0 0 0 0 {}\n",
            text.as_bytes()
                .iter()
                .map(|v| format!("{v:02x}"))
                .collect::<String>()
        );
        input.write_all(row.as_bytes()).unwrap();
        input.flush().unwrap();
        let mut answer = String::new();
        assert!(
            output.read_line(&mut answer).unwrap() > 0,
            "native SELECT text EOF case{cases} text{text:?}"
        );
        let actual = answer
            .split_whitespace()
            .map(|v| v.parse::<u32>().unwrap())
            .collect::<Vec<_>>();
        let (commands, lines, size) = choice_layouter(&font).parse(text);
        let expected = words(&commands, &lines, size.to_array());
        if actual != expected {
            let differences = actual
                .iter()
                .zip(&expected)
                .enumerate()
                .filter(|(_, (a, b))| a != b)
                .take(12)
                .collect::<Vec<_>>();
            panic!(
                "native SELECT text case{cases} text{text:?}: lengths{} vs{}; word differences{differences:?}",
                actual.len(),
                expected.len()
            );
        }
        cases += 1;
        command_count += commands.len();
        line_count += lines.len();
        word_count += actual.len();
    };
    let synthetic = [
        "",
        "A",
        "日本語の選択肢。",
        "@r",
        "@r二行目@r三行目",
        "@{bold@}regular",
        "@c900.赤@c0.黒@c.白",
        "@z150.大@z50.小@z.通常",
        "@bかんじ.@<漢字@>とルビ",
        "@vvoice_01.@+@-@/@x25.@y@|@k@w100.@t@e",
        "@[即時@]通常",
        "@a0.瞬時@a200.通常",
        "‘Quote’「括弧」……！？",
        "🐈𠮷※\u{10ffff}",
    ];
    for text in synthetic {
        compare(text);
    }
    for repeat in [20, 21, 40, 80] {
        for text in ["日本語と英語 ABC。", "（句読点、。、！？）", "漢字@r"] {
            compare(&text.repeat(repeat));
        }
    }
    let mut reader = scenario.instruction_reader(CodeAddress(scenario.header.code_offset));
    let mut selections = 0;
    let mut variants = 0;
    while (reader.position().0 as usize) < scenario.raw().len() {
        let remaining = &scenario.raw()[reader.position().0 as usize..];
        if remaining.len() < 16 && remaining.iter().all(|&b| b == 0) {
            break;
        }
        if let Instruction::Shared(SharedInstruction::Command(CompiletimeCommand::SELECT(
            command,
        ))) = reader.read().unwrap()
        {
            selections += 1;
            for label in command.variants.0 {
                variants += 1;
                compare(&label);
            }
        }
    }
    drop(compare);
    drop(input);
    assert_eq!(output.read(&mut [0; 1]).unwrap(), 0);
    assert!(child.wait().unwrap().success());
    assert!(selections > 0 && variants >= selections);
    println!(
        "GEROKASU2_SELECTION_TEXT_NATIVE cases={cases} selections={selections} authored_variants={variants} commands={command_count} lines={line_count} words={word_count} original_generic_UTF8_layout=verified"
    );
}
