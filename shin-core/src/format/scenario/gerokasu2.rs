//! Gerokasu2's ten-table UTF-8 scenario dialect (native constructor bb0c0).
//! Gerokasu (the first game) uses the same engine generation with a twelve-slot
//! header, nine Shift-JIS tables and no character gallery (native constructor
//! bc8d0); it is normalized into the same representation here.
//! Keep the Umineko tables and instruction reader independent of this format.
use std::{
    collections::BTreeSet,
    io::{Cursor, Seek},
};

use anyhow::{Context, Result, ensure};
use binrw::BinRead;
use bytes::Bytes;

use super::{
    ScenarioHeader,
    info::{CharacterBoxSegment, MusicBoxInfoItem},
    types::{U8SmallList, U16SmallList},
};
use crate::format::text::{U16String, utf8::U16Utf8String};

/// Which game's scenario wire format was supplied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Dialect {
    /// The first Gerokasu: Shift-JIS strings, twelve header table slots.
    Gerokasu,
    #[default]
    Gerokasu2,
}

pub mod bytecode;

#[derive(Debug, BinRead)]
pub struct MaskInfoItem {
    pub name: U16Utf8String,
}
#[derive(Debug, BinRead)]
pub struct PictureInfoItem {
    pub name: U16Utf8String,
    pub linked_cg_id: i16,
}
#[derive(Debug, BinRead)]
pub struct BustupInfoItem {
    pub name: U16Utf8String,
    pub emotion: U16Utf8String,
    pub lipsync_character_id: u16,
    /// Native MSGFACE worker8fb2c..8fb84 installs these as negated X/Y
    /// translations (properties20/21) and uniform scale (properties12/13).
    pub raw_placement: [i16; 3],
}
#[derive(Debug, BinRead)]
pub struct BgmInfoItem {
    pub name: U16Utf8String,
    pub display_name: U16Utf8String,
    pub linked_bgm_id: i16,
}
#[derive(Debug, BinRead)]
pub struct SeInfoItem {
    pub name: U16Utf8String,
}
#[derive(Debug, BinRead)]
pub struct MovieInfoItem {
    pub name: U16Utf8String,
    pub linked_picture_id: i16,
    pub volume_source: u8,
    pub transparency: u8,
    pub linked_bgm_id: i16,
}
#[derive(Debug, BinRead)]
pub struct VoiceMappingInfoItem {
    pub name_pattern: U16Utf8String,
    pub lipsync_character_ids: U8SmallList<u8>,
}
#[derive(Debug, BinRead)]
pub struct PictureBoxInfoItem {
    pub name: U16Utf8String,
    pub picture_ids: U16SmallList<u16>,
}

#[derive(BinRead)]
struct GerName {
    name: U16String,
}
#[derive(BinRead)]
struct GerPicture {
    name: U16String,
    linked_cg_id: i16,
}
/// Gerokasu bustups carry no emotion string; the name may contain `%DRESS%`.
/// The three words are `[scale percent, x, y]` (consumer: native MSGFACE).
#[derive(BinRead)]
struct GerBustup {
    name: U16String,
    lipsync_character_id: u16,
    placement: [i16; 3],
}
#[derive(BinRead)]
struct GerBgm {
    name: U16String,
    display_name: U16String,
    linked_bgm_id: i16,
}
#[derive(BinRead)]
struct GerMovie {
    name: U16String,
    linked_picture_id: i16,
    volume_source: u8,
    transparency: u8,
    linked_bgm_id: i16,
}
#[derive(BinRead)]
struct GerVoiceMapping {
    name_pattern: U16String,
    lipsync_character_ids: U8SmallList<u8>,
}
#[derive(BinRead)]
struct GerPictureBox {
    name: U16String,
    picture_ids: U16SmallList<u16>,
}

fn utf8(value: U16String) -> U16Utf8String {
    U16Utf8String::new(value.0)
}

#[derive(Debug)]
pub struct Gerokasu2Scenario {
    pub dialect: Dialect,
    pub header: ScenarioHeader,
    pub mask_info: Vec<MaskInfoItem>,
    pub picture_info: Vec<PictureInfoItem>,
    pub bustup_info: Vec<BustupInfoItem>,
    pub bgm_info: Vec<BgmInfoItem>,
    pub se_info: Vec<SeInfoItem>,
    pub movie_info: Vec<MovieInfoItem>,
    pub voice_mapping_info: Vec<VoiceMappingInfoItem>,
    pub picture_box_info: Vec<PictureBoxInfoItem>,
    pub music_box_info: Vec<MusicBoxInfoItem>,
    pub character_box_info: Vec<CharacterBoxSegment>,
    raw_data: Bytes,
}

impl Gerokasu2Scenario {
    pub fn new(data: Bytes) -> Result<Self> {
        let mut cursor = Cursor::new(&data[..]);
        let header = ScenarioHeader::read(&mut cursor).context("Gerokasu2 SNR header")?;
        if data.len() >= 0x28 && data[0x24..0x28] == 0x54u32.to_le_bytes() {
            return Self::new_gerokasu(header, data);
        }
        ensure!(
            header.size as usize == data.len(),
            "Gerokasu2 SNR file size mismatch"
        );
        ensure!(
            (header.unk2, header.unk3) == (1, 1),
            "Unrecognized Gerokasu2 SNR header"
        );
        let offsets = <[u32; 10]>::read_le(&mut cursor).context("Gerokasu2 table pointers")?;
        ensure!(
            offsets[0] == 0x4c,
            "Unexpected Gerokasu2 table pointer layout"
        );
        ensure!(
            (header.code_offset as usize) < data.len(),
            "Gerokasu2 scenario has no bytecode"
        );
        let ends = offsets[1..].iter().copied().chain([header.code_offset]);
        let sections = offsets
            .into_iter()
            .zip(ends)
            .enumerate()
            .map(|(index, (start, end))| {
                ensure!(
                    start < end && end as usize <= data.len(),
                    "Invalid Gerokasu2 table {index} span"
                );
                ensure!(
                    start % 4 == 0 && end % 4 == 0,
                    "Unaligned Gerokasu2 table {index}"
                );
                Ok(&data[start as usize..end as usize])
            })
            .collect::<Result<Vec<_>>>()?;
        let mut char_cursor = sized_cursor(sections[9])?;
        let mut character_box_info = Vec::new();
        while char_cursor.position() < char_cursor.get_ref().len() as u64 {
            character_box_info.push(
                CharacterBoxSegment::read_le(&mut char_cursor)
                    .context("Gerokasu2 character gallery")?,
            );
        }
        Ok(Self {
            dialect: Dialect::Gerokasu2,
            header,
            mask_info: table(sections[0], true).context("Gerokasu2 masks")?,
            picture_info: table(sections[1], true).context("Gerokasu2 pictures")?,
            bustup_info: table(sections[2], true).context("Gerokasu2 bustups")?,
            bgm_info: table(sections[3], true).context("Gerokasu2 BGM")?,
            se_info: table(sections[4], true).context("Gerokasu2 SE")?,
            movie_info: table(sections[5], true).context("Gerokasu2 movies")?,
            voice_mapping_info: table(sections[6], true).context("Gerokasu2 voice mappings")?,
            picture_box_info: table(sections[7], false).context("Gerokasu2 CG gallery")?,
            music_box_info: table(sections[8], false).context("Gerokasu2 music gallery")?,
            character_box_info,
            raw_data: data,
        })
    }

    /// Native bc8d0 reads twelve table offsets; the supplied file fills nine
    /// (masks..music gallery) and leaves the last three zero.
    fn new_gerokasu(header: ScenarioHeader, data: Bytes) -> Result<Self> {
        ensure!(
            header.size as usize == data.len(),
            "Gerokasu SNR file size mismatch"
        );
        ensure!(
            (header.unk2, header.unk3) == (1, 1),
            "Unrecognized Gerokasu SNR header"
        );
        let mut cursor = Cursor::new(&data[0x24..]);
        let slots = <[u32; 12]>::read_le(&mut cursor).context("Gerokasu table pointers")?;
        ensure!(
            slots[9..].iter().all(|&slot| slot == 0),
            "Unexpected Gerokasu table slots"
        );
        ensure!(
            (header.code_offset as usize) < data.len(),
            "Gerokasu scenario has no bytecode"
        );
        let ends = slots[1..9].iter().copied().chain([header.code_offset]);
        let sections = slots[..9]
            .iter()
            .copied()
            .zip(ends)
            .enumerate()
            .map(|(index, (start, end))| {
                ensure!(
                    start < end && end as usize <= data.len(),
                    "Invalid Gerokasu table {index} span"
                );
                Ok(&data[start as usize..end as usize])
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            dialect: Dialect::Gerokasu,
            header,
            mask_info: table::<GerName>(sections[0], true)
                .context("Gerokasu masks")?
                .into_iter()
                .map(|item| MaskInfoItem {
                    name: utf8(item.name),
                })
                .collect(),
            picture_info: table::<GerPicture>(sections[1], true)
                .context("Gerokasu pictures")?
                .into_iter()
                .map(|item| PictureInfoItem {
                    name: utf8(item.name),
                    linked_cg_id: item.linked_cg_id,
                })
                .collect(),
            bustup_info: table::<GerBustup>(sections[2], true)
                .context("Gerokasu bustups")?
                .into_iter()
                .map(|item| BustupInfoItem {
                    name: utf8(item.name),
                    emotion: U16Utf8String::new(""),
                    lipsync_character_id: item.lipsync_character_id,
                    raw_placement: item.placement,
                })
                .collect(),
            bgm_info: table::<GerBgm>(sections[3], true)
                .context("Gerokasu BGM")?
                .into_iter()
                .map(|item| BgmInfoItem {
                    name: utf8(item.name),
                    display_name: utf8(item.display_name),
                    linked_bgm_id: item.linked_bgm_id,
                })
                .collect(),
            se_info: table::<GerName>(sections[4], true)
                .context("Gerokasu SE")?
                .into_iter()
                .map(|item| SeInfoItem {
                    name: utf8(item.name),
                })
                .collect(),
            movie_info: table::<GerMovie>(sections[5], true)
                .context("Gerokasu movies")?
                .into_iter()
                .map(|item| MovieInfoItem {
                    name: utf8(item.name),
                    linked_picture_id: item.linked_picture_id,
                    volume_source: item.volume_source,
                    transparency: item.transparency,
                    linked_bgm_id: item.linked_bgm_id,
                })
                .collect(),
            voice_mapping_info: table::<GerVoiceMapping>(sections[6], true)
                .context("Gerokasu voice mappings")?
                .into_iter()
                .map(|item| VoiceMappingInfoItem {
                    name_pattern: utf8(item.name_pattern),
                    lipsync_character_ids: item.lipsync_character_ids,
                })
                .collect(),
            picture_box_info: table::<GerPictureBox>(sections[7], false)
                .context("Gerokasu CG gallery")?
                .into_iter()
                .map(|item| PictureBoxInfoItem {
                    name: utf8(item.name),
                    picture_ids: item.picture_ids,
                })
                .collect(),
            music_box_info: table(sections[8], false).context("Gerokasu music gallery")?,
            character_box_info: Vec::new(),
            raw_data: data,
        })
    }

    pub fn raw(&self) -> &[u8] {
        &self.raw_data
    }

    pub fn instruction_reader(
        &self,
        offset: super::instruction_elements::CodeAddress,
    ) -> bytecode::InstructionReader {
        bytecode::InstructionReader::with_dialect(self.raw_data.clone(), offset, self.dialect)
    }

    /// Gerokasu picture path (native be510): the first `%TIME%` becomes the
    /// time-of-day suffix (`a b c d d2 e r o`; indices above 7 use `a`).
    pub fn gerokasu_picture_path(name: &str, time: i32) -> String {
        const TIMES: [&str; 8] = ["a", "b", "c", "d", "d2", "e", "r", "o"];
        let suffix = TIMES[if (0..=7).contains(&time) { time as usize } else { 0 }];
        format!("/picture/{}.pic", name.replacen("%TIME%", suffix, 1))
    }

    /// Gerokasu bustup path (native be760): the first `%DRESS%` becomes the
    /// costume (`制服 首輪 過去 制服赤目`; indices above 3 use `制服`).
    pub fn gerokasu_bustup_path(name: &str, dress: i32) -> String {
        const DRESSES: [&str; 4] = ["制服", "首輪", "過去", "制服赤目"];
        let suffix = DRESSES[if (0..=3).contains(&dress) { dress as usize } else { 0 }];
        format!("/bustup/{}.bup", name.replacen("%DRESS%", suffix, 1))
    }

    /// The supplied game uses the medium-resolution bustups. Native bd26c
    /// builds /bustup/M/ for zero and /bustup/L/ for nonzero quality.
    /// Gerokasu keeps bustups in /bustup/ and marks substitutions with `?`.
    pub fn asset_paths(&self) -> BTreeSet<String> {
        if self.dialect == Dialect::Gerokasu {
            return self.gerokasu_asset_paths();
        }
        let mut paths = BTreeSet::new();
        for (folder, extension, names) in [
            (
                "mask",
                "msk",
                self.mask_info
                    .iter()
                    .map(|item| item.name.as_str())
                    .collect::<Vec<_>>(),
            ),
            (
                "picture",
                "pic",
                self.picture_info
                    .iter()
                    .map(|item| item.name.as_str())
                    .collect(),
            ),
            (
                "bustup/m",
                "bup",
                self.bustup_info
                    .iter()
                    .map(|item| item.name.as_str())
                    .collect(),
            ),
            (
                "bgm",
                "nxa",
                self.bgm_info
                    .iter()
                    .map(|item| item.name.as_str())
                    .collect(),
            ),
            (
                "se",
                "nxa",
                self.se_info.iter().map(|item| item.name.as_str()).collect(),
            ),
            (
                "movie",
                "mp4",
                self.movie_info
                    .iter()
                    .map(|item| item.name.as_str())
                    .collect(),
            ),
        ] {
            paths.extend(
                names
                    .into_iter()
                    .map(|name| format!("/{folder}/{}.{extension}", name.to_ascii_lowercase())),
            );
        }
        paths
    }
}

impl Gerokasu2Scenario {
    fn gerokasu_asset_paths(&self) -> BTreeSet<String> {
        let template = |name: &str| name.replace("%TIME%", "?").replace("%DRESS%", "?");
        let mut paths = BTreeSet::new();
        paths.extend(
            self.mask_info
                .iter()
                .map(|item| format!("/mask/{}.msk", item.name.as_str().to_ascii_lowercase())),
        );
        paths.extend(
            self.picture_info
                .iter()
                .map(|item| format!("/picture/{}.pic", template(item.name.as_str()))),
        );
        paths.extend(
            self.bustup_info
                .iter()
                .map(|item| format!("/bustup/{}.bup", template(item.name.as_str()))),
        );
        paths.extend(
            self.bgm_info
                .iter()
                .map(|item| format!("/bgm/{}.nxa", item.name.as_str().to_ascii_lowercase())),
        );
        paths.extend(
            self.se_info
                .iter()
                .map(|item| format!("/se/{}.nxa", item.name.as_str().to_ascii_lowercase())),
        );
        paths.extend(
            self.movie_info
                .iter()
                .map(|item| format!("/movie/{}.mp4", item.name.as_str().to_ascii_lowercase())),
        );
        paths
    }
}

fn sized_cursor(section: &[u8]) -> Result<Cursor<&[u8]>> {
    let mut cursor = Cursor::new(section);
    let length = u32::read_le(&mut cursor)? as usize;
    let end = length
        .checked_add(4)
        .context("Gerokasu2 table length overflow")?;
    ensure!(end <= section.len(), "Gerokasu2 table exceeds its span");
    // Character-gallery padding occupies four bytes in the supplied file.
    ensure!(
        section[end..].len() <= 4 && section[end..].iter().all(|&b| b == 0),
        "Invalid Gerokasu2 table padding"
    );
    Ok(Cursor::new(&section[4..end]))
}

fn table<T: for<'a> BinRead<Args<'a> = ()>>(section: &[u8], sized: bool) -> Result<Vec<T>> {
    let mut cursor = if sized {
        sized_cursor(section)?
    } else {
        Cursor::new(section)
    };
    let count = u32::read_le(&mut cursor)? as usize;
    ensure!(
        count <= cursor.get_ref().len(),
        "Impossible Gerokasu2 table count"
    );
    let entries = (0..count)
        .map(|_| T::read_le(&mut cursor))
        .collect::<binrw::BinResult<Vec<_>>>()?;
    let consumed = cursor.stream_position()? as usize;
    let remainder = &cursor.get_ref()[consumed..];
    ensure!(
        remainder.is_empty()
            || (!sized && remainder.len() <= 3 && remainder.iter().all(|&b| b == 0)),
        "Gerokasu2 table has unparsed bytes"
    );
    Ok(entries)
}

#[cfg(test)]
#[path = "gerokasu2_tests.rs"]
mod tests;
