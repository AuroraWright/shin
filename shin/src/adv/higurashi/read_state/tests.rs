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

struct History {
    bits: [u32; 8],
    trace: RefCell<String>,
    checkpoint_flags: u32,
    id: RefCell<u32>,
}
impl ReadServices for History {
    fn checkpoint(&mut self, _: Checkpoint, before: ReadState) -> Result<()> {
        self.trace.get_mut().push('K');
        self.checkpoint_flags = before.flags;
        Ok(())
    }
    fn message_is_read(&self, id: u32) -> bool {
        self.trace.borrow_mut().push('Q');
        *self.id.borrow_mut() = id;
        self.bits
            .get((id >> 5) as usize)
            .is_some_and(|word| word & (1u32 << (id & 31)) != 0)
    }
    fn message_completed(&mut self, _: u32) -> Result<()> {
        unreachable!("Constructor fixture never completes messages")
    }
}
fn message(code: &mut Vec<u8>, id: u32, auto: u8) {
    code.push(0x86);
    code.extend_from_slice(&(id | (u32::from(auto) << 24)).to_le_bytes());
    code.extend([0, 2, 0, b'A', 0]);
}

#[test]
#[ignore = "executes original updated VM decoder and command constructors/read bitset against the port"]
fn higurashi_command_read_state_matches_native_constructors_and_packed_ids() {
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut programs = 0;
    for argument in [0x7f, 0, 1, 0xe0, 0x7e] {
        for modes in [0, 1, 2, 3, 0x103] {
            for forced in [false, true] {
                for read_only in [false, true] {
                    for bits in [0, u32::MAX, 0xaaaa_aaaa, 0x5555_5555] {
                        for id in [0, 1, 2, 256, 257, 0x00ff_ffff] {
                            let mut code = Vec::new();
                            message(&mut code, id, 255);
                            code.extend([0xa2, argument]);
                            message(&mut code, if id == 1 { 2 } else { 1 }, 0);
                            code.push(0xa3);
                            message(&mut code, id, 1);
                            code.extend([0xa2, 0]);
                            message(&mut code, 0, 0);
                            code.push(0xa3);
                            code.extend([0, 0, 0]);
                            programs += 1;
                            let hex = code.iter().map(|b| format!("{b:02x}")).collect::<String>();
                            input.push_str(&format!(
                                "D {modes:x} {} {} 9",
                                u8::from(forced),
                                u8::from(read_only)
                            ));
                            for _ in 0..8 {
                                input.push_str(&format!(" {bits:x}"));
                            }
                            input.push_str(&format!(" {hex}\n"));
                            let mut reader = HigurashiInstructionReader::new(
                                code.into(),
                                HigurashiVariant::Updated,
                                0,
                                CodeAddress(0),
                            )
                            .unwrap();
                            let ctx = VmCtx::new(0, 42);
                            let mut state = CommandReadState::new();
                            let mut history = History {
                                bits: [bits; 8],
                                trace: RefCell::default(),
                                checkpoint_flags: u32::MAX,
                                id: RefCell::new(u32::MAX),
                            };
                            for _ in 0..9 {
                                let pc = reader.position();
                                let HigurashiInstruction::Command(command) = reader.read().unwrap()
                                else {
                                    panic!("Expected command")
                                };
                                let command = command.into_runtime_form(&ctx);
                                history.trace.get_mut().clear();
                                history.checkpoint_flags = u32::MAX;
                                *history.id.get_mut() = u32::MAX;
                                state.prepare(&command, &mut history).unwrap();
                                let s = state.state();
                                let trace = history.trace.borrow();
                                expected.push(format!(
                                    "D {:08x} {} {} {} {} {} {:08x} {:08x} {:08x} {:08x}",
                                    s.flags,
                                    s.fast_forward_blocked,
                                    s.is_read,
                                    state.message_flags(),
                                    u8::from(state.suppression(ReadPolicy {
                                        modes,
                                        forced,
                                        read_only
                                    })),
                                    if trace.is_empty() {
                                        "-"
                                    } else {
                                        trace.as_str()
                                    },
                                    history.checkpoint_flags,
                                    *history.id.borrow(),
                                    pc.0,
                                    reader.position().0
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_READ_STATE").unwrap())
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
    for (row, (line, expected)) in output.lines().zip(&expected).enumerate() {
        assert_eq!(line, expected, "read state row{row}");
    }
    println!(
        "Higurashi read state:{programs} native bytecode programs/{} constructor snapshots match native initial fields, EVBEGIN/EVEND, MSGSET packed-ID-minus-one/read-bit lookup and query/checkpoint order, message flags, EXIT block and per-command137d0",
        expected.len()
    );
}
