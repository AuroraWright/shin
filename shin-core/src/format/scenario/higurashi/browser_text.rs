//! Source strings consumed by LogMessage76a38, without live message kana fixup.
use super::{HigurashiScenario, bytecode::ScriptString};
use crate::format::{
    scenario::instruction_elements::{CodeAddress, MessageId, NumberSpec, Register},
    text::read_sjis_string,
};
use anyhow::{Result, ensure};
use binrw::BinRead;
use std::io::{Cursor, Read};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserText {
    Message(String),
    Choice {
        title: String,
        variants: Vec<String>,
    },
}
impl HigurashiScenario {
    /// Only the original string spans are decoded here. Numeric fields and
    /// feedback remain owned by the real scripter, with its normal decoder.
    pub fn browser_text(&self, position: CodeAddress) -> Result<Option<BrowserText>> {
        // Reuse the scenario's code-span validation before slicing its bytes.
        self.instruction_reader(position)?;
        let mut source = Cursor::new(&self.raw()[position.0 as usize..]);
        Ok(match u8::read(&mut source)? {
            0x86 => {
                MessageId::read_le(&mut source)?;
                u8::read(&mut source)?;
                NumberSpec::<i32>::read_le(&mut source)?;
                Some(BrowserText::Message(
                    ScriptString::<true, false>::read_le(&mut source)?.0,
                ))
            }
            0x8c => Some(BrowserText::Message(
                ScriptString::<true, false>::read_le(&mut source)?.0,
            )),
            0x8d => {
                u16::read_le(&mut source)?;
                u16::read_le(&mut source)?;
                Register::read_le(&mut source)?;
                NumberSpec::<i32>::read_le(&mut source)?;
                let title = ScriptString::<false, false>::read_le(&mut source)?.0;
                let size = u8::read(&mut source)? as usize;
                ensure!(size > 0, "Zero Higurashi choice string span");
                let mut data = vec![0; size];
                source.read_exact(&mut data)?;
                ensure!(
                    data.last() == Some(&0) && (size == 1 || data[size - 2] == 0),
                    "Unterminated browser choices"
                );
                let mut variants = Vec::new();
                for item in data[..size - 1].split_inclusive(|&b| b == 0) {
                    ensure!(item != [0], "Early browser choice-array terminator");
                    variants.push(read_sjis_string(&mut Cursor::new(item), Some(item.len()))?);
                }
                Some(BrowserText::Choice { title, variants })
            }
            _ => None,
        })
    }
}
