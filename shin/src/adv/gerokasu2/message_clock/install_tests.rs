//! Native MSGSET installation starts with retained scalar/window state, owns
//! original runtime vectors across requests, and executes their real reset.
use super::*;
use shin_core::{
    format::{
        font::gerokasu2::Gerokasu2Font,
        rom::RomReader,
        scenario::{
            gerokasu2::{Gerokasu2Scenario, bytecode::Instruction},
            instruction_elements::CodeAddress,
        },
    },
    layout::commands::CharFontType,
    primitives::stateless_reader::StatelessFile,
    vm::command::types::MessageTextLayout,
};
use std::{
    io::{Read, Write},
    process::{Child, ChildStdin, ChildStdout, Command as Process, Stdio},
};

struct Native {
    child: Child,
    input: Option<ChildStdin>,
    output: ChildStdout,
    cases: usize,
    words: usize,
    characters: usize,
    blocks: usize,
}
impl Native {
    fn new() -> Self {
        let mut child =
            Process::new(std::env::var_os("SHIN_GEROKASU2_MESSAGE_INSTALL_ORACLE").unwrap())
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
            output: child.stdout.take().unwrap(),
            child,
            cases: 0,
            words: 0,
            characters: 0,
            blocks: 0,
        }
    }
    fn words(&mut self, expected: &[u32], scope: &str) {
        let mut bytes = vec![0; expected.len() * 4];
        self.output.read_exact(&mut bytes).unwrap();
        let actual = bytes
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect::<Vec<_>>();
        let diffs = actual
            .iter()
            .zip(expected)
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .take(10)
            .collect::<Vec<_>>();
        assert!(
            diffs.is_empty(),
            "install case{} {scope} differences{diffs:?}",
            self.cases
        );
        self.words += expected.len();
    }
    fn compare(
        &mut self,
        clock: &mut MessageClock,
        normal: &Gerokasu2Font,
        bold: &Gerokasu2Font,
        source: &str,
        params: MessageParams,
        alignment: MessageTextLayout,
        speed: u8,
    ) {
        let mut input = Vec::new();
        for word in super::tests::state(clock).into_iter().chain([
            params.flags,
            params.mode as u32,
            alignment as u32,
            params.message_id,
            params.message_arg as u32,
            u32::from(speed) | (u32::from(params.animate_open) << 8),
            clock.outgoing.len() as u32,
            source.len() as u32,
        ]) {
            input.extend(word.to_le_bytes());
        }
        input.extend(source.as_bytes());
        for window in &clock.outgoing {
            for word in [
                window.mode as u32,
                window.slide.to_bits(),
                window.height.to_bits(),
            ] {
                input.extend(word.to_le_bytes());
            }
        }
        self.input.as_mut().unwrap().write_all(&input).unwrap();
        let (commands, lines, size) = shin_core::layout::gerokasu2::message_layouter(
            normal,
            bold,
            params.mode,
            alignment,
            speed,
        )
        .parse(source);
        clock.install(&commands, &lines, size, params).unwrap();
        let mut magic = [0; 4];
        self.output.read_exact(&mut magic).unwrap();
        assert_eq!(&magic, b"GMI1");
        self.words(
            &[
                clock.characters.len() as u32,
                clock.lines.len() as u32,
                clock.blocks.len() as u32,
                clock.outgoing.len() as u32,
            ],
            "counts",
        );
        self.words(&super::tests::state(clock), "clock/window/requests");
        assert_eq!(clock.text_size, size, "retained parser layout size");
        self.words(
            &[
                clock.first_line_width.to_bits(),
                size.x.to_bits(),
                size.y.to_bits(),
            ],
            "layout size/name width",
        );
        let mut expected = Vec::new();
        let mut glyphs = Vec::new();
        for (index, (c, runtime)) in commands
            .iter()
            .filter_map(|c| {
                if let Command::Char(c) = c {
                    Some(c)
                } else {
                    None
                }
            })
            .zip(&clock.characters)
            .enumerate()
        {
            let color = c.color.0;
            let argb = (color & 0xff00ff00) | ((color & 255) << 16) | ((color >> 16) & 255);
            expected.extend([
                runtime.time.to_bits(),
                0,
                c.line_index as u32,
                0,
                u32::from(c.is_rubi),
                c.position.x.to_bits(),
                c.position.y.to_bits(),
                c.width.to_bits(),
                c.height.to_bits(),
                c.horizontal_scale.to_bits(),
                c.scale.to_bits(),
                argb,
                runtime.rate.to_bits(),
                0,
                runtime.block as u32,
                (index * 4) as u32,
            ]);
            glyphs.extend([
                match c.font {
                    CharFontType::Regular => 0,
                    CharFontType::Bold => 1,
                },
                c.codepoint as u32,
            ]);
        }
        self.words(&expected, "complete initial character records");
        self.words(
            &clock
                .lines
                .iter()
                .flat_map(|l| l.map(f32::to_bits))
                .collect::<Vec<_>>(),
            "runtime lines",
        );
        for block in &clock.blocks {
            let mut row = [0; 7];
            row[1] = block.time.to_bits();
            let mut name = None;
            match &block.kind {
                BlockKind::Voice {
                    filename,
                    volume,
                    lipsync,
                    duration,
                } => {
                    row[0] = 0;
                    row[2] = volume.to_bits();
                    row[3] = u32::from(*lipsync);
                    row[4] = *duration;
                    row[6] = filename.len() as u32;
                    name = Some(filename);
                }
                BlockKind::Wait { last, auto, delay } => {
                    row[0] = 1;
                    row[2] = delay.to_bits();
                    row[3] = u32::from(*last);
                    row[4] = u32::from(*auto);
                }
                BlockKind::Section(i) => {
                    row[0] = 2;
                    row[2] = *i;
                }
                BlockKind::Sync(i) => {
                    row[0] = 3;
                    row[2] = *i;
                }
                BlockKind::VoiceSync { start, duration } => {
                    row[0] = 4;
                    row[2] = *start;
                    row[3] = *duration;
                }
                BlockKind::VoiceWait => row[0] = 5,
            }
            self.words(&row, "constructed typed block");
            if let Some(name) = name {
                let mut bytes = vec![0; name.len()];
                self.output.read_exact(&mut bytes).unwrap();
                assert_eq!(bytes, name.as_bytes());
            }
        }
        let windows = clock
            .outgoing
            .iter()
            .flat_map(|w| [w.mode as u32, w.slide.to_bits(), w.height.to_bits()])
            .collect::<Vec<_>>();
        self.words(&windows, "retained/new outgoing windows");
        self.words(&glyphs, "font roles/codepoints/handle order");
        self.cases += 1;
        self.characters += clock.characters.len();
        self.blocks += clock.blocks.len();
    }
    fn finish(mut self, name: &str) {
        self.input.take();
        assert_eq!(self.output.read(&mut [0; 1]).unwrap(), 0);
        assert!(self.child.wait().unwrap().success());
        println!(
            "GEROKASU2_MESSAGE_INSTALL_{name} cases={} characters={} blocks={} words={} original_reset_parser_layout_runtime_install_ownership=verified",
            self.cases, self.characters, self.blocks, self.words
        );
    }
}
fn assets() -> (Gerokasu2Font, Gerokasu2Font, Gerokasu2Scenario) {
    let mut rom = RomReader::new(
        StatelessFile::new(
            std::fs::File::open(std::env::var_os("SHIN_GEROKASU2_ROM").unwrap()).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let mut read = |path| {
        let mut bytes = Vec::new();
        let file = rom.find_file(path).unwrap();
        rom.open_file(file)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        bytes
    };
    (
        Gerokasu2Font::new(read("/newrodin-medium.fnt").into()).unwrap(),
        Gerokasu2Font::new(read("/system.fnt").into()).unwrap(),
        Gerokasu2Scenario::new(read("/main.snr").into()).unwrap(),
    )
}
fn params(mode: i32) -> MessageParams {
    MessageParams {
        flags: 0,
        mode,
        message_id: 42,
        message_arg: -1,
        animate_open: false,
    }
}

#[test]
#[ignore = "requires supplied ROM/executable/fonts and guarded native install oracle"]
fn native_all_supplied_message_installs_build_runtime_characters_blocks_lines_and_sizes() {
    let (normal, bold, scenario) = assets();
    let mut native = Native::new();
    let mut clock = MessageClock::new(&[], &[], Vec2::ZERO, params(0)).unwrap();
    let mut reader = scenario.instruction_reader(CodeAddress(scenario.header.code_offset));
    let mut messages = 0;
    while (reader.position().0 as usize) < scenario.raw().len() {
        let tail = &scenario.raw()[reader.position().0 as usize..];
        if tail.len() < 16 && tail.iter().all(|&b| b == 0) {
            break;
        }
        if let Instruction::MessageSet { text, .. } = reader.read().unwrap() {
            for mode in 0..6 {
                let mut p = params(mode);
                p.message_id = messages;
                // Exercise retained modal/background/height/cursor/requests,
                // plus real native destruction/reuse of the prior text.
                clock.outgoing.clear();
                clock.slide = 0.75;
                clock.background = 0.25;
                clock.height = 301.25;
                clock.height_target = 350.0;
                clock.cursor = vec2(123.5, 456.25);
                clock.voice_active = true;
                clock.voice_disabled = true;
                clock.autoplay_requested = true;
                clock.force_skip_requested = true;
                native.compare(
                    &mut clock,
                    &normal,
                    &bold,
                    text.as_str(),
                    p,
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

#[test]
#[ignore = "requires supplied ROM/executable/fonts and guarded native install oracle"]
fn native_window_transitions_invalid_modes_flags_empty_ruby_voice_and_request_retention() {
    let (normal, bold, _) = assets();
    let mut native = Native::new();
    let mut clock = MessageClock::new(&[], &[], Vec2::ZERO, params(0)).unwrap();
    let texts = [
        "",
        "@r",
        "名前@r@v声_長いファイル名_01234567890123456789.@+台詞@-@/@x25.@y@|@k終わり",
        "@r@bかんじ.@<漢字@>と@{bold@}と@[即時@]通常@w100.@t並行@e",
        "@r@c0.無色@a0.瞬時@a200.戻る@z150.大きい@z.標準",
    ];
    for previous in [-1, 0, 1, 2, 3, 4, 5, 31] {
        for next in [-1, 0, 1, 2, 3, 4, 5, 31] {
            for variant in 0..8 {
                for text in texts {
                    clock.params.mode = previous;
                    clock.slide = if variant & 1 == 0 { 0.0 } else { 0.625 };
                    clock.background = 0.375;
                    clock.height = 300.25;
                    clock.height_target = 400.5;
                    clock.modal_hidden = variant & 2 != 0;
                    clock.modal_slide = 0.25;
                    clock.autoplay_requested = true;
                    clock.force_skip_requested = true;
                    clock.outgoing.clear();
                    clock.outgoing.push(OutgoingWindow {
                        mode: 2,
                        slide: 0.25,
                        height: 259.0,
                    });
                    let mut p = params(next);
                    p.flags = variant;
                    p.animate_open = variant & 4 != 0;
                    p.message_arg = if variant & 2 == 0 { -5 } else { 123 };
                    native.compare(
                        &mut clock,
                        &normal,
                        &bold,
                        text,
                        p,
                        [
                            MessageTextLayout::Left,
                            MessageTextLayout::Center,
                            MessageTextLayout::Right,
                            MessageTextLayout::Justify,
                        ][(variant % 4) as usize],
                        [0, 50, 100, 255][(variant % 4) as usize],
                    );
                }
            }
        }
    }
    native.finish("SYNTHETIC");
}
