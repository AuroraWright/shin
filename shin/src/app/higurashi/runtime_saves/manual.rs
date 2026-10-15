//! Updated b2290/b2d58 mutations and b2984/b2f8c completion callbacks.
//! Settings own complete rows; only the frozen writer projects them to disk.
use super::{RuntimeGameData, SaveData, SaveDate, Slot, Slots};
use anyhow::{Result, ensure};
use std::sync::Arc;

/// ab700 takes a complete, independent settings copy after the slot mutation.
pub(in crate::app::higurashi) struct Snapshot {
    settings: SaveData,
    slots: Slots,
}
impl Snapshot {
    pub fn encode(&self) -> Result<Vec<u8>> {
        let bytes = self.slots.disk(&self.settings).encode()?;
        if tracing::enabled!(tracing::Level::DEBUG) {
            let fingerprint = bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
                (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3)
            });
            tracing::debug!(
                length = bytes.len(),
                buffer_fnv = fingerprint,
                "HIGURASHI_MANUAL_WRITE"
            );
        }
        Ok(bytes)
    }
}

enum Rollback {
    Save { slot: Slot, remembered: u8 },
    Delete,
}

/// One completion owns the rollback data and an immutable writer snapshot.
pub(in crate::app::higurashi) struct ManualWrite {
    selected: usize,
    rollback: Rollback,
    snapshot: Arc<Snapshot>,
}
impl ManualWrite {
    pub fn snapshot(&self) -> Arc<Snapshot> {
        self.snapshot.clone()
    }

    /// Native completion receives a success boolean. The request owner clears
    /// its pending task after this callback, on either outcome.
    pub fn complete(self, slots: &mut Slots, settings: &mut SaveData, success: bool) {
        if success {
            return;
        }
        match self.rollback {
            Rollback::Save { slot, remembered } => {
                slots.manual[self.selected] = slot;
                settings.save_menu_position = remembered;
            }
            // b2f8c restores validity only; the dormant payload is never erased.
            Rollback::Delete => slots.manual[self.selected].valid = true,
        }
    }
}

impl Slots {
    /// b2be4 final Yes copies complete currentData before the menu slides out;
    /// its owner queues the ADV factory only after that slide completes.
    pub fn load_manual(&self, settings: &mut SaveData, selected: usize) -> Result<RuntimeGameData> {
        ensure!(
            selected < self.manual.len(),
            "Invalid Higurashi manual save slot"
        );
        ensure!(
            self.manual[selected].valid,
            "Cannot load an empty Higurashi save slot"
        );
        let data = self.manual[selected].data.clone();
        settings.save_menu_position = selected as u8;
        Ok(data)
    }

    /// b2290 updates menu currentData's date before copying the full row.
    /// Date belongs to the menu even when the write later fails.
    pub fn begin_save(
        &mut self,
        settings: &mut SaveData,
        selected: usize,
        current: &mut RuntimeGameData,
        now: SaveDate,
    ) -> Result<ManualWrite> {
        ensure!(
            selected < self.manual.len(),
            "Invalid Higurashi manual save slot"
        );
        let rollback = Rollback::Save {
            slot: self.manual[selected].clone(),
            remembered: settings.save_menu_position,
        };
        current.date = now;
        self.manual[selected] = Slot {
            valid: true,
            data: current.clone(),
        };
        settings.save_menu_position = selected as u8;
        Ok(ManualWrite {
            selected,
            rollback,
            snapshot: Arc::new(Snapshot {
                settings: settings.clone(),
                slots: self.clone(),
            }),
        })
    }

    /// Delete changes no date, remembered row, or complete GameData payload.
    pub fn begin_delete(&mut self, settings: &SaveData, selected: usize) -> Result<ManualWrite> {
        ensure!(
            selected < self.manual.len(),
            "Invalid Higurashi manual save slot"
        );
        // b1a24 long-press gates on the old selection's validity before
        // selecting the contacted row; that destination can itself be empty.
        self.manual[selected].valid = false;
        Ok(ManualWrite {
            selected,
            rollback: Rollback::Delete,
            snapshot: Arc::new(Snapshot {
                settings: settings.clone(),
                slots: self.clone(),
            }),
        })
    }
}

#[cfg(test)]
mod tests;
