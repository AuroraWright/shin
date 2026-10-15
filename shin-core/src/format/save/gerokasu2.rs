//! Ger progress and game-specific v1 disk codec, independent of Umineko dates.
mod disk;
pub use disk::{GAME_KEY, GameData, SaveData, SaveDate, SaveVectors};

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProgressEntry {
    pub scenario_id: i32,
    pub random_seed: u32,
    pub save_position: u32,
    pub replay_target: u32,
    pub responses: Vec<u8>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Progress {
    active: usize,
    pub entries: [ProgressEntry; 2],
}
impl Progress {
    pub fn new(active: usize, entries: [ProgressEntry; 2]) -> anyhow::Result<Self> {
        anyhow::ensure!(active < 2, "Ger progress active index exceeds two entries");
        Ok(Self { active, entries })
    }
    pub fn active_index(&self) -> usize {
        self.active
    }
    pub fn active_entry(&self) -> &ProgressEntry {
        &self.entries[self.active]
    }
    pub fn active_entry_mut(&mut self) -> &mut ProgressEntry {
        &mut self.entries[self.active]
    }
    pub fn fresh(scenario_id: i32, random_seed: u32) -> Self {
        Self {
            active: 0,
            entries: [
                ProgressEntry {
                    scenario_id,
                    random_seed,
                    ..Default::default()
                },
                Default::default(),
            ],
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AutosaveMemory {
    pub valid: bool,
    /// Native ae400 packs a u16 year followed by month/day/hour/minute/second.
    pub packed_time: u64,
    pub progress: Progress,
}
pub fn packed_local_time() -> u64 {
    use chrono::{Datelike, Timelike};
    let date = chrono::Local::now();
    u64::from(date.year() as u16)
        | (u64::from(date.month()) << 16)
        | (u64::from(date.day()) << 24)
        | (u64::from(date.hour()) << 32)
        | (u64::from(date.minute()) << 40)
        | (u64::from(date.second()) << 48)
}
