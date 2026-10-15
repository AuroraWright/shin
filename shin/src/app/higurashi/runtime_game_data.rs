//! Native in-memory GameData owns two records even when record0 is active.
//! The disk GameData projection omits that inactive record;scene factories must
//! carry this complete owner instead of round-tripping through the save codec.
use crate::adv::higurashi::autosave::{ProgressEntry, SaveProgress};
use shin_core::format::save::higurashi::{GameData, GameDataEntry, SaveDate};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct RuntimeGameData {
    pub date: SaveDate,
    pub progress: SaveProgress,
    pub trailing_values: Vec<u16>,
    pub byte_variables: Vec<u8>,
}
fn entry(data: GameDataEntry) -> ProgressEntry {
    ProgressEntry {
        scenario_id: data.scenario_id,
        random_seed: data.random_seed,
        save_position: data.save_position,
        replay_target: data.replay_target,
        response_data: data.response_data,
    }
}
fn saved_entry(data: &ProgressEntry) -> GameDataEntry {
    GameDataEntry {
        scenario_id: data.scenario_id,
        random_seed: data.random_seed,
        save_position: data.save_position,
        replay_target: data.replay_target,
        response_data: data.response_data.clone(),
    }
}
impl From<GameData> for RuntimeGameData {
    fn from(data: GameData) -> Self {
        let active = usize::from(data.primary_entry.is_some());
        let entries = if let Some(first) = data.primary_entry {
            [entry(first), entry(data.entry)]
        } else {
            [
                entry(data.entry),
                ProgressEntry {
                    scenario_id: 0,
                    random_seed: 0,
                    save_position: 0,
                    replay_target: 0,
                    response_data: vec![],
                },
            ]
        };
        Self {
            date: data.date,
            progress: SaveProgress::new(active, u32::from(data.extra_word), entries)
                .expect("Two runtime progress records"),
            trailing_values: data.trailing_values,
            byte_variables: data.byte_variables,
        }
    }
}
impl RuntimeGameData {
    pub fn from_progress(
        progress: SaveProgress,
        date: SaveDate,
        trailing_values: Vec<u16>,
        byte_variables: Vec<u8>,
    ) -> Self {
        Self {
            date,
            progress,
            trailing_values,
            byte_variables,
        }
    }
    /// Existing on-disk representation;never use this projection for a scene
    /// replacement because the dormant record and upper extra-word bits vanish.
    pub fn save_data(&self) -> GameData {
        GameData {
            date: self.date,
            extra_word: self.progress.extra_word as u8,
            entry: saved_entry(self.progress.active_entry()),
            primary_entry: (self.progress.active_index() == 1)
                .then(|| saved_entry(&self.progress.entries[0])),
            trailing_values: self.trailing_values.clone(),
            byte_variables: self.byte_variables.clone(),
        }
    }
}

#[cfg(test)]
mod tests;
