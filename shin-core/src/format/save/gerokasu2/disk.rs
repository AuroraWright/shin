//! Original AF3D0/AF4C0 SaveData v1 and61E00/61F80 GameData wire formats.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_big_array::BigArray;
use shin_primitives::bit_utils::{BitBuffer, BitReadError, bit_read};

use super::super::{PersistData, Settings, UnsupportedSaveVersion, obfuscation};
use super::{Progress, ProgressEntry};

pub const GAME_KEY: u32 =
    super::super::SaveData::obfuscation_key_from_seed("幻想牢獄のカレイドスコープ２");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveData {
    pub save_menu_position: u8,
    pub play_seconds: u32,
    pub persist_data: PersistData,
    pub save_vectors: SaveVectors,
    pub settings: Settings,
    pub auto_save_slot: Option<GameData>,
    #[serde(with = "BigArray")]
    pub manual_save_slots: [Option<GameData>; 100],
}

impl Default for SaveData {
    fn default() -> Self {
        Self {
            save_menu_position: 0,
            play_seconds: 0,
            persist_data: Default::default(),
            save_vectors: Default::default(),
            settings: Default::default(),
            auto_save_slot: None,
            manual_save_slots: std::array::from_fn(|_| None),
        }
    }
}

/// Native readers accept physical lengths without Umineko's allocation-ahead
/// divisibility restriction. Preserve trailing zero words and unused nibbles.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveVectors {
    pub seen_messages: Vec<u32>,
    pub select_seen_choices: Vec<u32>,
    pub select_last_choices: Vec<u8>,
    pub unlocked_cgs: Vec<u32>,
    pub unlocked_bgms: Vec<u32>,
    pub unlocked_movies: Vec<u32>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveDate {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}
impl SaveDate {
    pub fn from_packed(time: u64) -> Self {
        Self {
            year: time as u16,
            month: (time >> 16) as u8,
            day: (time >> 24) as u8,
            hour: (time >> 32) as u8,
            minute: (time >> 40) as u8,
            second: (time >> 48) as u8,
        }
    }
    pub fn packed(self) -> u64 {
        u64::from(self.year)
            | (u64::from(self.month) << 16)
            | (u64::from(self.day) << 24)
            | (u64::from(self.hour) << 32)
            | (u64::from(self.minute) << 40)
            | (u64::from(self.second) << 48)
    }
    pub fn now() -> Self {
        Self::from_packed(super::packed_local_time())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameData {
    pub date: SaveDate,
    pub progress: Progress,
}
impl GameData {
    pub fn from_memory(memory: &super::AutosaveMemory) -> Option<Self> {
        memory.valid.then(|| Self {
            date: SaveDate::from_packed(memory.packed_time),
            progress: memory.progress.clone(),
        })
    }
    pub fn into_memory(self) -> super::AutosaveMemory {
        super::AutosaveMemory {
            valid: true,
            packed_time: self.date.packed(),
            progress: self.progress,
        }
    }
    fn read(reader: &mut BitBuffer) -> bit_read::Result<Self> {
        let date = SaveDate {
            year: reader.read_int(12)?,
            month: reader.read_int(4)?,
            day: reader.read_int(5)?,
            hour: reader.read_int(5)?,
            minute: reader.read_int(6)?,
            second: reader.read_int(6)?,
        };
        let active = usize::from(reader.read_bool()?);
        let mut entries = std::array::from_fn(|_| ProgressEntry::default());
        for entry in &mut entries[..=active] {
            reader.align();
            entry.scenario_id = reader.read_int(32)?;
            entry.random_seed = reader.read_int(32)?;
            entry.save_position = reader.read_int(32)?;
            entry.replay_target = entry.save_position;
            entry.responses = read_vec(reader, 32, |r| r.read_int(8))?;
        }
        Ok(Self {
            date,
            progress: Progress::new(active, entries).expect("one-bit active index"),
        })
    }
    fn write(&self, writer: &mut BitBuffer) {
        let date = self.date;
        writer.write_int(12, date.year);
        writer.write_int(4, date.month);
        writer.write_int(5, date.day);
        writer.write_int(5, date.hour);
        writer.write_int(6, date.minute);
        writer.write_int(6, date.second);
        writer.write_bool(self.progress.active_index() != 0);
        for entry in &self.progress.entries[..=self.progress.active_index()] {
            writer.align();
            writer.write_int(32, entry.scenario_id);
            writer.write_int(32, entry.random_seed);
            writer.write_int(32, entry.save_position);
            write_vec(&entry.responses, writer, 32, |v, w| w.write_int(8, *v));
        }
    }
}

fn read_vec<T>(
    reader: &mut BitBuffer,
    bits: u8,
    mut item: impl FnMut(&mut BitBuffer) -> bit_read::Result<T>,
) -> bit_read::Result<Vec<T>> {
    let count: u32 = reader.read_int(bits)?;
    // Original slot reader620A0 sign-extends the32-bit response length and
    //620D4 rejects its sign bit before allocation. All other counts are16-bit.
    if bits == 32 && count > i32::MAX as u32 {
        return Err(BitReadError::InvalidData {
            reason: "Negative Ger response vector length",
        });
    }
    // Do not reserve an untrusted32-bit response count before reading bytes.
    let mut result = Vec::new();
    for _ in 0..count {
        result.push(item(reader)?);
    }
    Ok(result)
}
fn write_vec<T>(
    values: &[T],
    writer: &mut BitBuffer,
    bits: u8,
    mut item: impl FnMut(&T, &mut BitBuffer),
) {
    writer.write_int(bits, values.len() as u32);
    for value in values {
        item(value, writer);
    }
}

impl SaveVectors {
    fn word_vectors(&self) -> [&Vec<u32>; 5] {
        [
            &self.seen_messages,
            &self.select_seen_choices,
            &self.unlocked_cgs,
            &self.unlocked_bgms,
            &self.unlocked_movies,
        ]
    }
    fn read(reader: &mut BitBuffer) -> bit_read::Result<Self> {
        reader.align();
        Ok(Self {
            seen_messages: read_vec(reader, 16, |r| r.read_int(32))?,
            select_seen_choices: read_vec(reader, 16, |r| r.read_int(32))?,
            select_last_choices: read_vec(reader, 16, |r| r.read_int(4))?,
            unlocked_cgs: read_vec(reader, 16, |r| r.read_int(32))?,
            unlocked_bgms: read_vec(reader, 16, |r| r.read_int(32))?,
            unlocked_movies: read_vec(reader, 16, |r| r.read_int(32))?,
        })
    }
    fn write(&self, writer: &mut BitBuffer) {
        writer.align();
        let vectors = self.word_vectors();
        for words in &vectors[..2] {
            write_vec(words, writer, 16, |v, w| w.write_int(32, *v));
        }
        write_vec(&self.select_last_choices, writer, 16, |v, w| {
            w.write_int(4, *v)
        });
        for words in &vectors[2..] {
            write_vec(words, writer, 16, |v, w| w.write_int(32, *v));
        }
    }
}

impl SaveData {
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::decode_raw(obfuscation::decode(bytes, GAME_KEY)?)
    }
    pub fn decode_raw(bytes: Vec<u8>) -> Result<Self> {
        Self::read(&mut BitBuffer::with_data(bytes, 0))
    }
    fn read(reader: &mut BitBuffer) -> Result<Self> {
        let version: u32 = reader.read_int(8)?;
        if version == 0 {
            return Ok(Self::default());
        }
        if version != 1 {
            return Err(UnsupportedSaveVersion(version).into());
        }
        let save_menu_position = reader.read_int(7)?;
        let play_seconds = reader.read_int(32)?;
        reader.align();
        let persist_data = PersistData(read_vec(reader, 16, |r| r.read_int(16))?);
        let save_vectors = SaveVectors::read(reader)?;
        let settings = Settings::read(reader)?;
        let auto_save_slot = bit_read::read_opt(reader, GameData::read)?;
        let manual_save_slots =
            bit_read::read_array(reader, |r| bit_read::read_opt(r, GameData::read))?;
        Ok(Self {
            save_menu_position,
            play_seconds,
            persist_data,
            save_vectors,
            settings,
            auto_save_slot,
            manual_save_slots,
        })
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        Ok(obfuscation::encode(&self.encode_raw()?, GAME_KEY))
    }
    pub fn encode_raw(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let mut writer = BitBuffer::empty();
        self.write(&mut writer);
        Ok(writer.into_data().0)
    }
    fn write(&self, writer: &mut BitBuffer) {
        writer.write_int(8, 1u8);
        writer.write_int(7, self.save_menu_position);
        writer.write_int(32, self.play_seconds);
        writer.align();
        write_vec(&self.persist_data.0, writer, 16, |v, w| w.write_int(16, *v));
        self.save_vectors.write(writer);
        writer.align();
        self.settings.write(writer);
        for slot in std::iter::once(&self.auto_save_slot).chain(&self.manual_save_slots) {
            writer.write_bool(slot.is_some());
            if let Some(slot) = slot {
                slot.write(writer);
            }
        }
    }
    fn validate(&self) -> Result<()> {
        ensure!(
            self.save_menu_position < 128,
            "Ger save menu position exceeds7 bits"
        );
        ensure!(
            self.persist_data.0.len() <= u16::MAX as usize,
            "Ger persistence length exceeds16 bits"
        );
        for words in self.save_vectors.word_vectors() {
            ensure!(
                words.len() <= u16::MAX as usize,
                "Ger index vector length exceeds16 bits"
            );
        }
        ensure!(
            self.save_vectors.select_last_choices.len() <= u16::MAX as usize
                && self
                    .save_vectors
                    .select_last_choices
                    .iter()
                    .all(|v| *v < 16),
            "Ger choice vector exceeds native encoding"
        );
        let s = &self.settings;
        ensure!(
            [
                s.v0_bgmvol,
                s.v1_sfxvol,
                s.v2_voicevol,
                s.v3_sysvol,
                s.v9_msgspeed,
                s.v10_skipspeed,
                s.v13_msgwinalpha
            ]
            .iter()
            .all(|v| *v < 128)
                && s.v7 < 4
                && s.v8 < 4,
            "Ger settings exceed native bit widths"
        );
        for slot in std::iter::once(&self.auto_save_slot)
            .chain(&self.manual_save_slots)
            .flatten()
        {
            let d = slot.date;
            ensure!(
                d.year < 4096
                    && d.month < 16
                    && d.day < 32
                    && d.hour < 32
                    && d.minute < 64
                    && d.second < 64,
                "Ger calendar exceeds native bit widths"
            );
            ensure!(
                slot.progress.active_index() < 2,
                "Ger active index exceeds one bit"
            );
            for entry in &slot.progress.entries[..=slot.progress.active_index()] {
                ensure!(
                    entry.responses.len() <= i32::MAX as usize,
                    "Ger response length exceeds signed native range"
                );
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
