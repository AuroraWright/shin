//! Complete native settings-owned GameData rows;the wire codec is a projection.
use super::{
    gameplay::bindings::{packed_date, unpacked_date},
    runtime_game_data::RuntimeGameData,
};
use crate::adv::higurashi::{
    autosave::AutosaveMemory,
    autosave::{ProgressEntry, SaveProgress},
};
use shin_core::format::save::higurashi::{GameData, SaveData, SaveDate};

#[derive(Clone, Debug)]
pub(super) struct Slot {
    pub valid: bool,
    pub data: RuntimeGameData,
}
#[derive(Clone, Debug)]
pub(super) struct Slots {
    pub auto: Slot,
    pub manual: [Slot; 100],
}
fn empty() -> RuntimeGameData {
    RuntimeGameData::from_progress(
        SaveProgress::new(
            0,
            0,
            std::array::from_fn(|_| ProgressEntry {
                scenario_id: 0,
                random_seed: 0,
                save_position: 0,
                replay_target: 0,
                response_data: vec![],
            }),
        )
        .unwrap(),
        SaveDate {
            year: 0,
            month: 0,
            day: 0,
            hour: 0,
            minute: 0,
            second: 0,
        },
        vec![],
        vec![],
    )
}
impl Slot {
    fn from_saved(data: &Option<GameData>) -> Self {
        Self {
            valid: data.is_some(),
            data: data
                .clone()
                .map(RuntimeGameData::from)
                .unwrap_or_else(empty),
        }
    }
    fn disk(&self) -> Option<GameData> {
        self.valid.then(|| {
            let mut data = self.data.save_data();
            // Native aaaa8 writes the low3/10 bits from full runtime words.
            data.extra_word &= 7;
            for value in &mut data.trailing_values {
                *value &= 1023;
            }
            data
        })
    }
}
impl Slots {
    pub fn loaded(saved: &SaveData) -> Self {
        Self {
            auto: Slot::from_saved(&saved.auto_save_slot),
            manual: std::array::from_fn(|i| Slot::from_saved(&saved.manual_save_slots[i])),
        }
    }
    pub fn update_auto(&mut self, memory: &AutosaveMemory) {
        self.auto.valid = memory.valid;
        if memory.valid {
            self.auto.data.progress = memory.progress.clone();
            self.auto.data.date = unpacked_date(memory.packed_time);
        }
    }
    pub fn autosave(&self) -> AutosaveMemory {
        AutosaveMemory {
            valid: self.auto.valid,
            packed_time: packed_date(self.auto.data.date),
            progress: self.auto.data.progress.clone(),
        }
    }
    pub fn disk(&self, base: &SaveData) -> SaveData {
        let mut result = base.clone();
        // Native aa9b4's choice vector writer stores four bits per byte.
        // aa00c and live settings copies retain all eight bits in memory.
        for choice in &mut result.save_vectors.select_last_choices {
            *choice &= 15;
        }
        result.auto_save_slot = self.auto.disk();
        result.manual_save_slots = std::array::from_fn(|i| self.manual[i].disk());
        result
    }
}
#[cfg(test)]
mod disk_projection_tests;
pub(super) mod manual;
#[cfg(test)]
mod tests;
