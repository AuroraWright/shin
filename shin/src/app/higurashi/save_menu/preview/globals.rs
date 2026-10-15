//! Native capture clones can acquire TIPS independently of thumbnail publication.
//! Serializing this bit service with main-thread snapshots avoids stale copies.
use super::temporary::AcquiredBits;
use anyhow::{Context, Result};
use parking_lot::Mutex;
use shin_core::format::save::higurashi::SaveData;
use std::sync::Arc;

#[derive(Clone)]
pub(in crate::app::higurashi::save_menu) struct Snapshot {
    pub asset_setting: u8,
    pub route_navigation: bool,
    pub tips: Vec<u32>,
}
impl Snapshot {
    pub fn new(saved: &SaveData) -> Self {
        Self {
            asset_setting: u8::from(saved.settings.extra_flag),
            route_navigation: saved.settings.common.v14_showroutenavi,
            tips: saved.save_vectors.extra_words_0.clone(),
        }
    }
}
pub(super) struct State {
    pub snapshot: Snapshot,
    pending: Vec<usize>,
}
pub(super) type Shared = Arc<Mutex<State>>;
fn set(words: &mut Vec<u32>, id: usize) -> Result<()> {
    let bit = id.checked_mul(2).context("TIPS acquired index overflow")?;
    let word = bit / 32;
    if word >= words.len() {
        let count = word
            .checked_add(1)
            .and_then(|n| n.checked_next_multiple_of(256))
            .context("TIPS acquired vector overflow")?;
        words.try_reserve(count - words.len())?;
        words.resize(count, 0);
    }
    words[word] |= 1 << (bit % 32);
    Ok(())
}
impl State {
    pub fn shared(snapshot: Snapshot) -> Shared {
        Arc::new(Mutex::new(Self {
            snapshot,
            pending: Vec::new(),
        }))
    }
    /// Apply only worker acquisitions; preserve concurrent read flags and the
    /// current physical padding. The worker reads this refreshed bank next.
    pub fn synchronize(&mut self, saved: &mut SaveData) -> Result<()> {
        for &id in &self.pending {
            set(&mut saved.save_vectors.extra_words_0, id)?;
        }
        self.pending.clear();
        self.snapshot = Snapshot::new(saved);
        Ok(())
    }
}
impl AcquiredBits for State {
    fn acquired(&self, id: usize) -> bool {
        id.checked_mul(2).is_some_and(|bit| {
            self.snapshot
                .tips
                .get(bit / 32)
                .is_some_and(|word| word & (1 << (bit % 32)) != 0)
        })
    }
    fn acquire(&mut self, id: usize) -> Result<()> {
        set(&mut self.snapshot.tips, id)?;
        self.pending.push(id);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
