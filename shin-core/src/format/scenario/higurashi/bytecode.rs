//! Explicit older script dialect. Umineko command decoders are never used.
//!
//! Updated wire layouts follow its native dispatch table/readers. The original
//! MSGINIT/MSGFACE layouts follow their native readers; ShinDataUtil's byte
//! descriptor for original MSGFACE is narrower than the actual NumberSpec.
//! Raw argument names do not claim native engine semantics or runtime support.
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Read, Seek},
};

use anyhow::{Context, Result, ensure};
use binrw::{BinRead, BinResult, Endian};
use bytes::Bytes;

use super::HigurashiVariant;
use crate::format::{
    scenario::{
        instruction_elements::{BitmaskNumberArray, CodeAddress, MessageId, NumberSpec, Register},
        instructions::{Expression, ExpressionTerm, Instruction},
        types::{U8SmallList, U8SmallNumberList},
    },
    text::{decode_string_fixup, read_sjis_string},
};

/// Length-delimited text, parsed inside its declared span (including NUL).
/// This avoids the shared string/array readers' unbounded terminator scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptString<const LONG: bool, const FIXUP: bool>(pub String);
pub type ShortString = ScriptString<false, false>;
pub type ShortText = ScriptString<false, true>;
pub type LongText = ScriptString<true, true>;

fn invalid(pos: u64, message: &str) -> binrw::Error {
    binrw::Error::AssertFail {
        pos,
        message: message.to_owned(),
    }
}

fn string_span<R: Read + Seek>(reader: &mut R, long: bool) -> BinResult<Vec<u8>> {
    let pos = reader.stream_position()?;
    let size = if long {
        u16::read_le(reader)? as usize
    } else {
        u8::read(reader)? as usize
    };
    if size == 0 {
        return Err(invalid(pos, "Zero Higurashi string length"));
    }
    let mut data = vec![0; size];
    reader.read_exact(&mut data)?;
    if data.last() != Some(&0) {
        return Err(invalid(pos, "Unterminated Higurashi string span"));
    }
    Ok(data)
}

impl<const LONG: bool, const FIXUP: bool> BinRead for ScriptString<LONG, FIXUP> {
    type Args<'a> = ();
    fn read_options<R: Read + Seek>(reader: &mut R, _: Endian, _: ()) -> BinResult<Self> {
        let pos = reader.stream_position()?;
        let data = string_span(reader, LONG)?;
        if data[..data.len() - 1].contains(&0) {
            return Err(invalid(pos, "Embedded NUL in Higurashi string"));
        }
        let text = read_sjis_string(&mut Cursor::new(&data), Some(data.len()))?;
        Ok(Self(if FIXUP {
            decode_string_fixup(&text)
        } else {
            text
        }))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceStrings(pub Vec<String>);
impl BinRead for ChoiceStrings {
    type Args<'a> = ();
    fn read_options<R: Read + Seek>(reader: &mut R, _: Endian, _: ()) -> BinResult<Self> {
        let pos = reader.stream_position()?;
        let data = string_span(reader, false)?;
        if data.len() > 1 && data[data.len() - 2] != 0 {
            return Err(invalid(pos, "Missing Higurashi choice-array terminator"));
        }
        let mut strings = Vec::new();
        // The final extra NUL ends the array; lengths are bytes, not strings.
        for item in data[..data.len() - 1].split_inclusive(|&b| b == 0) {
            if item == [0] {
                return Err(invalid(pos, "Early Higurashi choice-array terminator"));
            }
            let text = read_sjis_string(&mut Cursor::new(item), Some(item.len()))?;
            strings.push(decode_string_fixup(&text));
        }
        Ok(Self(strings))
    }
}

#[allow(non_camel_case_types, clippy::upper_case_acronyms)]
#[derive(Debug, Clone, PartialEq, Eq, BinRead)]
#[br(little, import(variant: HigurashiVariant))]
pub enum HigurashiCommand {
    #[br(magic(0x00u8))]
    EXIT { arg1: u8, arg2: NumberSpec },
    #[br(magic(0x81u8))]
    SGET { dest: Register, slot: NumberSpec },
    #[br(magic(0x82u8))]
    SSET { args: [NumberSpec; 2] },
    #[br(magic(0x83u8))]
    WAIT {
        allow_interrupt: u8,
        amount: NumberSpec,
    },
    #[br(magic(0x85u8))]
    MSGINIT {
        style: NumberSpec,
        #[br(if(variant == HigurashiVariant::Updated))]
        updated_options: Option<[NumberSpec; 2]>,
    },
    #[br(magic(0x86u8))]
    MSGSET {
        msg_id: MessageId,
        auto_wait: u8,
        argument: NumberSpec,
        text: LongText,
    },
    #[br(magic(0x87u8))]
    MSGWAIT { signal: NumberSpec },
    #[br(magic(0x88u8))]
    MSGSIGNAL {},
    #[br(magic(0x89u8))]
    MSGSYNC { args: [NumberSpec; 2] },
    #[br(magic(0x8au8))]
    MSGCLOSE { wait: u8 },
    #[br(magic(0x8bu8))]
    MSGFACE {
        #[br(if(variant == HigurashiVariant::Original))]
        original_argument: Option<NumberSpec>,
    },
    #[br(magic(0x8cu8))]
    LOGSET { text: LongText },
    #[br(magic(0x8du8))]
    SELECT {
        base: u16,
        index: u16,
        dest: Register,
        visibility: NumberSpec,
        title: ShortText,
        variants: ChoiceStrings,
    },
    #[br(magic(0x8eu8))]
    WIPE {
        args: [NumberSpec; 3],
        params: BitmaskNumberArray,
    },
    #[br(magic(0x8fu8))]
    WIPEWAIT {},
    #[br(magic(0x90u8))]
    BGMPLAY { args: [NumberSpec; 4] },
    #[br(magic(0x91u8))]
    BGMSTOP { fade: NumberSpec },
    #[br(magic(0x92u8))]
    BGMVOL { args: [NumberSpec; 2] },
    #[br(magic(0x93u8))]
    BGMWAIT { status: NumberSpec },
    #[br(magic(0x94u8))]
    BGMSYNC { time: NumberSpec },
    #[br(magic(0x95u8))]
    SEPLAY { args: [NumberSpec; 7] },
    #[br(magic(0x96u8))]
    SESTOP { args: [NumberSpec; 2] },
    #[br(magic(0x97u8))]
    SESTOPALL { fade: NumberSpec },
    #[br(magic(0x98u8))]
    SEVOL { args: [NumberSpec; 3] },
    #[br(magic(0x99u8))]
    SEPAN { args: [NumberSpec; 3] },
    #[br(magic(0x9au8))]
    SEWAIT { args: [NumberSpec; 2] },
    #[br(magic(0x9bu8))]
    SEONCE { args: [NumberSpec; 5] },
    #[br(magic(0x9cu8))]
    VOICEPLAY {
        name: ShortString,
        args: [NumberSpec; 2],
    },
    #[br(magic(0x9du8))]
    VOICESTOP {},
    #[br(magic(0x9eu8))]
    VOICEWAIT { status: NumberSpec },
    #[br(magic(0xa0u8))]
    //2630 stores the raw Shift-JIS pointer; menu1aebc/c4990 decodes it without
    // the message text's half-width-katakana expansion.
    SAVEINFO { level: NumberSpec, info: ShortString },
    #[br(magic(0xa1u8))]
    AUTOSAVE {},
    #[br(magic(0xa2u8))]
    EVBEGIN { argument: NumberSpec },
    #[br(magic(0xa3u8))]
    EVEND {},
    #[br(magic(0xb0u8))]
    TROPHY { id: NumberSpec },
    #[br(magic(0xc0u8))]
    LAYERINIT { id: NumberSpec },
    #[br(magic(0xc1u8))]
    LAYERLOAD {
        args: [NumberSpec; 3],
        params: BitmaskNumberArray,
    },
    #[br(magic(0xc2u8))]
    LAYERUNLOAD { args: [NumberSpec; 2] },
    #[br(magic(0xc3u8))]
    LAYERCTRL {
        args: [NumberSpec; 2],
        params: BitmaskNumberArray,
    },
    #[br(magic(0xc4u8))]
    LAYERWAIT {
        id: NumberSpec,
        properties: U8SmallNumberList,
    },
    #[br(magic(0xc5u8))]
    LAYERBACK { argument: NumberSpec },
    #[br(magic(0xc6u8))]
    LAYERSWAP { args: [NumberSpec; 2] },
    #[br(magic(0xc7u8))]
    LAYERSELECT { args: [NumberSpec; 2] },
    #[br(magic(0xc8u8))]
    MOVIEWAIT { args: [NumberSpec; 2] },
    #[br(magic(0xc9u8))]
    FEELICON { args: [NumberSpec; 5] },
    #[br(magic(0xd0u8))]
    TIPSGET { ids: U8SmallNumberList },
    #[br(magic(0xd1u8))]
    CHARSELECT {
        index: u16,
        dest: Register,
        argument: NumberSpec,
    },
    #[br(magic(0xd2u8))]
    OTSUGET { argument: NumberSpec },
    #[br(magic(0xd3u8))]
    CHART { mode: u8, ids: U8SmallNumberList },
    #[br(magic(0xd4u8))]
    SNRSEL { argument: NumberSpec },
    #[br(magic(0xd5u8))]
    KAKERA {},
    #[br(magic(0xd6u8))]
    KAKERAGET {
        argument: NumberSpec,
        ids: U8SmallNumberList,
    },
    #[br(magic(0xd7u8))]
    QUIZ {
        dest: Register,
        args: [NumberSpec; 3],
    },
    #[br(magic(0xd8u8))]
    FAKESELECT {},
    #[br(magic(0xd9u8))]
    UNLOCK { kind: u8, ids: U8SmallNumberList },
    #[br(magic(0xdau8))]
    OpcodeDA {
        dest: Register,
        argument: NumberSpec,
    },
    #[br(magic(0xdbu8))]
    OpcodeDB { args: [NumberSpec; 2] },
    #[br(magic(0xffu8))]
    DEBUGOUT {
        format: ShortString,
        args: U8SmallNumberList,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HigurashiInstruction {
    /// Shared wire fields/control representation. The separate Higurashi VM
    /// applies older arithmetic rules, including its narrower RND sampling.
    Control(Instruction),
    CallTable {
        index: NumberSpec,
        table: U8SmallList<CodeAddress>,
    },
    Command(HigurashiCommand),
}

#[derive(Clone)]
pub struct HigurashiInstructionReader {
    cursor: Cursor<Bytes>,
    variant: HigurashiVariant,
    code_start: u32,
}

impl HigurashiInstructionReader {
    pub(crate) fn matches_scenario(&self, scenario: &super::HigurashiScenario) -> bool {
        self.variant == scenario.variant
            && self.code_start == scenario.header.code_offset
            && self.cursor.get_ref().as_ref() == scenario.raw()
    }
    pub fn new(
        data: Bytes,
        variant: HigurashiVariant,
        code_start: u32,
        offset: CodeAddress,
    ) -> Result<Self> {
        ensure!(
            code_start <= offset.0 && (offset.0 as usize) < data.len(),
            "Higurashi instruction address outside code: {offset}"
        );
        let mut cursor = Cursor::new(data);
        cursor.set_position(offset.0 as u64);
        Ok(Self {
            cursor,
            variant,
            code_start,
        })
    }
    pub fn position(&self) -> CodeAddress {
        CodeAddress(self.cursor.position() as u32)
    }
    /// Decode the static closure from the current PC, rejecting overlapping
    /// instructions and branches into operand bytes or outside the code span.
    pub fn reachable_spans(&mut self) -> Result<BTreeMap<CodeAddress, CodeAddress>> {
        let mut pending = BTreeSet::from([self.position()]);
        let mut spans = BTreeMap::<CodeAddress, CodeAddress>::new();
        while let Some(start) = pending.pop_first() {
            if spans.contains_key(&start) {
                continue;
            }
            if let Some((_, end)) = spans.range(..start).next_back() {
                ensure!(
                    *end <= start,
                    "Higurashi branch enters an operand at {start}"
                );
            }
            self.set_position(start)?;
            let instruction = self.read()?;
            let end = self.position();
            if let Some((following, _)) = spans.range(start..).next() {
                ensure!(
                    end <= *following,
                    "Overlapping Higurashi instruction at {start}"
                );
            }
            pending.extend(instruction.successors(end));
            spans.insert(start, end);
        }
        Ok(spans)
    }
    pub fn set_position(&mut self, offset: CodeAddress) -> Result<()> {
        ensure!(
            self.code_start <= offset.0 && (offset.0 as usize) < self.cursor.get_ref().len(),
            "Higurashi instruction address outside code: {offset}"
        );
        self.cursor.set_position(offset.0 as u64);
        Ok(())
    }
    /// Reads transactionally: failures retain the start PC for useful diagnostics.
    pub fn read(&mut self) -> Result<HigurashiInstruction> {
        let start = self.cursor.position();
        let result = self
            .read_inner()
            .with_context(|| format!("Higurashi instruction at {start:08x}"));
        if result.is_err() {
            self.cursor.set_position(start);
        }
        result
    }
    fn read_inner(&mut self) -> Result<HigurashiInstruction> {
        let start = self.cursor.position();
        let opcode = u8::read(&mut self.cursor)?;
        macro_rules! field {
            () => {
                BinRead::read_le(&mut self.cursor)?
            };
        }
        let instruction = match opcode {
            0x42 => {
                let dest = field!();
                let mut terms = Vec::new();
                loop {
                    let at = self.cursor.position();
                    let tag = u8::read(&mut self.cursor)?;
                    if tag & 0x80 != 0 {
                        break;
                    }
                    if tag < 0x20 {
                        self.cursor.set_position(at);
                        terms.push(ExpressionTerm::read_le(&mut self.cursor)?);
                    }
                }
                HigurashiInstruction::Control(Instruction::exp {
                    dest,
                    expr: Expression::new(terms)?,
                })
            }
            0x4a => {
                let index = field!();
                let table = U8SmallList::<CodeAddress>::read_le(&mut self.cursor)?;
                HigurashiInstruction::Control(Instruction::jt {
                    index,
                    table: table.0.into_iter().collect(),
                })
            }
            0x4b => HigurashiInstruction::CallTable {
                index: field!(),
                table: field!(),
            },
            0x40 | 0x41 | 0x46..=0x49 | 0x4c..=0x50 => {
                self.cursor.set_position(start);
                HigurashiInstruction::Control(Instruction::read_opcode(&mut self.cursor, opcode)?)
            }
            0 | 0x81..=0xff => {
                self.cursor.set_position(start);
                HigurashiInstruction::Command(HigurashiCommand::read_le_args(
                    &mut self.cursor,
                    (self.variant,),
                )?)
            }
            _ => return Err(invalid(start, "Unsupported Higurashi control opcode").into()),
        };
        Ok(instruction)
    }
}

impl HigurashiInstruction {
    /// Static successors, including both outcomes of conditions/table lookups.
    /// Calls include their continuation. This is reachability, not a playthrough.
    pub fn successors(&self, next: CodeAddress) -> Vec<CodeAddress> {
        use Instruction::*;
        match self {
            Self::Control(j { target }) => vec![*target],
            Self::Control(gosub { target } | call { target, .. } | jc { target, .. }) => {
                vec![*target, next]
            }
            Self::Control(jt { table, .. }) => table.0.iter().copied().chain([next]).collect(),
            Self::CallTable { table, .. } => table.0.iter().copied().chain([next]).collect(),
            Self::Control(retsub {} | r#return {}) => vec![],
            Self::Command(HigurashiCommand::EXIT { arg1: 0, .. }) => vec![],
            _ => vec![next],
        }
    }
}

#[cfg(test)]
#[path = "bytecode_tests.rs"]
mod tests;
