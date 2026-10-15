//! Higurashi settings+28 message bits. Persistent settings and full VM/ADV
//! checkpoint storage remain separate owners; this is the native memory state.
use super::{
    input::ReadState,
    read_state::{Checkpoint, ReadServices},
};
use anyhow::Result;
use shin_core::format::save::index_set::IndexSet;

#[derive(Clone, Debug)]
pub struct MessageReadHistory {
    bits: IndexSet,
}
impl MessageReadHistory {
    pub fn new() -> Self {
        Self {
            bits: IndexSet::new(),
        }
    }
    pub fn from_saved_words(words: Vec<u32>) -> Self {
        Self { bits: IndexSet::from_words(words) }
    }
    pub fn contains(&self, native_id: u32) -> bool {
        self.bits.contains(native_id as usize)
    }
    /// Native16760: no event, mode or pause gate; negative IDs/no VM do nothing.
    /// Shared IndexSet matches nativea9f2c's 256-word logical growth blocks.
    pub fn record_completion(&mut self, vm_present: bool, native_id: u32) -> bool {
        vm_present && (native_id as i32) >= 0 && self.bits.add(native_id as usize)
    }
    pub fn bits(&self) -> &IndexSet {
        &self.bits
    }
}

/// A concrete read-history owner plus a required snapshot service. Constructing
/// this adapter does not invent checkpoint storage or perform disk writes.
pub struct ReadHistoryServices<C> {
    pub history: MessageReadHistory,
    checkpoint: C,
}
impl<C: FnMut(Checkpoint, ReadState) -> Result<()>> ReadHistoryServices<C> {
    pub fn new(history: MessageReadHistory, checkpoint: C) -> Self {
        Self {
            history,
            checkpoint,
        }
    }
}
impl<C: FnMut(Checkpoint, ReadState) -> Result<()>> ReadServices for ReadHistoryServices<C> {
    fn checkpoint(&mut self, kind: Checkpoint, before: ReadState) -> Result<()> {
        (self.checkpoint)(kind, before)
    }
    fn message_is_read(&self, native_id: u32) -> bool {
        self.history.contains(native_id)
    }
    fn message_completed(&mut self, native_id: u32) -> Result<()> {
        self.history.record_completion(true, native_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
