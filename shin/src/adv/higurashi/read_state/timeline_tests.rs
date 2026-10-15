use super::*;
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
};

fn words(t: MessageTimeline) -> [u32; 9] {
    [
        t.boundary,
        t.previous_message,
        t.current_message,
        0,
        0,
        0,
        t.replay_target,
        t.normal_messages,
        t.quick_return_messages,
    ]
}
struct Services {
    checkpoint: bool,
    flags: u32,
    trace: RefCell<String>,
    id: RefCell<u32>,
    fail: bool,
}
impl ReadServices for Services {
    fn checkpoint(&mut self, _: Checkpoint, before: ReadState) -> Result<()> {
        if self.fail {
            anyhow::bail!("checkpoint failed");
        }
        self.checkpoint = true;
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
fn message(code: &mut Vec<u8>, id: u32) {
    code.push(0x86);
    code.extend_from_slice(&id.to_le_bytes());
    code.extend([0, 2, 0, b'A', 0]);
}

#[test]
#[ignore = "original event/message constructors with every native timeline word/checkpoint order"]
fn higurashi_message_event_timeline_matches_native_counters_and_checkpoint_order() {
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut programs = 0;
    for argument in [0x7f, 0, 1, 0xe0, 0x7e] {
        for seed in [0, 1, 198, 199, 200, u32::MAX - 1, u32::MAX] {
            for id in [0, 1, 2] {
                let mut code = Vec::new();
                message(&mut code, id);
                code.extend([0xa2, argument]);
                message(&mut code, 2);
                code.push(0xa3);
                message(&mut code, id);
                code.extend([0xa2, 0]);
                message(&mut code, 0);
                code.push(0xa3);
                code.extend([0, 0, 0]);
                let mut state = CommandReadState::new();
                state.timeline = MessageTimeline {
                    boundary: seed,
                    previous_message: seed.wrapping_add(11),
                    current_message: seed.wrapping_add(22),
                    replay_target: seed.wrapping_add(33),
                    normal_messages: seed.wrapping_add(44),
                    quick_return_messages: seed.wrapping_add(55),
                    ..Default::default()
                };
                input.push_str("D 2 0 1 9 1 1 1 1 1 1 1 1");
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
                for _ in 0..9 {
                    let pc = reader.position();
                    let HigurashiInstruction::Command(command) = reader.read().unwrap() else {
                        panic!("Expected command")
                    };
                    let command = command.into_runtime_form(&ctx);
                    let before = state.timeline;
                    let mut services = Services {
                        checkpoint: false,
                        flags: u32::MAX,
                        trace: RefCell::default(),
                        id: RefCell::new(u32::MAX),
                        fail: true,
                    };
                    let old = state_tuple(&state);
                    if matches!(command, Cmd::EVBEGIN { .. } | Cmd::EVEND { .. })
                        || matches!(command, Cmd::MSGSET { .. })
                            && state.state().flags & 0x80000000 != 0
                    {
                        assert!(state.prepare(&command, &mut services).is_err());
                        assert_eq!(state_tuple(&state), old);
                    }
                    services.fail = false;
                    state.prepare(&command, &mut services).unwrap();
                    let s = state.state();
                    let trace = services.trace.borrow();
                    let mut line = format!(
                        "T {:08x} {} {} {} {} {} {:08x} {:08x} {:08x} {:08x}",
                        s.flags,
                        s.fast_forward_blocked,
                        s.is_read,
                        state.message_flags(),
                        u8::from(state.suppression(ReadPolicy {
                            modes: 2,
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
                        .chain(if services.checkpoint {
                            words(before)
                        } else {
                            [u32::MAX; 9]
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
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_TIMELINE").unwrap())
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
        assert_eq!(actual, expected, "timeline row{row}");
    }
    println!(
        "Higurashi message/event timeline:{programs} native programs/{} constructor snapshots match boundary/previous/current/replay/message counters, checkpoint-before-mutation order and unsigned wrap; failed checkpoints preserve fields",
        expected.len()
    );
}

fn state_tuple(state: &CommandReadState) -> (u32, u8, u8, MessageTimeline) {
    let s = state.state();
    (s.flags, s.fast_forward_blocked, s.is_read, state.timeline)
}
