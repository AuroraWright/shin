//! Native1185c's sparse memory checkpoint schedule. Payload ownership is
//! explicit: callers must capture all ADV/VM/route state needed for restoration.
use super::{
    layers::LayerBanks,
    progress::ProgressMemory,
    read_history::MessageReadHistory,
    read_state::{CommandReadState, ReadServices},
};
use anyhow::{Result, bail};
use shin_core::vm::higurashi::{HigurashiReplayState, HigurashiScripter};
use std::sync::Arc;

#[derive(Clone)]
pub struct Checkpoint<T> {
    pub boundary: u32,
    pub state: T,
}
#[derive(Clone)]
pub struct SparseCheckpoints<T> {
    entries: Vec<Checkpoint<T>>,
}

/// Original1104c restores the newest payload but copies only earlier entries.
/// Original112cc then searches that copied archive and discards the selected
/// entry as well as every newer one. The source archive remains immutable.
pub struct CheckpointFork<T> {
    pub archive: SparseCheckpoints<T>,
    pub restored: Option<Checkpoint<T>>,
}
impl<T: Clone> SparseCheckpoints<T> {
    /// Standalone1104c excludes the newest entry from the copied archive but
    /// restores that payload directly, without112cc's predecessor lookup.
    pub(super) fn clone_newest(&self) -> CheckpointFork<T> {
        let mut archive = self.clone();
        let restored = archive.entries.pop();
        CheckpointFork { archive, restored }
    }
    pub fn fork_for_restore(&self, saved_position: u32) -> CheckpointFork<T> {
        let mut archive = self.clone();
        let mut restored = archive.entries.pop();
        // Use the native lower_bound loop, including its unsigned comparisons.
        // Do not sort or assume wrapping boundaries remain monotonic.
        let mut first = 0;
        let mut remaining = archive.entries.len();
        while remaining != 0 {
            let half = remaining / 2;
            let middle = first + half;
            if archive.entries[middle].boundary < saved_position {
                first = middle + 1;
                remaining -= half + 1;
            } else {
                remaining = half;
            }
        }
        let selected = first.saturating_sub(1);
        if let Some(entry) = archive.entries.get(selected) {
            restored = Some(entry.clone());
            archive.entries.truncate(selected);
        }
        CheckpointFork { archive, restored }
    }
}

/// Semantic payload of native0000..8408. Live drawables, tweens, command
/// continuations, persistent settings and the response bytes are outside it.
/// Playback descriptors and both layer banks are retained for reconstruction.
#[derive(Clone)]
pub struct AdvCheckpoint {
    pub vm: HigurashiReplayState,
    pub read: CommandReadState,
    pub layers: Box<LayerBanks>,
    pub extra_words: Vec<u32>,
    pub response_cursor: usize,
}

pub struct CheckpointMemory {
    archive: SparseCheckpoints<Arc<AdvCheckpoint>>,
    /// Native8cb8: mode0 CHART node IDs, preserving order and duplicates.
    pub extra_words: Vec<u32>,
    pub commits_enabled: bool,
}
pub struct RestoredMemory {
    pub memory: CheckpointMemory,
    pub progress: ProgressMemory,
    /// None means native1104c used the initial scenario state. A present payload
    /// must rebuild its live layers/audio/message before normal execution.
    pub restored: Option<Arc<AdvCheckpoint>>,
}
impl CheckpointMemory {
    /// Native1155c/11664 start from1104c, with a disabled commit gate. A
    /// populated archive replaces the active pair with the source boundary;
    /// the empty-archive path keeps current GameData's pair unchanged.
    pub(super) fn clone_for_search(
        &self,
        source_boundary: u32,
        progress: &ProgressMemory,
    ) -> Result<RestoredMemory> {
        let fork = self.archive.clone_newest();
        let restored = fork.restored.map(|entry| entry.state);
        let active = progress.progress().active_entry();
        let (saved, target) = if restored.is_some() {
            (source_boundary, source_boundary)
        } else { (active.save_position, active.replay_target) };
        let cursor = restored.as_ref().map_or(0, |state| state.response_cursor);
        let extra_words = restored.as_ref().map_or_else(Vec::new, |state| state.extra_words.clone());
        Ok(RestoredMemory {
            memory: Self { archive: fork.archive, extra_words, commits_enabled: false },
            progress: progress.fork_for_replay(saved, target, cursor)?,
            restored,
        })
    }
    pub fn new(extra_words: Vec<u32>) -> Self {
        Self {
            archive: SparseCheckpoints::new(),
            extra_words,
            commits_enabled: true,
        }
    }
    pub fn entries(&self) -> &[Checkpoint<Arc<AdvCheckpoint>>] {
        self.archive.entries()
    }
    /// Commit ordered variable queues even when this bucket already exists.
    /// A failed payload leaves the archive unchanged; committed writes retain
    /// their prefix, matching the existing variable owner recovery contract.
    pub fn capture(
        &mut self,
        vm: &HigurashiScripter,
        read: &CommandReadState,
        layers: &LayerBanks,
        progress: &mut ProgressMemory,
    ) -> Result<bool> {
        progress.commit_pending(self.commits_enabled)?;
        let boundary = read.timeline().boundary;
        self.archive.capture_if_needed(boundary, || {
            Ok(Arc::new(AdvCheckpoint {
                vm: vm.checkpoint_for_replay()?,
                read: read.clone(),
                layers: Box::new(layers.clone()),
                extra_words: self.extra_words.clone(),
                response_cursor: progress.responses().1,
            }))
        })
    }
    /// Native1104c/112cc copies current progress rather than rolling back its
    /// vectors or globals. Only the response cursor comes from the checkpoint.
    pub fn fork_for_restore(
        &self,
        saved_position: u32,
        replay_target: u32,
        progress: &ProgressMemory,
    ) -> Result<RestoredMemory> {
        let fork = self.archive.fork_for_restore(saved_position);
        let restored = fork.restored.map(|entry| entry.state);
        let cursor = restored.as_ref().map_or(0, |state| state.response_cursor);
        let progress = progress.fork_for_replay(saved_position, replay_target, cursor)?;
        let extra_words = restored
            .as_ref()
            .map_or_else(Vec::new, |state| state.extra_words.clone());
        Ok(RestoredMemory {
            memory: Self {
                archive: fork.archive,
                extra_words,
                commits_enabled: true,
            },
            progress,
            restored,
        })
    }
}

/// Concrete constructor-time capture service, used by the native runner.
pub struct CheckpointServices<'a> {
    pub memory: &'a mut CheckpointMemory,
    pub progress: &'a mut ProgressMemory,
    pub history: &'a mut MessageReadHistory,
}
impl ReadServices for CheckpointServices<'_> {
    fn checkpoint(
        &mut self,
        _: super::read_state::Checkpoint,
        _: super::input::ReadState,
    ) -> Result<()> {
        bail!("Checkpoint capture requires the runner's VM/ADV constructor context")
    }
    fn checkpoint_owner(&mut self) -> Option<(&mut CheckpointMemory, &mut ProgressMemory)> {
        Some((self.memory, self.progress))
    }
    fn message_is_read(&self, id: u32) -> bool {
        self.history.contains(id)
    }
    fn message_completed(&mut self, id: u32) -> Result<()> {
        self.history.record_completion(true, id);
        Ok(())
    }
}
pub(super) struct CaptureServices<'a, S> {
    pub services: &'a mut S,
    pub vm: &'a HigurashiScripter,
    pub read: &'a CommandReadState,
    pub layers: &'a LayerBanks,
}
impl<S: ReadServices> ReadServices for CaptureServices<'_, S> {
    fn chart(&mut self, mode: u8, ids: &[i32]) -> Result<()> {
        self.services.chart(mode, ids)
    }
    fn checkpoint(
        &mut self,
        kind: super::read_state::Checkpoint,
        before: super::input::ReadState,
    ) -> Result<()> {
        if let Some((memory, progress)) = self.services.checkpoint_owner() {
            memory.capture(self.vm, self.read, self.layers, progress)?;
            Ok(())
        } else {
            self.services.checkpoint(kind, before)
        }
    }
    fn message_is_read(&self, id: u32) -> bool {
        self.services.message_is_read(id)
    }
    fn message_completed(&mut self, id: u32) -> Result<()> {
        self.services.message_completed(id)
    }
}
impl<T> SparseCheckpoints<T> {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
    pub fn entries(&self) -> &[Checkpoint<T>] {
        &self.entries
    }
    /// Native11900..11928 uses unsigned boundary/200+1 against vector length.
    /// A gap creates one entry per call; it does not fabricate missing entries.
    /// Commit queues are serviced by their owner before this sparse decision.
    pub fn capture_if_needed(
        &mut self,
        boundary: u32,
        capture: impl FnOnce() -> Result<T>,
    ) -> Result<bool> {
        if boundary / 200 + 1 <= self.entries.len() as u32 {
            return Ok(false);
        }
        let state = capture()?;
        self.entries.push(Checkpoint { boundary, state });
        Ok(true)
    }
}

#[cfg(test)]
mod restoration_tests;
#[cfg(test)]
mod tests;
