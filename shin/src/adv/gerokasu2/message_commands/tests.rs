use super::super::message_clock::OutgoingWindow;
use super::*;
use shin_core::{
    format::scenario::instruction_elements::{MessageId, NumberSpec, U8Bool},
    layout::commands::{Command, Wait},
    vm::{
        IntoRuntimeForm, VmCtx,
        command::{CompiletimeCommand as C, compiletime as c},
    },
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
}
impl Native {
    fn new() -> Self {
        let mut child =
            Process::new(std::env::var_os("SHIN_GEROKASU2_MESSAGE_COMMAND_ORACLE").unwrap())
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
        }
    }
    fn check(
        &mut self,
        row: [u32; 48],
        text: &str,
        windows: &[OutgoingWindow],
        expected: [u32; 46],
    ) {
        let input = self.input.as_mut().unwrap();
        for word in row {
            input.write_all(&word.to_le_bytes()).unwrap();
        }
        input.write_all(text.as_bytes()).unwrap();
        for window in windows {
            for word in [
                window.mode as u32,
                window.slide.to_bits(),
                window.height.to_bits(),
            ] {
                input.write_all(&word.to_le_bytes()).unwrap();
            }
        }
        input.flush().unwrap();
        let mut magic = [0; 4];
        self.output.read_exact(&mut magic).unwrap();
        assert_eq!(&magic, b"GMQ1");
        let actual: [u32; 46] = std::array::from_fn(|_| {
            let mut bytes = [0; 4];
            self.output.read_exact(&mut bytes).unwrap();
            u32::from_le_bytes(bytes)
        });
        assert_eq!(
            actual,
            expected,
            "Native command case {} op={} arguments={:?}",
            self.cases,
            row[30],
            &row[33..]
        );
        self.cases += 1;
    }
    fn finish(mut self, name: &str) {
        drop(self.input.take());
        assert!(self.child.wait().unwrap().success());
        eprintln!(
            "GEROKASU2_MESSAGE_COMMANDS group={name} cases={} words={} original_factories_polls_queries_close_captured_install=verified",
            self.cases,
            self.cases * 46
        );
    }
}
impl Drop for Native {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn clock(finished: bool) -> MessageClock {
    MessageClock::new(
        if finished {
            &[]
        } else {
            &[Command::Wait(Wait {
                time: 0.0,
                line_index: 0,
                is_last_wait: true,
                is_auto_click: false,
            })]
        },
        &[],
        glam::Vec2::ZERO,
        MessageParams {
            flags: 1,
            mode: 3,
            message_id: 99,
            message_arg: 7,
            animate_open: false,
        },
    )
    .unwrap()
}
fn state(clock: &MessageClock) -> [u32; 30] {
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
        0,
        0,
        u32::from(clock.skip_speed),
    ]
}
fn shared(command: C) -> Gerokasu2RuntimeCommand {
    Gerokasu2RuntimeCommand::Shared(command.into_runtime_form(&VmCtx::new(0, 42)))
}

#[derive(Clone, Copy)]
struct Case {
    op: u32,
    a: i32,
    b: i32,
    style: i32,
    asynchronous: bool,
    skip: bool,
    voice: Option<VoiceTimeline>,
    read: bool,
    event: i32,
    started: bool,
    auto: bool,
}
impl Default for Case {
    fn default() -> Self {
        Self {
            op: 0,
            a: 0,
            b: 0,
            style: 0,
            asynchronous: false,
            skip: false,
            voice: None,
            read: false,
            event: -1,
            started: false,
            auto: false,
        }
    }
}
fn check(native: &mut Native, mut clock: MessageClock, case: Case) {
    let text = "日本語 command text with a long native string";
    let mut row = [0; 48];
    row[..30].copy_from_slice(&state(&clock));
    row[30..].copy_from_slice(&[
        case.op,
        clock.blocks.len() as u32,
        clock.outgoing.len() as u32,
        case.a as u32,
        case.b as u32,
        case.style as u32,
        u32::from(case.asynchronous),
        u32::from(case.skip),
        u32::from(case.voice.is_some()),
        u32::from(case.voice.is_some_and(|v| v.playing)),
        case.voice.map_or(0, |v| v.position as u32),
        case.voice.map_or(0, |v| (v.position >> 32) as u32),
        u32::from(case.read),
        case.event as u32,
        u32::from(case.started),
        u32::from(case.auto),
        17,
        text.len() as u32,
    ]);
    let windows = clock.outgoing.clone();
    let mut commands = MessageCommandState::new(case.style, case.asynchronous);
    commands.retained_text = Some(text.to_owned());
    let context = MessageCommandContext {
        was_read: case.read,
        event_mode: case.event,
        skip: case.skip,
        message_speed: 50,
    };
    let mut capture = [0; 16];
    capture[1] = case.style as u32;
    capture[2] = u32::from(case.asynchronous);
    capture[3] = 1;
    if case.op <= 3 {
        capture[0] = match case.op {
            0 => u32::from(clock.sync_is_waiting(case.a)),
            1 => u32::from(clock.busy()),
            2 => u32::from(clock.voice_sync_is_waiting(case.a, (case.b as i64) as u64, case.voice)),
            3 => {
                clock.close(case.a != 0);
                0
            }
            _ => unreachable!(),
        };
    } else {
        let command = match case.op {
            4 => Gerokasu2RuntimeCommand::MessageInit { style: case.a },
            5 => shared(C::MSGWAIT(c::MSGWAIT {
                signal_num: NumberSpec::constant(case.a),
            })),
            6 => shared(C::MSGSIGNAL(c::MSGSIGNAL {})),
            7 => shared(C::MSGSYNC(c::MSGSYNC {
                voice_index: NumberSpec::constant(case.a),
                sync_time: NumberSpec::constant(case.b),
            })),
            8 => shared(C::MSGCLOSE(c::MSGCLOSE {
                wait_for_close: U8Bool(case.a != 0),
            })),
            9 | 10 => Gerokasu2RuntimeCommand::MessageSet {
                msg_id: MessageId(case.a as u32),
                auto_wait: case.auto,
                message_arg: case.b,
                text: text.to_owned(),
            },
            _ => unreachable!(),
        };
        let action = if case.op == 8 && case.started && !case.asynchronous {
            commands.retained_text = None;
            commands.pending = (case.a != 0).then_some(Pending::Close);
            MessageAction::Accepted
        } else {
            commands
                .begin(&command, Some(&mut clock), context)
                .unwrap()
                .unwrap()
        };
        if case.op == 10 {
            let MessageAction::Install(install) = action else {
                panic!("Missing native installation")
            };
            capture[6] = 1;
            capture[7] = 1;
            capture[8] = 17;
            capture[9] = 1;
            capture[10..].copy_from_slice(&[
                install.params.flags,
                install.params.mode as u32,
                install.alignment as u32,
                install.params.message_id,
                install.params.message_arg as u32,
                u32::from(install.params.animate_open),
            ]);
        }
        capture[0] =
            u32::from(commands.poll(Some(&clock), case.voice) == MessageCommandStatus::Complete);
        capture[1] = commands.style() as u32;
        capture[2] = u32::from(commands.async_message_active());
        capture[3] = u32::from(commands.retained_text().is_some());
        capture[4] = if case.op == 8 {
            u32::from(case.started || !case.asynchronous)
        } else {
            u32::from(case.op >= 9)
        };
        capture[5] = u32::from(case.op >= 5 && !(case.op == 8 && case.asynchronous));
    }
    let mut expected = [0; 46];
    expected[..30].copy_from_slice(&state(&clock));
    expected[30..].copy_from_slice(&capture);
    native.check(row, text, &windows, expected);
}

#[test]
fn native_queries_and_command_polls() {
    if std::env::var_os("SHIN_GEROKASU2_MESSAGE_COMMAND_ORACLE").is_none() {
        return;
    }
    let mut native = Native::new();
    for open in [false, true] {
        for slide in [
            -1.0,
            0.0,
            0.25,
            1.0,
            2.0,
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
        ] {
            for finished in [false, true] {
                for outgoing in [
                    vec![],
                    vec![0.0],
                    vec![0.5],
                    vec![-1.0, f32::NAN, 0.2],
                    vec![f32::NAN, -1.0, 0.0],
                ] {
                    for op in [0, 1, 3, 5, 8, 9] {
                        for a in [-1, 0, 1] {
                            let mut clock = clock(finished);
                            clock.open = open;
                            clock.slide = slide;
                            clock.background = 0.8;
                            clock.modal_hidden = true;
                            clock.modal_slide = 0.25;
                            clock.outgoing = outgoing
                                .iter()
                                .map(|slide| OutgoingWindow {
                                    mode: 4,
                                    slide: *slide,
                                    height: 100.0,
                                })
                                .collect();
                            // MSGCLOSE's wire flag is a bool byte, not a signed NumberSpec.
                            check(
                                &mut native,
                                clock,
                                Case {
                                    op,
                                    a: if op == 8 { i32::from(a != 0) } else { a },
                                    auto: true,
                                    ..Default::default()
                                },
                            );
                        }
                    }
                }
            }
        }
    }
    for sections in [0, 1, 2, i32::MAX as u32, 0x80000000, u32::MAX] {
        for signal in [i32::MIN, -1, 0, 1, 2, i32::MAX] {
            for finished in [false, true] {
                for op in [0, 5] {
                    let mut clock = clock(finished);
                    clock.sections = sections;
                    check(
                        &mut native,
                        clock,
                        Case {
                            op,
                            a: signal,
                            ..Default::default()
                        },
                    );
                }
            }
        }
    }
    for total in [0, 1, 2] {
        for counter in [i32::MIN, -1, 0, 1, i32::MAX] {
            for index in [i32::MIN, -1, 0, 1, i32::MAX] {
                for time in [i32::MIN, -1, 0, 1, i32::MAX] {
                    for position in [0, 1, 1000, u64::MAX] {
                        for voice in [
                            None,
                            Some(VoiceTimeline {
                                playing: false,
                                position,
                            }),
                            Some(VoiceTimeline {
                                playing: true,
                                position,
                            }),
                        ] {
                            for op in [2, 7] {
                                let mut clock = clock(false);
                                clock.total_voices = total;
                                clock.voice_counter = counter;
                                check(
                                    &mut native,
                                    clock,
                                    Case {
                                        op,
                                        a: index,
                                        b: time,
                                        voice,
                                        ..Default::default()
                                    },
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    for asynchronous in [false, true] {
        for skip in [false, true] {
            for started in [false, true] {
                for wait in [false, true] {
                    let mut clock = clock(false);
                    if started {
                        clock.close(true);
                    }
                    check(
                        &mut native,
                        clock,
                        Case {
                            op: 8,
                            a: i32::from(wait),
                            asynchronous,
                            skip,
                            started,
                            ..Default::default()
                        },
                    );
                }
            }
        }
    }
    for syncs in [0, 1, u32::MAX] {
        let mut clock = clock(false);
        clock.syncs = syncs;
        check(
            &mut native,
            clock,
            Case {
                op: 6,
                ..Default::default()
            },
        );
    }
    native.finish("queries-polls");
}

#[test]
fn native_msgset_style_flags_id_and_callbacks() {
    if std::env::var_os("SHIN_GEROKASU2_MESSAGE_COMMAND_ORACLE").is_none() {
        return;
    }
    let mut native = Native::new();
    for style in (0..512).chain([i32::MIN, -1, i32::MAX]) {
        check(
            &mut native,
            clock(false),
            Case {
                op: 4,
                a: style,
                style: 15,
                ..Default::default()
            },
        );
        for read in [false, true] {
            for event in [i32::MIN, -1, 0, i32::MAX] {
                for skip in [false, true] {
                    for auto in [false, true] {
                        let id = match style & 3 {
                            0 => 0,
                            1 => 123,
                            2 => i32::MIN,
                            _ => -1,
                        };
                        check(
                            &mut native,
                            clock(style & 1 != 0),
                            Case {
                                op: 10,
                                a: id,
                                b: style.wrapping_mul(19),
                                style,
                                read,
                                event,
                                skip,
                                auto,
                                ..Default::default()
                            },
                        );
                    }
                }
            }
        }
    }
    native.finish("install-caller");
}

#[test]
fn pending_commands_survive_poll_and_inhibited_close_retains_text() {
    let mut state = MessageCommandState::new(0, false);
    let mut clock = clock(false);
    let context = MessageCommandContext {
        was_read: false,
        event_mode: -1,
        skip: false,
        message_speed: 50,
    };
    let set = Gerokasu2RuntimeCommand::MessageSet {
        msg_id: MessageId(5),
        auto_wait: false,
        message_arg: 0,
        text: "Retain".to_owned(),
    };
    state.begin(&set, Some(&mut clock), context).unwrap();
    state
        .begin(
            &shared(C::MSGCLOSE(c::MSGCLOSE {
                wait_for_close: U8Bool(true),
            })),
            Some(&mut clock),
            context,
        )
        .unwrap();
    assert!(clock.open);
    assert_eq!(state.retained_text(), Some("Retain"));
    assert!(!state.command_pending());
    state
        .begin(
            &shared(C::MSGWAIT(c::MSGWAIT {
                signal_num: NumberSpec::constant(-1),
            })),
            Some(&mut clock),
            context,
        )
        .unwrap();
    assert!(!state.async_message_active());
    assert_eq!(
        state.poll(Some(&clock), None),
        MessageCommandStatus::Pending
    );
    assert!(state.begin(&set, Some(&mut clock), context).is_err());
    clock.block_index = clock.blocks.len();
    clock.slide = 1.0;
    assert_eq!(
        state.poll(Some(&clock), None),
        MessageCommandStatus::Complete
    );
    state
        .begin(
            &shared(C::MSGCLOSE(c::MSGCLOSE {
                wait_for_close: U8Bool(true),
            })),
            Some(&mut clock),
            context,
        )
        .unwrap();
    assert_eq!(state.retained_text(), None);
    assert!(!clock.open);
    assert_eq!(
        state.poll(Some(&clock), None),
        MessageCommandStatus::Pending
    );
    clock.slide = 0.0;
    assert_eq!(
        state.poll(Some(&clock), None),
        MessageCommandStatus::Complete
    );
    assert_eq!(
        clock.blocks.len(),
        1,
        "Close retains the message's runtime data"
    );
}
