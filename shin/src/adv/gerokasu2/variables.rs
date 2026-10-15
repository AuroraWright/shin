//! Ger settings signed16 storage and VM92d0 pending full32 writes.
use anyhow::{Result, ensure};
use shin_core::format::save::PersistData;
#[derive(Clone, Debug)]
pub struct GerVariables {
    committed: PersistData,
    pending: Vec<(u32, i32)>,
    pub commits_enabled: bool,
}
impl Default for GerVariables {
    fn default() -> Self {
        Self {
            committed: PersistData::new(),
            pending: Vec::new(),
            commits_enabled: true,
        }
    }
}
impl GerVariables {
    pub fn committed(&self) -> &PersistData {
        &self.committed
    }
    pub fn replace_committed(&mut self, value: PersistData) {
        self.committed = value;
    }
    pub fn pending(&self) -> &[(u32, i32)] {
        &self.pending
    }
    pub fn get(&self, slot: i32) -> i32 {
        self.pending
            .iter()
            .find_map(|&(s, v)| (s == slot as u32).then_some(v))
            .unwrap_or_else(|| self.committed.get(slot as u32 as usize))
    }
    pub fn set(&mut self, slot: i32, value: i32) {
        let slot = slot as u32;
        if let Some((_, v)) = self.pending.iter_mut().find(|(s, _)| *s == slot) {
            *v = value;
        } else {
            self.pending.push((slot, value));
        }
    }
    pub fn set_replay(&mut self, slot: i32, value: i32, boundary: u32, target: u32) {
        if boundary >= target {
            self.set(slot, value);
        }
    }
    pub fn commit(&mut self) -> Result<()> {
        if self.commits_enabled {
            // Malformed negative/huge slots would make native unsigned growth
            // request gigabytes. Supplied-script slots fit this bounded owner.
            ensure!(
                self.pending.iter().all(|(s, _)| *s <= 65535),
                "Ger persistent commit slot exceeds bounded storage"
            );
            for (slot, value) in self.pending.drain(..) {
                self.committed.set(slot as usize, value);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
