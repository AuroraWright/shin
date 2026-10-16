use super::*;
use shin_core::{
    format::scenario::instruction_elements::{MessageId, NumberSpec},
    vm::{
        IntoRuntimeForm, VmCtx,
        command::{CompiletimeCommand, compiletime},
    },
};
use std::{
    io::{Read, Write},
    process::{Command as Process, Stdio},
};

fn shared(command: CompiletimeCommand) -> Cmd {
    Cmd::Shared(command.into_runtime_form(&VmCtx::new(0, 42)))
}

#[test]
fn ger_descriptors_clone_retains_constructor_targets_and_rejects_aliased_slots() {
    let mut descriptors = Descriptors::default();
    descriptors
        .prepare(
            &Cmd::Audio(Audio::BgmPlay {
                id: 7,
                fade: 99,
                loops: 0,
                volume: 2500,
            }),
            false,
        )
        .unwrap();
    descriptors
        .prepare(
            &Cmd::MessageSet {
                msg_id: MessageId(1),
                auto_wait: false,
                message_arg: 44,
                text: "".into(),
            },
            true,
        )
        .unwrap();
    descriptors.auxiliary_words = vec![0, u32::MAX, 42];
    let snapshot = descriptors.clone();
    descriptors
        .prepare(&Cmd::Audio(Audio::BgmStop { fade: 1 }), false)
        .unwrap();
    assert_eq!(snapshot.music.id, 7);
    assert_eq!(snapshot.music.volume, 2.5);
    assert_eq!(snapshot.message.text.as_deref(), Some(""));
    assert_eq!(snapshot.current_was_read, 1);
    assert_eq!(snapshot.auxiliary_words, [0, u32::MAX, 42]);
    assert_eq!(
        descriptors.music.volume, 2.5,
        "STOP preserves stale parameters"
    );
    assert!(
        descriptors
            .prepare(&Cmd::Audio(Audio::SeStop { slot: 32, fade: 0 }), false)
            .is_err()
    );
    assert!(
        descriptors
            .prepare(&Cmd::Audio(Audio::SeStop { slot: -1, fade: 0 }), false)
            .is_err()
    );
}

struct Native {
    child: std::process::Child,
    input: Option<std::process::ChildStdin>,
    output: std::process::ChildStdout,
    descriptor: Descriptors,
    snapshots: usize,
    words: usize,
    captures: u32,
    boundary: u32,
    captured: [u32; 8],
}

impl Native {
    fn new() -> Self {
        let mut child = Process::new(
            std::env::var_os("SHIN_GEROKASU2_DESCRIPTORS_ORACLE").expect("descriptor oracle"),
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
            output: child.stdout.take().unwrap(),
            child,
            descriptor: Descriptors::default(),
            snapshots: 0,
            words: 0,
            captures: 0,
            boundary: 0,
            captured: [0; 8],
        }
    }
    fn compare(
        &mut self,
        op: u32,
        args: &[i32],
        command: Option<Cmd>,
        read: bool,
        text: Option<&str>,
    ) {
        let mut request = [0u32; 16];
        request[0] = op;
        for (slot, value) in request[1..].iter_mut().zip(args) {
            *slot = *value as u32;
        }
        request[14] = u32::from(read);
        if let Some(text) = text {
            request[5] = text.len() as u32;
        }
        let input = self.input.as_mut().unwrap();
        for word in request {
            input.write_all(&word.to_le_bytes()).unwrap();
        }
        if let Some(text) = text {
            input.write_all(text.as_bytes()).unwrap();
        }
        input.flush().unwrap();
        if op == 0 {
            self.descriptor = Descriptors::default();
            self.captures = 0;
            self.boundary = 0;
            self.captured = [0; 8];
        }
        if op == 10 {
            let d = &self.descriptor;
            self.captured = [
                u32::from(d.message.asynchronous),
                d.message.style as u32,
                u32::from(d.message.text.is_some()),
                d.message.face as u32,
                u32::from(d.current_was_read),
                u32::from(d.restart_required),
                u32::from(d.system_controls),
                self.boundary,
            ];
            self.captures += 1;
            self.boundary = self.boundary.wrapping_add(1);
        }
        if let Some(command) = command {
            self.descriptor.prepare(&command, read).unwrap();
        }
        let mut prefix = [0u8; 8];
        self.output.read_exact(&mut prefix).unwrap();
        assert_eq!(&prefix[..4], b"GDS1");
        let count = u32::from_le_bytes(prefix[4..].try_into().unwrap()) as usize;
        assert_eq!(count, 242);
        let mut bytes = vec![0u8; count * 4];
        self.output.read_exact(&mut bytes).unwrap();
        let actual = bytes
            .chunks_exact(4)
            .map(|v| u32::from_le_bytes(v.try_into().unwrap()))
            .collect::<Vec<_>>();
        let d = &self.descriptor;
        let mut expected = vec![d.music.id as u32, d.music.volume.to_bits(), d.music.count];
        for e in d.effects {
            expected.extend([
                e.id as u32,
                e.volume.to_bits(),
                e.pan.to_bits(),
                e.speed.to_bits(),
            ]);
        }
        let text = d.message.text.as_deref();
        expected.extend([
            u32::from(d.message.asynchronous),
            d.message.style as u32,
            d.message.face as u32,
            u32::from(d.current_was_read),
            u32::from(d.restart_required),
            u32::from(d.system_controls),
            u32::from(text.is_some()),
            text.unwrap_or("").len() as u32,
        ]);
        expected.extend(d.message_properties.iter().map(|&v| v as u32));
        expected.extend(d.default_transition.iter().map(|&v| v as u32));
        expected.extend(d.opaque_tail);
        if actual != expected {
            let differences = actual
                .iter()
                .zip(&expected)
                .enumerate()
                .filter(|(_, (a, b))| a != b)
                .take(12)
                .collect::<Vec<_>>();
            panic!(
                "native descriptor snapshot{} op{op} differences{differences:?}",
                self.snapshots
            );
        }
        let mut actual_text = vec![0; actual[138] as usize];
        self.output.read_exact(&mut actual_text).unwrap();
        assert_eq!(actual_text, text.unwrap_or("").as_bytes());
        let mut capture = [0; 36];
        self.output.read_exact(&mut capture).unwrap();
        let values = capture
            .chunks_exact(4)
            .map(|v| u32::from_le_bytes(v.try_into().unwrap()))
            .collect::<Vec<_>>();
        assert_eq!(values[0], self.captures);
        assert_eq!(values[1..], self.captured);
        self.snapshots += 1;
        self.words += actual.len() + values.len();
    }
    fn finish(mut self) {
        self.input.take();
        assert_eq!(self.output.read(&mut [0; 1]).unwrap(), 0);
        assert!(self.child.wait().unwrap().success());
        println!(
            "GEROKASU2_DESCRIPTORS snapshots={} words={} original_constructor_targets_and_pre_message_checkpoint=verified",
            self.snapshots, self.words
        );
    }
}

#[test]
#[ignore = "requires exact executable, fonts and original descriptor constructor oracle"]
fn ger_native_descriptors_audio_message_sys_exit_message_properties_and_checkpoint_order() {
    let mut native = Native::new();
    native.compare(0, &[], None, false, None);
    for value in [i32::MIN, -1001, -1, 0, 1, 999, 1000, 2001, i32::MAX] {
        for loops in [-1, 0, 1] {
            native.compare(
                1,
                &[7, 99, loops, value],
                Some(Cmd::Audio(Audio::BgmPlay {
                    id: 7,
                    fade: 99,
                    loops,
                    volume: value,
                })),
                false,
                None,
            );
            native.compare(
                3,
                &[value, 100],
                Some(Cmd::Audio(Audio::BgmVolume {
                    volume: value,
                    fade: 100,
                })),
                false,
                None,
            );
            native.compare(
                2,
                &[0],
                Some(Cmd::Audio(Audio::BgmStop { fade: 0 })),
                false,
                None,
            );
            for slot in [0, 1, 31] {
                native.compare(
                    4,
                    &[
                        slot,
                        3,
                        20,
                        loops,
                        value,
                        value.wrapping_abs().wrapping_neg(),
                        value,
                    ],
                    Some(Cmd::Audio(Audio::SePlay {
                        slot,
                        id: 3,
                        fade: 20,
                        no_repeat: loops,
                        volume: value,
                        pan: value.wrapping_abs().wrapping_neg(),
                        speed: value,
                    })),
                    false,
                    None,
                );
                native.compare(
                    7,
                    &[slot, value, 100],
                    Some(Cmd::Audio(Audio::SeVolume {
                        slot,
                        volume: value,
                        fade: 100,
                    })),
                    false,
                    None,
                );
                native.compare(
                    8,
                    &[slot, value, 10],
                    Some(Cmd::Audio(Audio::SePan {
                        slot,
                        pan: value,
                        fade: 10,
                    })),
                    false,
                    None,
                );
                native.compare(
                    5,
                    &[slot, 10],
                    Some(Cmd::Audio(Audio::SeStop { slot, fade: 10 })),
                    false,
                    None,
                );
            }
        }
    }
    native.compare(
        6,
        &[30],
        Some(Cmd::Audio(Audio::SeStopAll { fade: 30 })),
        false,
        None,
    );
    for style in [-1, 0, 4, 0x13, 0x35, i32::MIN, i32::MAX] {
        native.compare(9, &[style], Some(Cmd::MessageInit { style }), false, None);
        for face in [-1, 0, 4] {
            native.compare(13, &[face], Some(Cmd::MessageFace { face }), false, None);
            for (id, read) in [(-1, false), (3, false), (3, true)] {
                for auto_wait in [false, true] {
                    let text = if auto_wait {
                        ""
                    } else {
                        "半角ｶﾅ😀 日本語"
                    };
                    native.compare(
                        10,
                        &[i32::from(auto_wait), id, 123],
                        Some(Cmd::MessageSet {
                            msg_id: MessageId(id as u32),
                            auto_wait,
                            message_arg: 123,
                            text: text.into(),
                        }),
                        read,
                        Some(text),
                    );
                    native.compare(
                        12,
                        &[0],
                        Some(shared(CompiletimeCommand::MSGCLOSE(
                            compiletime::MSGCLOSE {
                        wait_for_close: shin_core::format::scenario::instruction_elements::U8Bool(false),
                            },
                        ))),
                        false,
                        None,
                    );
                    native.compare(
                        11,
                        &[-1],
                        Some(shared(CompiletimeCommand::MSGWAIT(compiletime::MSGWAIT {
                            signal_num: NumberSpec::constant(-1),
                        }))),
                        false,
                        None,
                    );
                    native.compare(
                        12,
                        &[1],
                        Some(shared(CompiletimeCommand::MSGCLOSE(
                            compiletime::MSGCLOSE {
                                wait_for_close:
                                    shin_core::format::scenario::instruction_elements::U8Bool(true),
                            },
                        ))),
                        false,
                        None,
                    );
                }
            }
        }
    }
    for call_id in [0, 1, -1] {
        for argument in [-1, 0, 1] {
            native.compare(
                14,
                &[call_id, argument],
                Some(shared(CompiletimeCommand::SYSCALL(compiletime::SYSCALL {
                    call_id: NumberSpec::constant(call_id),
                    argument: NumberSpec::constant(argument),
                }))),
                false,
                None,
            );
        }
    }
    for flag in [1, 255, 0, 1] {
        native.compare(
            15,
            &[flag, 17],
            Some(shared(CompiletimeCommand::EXIT(compiletime::EXIT {
                arg1: flag as u8,
                arg2: NumberSpec::constant(17),
            }))),
            false,
            None,
        );
    }
    for property in 0..90 {
        for flags in [0, 0x80] {
            let target = if flags == 0 { i32::MAX } else { 3 };
            native.compare(
                16,
                &[-5, property, target, 0, flags, 0, 0, 0, 0, 0],
                Some(Cmd::LayerCtrl {
                    layer_id: -5,
                    property_id: property,
                    params: (target, 0, flags, 0, 0, 0, 0, 0),
                }),
                false,
                None,
            );
        }
    }
    for ty in [-1, 0, 1, 9, i32::MAX] {
        let params = (0, -1, 2, -3, 4, -5, 6, i32::MIN);
        native.compare(
            17,
            &[
                ty, 0x8123, -20, params.0, params.1, params.2, params.3, params.4, params.5,
                params.6, params.7,
            ],
            Some(Cmd::TransitionSet {
                ty,
                flags: 0x8123,
                duration: -20,
                params,
            }),
            false,
            None,
        );
    }
    native.finish();
}
