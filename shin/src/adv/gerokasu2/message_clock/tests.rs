use super::*;
use shin_core::{layout::commands, primitives::color::UnormColor};
use std::{
    io::{Read, Write},
    process::{Child, ChildStdin, ChildStdout, Command as Process, Stdio},
};

#[derive(Default)]
struct Effects {
    playing: bool,
    result: bool,
    words: Vec<u32>,
}
impl Effects {
    fn name(&mut self, kind: u32, name: &str) {
        self.words.extend([kind, name.len() as u32]);
        self.words.extend(name.bytes().map(u32::from));
    }
}
impl MessageEffects for Effects {
    fn voice_is_playing(&self) -> bool {
        self.playing
    }
    fn stop_voice(&mut self) {
        self.words.push(3);
    }
    fn play_voice(&mut self, r: VoiceRequest) -> bool {
        self.name(2, &r.filename);
        self.words.extend([
            r.segment_start as u32,
            (r.segment_start >> 32) as u32,
            r.segment_duration as u32,
            (r.segment_duration >> 32) as u32,
            if r.lipsync { 3 } else { 2 },
            r.volume.to_bits(),
        ]);
        self.result
    }
    fn observe_voice(&mut self, name: &str) {
        self.name(1, name);
    }
    fn completed(&mut self, id: u32) {
        self.words.extend([4, id]);
    }
}
fn params(mode: i32) -> MessageParams {
    MessageParams {
        flags: 0,
        mode,
        message_id: 123,
        message_arg: -1,
        animate_open: false,
    }
}
fn layout() -> (Vec<Command>, Vec<LineInfo>) {
    (
        vec![
            Command::Char(commands::Char {
                time: 0.0,
                line_index: 0,
                font: commands::CharFontType::Regular,
                codepoint: 'A',
                is_rubi: false,
                cant_be_at_line_start: false,
                cant_be_at_line_end: false,
                has_rubi: false,
                width: 10.0,
                height: 40.0,
                position: Vec2::ZERO,
                horizontal_scale: 0.5,
                scale: 0.5,
                color: UnormColor::WHITE,
                fade: 0.2,
            }),
            Command::Wait(commands::Wait {
                time: 0.2,
                line_index: 0,
                is_last_wait: true,
                is_auto_click: false,
            }),
        ],
        vec![LineInfo {
            width: 10.0,
            y_position: 0.0,
            line_height: 60.0,
            baseline_ascent: 55.0,
            rubi_height: 20.0,
        }],
    )
}

#[test]
fn clock_guards_shared_frames_preserves_modes_and_advances_real_glyph_fades() {
    let (commands, lines) = layout();
    let mut clock = MessageClock::new(&commands, &lines, Vec2::ZERO, params(0)).unwrap();
    let mut effects = Effects::default();
    let mut frame = FrameId::default();
    frame.advance();
    clock.update(frame, Ticks::from_f32(1.0), true, &mut effects);
    let time = clock.time;
    let progress = clock.characters[0].progress;
    clock.update(frame, Ticks::from_f32(1.0), true, &mut effects);
    assert_eq!(clock.time, time);
    assert_eq!(clock.characters[0].progress, progress);
    assert!(progress > 0.0 && progress < 1.0);
    assert_eq!(clock.params.message_arg, 9);
    clock
        .install(&commands, &lines, Vec2::ZERO, params(3))
        .unwrap();
    assert!(
        clock.outgoing.is_empty(),
        "mode0/3 transition shares its window"
    );
    clock
        .install(&commands, &lines, Vec2::ZERO, params(1))
        .unwrap();
    assert_eq!(clock.outgoing.len(), 1);
    assert_eq!(clock.height, 1080.0);
}

pub(super) fn state(clock: &MessageClock) -> [u32; 30] {
    [
        clock.params.flags,
        clock.params.mode as u32,
        clock.params.message_id,
        clock.params.message_arg as u32,
        u32::from(clock.open),
        clock.slide.to_bits(),
        clock.background.to_bits(),
        u32::from(clock.modal_hidden),
        clock.modal_slide.to_bits(),
        clock.height.to_bits(),
        clock.height_target.to_bits(),
        clock.block_index as u32,
        clock.time.to_bits(),
        clock.wait_kind as u32,
        clock.skip_delay.to_bits(),
        clock.voice_delay.to_bits(),
        u32::from(clock.voice_active),
        u32::from(clock.voice_disabled),
        clock.sections,
        clock.syncs,
        clock.ticks_since_wait.to_bits(),
        clock.cursor.x.to_bits(),
        clock.cursor.y.to_bits(),
        clock.cursor_line as u32,
        clock.total_voices,
        clock.voice_block as u32,
        clock.voice_counter as u32,
        u32::from(clock.autoplay_requested),
        u32::from(clock.force_skip_requested),
        u32::from(clock.skip_speed),
    ]
}
struct Native {
    child: Child,
    input: Option<ChildStdin>,
    output: ChildStdout,
    steps: usize,
    words: usize,
}
impl Native {
    fn new() -> Self {
        let mut child =
            Process::new(std::env::var_os("SHIN_GEROKASU2_MESSAGE_CLOCK_ORACLE").unwrap())
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
            steps: 0,
            words: 0,
        }
    }
    fn compare(
        &mut self,
        clock: &mut MessageClock,
        ticks: f32,
        animations: bool,
        effects: &mut Effects,
    ) {
        self.compare_operation(clock, ticks, animations, effects, 0, false);
    }
    fn compare_operation(
        &mut self,
        clock: &mut MessageClock,
        ticks: f32,
        animations: bool,
        effects: &mut Effects,
        operation: u32,
        keep_voice: bool,
    ) {
        let input = self.input.as_mut().unwrap();
        let mut data = Vec::new();
        let mut push = |words: &[u32]| {
            for word in words {
                data.extend(word.to_le_bytes());
            }
        };
        push(&[
            clock.characters.len() as u32,
            clock.lines.len() as u32,
            clock.blocks.len() as u32,
            clock.outgoing.len() as u32,
        ]);
        push(&state(clock));
        push(&[
            u32::from(animations),
            ticks.to_bits(),
            u32::from(effects.playing),
            u32::from(effects.result),
            operation,
            u32::from(keep_voice),
        ]);
        for c in &clock.characters {
            push(&[
                c.time.to_bits(),
                c.line as u32,
                c.block as u32,
                c.position.x.to_bits(),
                c.position.y.to_bits(),
                c.width.to_bits(),
                c.rate.to_bits(),
                c.progress.to_bits(),
                0,
            ]);
        }
        for l in &clock.lines {
            push(&l.map(f32::to_bits));
        }
        drop(push);
        for block in &clock.blocks {
            let mut words = [0; 7];
            words[1] = block.time.to_bits();
            let mut name = None;
            match &block.kind {
                BlockKind::Voice {
                    filename,
                    volume,
                    lipsync,
                    duration,
                } => {
                    words[0] = 0;
                    words[2] = volume.to_bits();
                    words[3] = u32::from(*lipsync);
                    words[4] = *duration;
                    words[6] = filename.len() as u32;
                    name = Some(filename);
                }
                BlockKind::Wait { last, auto, delay } => {
                    words[0] = 1;
                    words[2] = delay.to_bits();
                    words[3] = u32::from(*last);
                    words[4] = u32::from(*auto);
                }
                BlockKind::Section(i) => {
                    words[0] = 2;
                    words[2] = *i;
                }
                BlockKind::Sync(i) => {
                    words[0] = 3;
                    words[2] = *i;
                }
                BlockKind::VoiceSync { start, duration } => {
                    words[0] = 4;
                    words[2] = *start;
                    words[3] = *duration;
                }
                BlockKind::VoiceWait => words[0] = 5,
            }
            for word in words {
                data.extend(word.to_le_bytes());
            }
            if let Some(name) = name {
                data.extend(name.as_bytes());
            }
        }
        for w in &clock.outgoing {
            for word in [w.mode as u32, w.slide.to_bits(), w.height.to_bits()] {
                data.extend(word.to_le_bytes());
            }
        }
        input.write_all(&data).unwrap();
        let mut header = [0; 24];
        self.output.read_exact(&mut header).unwrap();
        assert_eq!(&header[..4], b"GMC1");
        let counts = header[4..20]
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect::<Vec<_>>();
        effects.words.clear();
        let result = match operation {
            0 => {
                clock.tick(ticks, animations, effects);
                false
            }
            1 => clock.try_advance(keep_voice, effects),
            2 => {
                clock.fast_forward(effects);
                false
            }
            _ => panic!("unexpected operation"),
        };
        assert_eq!(
            u32::from_le_bytes(header[20..24].try_into().unwrap()),
            u32::from(result)
        );
        assert_eq!(
            counts,
            [
                clock.characters.len() as u32,
                clock.lines.len() as u32,
                clock.blocks.len() as u32,
                clock.outgoing.len() as u32
            ]
        );
        let mut expected = state(clock).to_vec();
        expected.extend(clock.characters.iter().map(|c| c.progress.to_bits()));
        expected.extend(clock.lines.iter().map(|l| l[4].to_bits()));
        for w in &clock.outgoing {
            expected.extend([w.mode as u32, w.slide.to_bits(), w.height.to_bits()]);
        }
        let mut actual = vec![0; expected.len() * 4];
        self.output.read_exact(&mut actual).unwrap();
        let actual = actual
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect::<Vec<_>>();
        let differences = actual
            .iter()
            .zip(&expected)
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .take(10)
            .collect::<Vec<_>>();
        assert!(
            differences.is_empty(),
            "native clock step{} differences{differences:?}",
            self.steps
        );
        let mut count = [0; 4];
        self.output.read_exact(&mut count).unwrap();
        let count = u32::from_le_bytes(count) as usize;
        let mut events = vec![0; count * 4];
        self.output.read_exact(&mut events).unwrap();
        let events = events
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect::<Vec<_>>();
        assert_eq!(
            events, effects.words,
            "native clock effects step{}",
            self.steps
        );
        self.steps += 1;
        self.words += expected.len() + events.len();
    }
    fn finish(mut self, name: &str) {
        self.input.take();
        assert_eq!(self.output.read(&mut [0; 1]).unwrap(), 0);
        assert!(self.child.wait().unwrap().success());
        println!(
            "GEROKASU2_MESSAGE_CLOCK_{name} steps={} words={} original_update_state_and_effects=verified",
            self.steps, self.words
        );
    }
}

#[test]
#[ignore = "requires supplied executable/fonts and guarded native clock oracle"]
fn native_clock_fades_slides_height_waits_voice_sync_and_effects_match() {
    let (commands, lines) = layout();
    let mut native = Native::new();
    let mut effects = Effects::default();
    for mode in 0..6 {
        for flags in [0, 2] {
            for ticks in [0.0, 0.1, 0.5, 1.0, 2.5, 10.0] {
                let mut p = params(mode);
                p.flags = flags;
                let mut clock = MessageClock::new(&commands, &lines, Vec2::ZERO, p).unwrap();
                for frame in 0..60 {
                    clock.autoplay_requested = frame > 25;
                    effects.playing = frame % 7 < 3;
                    native.compare(&mut clock, ticks, true, &mut effects);
                }
            }
        }
    }
    for mode in 0..6 {
        let mut p = params(mode);
        p.animate_open = true;
        let mut clock = MessageClock::new(&commands, &lines, Vec2::ZERO, p).unwrap();
        for frame in 0..40 {
            clock.modal_hidden = (10..20).contains(&frame);
            clock.open = frame < 25;
            native.compare(&mut clock, 0.8, frame % 3 != 0, &mut effects);
        }
    }
    native.finish("SYNTHETIC");
}

fn voice_layout() -> (Vec<Command>, Vec<LineInfo>) {
    let (mut commands, mut lines) = layout();
    commands.pop();
    lines.resize_with(130, || LineInfo {
        width: 10.0,
        y_position: 0.0,
        line_height: 60.0,
        baseline_ascent: 55.0,
        rubi_height: 20.0,
    });
    lines[129].y_position = 290.25;
    lines[129].line_height = 60.25;
    commands.insert(
        0,
        Command::Voice(commands::Voice {
            time: 0.0,
            line_index: 0,
            filename: "声_日本語_very_long_voice_path_for_heap_string".into(),
            volume: 0.73,
            lipsync_enabled: true,
            segment_duration: u32::MAX,
        }),
    );
    commands.extend([
        Command::Section(commands::Section {
            time: 0.1,
            line_index: 0,
            index: 42,
        }),
        Command::Wait(commands::Wait {
            time: 0.2,
            line_index: 0,
            is_last_wait: false,
            is_auto_click: false,
        }),
        Command::Sync(commands::Sync {
            time: 0.3,
            line_index: 0,
            index: 0,
        }),
        Command::VoiceSync(commands::VoiceSync {
            time: 0.4,
            line_index: 0,
            segment_start: u32::MAX,
            segment_duration: 123456789,
        }),
        Command::VoiceWait(commands::VoiceWait {
            time: 0.5,
            line_index: 0,
        }),
        Command::Voice(commands::Voice {
            time: 0.6,
            line_index: 129,
            filename: "v".into(),
            volume: -0.25,
            lipsync_enabled: false,
            segment_duration: 0,
        }),
        Command::Wait(commands::Wait {
            time: 0.7,
            line_index: 129,
            is_last_wait: true,
            is_auto_click: false,
        }),
    ]);
    let Command::Char(c) = &mut commands[1] else {
        unreachable!()
    };
    c.line_index = 129;
    c.position = vec2(11.25, 291.25);
    (commands, lines)
}

#[test]
#[ignore = "requires supplied executable/fonts and guarded native clock oracle"]
fn native_voice_wait_sync_skip_click_and_fast_forward_matrix() {
    let (commands, lines) = voice_layout();
    let mut native = Native::new();
    let mut effects = Effects::default();
    for flags in [0, 2] {
        for block in 0..=8 {
            for wait in [-1, 0, 1] {
                for variant in 0u32..64 {
                    for operation in 0..3 {
                        let mut p = params((variant % 6) as i32);
                        p.flags = flags;
                        let mut clock =
                            MessageClock::new(&commands, &lines, Vec2::ZERO, p).unwrap();
                        clock.block_index = block;
                        clock.time = 0.65;
                        clock.wait_kind = wait;
                        clock.voice_active = variant & 1 != 0;
                        clock.voice_disabled = variant & 2 != 0;
                        clock.voice_counter = if variant & 4 != 0 { i32::MAX } else { -1 };
                        clock.syncs = if variant & 8 != 0 { 1 } else { u32::MAX };
                        clock.skip_speed = [0, 50, 100, 255][(variant % 4) as usize];
                        clock.skip_delay = if variant & 16 != 0 { 0.005 } else { 0.0 };
                        clock.voice_delay = 0.03;
                        clock.autoplay_requested = variant & 32 != 0;
                        clock.force_skip_requested = variant % 3 == 0;
                        clock.modal_hidden = variant == 17;
                        clock.modal_slide = if variant == 18 { 0.25 } else { 1.0 };
                        clock.open = variant != 19;
                        clock.slide = if variant == 20 { 0.25 } else { 1.0 };
                        clock.characters[0].progress = if variant % 3 == 1 { 0.5 } else { 1.0 };
                        clock.height_target = if variant % 3 == 2 {
                            1100.0
                        } else {
                            clock.height
                        };
                        if let BlockKind::Wait { auto, .. } = &mut clock.blocks[7].kind {
                            *auto = variant & 8 != 0;
                        }
                        clock.outgoing = vec![
                            OutgoingWindow {
                                mode: 1,
                                slide: 0.02,
                                height: 1080.0,
                            },
                            OutgoingWindow {
                                mode: 2,
                                slide: 0.25,
                                height: 259.0,
                            },
                        ];
                        effects.playing = variant & 4 != 0;
                        effects.result = variant & 8 != 0;
                        native.compare_operation(
                            &mut clock,
                            2.5,
                            true,
                            &mut effects,
                            operation,
                            variant & 16 != 0,
                        );
                    }
                }
            }
        }
    }
    // Stateful chains exercise voice completion between segments and clicks,
    // instead of assuming a single snapshot's backend status stays constant.
    for variant in 0..24 {
        let mut clock =
            MessageClock::new(&commands, &lines, Vec2::ZERO, params(variant % 6)).unwrap();
        for frame in 0..100 {
            effects.playing = frame % 11 < 3;
            effects.result = variant % 2 != 0;
            clock.syncs = u32::from(frame > 25);
            clock.autoplay_requested = frame > 40;
            clock.force_skip_requested = variant % 3 == 0 && frame == 50;
            let op = if frame % 13 == 0 {
                1
            } else if variant % 3 == 1 && frame == 75 {
                2
            } else {
                0
            };
            native.compare_operation(
                &mut clock,
                1.25,
                frame % 5 != 0,
                &mut effects,
                op,
                variant % 2 == 0,
            );
        }
    }
    native.finish("VOICE_ADVANCE");
}

#[test]
#[ignore = "requires supplied ROM/executable/fonts and guarded native clock oracle"]
fn native_authored_message_clocks_match_original_update() {
    use shin_core::{
        format::{
            font::gerokasu2::Gerokasu2Font,
            rom::RomReader,
            scenario::{
                gerokasu2::{Gerokasu2Scenario, bytecode::Instruction},
                instruction_elements::CodeAddress,
            },
        },
        primitives::stateless_reader::StatelessFile,
    };
    let mut rom = RomReader::new(
        StatelessFile::new(
            std::fs::File::open(std::env::var_os("SHIN_GEROKASU2_ROM").unwrap()).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let mut read = |path| {
        let file = rom.find_file(path).unwrap();
        let mut b = Vec::new();
        rom.open_file(file).unwrap().read_to_end(&mut b).unwrap();
        b
    };
    let normal = Gerokasu2Font::new(read("/newrodin-medium.fnt").into()).unwrap();
    let bold = Gerokasu2Font::new(read("/system.fnt").into()).unwrap();
    let scenario = Gerokasu2Scenario::new(read("/main.snr").into()).unwrap();
    let mut reader = scenario.instruction_reader(CodeAddress(scenario.header.code_offset));
    let mut native = Native::new();
    let mut effects = Effects::default();
    let mut messages = 0;
    while (reader.position().0 as usize) < scenario.raw().len() {
        let tail = &scenario.raw()[reader.position().0 as usize..];
        if tail.len() < 16 && tail.iter().all(|&b| b == 0) {
            break;
        }
        if let Instruction::MessageSet { text, .. } = reader.read().unwrap() {
            let (commands, lines, size) = shin_core::layout::gerokasu2::message_layouter(
                &normal,
                &bold,
                0,
                shin_core::vm::command::types::MessageTextLayout::Justify,
                50,
            )
            .parse(text.as_str());
            let mut clock = MessageClock::new(&commands, &lines, size, params(0)).unwrap();
            for frame in 0..16 {
                clock.syncs = 100;
                clock.autoplay_requested = frame > 10;
                native.compare(&mut clock, 2.5, true, &mut effects);
            }
            messages += 1;
        }
    }
    assert_eq!(messages, 5926);
    native.finish("SUPPLIED");
}
