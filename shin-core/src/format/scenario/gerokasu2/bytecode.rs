//! Later-engine bytecode boundary: UTF-8 strings, MSGFACE and the moved QUIZ.
//! Shared instructions reuse the engine's operand types. Gerokasu2Scripter
//! executes this semantic AST; ADV effects remain a separate integration step.
use std::io::{Cursor, Read};

use anyhow::{Context, Result, ensure};
use binrw::BinRead;
use bytes::Bytes;

use crate::{
    format::{
        scenario::{
            instruction_elements::{CodeAddress, MessageId, NumberSpec, Register, U8Bool},
            instructions::Instruction as SharedInstruction,
            types::{Pad4, U16SmallList},
        },
        text::{StringArray, U16FixupString, U16String, utf8::U16Utf8String},
    },
    vm::command::{CompiletimeCommand, compiletime},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Instruction {
    /// Nativebfafc retains the complete byte; interruption tests allow==1.
    Wait {
        allow_interrupt: u8,
        amount: NumberSpec,
    },
    /// Native beea0 evaluates the source once, then writes each u16 register.
    StoreMany {
        value: NumberSpec,
        destinations: U16SmallList<Register, 32>,
    },
    /// Native bf03c writes a selected u16 destination; an unsigned out-of-range
    /// index skips the complete table without changing any register.
    StoreTable {
        value: NumberSpec,
        index: NumberSpec,
        destinations: U16SmallList<Register, 32>,
    },
    /// Normalized semantic nodes for the scoped Gerokasu2 VM. The Umineko
    /// wire writer must not be used to re-encode Gerokasu2 opcodes or strings.
    Shared(SharedInstruction),
    /// Native be1fc returns1, so the dispatcher advances to the next opcode
    /// without an ADV command or operands (c1218 branches back to c11e0).
    Noop { opcode: u8 },
    /// Native bfd78 reads one NumberSpec; factory eb4 retains it at VM+9100.
    MessageFace { face: NumberSpec },
    /// Gerokasu2 adds a numeric operand before the UTF-8 text. Keep the wire
    /// message ID; native bfbe4 subtracts one when constructing the command.
    /// The additional operand reaches MessageLayer+f4, independently of the
    /// MSGFACE selection at VM+9100. Its final meaning remains under review.
    MessageSet {
        msg_id: MessageId,
        auto_wait: U8Bool,
        message_arg: NumberSpec,
        text: U16Utf8String,
    },
    /// Native bf390: valid unsigned index pushes the end-of-table return
    /// address and calls the selected target. Out-of-range skips the table.
    CallTable {
        index: NumberSpec,
        targets: U16SmallList<CodeAddress, 32>,
    },
    /// Native bf740/bf860 search fixed four-byte NumberSpec slots, returning
    /// the first equal/unequal slot at or after start, or -1.
    SearchTable {
        equal: bool,
        dest: Register,
        value: NumberSpec,
        start: NumberSpec,
        table: U16SmallList<Pad4<NumberSpec>, 32>,
    },
    /// Native bf978 reads a destination and two numeric resource-query words.
    /// Native selector100 returns the signed bustup lipsync ID; all other
    /// selectors and invalid indices return zero in this executable.
    ResourceQuery {
        dest: Register,
        selector: NumberSpec,
        index: NumberSpec,
    },
}

#[derive(Clone)]
pub struct InstructionReader {
    cursor: Cursor<Bytes>,
}

impl InstructionReader {
    pub fn new(data: Bytes, address: CodeAddress) -> Self {
        let mut cursor = Cursor::new(data);
        cursor.set_position(address.0 as u64);
        Self { cursor }
    }

    pub fn position(&self) -> CodeAddress {
        CodeAddress(self.cursor.position() as u32)
    }

    pub fn set_position(&mut self, address: CodeAddress) -> Result<()> {
        ensure!(
            (address.0 as usize) < self.cursor.get_ref().len(),
            "Gerokasu2 code address is outside the file"
        );
        self.cursor.set_position(u64::from(address.0));
        Ok(())
    }

    pub fn read(&mut self) -> Result<Instruction> {
        let start = self.cursor.position();
        let result = self.read_inner();
        if result.is_err() {
            self.cursor.set_position(start);
        }
        result.with_context(|| format!("Gerokasu2 instruction at {start:#x}"))
    }

    fn read_inner(&mut self) -> Result<Instruction> {
        let opcode = u8::read_le(&mut self.cursor)?;
        macro_rules! field {
            () => {
                BinRead::read_le(&mut self.cursor)?
            };
        }
        macro_rules! command { ($command:ident, $($field:ident: $value:expr),* $(,)?) => {
            Instruction::Shared(SharedInstruction::Command(CompiletimeCommand::$command(compiletime::$command { $($field: $value),* })))
        }; }
        Ok(match opcode {
            0x83 => Instruction::Wait {
                allow_interrupt: field!(),
                amount: field!(),
            },
            0x43 => Instruction::StoreMany {
                value: field!(),
                destinations: field!(),
            },
            0x45 => Instruction::StoreTable {
                value: field!(),
                index: field!(),
                destinations: field!(),
            },
            0x4b => Instruction::CallTable {
                index: field!(),
                targets: field!(),
            },
            0x51 | 0x52 => Instruction::SearchTable {
                equal: opcode == 0x51,
                dest: field!(),
                value: field!(),
                start: field!(),
                table: field!(),
            },
            0x53 => Instruction::ResourceQuery {
                dest: field!(),
                selector: field!(),
                index: field!(),
            },
            0x8b => Instruction::MessageFace { face: field!() },
            0xe0 => command!(QUIZ, dest: field!(), arg: field!()),
            // Exact be1fc slots in the supplied native dispatch table. In
            // particular e2 consumes no Umineko QUIZ operands in this game.
            0x01..=0x3f
            | 0x54..=0x80
            | 0x84
            | 0x8c
            | 0xa7..=0xaf
            | 0xb2..=0xbf
            | 0xc8
            | 0xd0..=0xdf
            | 0xe1..=0xfe => Instruction::Noop { opcode },
            0x86 => Instruction::MessageSet {
                msg_id: field!(),
                auto_wait: field!(),
                message_arg: field!(),
                text: U16Utf8String::read_le(&mut self.cursor)?,
            },
            0x8d => command!(SELECT,
                choice_set_base: field!(), choice_index: field!(), dest: field!(), choice_visibility_mask: field!(),
                choice_title: U16String::new(U16Utf8String::read_le(&mut self.cursor)?.0), variants: string_array(&mut self.cursor)?),
            0x9c => {
                command!(VOICEPLAY, name: U16String::new(U16Utf8String::read_le(&mut self.cursor)?.0), volume: field!(), flags: field!())
            }
            0xa0 => {
                command!(SAVEINFO, level: field!(), info: U16FixupString::new(U16Utf8String::read_le(&mut self.cursor)?.0))
            }
            0xff => {
                command!(DEBUGOUT, format: U16String::new(U16Utf8String::read_le(&mut self.cursor)?.0), args: field!())
            }
            _ => {
                self.cursor.set_position(self.cursor.position() - 1);
                Instruction::Shared(SharedInstruction::read_opcode(&mut self.cursor, opcode)?)
            }
        })
    }
}

fn string_array(cursor: &mut Cursor<Bytes>) -> Result<StringArray> {
    let length = u16::read_le(cursor)? as usize;
    ensure!(length > 0, "Empty Gerokasu2 choice string span");
    let mut data = vec![0; length];
    cursor.read_exact(&mut data)?;
    ensure!(
        data.pop() == Some(0),
        "Unterminated Gerokasu2 choice string array"
    );
    let mut strings = Vec::new();
    if !data.is_empty() {
        ensure!(data.pop() == Some(0), "Unterminated Gerokasu2 choice label");
        for value in data.split(|&byte| byte == 0) {
            ensure!(!value.is_empty(), "Empty Gerokasu2 choice label");
            strings.push(std::str::from_utf8(value)?.to_owned());
        }
    }
    Ok(StringArray::new(strings))
}

#[cfg(test)]
mod tests {
    use super::*;
    use binrw::BinWrite;

    #[test]
    fn gerokasu2_message_face_and_quiz_keep_dialects_distinct() {
        let mut reader = InstructionReader::new(
            Bytes::from_static(&[0x8b, 1, 0xe0, 0, 0, 2]),
            CodeAddress(0),
        );
        assert_eq!(
            reader.read().unwrap(),
            Instruction::MessageFace {
                face: NumberSpec::constant(1)
            }
        );
        assert_eq!(reader.position(), CodeAddress(2));
        assert!(matches!(
            reader.read().unwrap(),
            Instruction::Shared(SharedInstruction::Command(CompiletimeCommand::QUIZ(_)))
        ));
        assert_eq!(reader.position(), CodeAddress(6));
        for opcode in 0xe1..=0xe4 {
            let mut reader =
                InstructionReader::new(Bytes::from(vec![opcode, 0, 0, 0]), CodeAddress(0));
            assert_eq!(reader.read().unwrap(), Instruction::Noop { opcode });
            assert_eq!(reader.position(), CodeAddress(1));
        }
        // Existing Umineko decoding still uses e2 for QUIZ and rejects 8b.
        assert!(
            crate::format::scenario::InstructionReader::new(
                Bytes::from_static(&[0x8b, 1]),
                CodeAddress(0)
            )
            .read()
            .is_err()
        );
        assert!(matches!(
            crate::format::scenario::InstructionReader::new(
                Bytes::from_static(&[0xe2, 0, 0, 2]),
                CodeAddress(0)
            )
            .read()
            .unwrap(),
            SharedInstruction::Command(CompiletimeCommand::QUIZ(_))
        ));
    }

    #[test]
    fn gerokasu2_dialogue_preserves_utf8_and_truncation_position() {
        let mut bytes = Cursor::new(vec![0x86, 0, 0, 0, 1, 0x7f]);
        bytes.set_position(6);
        U16Utf8String::new("かわいい@r日本")
            .write_le(&mut bytes)
            .unwrap();
        let data = bytes.into_inner();
        let mut reader = InstructionReader::new(Bytes::from(data.clone()), CodeAddress(0));
        let Instruction::MessageSet {
            text, message_arg, ..
        } = reader.read().unwrap()
        else {
            panic!("Expected MSGSET")
        };
        assert_eq!(text.as_str(), "かわいい@r日本");
        assert_eq!(message_arg, NumberSpec::constant(-1));
        assert_eq!(reader.position().0 as usize, data.len());
        for end in 0..data.len() {
            let mut reader =
                InstructionReader::new(Bytes::copy_from_slice(&data[..end]), CodeAddress(0));
            assert!(reader.read().is_err());
            assert_eq!(reader.position(), CodeAddress(0));
        }
    }
}
