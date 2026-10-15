//! Updated Higurashi SaveData v2 (aa7c8/aa9b4/aaaa8), independent of Umineko v1.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_big_array::BigArray;
use shin_primitives::bit_utils::{BitBuffer, bit_read};

use super::{PersistData, UnsupportedSaveVersion, obfuscation};

pub const GAME_KEY: u32 = super::SaveData::obfuscation_key_from_seed("ひぐらしのなく頃に奉");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveData {
    pub save_menu_position: u8,
    pub play_seconds: u32,
    /// Native global word+8. Its constructor value is1039; retain unknown bits.
    pub extra_word: u32,
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
            extra_word: 0x40f,
            persist_data: PersistData::default(),
            save_vectors: SaveVectors::default(),
            settings: Settings::default(),
            auto_save_slot: None,
            manual_save_slots: std::array::from_fn(|_| None),
        }
    }
}

/// Original physical word vectors are retained, including unused zero words.
/// Unlike Umineko's IndexSet reader, native aa2d8 accepts any serialized length.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveVectors {
    pub seen_messages: Vec<u32>,
    pub select_seen_choices: Vec<u32>,
    pub select_last_choices: Vec<u8>,
    pub unlocked_cgs: Vec<u32>,
    pub unlocked_bgms: Vec<u32>,
    pub unlocked_movies: Vec<u32>,
    pub extra_words_0: Vec<u32>,
    pub extra_words_1: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Shared storage layout with older defaults. The legacy field names come
    /// from Umineko; Higurashi's UI bindings must be established independently.
    pub common: super::Settings,
    pub extra_flag: bool,
    pub extra_percentage: u8,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            common: super::Settings {
                v0_bgmvol: 40,
                v1_sfxvol: 60,
                v2_voicevol: 80,
                v3_sysvol: 40,
                v7: 2,
                v8: 1,
                v18_showsongtitle: true,
                ..super::Settings::default()
            },
            extra_flag: false,
            extra_percentage: 50,
        }
    }
}

/// Native5e3c4 preserves packed calendar components without validating them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveDate {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}
impl SaveDate {
    /// Use the same system-local clock as the shared Umineko save owner.
    pub fn now() -> Self {
        use chrono::{Datelike, Timelike};
        let date = chrono::Local::now();
        Self { year: date.year() as u16, month: date.month() as u8, day: date.day() as u8,
            hour: date.hour() as u8, minute: date.minute() as u8, second: date.second() as u8 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameDataEntry {
    pub scenario_id: i32,
    pub random_seed: u32,
    pub save_position: u32,
    /// Runtime+0x24 is reconstructed from save_position by the native reader.
    pub replay_target: u32,
    pub response_data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameData {
    pub date: SaveDate,
    pub extra_word: u8,
    /// Active final entry, with entry zero retained when the active index is one.
    pub entry: GameDataEntry,
    pub primary_entry: Option<GameDataEntry>,
    /// Native+0x68 vector<u16>, serialized as10-bit values in version2 only
    /// when the secondary entry is active. Native writes omit it otherwise.
    pub trailing_values: Vec<u16>,
    /// Native+0x80 byte variable bank, introduced in version2.
    pub byte_variables: Vec<u8>,
}

fn read_vec<T>(
    reader: &mut BitBuffer,
    length_bits: u8,
    mut item: impl FnMut(&mut BitBuffer) -> bit_read::Result<T>,
) -> bit_read::Result<Vec<T>> {
    let length: u32 = reader.read_int(length_bits)?;
    // Read before allocating from an untrusted32-bit response count.
    (0..length).map(|_| item(reader)).collect()
}

fn write_vec<T>(
    values: &[T],
    writer: &mut BitBuffer,
    length_bits: u8,
    mut item: impl FnMut(&T, &mut BitBuffer),
) {
    writer.write_int(length_bits, values.len() as u32);
    for value in values {
        item(value, writer);
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
        if version > 2 {
            return Err(UnsupportedSaveVersion(version).into());
        }
        let save_menu_position = reader.read_int(7)?;
        let play_seconds = reader.read_int(32)?;
        let extra_word = reader.read_int(32)?;
        reader.align();
        let persist_data = PersistData(read_vec(reader, 16, |r| r.read_int(16))?);
        let save_vectors = SaveVectors::read(reader)?;
        let common = super::Settings::read(reader)?;
        let settings = Settings {
            common,
            extra_flag: version > 1 && reader.read_bool()?,
            extra_percentage: if version > 1 { reader.read_int(7)? } else { 80 },
        };
        let auto_save_slot = bit_read::read_opt(reader, |r| GameData::read(r, version))?;
        let manual_save_slots = bit_read::read_array(reader, |r| {
            bit_read::read_opt(r, |r| GameData::read(r, version))
        })?;
        Ok(Self {
            save_menu_position,
            play_seconds,
            extra_word,
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
        writer.write_int(8, 2u8);
        writer.write_int(7, self.save_menu_position);
        writer.write_int(32, self.play_seconds);
        writer.write_int(32, self.extra_word);
        writer.align();
        write_vec(&self.persist_data.0, writer, 16, |v, w| w.write_int(16, *v));
        self.save_vectors.write(writer);
        // Native a9aac aligns after the possibly odd nibble choice vector.
        writer.align();
        self.settings.common.write(writer);
        writer.write_bool(self.settings.extra_flag);
        writer.write_int(7, self.settings.extra_percentage);
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
            "save menu position exceeds7 bits"
        );
        ensure!(
            self.persist_data.0.len() <= u16::MAX as usize,
            "persistent vector exceeds16-bit length"
        );
        for words in self.save_vectors.word_vectors() {
            ensure!(
                words.len() <= u16::MAX as usize,
                "progress vector exceeds16-bit length"
            );
        }
        ensure!(
            self.save_vectors.select_last_choices.len() <= u16::MAX as usize
                && self
                    .save_vectors
                    .select_last_choices
                    .iter()
                    .all(|v| *v < 16),
            "choice vector exceeds its native encoding"
        );
        let settings = &self.settings.common;
        ensure!(
            [
                settings.v0_bgmvol,
                settings.v1_sfxvol,
                settings.v2_voicevol,
                settings.v3_sysvol,
                settings.v9_msgspeed,
                settings.v10_skipspeed,
                settings.v13_msgwinalpha,
                self.settings.extra_percentage
            ]
            .iter()
            .all(|v| *v < 128)
                && settings.v7 < 4
                && settings.v8 < 4,
            "settings exceed native bit widths"
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
                    && d.second < 64
                    && slot.extra_word < 8,
                "slot header exceeds native bit widths"
            );
            ensure!(
                slot.byte_variables.len() < 256,
                "byte variable bank exceeds8-bit length"
            );
            if slot.primary_entry.is_some() {
                ensure!(
                    slot.trailing_values.len() < 256
                        && slot.trailing_values.iter().all(|v| *v < 1024),
                    "trailing values exceed native bit widths"
                );
            }
            for entry in std::iter::once(&slot.entry).chain(&slot.primary_entry) {
                ensure!(
                    entry.response_data.len() <= i32::MAX as usize,
                    "response vector exceeds signed native length"
                );
            }
        }
        Ok(())
    }
}

impl SaveVectors {
    fn word_vectors(&self) -> [&Vec<u32>; 7] {
        [
            &self.seen_messages,
            &self.select_seen_choices,
            &self.unlocked_cgs,
            &self.unlocked_bgms,
            &self.unlocked_movies,
            &self.extra_words_0,
            &self.extra_words_1,
        ]
    }
    fn read(reader: &mut BitBuffer) -> bit_read::Result<Self> {
        reader.align();
        let mut words = || read_vec(reader, 16, |r| r.read_int(32));
        let seen_messages = words()?;
        let select_seen_choices = words()?;
        let select_last_choices = read_vec(reader, 16, |r| r.read_int(4))?;
        Ok(Self {
            seen_messages,
            select_seen_choices,
            select_last_choices,
            unlocked_cgs: read_vec(reader, 16, |r| r.read_int(32))?,
            unlocked_bgms: read_vec(reader, 16, |r| r.read_int(32))?,
            unlocked_movies: read_vec(reader, 16, |r| r.read_int(32))?,
            extra_words_0: read_vec(reader, 16, |r| r.read_int(32))?,
            extra_words_1: read_vec(reader, 16, |r| r.read_int(32))?,
        })
    }
    fn write(&self, writer: &mut BitBuffer) {
        writer.align();
        let vectors = self.word_vectors();
        for vector in &vectors[..2] {
            write_vec(vector, writer, 16, |v, w| w.write_int(32, *v));
        }
        write_vec(&self.select_last_choices, writer, 16, |v, w| {
            w.write_int(4, *v)
        });
        for vector in &vectors[2..] {
            write_vec(vector, writer, 16, |v, w| w.write_int(32, *v));
        }
    }
}

impl GameData {
    fn read(reader: &mut BitBuffer, version: u32) -> bit_read::Result<Self> {
        let date = SaveDate {
            year: reader.read_int(12)?,
            month: reader.read_int(4)?,
            day: reader.read_int(5)?,
            hour: reader.read_int(5)?,
            minute: reader.read_int(6)?,
            second: reader.read_int(6)?,
        };
        let secondary = reader.read_bool()?;
        let extra_word = reader.read_int(3)?;
        let first = GameDataEntry::read(reader)?;
        let (primary_entry, entry) = if secondary {
            (Some(first), GameDataEntry::read(reader)?)
        } else {
            (None, first)
        };
        let trailing_values = if secondary {
            read_vec(reader, 8, |r| r.read_int(if version >= 2 { 10 } else { 8 }))?
        } else {
            Vec::new()
        };
        // Version1 reader5e588 exits immediately for an empty trailing vector.
        let byte_variables = if version >= 2 {
            read_vec(reader, 8, |r| r.read_int(8))?
        } else {
            Vec::new()
        };
        Ok(Self {
            date,
            extra_word,
            entry,
            primary_entry,
            trailing_values,
            byte_variables,
        })
    }
    fn write(&self, writer: &mut BitBuffer) {
        let d = self.date;
        writer.write_int(12, d.year);
        writer.write_int(4, d.month);
        writer.write_int(5, d.day);
        writer.write_int(5, d.hour);
        writer.write_int(6, d.minute);
        writer.write_int(6, d.second);
        writer.write_bool(self.primary_entry.is_some());
        writer.write_int(3, self.extra_word);
        if let Some(entry) = &self.primary_entry {
            entry.write(writer);
        }
        self.entry.write(writer);
        if self.primary_entry.is_some() {
            write_vec(&self.trailing_values, writer, 8, |v, w| w.write_int(10, *v));
        }
        write_vec(&self.byte_variables, writer, 8, |v, w| w.write_int(8, *v));
    }
}

impl GameDataEntry {
    fn read(reader: &mut BitBuffer) -> bit_read::Result<Self> {
        reader.align();
        let scenario_id = reader.read_int(32)?;
        let random_seed = reader.read_int(32)?;
        let save_position = reader.read_int(32)?;
        let response_data = read_vec(reader, 32, |r| r.read_int(8))?;
        Ok(Self {
            scenario_id,
            random_seed,
            save_position,
            replay_target: save_position,
            response_data,
        })
    }
    fn write(&self, writer: &mut BitBuffer) {
        writer.align();
        writer.write_int(32, self.scenario_id);
        writer.write_int(32, self.random_seed);
        writer.write_int(32, self.save_position);
        write_vec(&self.response_data, writer, 32, |v, w| w.write_int(8, *v));
    }
}

#[cfg(test)]
mod tests;
