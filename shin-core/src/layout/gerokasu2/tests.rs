use super::*;
use crate::{
    format::{
        font::gerokasu2::Gerokasu2Font,
        rom::RomReader,
        scenario::{
            gerokasu2::{Gerokasu2Scenario, bytecode::Instruction},
            instruction_elements::CodeAddress,
        },
    },
    layout::{
        LineInfo,
        commands::{CharFontType, Command},
    },
    primitives::stateless_reader::StatelessFile,
};
use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Child, ChildStdin, ChildStdout, Command as Process, Stdio},
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

struct Native {
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
    cases: usize,
    commands: usize,
    lines: usize,
    words: usize,
}
impl Native {
    fn new() -> Self {
        let mut child = Process::new(
            std::env::var_os("SHIN_GEROKASU2_MESSAGE_LAYOUT_ORACLE").expect("layout oracle"),
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
        Self {
            input: child.stdin.take(),
            output: BufReader::new(child.stdout.take().unwrap()),
            child,
            cases: 0,
            commands: 0,
            lines: 0,
            words: 0,
        }
    }
    fn compare(
        &mut self,
        normal: &Gerokasu2Font,
        bold: &Gerokasu2Font,
        text: &str,
        mode: i32,
        alignment: MessageTextLayout,
        speed: u8,
    ) {
        let row = format!(
            "{mode} 7 {:x} {} 0 0 {:x} {:x} {:x} {:x} 999 {} 200 {}\n",
            1200f32.to_bits(),
            alignment as i32,
            4f32.to_bits(),
            20f32.to_bits(),
            40f32.to_bits(),
            1f32.to_bits(),
            if mode == 2 { 100 } else { speed },
            text.as_bytes()
                .iter()
                .map(|v| format!("{v:02x}"))
                .collect::<String>()
        );
        self.input
            .as_mut()
            .unwrap()
            .write_all(row.as_bytes())
            .unwrap();
        let mut answer = String::new();
        assert!(
            self.output.read_line(&mut answer).unwrap() > 0,
            "native layout EOF case{} text{text:?}",
            self.cases
        );
        let actual = answer
            .split_whitespace()
            .map(|v| v.parse::<u32>().unwrap())
            .collect::<Vec<_>>();
        let (commands, lines, size) =
            message_layouter(normal, bold, mode, alignment, speed).parse(text);
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
                "native message layout case{} mode{mode} alignment{alignment:?} speed{speed} text{text:?}: lengths{} vs{}; word differences{differences:?}",
                self.cases,
                actual.len(),
                expected.len()
            );
        }
        self.cases += 1;
        self.commands += commands.len();
        self.lines += lines.len();
        self.words += actual.len();
    }
    fn finish(mut self, name: &str) {
        self.input.take();
        assert_eq!(self.output.read(&mut [0; 1]).unwrap(), 0);
        assert!(self.child.wait().unwrap().success());
        println!(
            "GEROKASU2_MESSAGE_LAYOUT_{name} cases={} commands={} lines={} words={} original_UTF8_parser_and_layout=verified",
            self.cases, self.commands, self.lines, self.words
        );
    }
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
                        CharFontType::Bold => 1,
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
#[ignore = "requires supplied ROM, executable and guarded native message layout oracle"]
fn native_synthetic_UTF8_tags_names_modes_and_alignment_match_shared_layout() {
    let (normal, bold, _) = read_assets();
    let mut native = Native::new();
    let texts = [
        "",
        "@r",
        "Name@rHello あ！",
        "名前@r「日本語と英語 ABC。」",
        "人物@r🐈𠮷※\u{10ffff}",
        "人@r@v声_01.@+台詞@-@/@x25.@y@|@k終わり",
        "@r@c900.赤@c0.黒@c.白@z150.大@z.元@a0.瞬時@a200.戻る",
        "@r@bかんじ.@<漢字@>と@{bold@}と@[即時@]通常@w100.@t並行@e",
        "名前がとても長いので横幅を調整する必要があります@r『引用（入れ子）です。』@r次の行。",
    ];
    for text in texts {
        for mode in 0..6 {
            for alignment in [
                MessageTextLayout::Justify,
                MessageTextLayout::Left,
                MessageTextLayout::Center,
                MessageTextLayout::Right,
            ] {
                for speed in [0, 50, 100] {
                    native.compare(&normal, &bold, text, mode, alignment, speed);
                }
            }
        }
    }
    native.finish("SYNTHETIC");
}

#[test]
#[ignore = "requires supplied ROM, executable and guarded native message layout oracle"]
fn native_all_authored_messages_match_shared_UTF8_parser_layout_and_profiles() {
    let (normal, bold, scenario) = read_assets();
    let mut reader = scenario.instruction_reader(CodeAddress(scenario.header.code_offset));
    let mut native = Native::new();
    let mut messages = 0;
    while (reader.position().0 as usize) < scenario.raw().len() {
        let remaining = &scenario.raw()[reader.position().0 as usize..];
        if remaining.len() < 16 && remaining.iter().all(|&b| b == 0) {
            break;
        }
        if let Instruction::MessageSet { text, .. } = reader.read().unwrap() {
            for mode in 0..6 {
                native.compare(
                    &normal,
                    &bold,
                    text.as_str(),
                    mode,
                    MessageTextLayout::Justify,
                    50,
                );
            }
            messages += 1;
        }
    }
    assert_eq!(messages, 5926);
    native.finish("SUPPLIED");
}
