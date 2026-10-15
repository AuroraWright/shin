//! Updated CHART ec54 constructor: VM8cb8 node list and settings+d0 unlocks.
//! The node list is checkpointed; the global unlock bank survives replay forks.
use parking_lot::Mutex;
use shin_core::format::save::index_set::IndexSet;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct ChartUnlocks {
    bits: Arc<Mutex<IndexSet>>,
}
impl ChartUnlocks {
    pub fn from_saved_words(words: Vec<u32>) -> Self {
        Self {
            bits: Arc::new(Mutex::new(IndexSet::from_words(words))),
        }
    }
    pub fn saved_words(&self) -> Vec<u32> {
        self.bits.lock().words().to_vec()
    }
    /// Every constructor sets the global bit. Only mode0 appends the ID to the
    /// VM list, preserving order and duplicates even when already unlocked.
    pub fn construct(&self, mode: u8, ids: &[i32], nodes: &mut Vec<u32>) {
        let mut bits = self.bits.lock();
        for &id in ids {
            let id = id as u32;
            if mode == 0 {
                nodes.push(id);
            }
            bits.add(id as usize);
        }
    }
}

#[cfg(test)]
pub(super) mod tests;
