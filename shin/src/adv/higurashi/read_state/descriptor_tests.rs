use super::super::checkpoints::SparseCheckpoints;
use super::*;
use shin_core::format::scenario::instruction_elements::{MessageId, Register};
use std::{
    io::Write,
    process::{Command, Stdio},
};

struct Services<'a> {
    before: CommandReadState,
    archive: &'a mut SparseCheckpoints<CommandReadState>,
    calls: &'a mut u32,
    fail: bool,
}
impl ReadServices for Services<'_> {
    fn checkpoint(&mut self, _: Checkpoint, _: ReadState) -> Result<()> {
        anyhow::ensure!(!self.fail, "checkpoint unavailable");
        *self.calls += 1;
        self.archive
            .capture_if_needed(self.before.timeline.boundary, || Ok(self.before.clone()))?;
        Ok(())
    }
    fn message_is_read(&self, _: u32) -> bool {
        panic!("negative fixture message has no read lookup")
    }
    fn message_completed(&mut self, _: u32) -> Result<()> {
        unreachable!()
    }
}
fn choice() -> Cmd {
    Cmd::SELECT {
        base: 0,
        index: 0,
        dest: Register::from_regular_register(0),
        visibility: 0,
        title: String::new(),
        variants: Vec::new(),
    }
}
fn words(t: MessageTimeline) -> [u32; 9] {
    [
        t.boundary,
        t.previous_message,
        t.current_message,
        t.previous_chapter,
        t.current_chapter,
        t.resume_position,
        t.replay_target,
        t.normal_messages,
        t.quick_return_messages,
    ]
}
fn hash_bytes(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    })
}
fn descriptor_hash(state: &CommandReadState) -> u64 {
    let d = state.descriptors();
    let mut values = vec![d.music.id as u32, d.music.volume.to_bits(), d.music.count];
    for e in d.effects {
        values.extend([
            e.id as u32,
            e.volume.to_bits(),
            e.pan.to_bits(),
            e.speed.to_bits(),
        ]);
    }
    values.extend([
        u32::from(d.message.wait_flag),
        d.message.style as u32,
        d.message.options[0] as u32,
        d.message.options[1] as u32,
        u32::from(d.message.text.is_some()),
    ]);
    let text = d
        .message
        .text
        .as_ref()
        .map_or(0, |s| hash_bytes(s.as_bytes()));
    values.extend([text as u32, (text >> 32) as u32]);
    hash_bytes(
        &values
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>(),
    )
}
fn snapshot(
    state: &CommandReadState,
    archive: &SparseCheckpoints<CommandReadState>,
    calls: u32,
) -> String {
    let latest = archive.entries().last();
    let mut line = format!(
        "D {:016x} {} {} {:08x} {:016x}",
        descriptor_hash(state),
        calls,
        archive.entries().len(),
        latest.map_or(u32::MAX, |s| s.boundary),
        latest.map_or(0, |s| descriptor_hash(&s.state))
    );
    for word in words(state.timeline) {
        line.push_str(&format!(" {word:08x}"));
    }
    line.push_str(&format!(
        " {:08x} {}",
        state.state.flags, state.state.is_read
    ));
    line
}

#[test]
#[ignore = "executes original updated audio/message/SELECT constructors, full 1185c captures and bounded 11400 restore"]
fn higurashi_descriptors_match_native_constructors_snapshots_and_select_boundaries() {
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut profiles = 0;
    for boundary in [0, 198, 199, 200, 399, u32::MAX - 1, u32::MAX] {
        for auto_wait in [0, 1, 2, 127, 128, 255] {
            for raw in [i32::MIN, -1000, -1, 0, 1000, i32::MAX] {
                let mut state = CommandReadState::new();
                state.timeline.boundary = boundary;
                let mut archive = SparseCheckpoints::new();
                let mut calls = 0;
                input.push_str(&format!("N {boundary:x}\n"));
                expected.push(snapshot(&state, &archive, calls));
                let message = |wait, text: &str| Cmd::MSGSET {
                    msg_id: MessageId(0),
                    auto_wait: wait,
                    argument: 0,
                    text: text.into(),
                };
                let operations = vec![
                    (
                        format!("P 7 -10 0 {raw}"),
                        Cmd::BGMPLAY {
                            args: [7, -10, 0, raw],
                        },
                    ),
                    (
                        format!("E 0 9 10 0 {raw} {raw} {raw}"),
                        Cmd::SEPLAY {
                            args: [0, 9, 10, 0, raw, raw, raw],
                        },
                    ),
                    (
                        format!("E 31 8 -10 -1 {raw} {raw} {raw}"),
                        Cmd::SEPLAY {
                            args: [31, 8, -10, -1, raw, raw, raw],
                        },
                    ),
                    (
                        "I -1 2147483647 -2147483648".into(),
                        Cmd::MSGINIT {
                            style: -1,
                            updated_options: Some([i32::MAX, i32::MIN]),
                        },
                    ),
                    ("Q".into(), choice()),
                    (format!("M {auto_wait} 0"), message(auto_wait, "")),
                    ("Q".into(), choice()), // Empty text is non-null.
                    ("W 0".into(), Cmd::MSGWAIT { signal: 0 }),
                    ("C 255".into(), Cmd::MSGCLOSE { wait: 255 }),
                    ("Q".into(), choice()), // close depends on retained flag, not its argument.
                    ("M 1 1".into(), message(1, "First")),
                    ("Q".into(), choice()),
                    ("W 2147483647".into(), Cmd::MSGWAIT { signal: i32::MAX }),
                    ("C 0".into(), Cmd::MSGCLOSE { wait: 0 }),
                    ("Q".into(), choice()),
                    ("M 0 2".into(), message(0, "Second")),
                    ("W -2147483648".into(), Cmd::MSGWAIT { signal: i32::MIN }),
                    ("C 1".into(), Cmd::MSGCLOSE { wait: 1 }),
                    ("Q".into(), choice()),
                    (
                        format!("P 3 0 -1 {raw}"),
                        Cmd::BGMPLAY {
                            args: [3, 0, -1, raw],
                        },
                    ),
                    ("S 120".into(), Cmd::BGMSTOP { fade: 120 }),
                    (format!("V {raw} -120"), Cmd::BGMVOL { args: [raw, -120] }),
                    ("T 0 120".into(), Cmd::SESTOP { args: [0, 120] }),
                    (
                        format!("F 31 {raw} 120"),
                        Cmd::SEVOL {
                            args: [31, raw, 120],
                        },
                    ),
                    (
                        format!("H 0 {raw} -120"),
                        Cmd::SEPAN {
                            args: [0, raw, -120],
                        },
                    ),
                    ("A -120".into(), Cmd::SESTOPALL { fade: -120 }),
                    ("M 0 3".into(), message(0, "Same")),
                    ("M 0 4".into(), message(0, "Same")),
                ];
                for (line, command) in operations {
                    let before = state.clone();
                    let before_hash = descriptor_hash(&before);
                    let before_timeline = state.timeline;
                    let should_capture = matches!(command, Cmd::MSGSET { .. })
                        || matches!(command, Cmd::SELECT { .. })
                            && state.descriptors.message.text.is_none();
                    if should_capture {
                        let mut services = Services {
                            before: before.clone(),
                            archive: &mut archive,
                            calls: &mut calls,
                            fail: true,
                        };
                        assert!(state.prepare(&command, &mut services).is_err());
                        assert_eq!(descriptor_hash(&state), before_hash);
                        assert_eq!(state.timeline, before_timeline);
                    }
                    state
                        .prepare(
                            &command,
                            &mut Services {
                                before: before.clone(),
                                archive: &mut archive,
                                calls: &mut calls,
                                fail: false,
                            },
                        )
                        .unwrap();
                    assert_eq!(descriptor_hash(&before), before_hash); // Cloned strings/records never change.
                    input.push_str(&line);
                    input.push('\n');
                    expected.push(snapshot(&state, &archive, calls));
                }
                // Restore both earliest and latest captures through the native copy path.
                for which in [archive.entries().len() - 1, 0] {
                    state = archive.entries()[which].state.clone();
                    input.push_str(&format!("R {which}\n"));
                    expected.push(snapshot(&state, &archive, calls));
                    let command = message(1, "First");
                    let before = state.clone();
                    state
                        .prepare(
                            &command,
                            &mut Services {
                                before,
                                archive: &mut archive,
                                calls: &mut calls,
                                fail: false,
                            },
                        )
                        .unwrap();
                    input.push_str("M 1 1\n");
                    expected.push(snapshot(&state, &archive, calls));
                }
                profiles += 1;
            }
        }
    }
    let executable =
        std::env::var("HIGURASHI_NATIVE_DESCRIPTORS").expect("build native-descriptors.c first");
    let image = std::env::var("HIGURASHI_NATIVE_IMAGE")
        .unwrap_or_else(|_| "higurashi/uncompressed_executable_update".into());
    let mut child = Command::new(executable)
        .arg(image)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(output.status.success(), "{output:?}");
    let actual = String::from_utf8(output.stdout).unwrap();
    let lines = actual.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), expected.len());
    for (i, (actual, expected)) in lines.iter().zip(&expected).enumerate() {
        assert_eq!(*actual, expected, "snapshot {i}");
    }
    println!(
        "Higurashi descriptors: {profiles} profiles, {} complete descriptor/timeline snapshots against original constructors/captures/bounded restores; float bits, 32 SE records, stale stop parameters, empty/non-null text, conditional SELECT, pre-mutation capture failures and wrapping boundaries",
        expected.len()
    );
}

#[test]
fn higurashi_descriptor_alias_slots_fail_without_changing_owned_state() {
    let mut state = CommandReadState::new();
    let before = descriptor_hash(&state);
    let mut archive = SparseCheckpoints::new();
    let mut calls = 0;
    for slot in [-1, 32, i32::MIN, i32::MAX] {
        for command in [
            Cmd::SEPLAY {
                args: [slot, 1, 0, 0, 1000, 0, 1000],
            },
            Cmd::SESTOP { args: [slot, 0] },
            Cmd::SEVOL {
                args: [slot, 1000, 0],
            },
            Cmd::SEPAN { args: [slot, 0, 0] },
        ] {
            let frozen = state.clone();
            assert!(
                state
                    .prepare(
                        &command,
                        &mut Services {
                            before: frozen,
                            archive: &mut archive,
                            calls: &mut calls,
                            fail: false
                        }
                    )
                    .is_err()
            );
            assert_eq!(descriptor_hash(&state), before);
            assert_eq!(calls, 0);
        }
    }
}
