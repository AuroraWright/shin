//! Higurashi scenario metadata and bytecode, separate from the newer Umineko VM.
//!
//! Layouts are checked against both supplied ROMs. Movie fields and the final
//! chart table retain their raw values until their native consumers are traced.
use std::io::Cursor;

use anyhow::{Context, Result, bail, ensure};
use binrw::BinRead;
use bytes::Bytes;

pub mod bytecode;
pub mod browser_text;

use super::{
    ScenarioHeader,
    info::MusicBoxInfoItem,
    types::{U8SmallList, U8String, U16SmallList},
};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum HigurashiVariant {
    Original,
    Updated,
}

#[derive(Debug, BinRead)]
pub struct MaskInfoItem {
    pub name: U8String,
}

#[derive(Debug, BinRead)]
pub struct PictureInfoItem {
    pub name: U8String,
    pub linked_cg_id: i16,
}

#[derive(Debug, BinRead)]
pub struct BustupInfoItem {
    pub name: U8String,
    pub emotion: U8String,
    pub lipsync_character_id: u16,
}

#[derive(Debug, BinRead)]
pub struct BgmInfoItem {
    pub name: U8String,
    pub display_name: U8String,
    pub linked_bgm_id: i16,
}

#[derive(Debug, BinRead)]
pub struct SeInfoItem {
    pub name: U8String,
}

#[derive(Debug, BinRead)]
pub struct MovieInfoItem {
    pub name: U8String,
    /// Two signed words follow the name; no Umineko transparency/volume bytes.
    pub raw_fields: [i16; 2],
}

#[derive(Debug, BinRead)]
pub struct VoiceMappingInfoItem {
    pub name_pattern: U8String,
    pub lipsync_character_ids: U8SmallList<u8>,
}

#[derive(Debug, BinRead)]
pub struct PictureBoxInfoItem {
    pub name: U8String,
    pub picture_ids: U16SmallList<u16>,
}

#[derive(Debug)]
pub struct TipsInfoItem {
    pub id: u16,
    /// Updated tip script scenario ID (native bc7f4/d9d7c/15c64).
    /// The original scenario omits this word.
    pub updated_field: Option<u16>,
    pub texture_path: U8String,
}

/// The older scenario dialect. Deliberately offers no Umineko Scripter.
#[derive(Debug)]
pub struct HigurashiScenario {
    pub header: ScenarioHeader,
    pub variant: HigurashiVariant,
    pub mask_info: Vec<MaskInfoItem>,
    pub picture_info: Vec<PictureInfoItem>,
    pub bustup_info: Vec<BustupInfoItem>,
    pub bgm_info: Vec<BgmInfoItem>,
    pub se_info: Vec<SeInfoItem>,
    pub movie_info: Vec<MovieInfoItem>,
    pub voice_mapping_info: Vec<VoiceMappingInfoItem>,
    pub picture_box_info: Vec<PictureBoxInfoItem>,
    pub music_box_info: Vec<MusicBoxInfoItem>,
    pub tips_info: Vec<TipsInfoItem>,
    pub chart_record_count: u32,
    pub chart_data: Bytes,
    raw_data: Bytes,
}

impl HigurashiScenario {
    pub fn new(data: Bytes) -> Result<Self> {
        let mut cursor = Cursor::new(&data[..]);
        let header = ScenarioHeader::read(&mut cursor).context("Higurashi SNR header")?;
        ensure!(
            header.size as usize == data.len(),
            "Higurashi SNR file size mismatch"
        );
        let variant = match (header.unk2, header.unk3) {
            (63, 129) => HigurashiVariant::Original,
            (65, 133) => HigurashiVariant::Updated,
            pair => bail!("Unrecognized Higurashi SNR header: {pair:?}"),
        };
        let offsets = <[u32; 11]>::read_le(&mut cursor).context("Higurashi table pointers")?;
        ensure!(
            offsets[0] == 0x50,
            "Unexpected Higurashi table pointer layout"
        );
        let mut ends = offsets[1..].to_vec();
        ends.push(header.code_offset);
        let mut sections = Vec::new();
        for (index, (&start, &end)) in offsets.iter().zip(&ends).enumerate() {
            ensure!(
                start < end && end as usize <= data.len(),
                "Invalid Higurashi table {index} span"
            );
            ensure!(
                start % 4 == 0 && end % 4 == 0,
                "Unaligned Higurashi table {index}"
            );
            sections.push(data.slice(start as usize..end as usize));
        }
        ensure!(
            (header.code_offset as usize) < data.len(),
            "Higurashi scenario has no bytecode"
        );
        let mut tips_cursor = sized_cursor(&sections[9], 3).context("Higurashi Tips table")?;
        let tips_count = read_count(&mut tips_cursor)?;
        let mut tips_info = Vec::new();
        for _ in 0..tips_count {
            tips_info.push(TipsInfoItem {
                id: u16::read_le(&mut tips_cursor)?,
                updated_field: if variant == HigurashiVariant::Updated {
                    Some(u16::read_le(&mut tips_cursor)?)
                } else {
                    None
                },
                texture_path: U8String::read_le(&mut tips_cursor)?,
            });
        }
        finish(&tips_cursor, 0).context("Higurashi Tips records")?;
        // The bytecode begins on a 16-byte boundary, unlike table alignment.
        let mut chart_cursor = sized_cursor(&sections[10], 15).context("Higurashi chart table")?;
        let chart_record_count = read_count(&mut chart_cursor)?;
        // Record shapes are not established yet. Preserve the payload exactly.
        let chart_data =
            Bytes::copy_from_slice(&chart_cursor.get_ref()[chart_cursor.position() as usize..]);

        Ok(Self {
            header,
            variant,
            mask_info: sized_table(&sections[0]).context("Higurashi masks")?,
            picture_info: sized_table(&sections[1]).context("Higurashi pictures")?,
            bustup_info: sized_table(&sections[2]).context("Higurashi bustups")?,
            bgm_info: sized_table(&sections[3]).context("Higurashi BGM")?,
            se_info: sized_table(&sections[4]).context("Higurashi SE")?,
            movie_info: sized_table(&sections[5]).context("Higurashi movies")?,
            voice_mapping_info: sized_table(&sections[6]).context("Higurashi voices")?,
            picture_box_info: simple_table(&sections[7]).context("Higurashi CG gallery")?,
            music_box_info: simple_table(&sections[8]).context("Higurashi music gallery")?,
            tips_info,
            chart_record_count,
            chart_data,
            raw_data: data,
        })
    }

    pub fn raw(&self) -> &[u8] {
        &self.raw_data
    }

    pub fn instruction_reader(
        &self,
        offset: super::instruction_elements::CodeAddress,
    ) -> Result<bytecode::HigurashiInstructionReader> {
        bytecode::HigurashiInstructionReader::new(
            self.raw_data.clone(),
            self.variant,
            self.header.code_offset,
            offset,
        )
    }
}

fn finish(cursor: &Cursor<&[u8]>, max_padding: usize) -> Result<()> {
    let remaining = &cursor.get_ref()[cursor.position() as usize..];
    ensure!(
        remaining.len() <= max_padding && remaining.iter().all(|&b| b == 0),
        "Unexpected Higurashi table trailing data ({} bytes)",
        remaining.len()
    );
    Ok(())
}

fn sized_cursor(section: &[u8], max_padding: usize) -> Result<Cursor<&[u8]>> {
    let mut cursor = Cursor::new(section);
    let size = u32::read_le(&mut cursor)? as usize;
    ensure!(
        size >= 4 && size <= section.len() - 4,
        "Invalid Higurashi table size"
    );
    let mut padding = Cursor::new(section);
    padding.set_position((size + 4) as u64);
    finish(&padding, max_padding)?;
    Ok(Cursor::new(&section[4..size + 4]))
}

fn read_count(cursor: &mut Cursor<&[u8]>) -> Result<u32> {
    let count = u32::read_le(cursor)?;
    // Every decoded record has at least one byte. Avoid unbounded allocation
    // from a corrupt count while still leaving read errors to the typed parser.
    ensure!(
        count as usize <= cursor.get_ref().len(),
        "Invalid Higurashi table count"
    );
    Ok(count)
}

fn records<T: for<'a> BinRead<Args<'a> = ()>>(
    mut cursor: Cursor<&[u8]>,
    max_padding: usize,
) -> Result<Vec<T>> {
    let count = read_count(&mut cursor)?;
    let mut entries = Vec::new();
    for _ in 0..count {
        entries.push(T::read_le(&mut cursor)?);
    }
    finish(&cursor, max_padding)?;
    Ok(entries)
}

fn sized_table<T: for<'a> BinRead<Args<'a> = ()>>(section: &[u8]) -> Result<Vec<T>> {
    records(sized_cursor(section, 3)?, 0)
}

fn simple_table<T: for<'a> BinRead<Args<'a> = ()>>(section: &[u8]) -> Result<Vec<T>> {
    records(Cursor::new(section), 3)
}

#[cfg(test)]
#[path = "higurashi_tests.rs"]
mod tests;
