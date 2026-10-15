//! Updated4738..47e0 EXIT Bonus ->17490(flags0), independent fresh GameData.
use super::super::runtime_game_data::RuntimeGameData;
use crate::adv::higurashi::autosave::{ProgressEntry, SaveProgress};
use shin_core::format::save::higurashi::SaveDate;

#[cfg(test)]
mod native_tests;

pub(super) struct SceneRequest {
    pub data: RuntimeGameData,
    pub flags: u32,
}

/// The caller draws exactly one cca00 seed when EXIT emits its destination,
/// before the source retires. A retained request never draws again on retry.
pub(super) fn fresh(seed: u32) -> RuntimeGameData {
    let empty = || ProgressEntry {
        scenario_id: 0,
        random_seed: 0,
        save_position: 0,
        replay_target: 0,
        response_data: Vec::new(),
    };
    let mut primary = empty();
    primary.scenario_id = 10010;
    primary.random_seed = seed;
    RuntimeGameData {
        // The native stack fragment does not assign the date or dormant
        // record's fixed scalars. These zeros are deterministic desktop
        // initialization, not source copying or a claim of native zeroing.
        date: SaveDate {
            year: 0,
            month: 0,
            day: 0,
            hour: 0,
            minute: 0,
            second: 0,
        },
        progress: SaveProgress::new(0, 0, [primary, empty()])
            .expect("Fresh Bonus has two records and active index zero"),
        trailing_values: Vec::new(),
        byte_variables: Vec::new(),
    }
}
