use super::*;
use anyhow::ensure;
use shin_core::{
    format::scenario::{
        higurashi::{
            HigurashiVariant,
            bytecode::{HigurashiInstruction, HigurashiInstructionReader},
        },
        instruction_elements::CodeAddress,
    },
    vm::VmCtx,
};
use std::{
    cell::RefCell,
    io::Write,
    process::{Command, Stdio},
    sync::Arc,
};

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
fn labels(
    metadata: &super::super::save_metadata::SaveMetadata,
    pool: &[(Arc<str>, u32)],
) -> [u32; 4] {
    let [a, b] = metadata.current();
    let [c, d] = metadata.saved();
    [a, b, c, d].map(|label| pool.iter().find(|(s, _)| Arc::ptr_eq(s, label)).unwrap().1)
}
struct Services {
    checkpoint: Option<Checkpoint>,
    flags: u32,
    trace: RefCell<String>,
    id: RefCell<u32>,
    fail: bool,
}
impl ReadServices for Services {
    fn checkpoint(&mut self, kind: Checkpoint, before: ReadState) -> Result<()> {
        ensure!(!self.fail, "checkpoint failed");
        self.checkpoint = Some(kind);
        self.flags = before.flags;
        self.trace.get_mut().push('K');
        Ok(())
    }
    fn message_is_read(&self, id: u32) -> bool {
        self.trace.borrow_mut().push('Q');
        *self.id.borrow_mut() = id;
        id == 0
    }
    fn message_completed(&mut self, _: u32) -> Result<()> {
        unreachable!()
    }
}
fn info(code: &mut Vec<u8>, level: u8, text: &str) {
    code.extend([0xa0, level, (text.len() + 1) as u8]);
    code.extend_from_slice(text.as_bytes());
    code.push(0);
}
fn message(code: &mut Vec<u8>, id: u32) {
    code.push(0x86);
    code.extend_from_slice(&id.to_le_bytes());
    code.extend([0, 2, 0, b'A', 0]);
}

#[test]
#[ignore = "original updated SAVEINFO/AUTOSAVE/event/message constructors, metadata initialization and checkpoint ordering"]
fn higurashi_chapters_match_native_metadata_aliases_counters_and_pre_capture_order() {
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut programs = 0;
    for modes in [0, 2, 0x103] {
        for event in [0x7f, 0, 1, 0xe0, 0x7e] {
            for seed in [0u32, 1, 198, 199, 200, u32::MAX - 1, u32::MAX] {
                for id in [0, 1, 2] {
                    let mut code = Vec::new();
                    info(&mut code, 0, "First");
                    info(&mut code, 1, "Chapter");
                    code.push(0xa1);
                    info(&mut code, 1, "Future");
                    message(&mut code, id);
                    code.extend([0xa2, event]);
                    message(&mut code, 2);
                    code.push(0xa1);
                    info(&mut code, 0, "First"); // Same contents, distinct native pointer.
                    code.push(0xa3);
                    message(&mut code, id);
                    code.push(0xa1);
                    info(&mut code, 1, "");
                    code.push(0xa1);
                    code.extend([0, 0, 0]);
                    let mut state = CommandReadState::new();
                    state.timeline = MessageTimeline {
                        boundary: seed,
                        previous_message: seed.wrapping_add(11),
                        current_message: seed.wrapping_add(22),
                        previous_chapter: seed.wrapping_add(33),
                        current_chapter: seed.wrapping_add(44),
                        resume_position: seed.wrapping_add(55),
                        replay_target: seed.wrapping_add(66),
                        normal_messages: seed.wrapping_add(77),
                        quick_return_messages: seed.wrapping_add(88),
                    };
                    let mut pool = vec![(state.metadata.current()[0].clone(), 0)];
                    assert_eq!(labels(&state.metadata, &pool), [0; 4]);
                    input.push_str(&format!("D {modes:x} 0 1 15 1 1 1 1 1 1 1 1"));
                    for word in words(state.timeline) {
                        input.push_str(&format!(" {word:x}"));
                    }
                    input.push_str(&format!(
                        " {}\n",
                        code.iter().map(|b| format!("{b:02x}")).collect::<String>()
                    ));
                    let mut reader = HigurashiInstructionReader::new(
                        code.into(),
                        HigurashiVariant::Updated,
                        0,
                        CodeAddress(0),
                    )
                    .unwrap();
                    let ctx = VmCtx::new(0, 42);
                    for _ in 0..15 {
                        let pc = reader.position();
                        let HigurashiInstruction::Command(command) = reader.read().unwrap() else {
                            panic!()
                        };
                        let command = command.into_runtime_form(&ctx);
                        let before = state.clone();
                        let before_ids = labels(&before.metadata, &pool);
                        let checkpoint = matches!(
                            command,
                            Cmd::EVBEGIN { .. } | Cmd::EVEND {} | Cmd::AUTOSAVE {}
                        ) || matches!(command, Cmd::MSGSET { .. })
                            && state.state.flags & 0x8000_0000 != 0;
                        let mut services = Services {
                            checkpoint: None,
                            flags: u32::MAX,
                            trace: RefCell::default(),
                            id: RefCell::new(u32::MAX),
                            fail: true,
                        };
                        if checkpoint {
                            assert!(state.prepare(&command, &mut services).is_err());
                            assert_eq!(words(state.timeline), words(before.timeline));
                            assert_eq!(labels(&state.metadata, &pool), before_ids);
                            assert_eq!(
                                (
                                    state.state.flags,
                                    state.state.is_read,
                                    state.state.fast_forward_blocked
                                ),
                                (
                                    before.state.flags,
                                    before.state.is_read,
                                    before.state.fast_forward_blocked
                                )
                            );
                        }
                        services.fail = false;
                        state.prepare(&command, &mut services).unwrap();
                        if let Cmd::SAVEINFO { level, info } = &command {
                            assert_eq!(&*state.metadata.current()[*level as usize], info);
                            pool.push((
                                state.metadata.current()[*level as usize].clone(),
                                pc.0 + 4,
                            ));
                        }
                        let after_ids = labels(&state.metadata, &pool);
                        assert_eq!(labels(&before.metadata, &pool), before_ids); // Snapshot aliases stay immutable.
                        if matches!(command, Cmd::AUTOSAVE {}) {
                            assert_eq!(services.checkpoint, Some(Checkpoint::Autosave));
                            assert_eq!(after_ids[2..], before_ids[..2]);
                        }
                        let trace = services.trace.borrow();
                        let s = state.state;
                        let mut line = format!(
                            "T {:08x} {} {} {} {} {} {:08x} {:08x} {:08x} {:08x}",
                            s.flags,
                            s.fast_forward_blocked,
                            s.is_read,
                            state.message_flags(),
                            u8::from(state.suppression(ReadPolicy {
                                modes,
                                forced: false,
                                read_only: true
                            })),
                            if trace.is_empty() {
                                "-"
                            } else {
                                trace.as_str()
                            },
                            services.flags,
                            *services.id.borrow(),
                            pc.0,
                            reader.position().0
                        );
                        for word in words(state.timeline)
                            .into_iter()
                            .chain(if checkpoint {
                                words(before.timeline)
                            } else {
                                [u32::MAX; 9]
                            })
                            .chain(after_ids)
                            .chain(if checkpoint {
                                before_ids
                            } else {
                                [u32::MAX; 4]
                            })
                        {
                            line.push_str(&format!(" {word:08x}"));
                        }
                        expected.push(line);
                    }
                    programs += 1;
                }
            }
        }
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_CHAPTERS").unwrap())
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
    let native = String::from_utf8(output.stdout).unwrap();
    assert_eq!(native.lines().count(), expected.len());
    for (row, (actual, expected)) in native.lines().zip(&expected).enumerate() {
        assert_eq!(actual, expected, "native chapter row{row}");
    }
    println!(
        "Higurashi chapters:{programs} native programs/{} snapshots match all9 timeline words, four label pointer identities, retained chapter copies, initialization, checkpoint-before-mutation, failures and unsigned wrap",
        expected.len()
    );
}
