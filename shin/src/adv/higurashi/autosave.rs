//! Updated AUTOSAVE polls and the progress copy at11780. Memory state only;
//! native disk codecs, loaded settings defaults and UI notification rendering
//! have separate owners. The later Umineko GameData has a different entry.
use anyhow::{Result, ensure};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgressEntry {
    pub scenario_id: i32,
    pub random_seed: u32,
    /// Native entry+8:1102c compares this boundary with the current VM boundary.
    pub save_position: u32,
    /// Native entry+c: variable replay setters compare against this boundary.
    pub replay_target: u32,
    pub response_data: Vec<u8>,
}

/// The fields11780 copies from GameData+0c..+67. The source's valid/date fields
/// and trailing vectors at+68/+80 are deliberately outside this copy payload.
/// Active progress and autosave memory are supplied explicitly, not defaults.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveProgress {
    active: usize,
    pub extra_word: u32,
    pub entries: [ProgressEntry; 2],
}
impl SaveProgress {
    pub fn new(active: usize, extra_word: u32, entries: [ProgressEntry; 2]) -> Result<Self> {
        ensure!(
            active < 2,
            "Higurashi progress active index exceeds two entries"
        );
        Ok(Self {
            active,
            extra_word,
            entries,
        })
    }
    pub fn active_index(&self) -> usize {
        self.active
    }
    pub fn active_entry(&self) -> &ProgressEntry {
        &self.entries[self.active]
    }
    pub fn set_active_index(&mut self, active: usize) -> Result<()> {
        ensure!(active < 2, "Higurashi progress active index exceeds two entries");
        self.active = active;
        Ok(())
    }
}

/// Settings+108: valid flag, packed native clock value, and copied progress.
/// This owner does not serialize files or acknowledge a platform save request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutosaveMemory {
    pub valid: bool,
    pub packed_time: u64,
    pub progress: SaveProgress,
}
impl AutosaveMemory {
    /// Original11780, also called by MSGSET's first live poll. No notification
    /// accompanies this copy. Failed services leave their prefix for retry.
    pub fn capture_message_start(
        &mut self,
        progress: &mut super::progress::ProgressMemory,
        boundary: u32,
        services: &mut impl AutosaveServices,
    ) -> Result<()> {
        let active = progress.progress().active_index();
        let entry = &mut progress.progress_mut().entries[active];
        entry.save_position = boundary;
        entry.replay_target = boundary;
        self.valid = true;
        let time = services.packed_time()?;
        self.capture(progress.progress(), time);
        services.captured(self)
    }
    pub(super) fn capture(&mut self, progress: &SaveProgress, packed_time: u64) {
        // Both entries' complete response vectors, including unused tails,
        // are copied after the native clock service returns.
        self.packed_time = packed_time;
        self.progress.clone_from(progress);
    }
}

pub trait AutosaveServices {
    /// Nativea9790 result, supplied by the platform clock owner. No implicit
    /// use of host time or timezone while loaded/native date encoding is pending.
    fn packed_time(&mut self) -> Result<u64>;
    /// Publish the already captured native memory to the loaded SaveData owner
    /// before notification construction. Memory-only callers need no mirror.
    fn captured(&mut self, _: &AutosaveMemory) -> Result<()> {
        Ok(())
    }
    /// Native7f60 combines the captured labels with one space and queues55e10.
    /// A failure leaves notification pending and must not acknowledge this item.
    fn notify(&mut self, text: &str) -> Result<()>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Execution {
    Live,
    Replay,
}

#[derive(Clone, Debug)]
enum LivePhase {
    New,
    Clock(String),
    Publish(String),
    Notify(String),
    Complete,
}

#[derive(Clone, Debug)]
pub struct AutosaveCommand {
    armed: u8,
    reached: u8,
    live: LivePhase,
}
impl AutosaveCommand {
    pub fn new() -> Self {
        // Native2820 STRH1 at command+19: little-endian bytes1,0.
        Self {
            armed: 1,
            reached: 0,
            live: LivePhase::New,
        }
    }
    pub fn flags(&self) -> [u8; 2] {
        [self.armed, self.reached]
    }
    /// Native7f14 returns false at/after the active saved boundary, changes
    /// flags to0,1, and leaves this command installed for reconstruction.
    pub fn poll_replay(&mut self, progress: &SaveProgress, boundary: u32) -> Result<bool> {
        ensure!(
            matches!(self.live, LivePhase::New),
            "Autosave live effects must finish before changing execution mode"
        );
        Ok(if boundary >= progress.active_entry().save_position {
            self.armed = 0;
            self.reached = 1;
            false
        } else {
            true
        })
    }
    /// Native7f60 copies progress before notifying and returns true. Rust
    /// service failures retain the acknowledged copy and retry only the pending
    /// notification; this recovery path is additional to native infallible UI.
    pub fn poll_live(
        &mut self,
        progress: &mut SaveProgress,
        memory: &mut AutosaveMemory,
        boundary: u32,
        metadata: &super::save_metadata::SaveMetadata,
        services: &mut impl AutosaveServices,
    ) -> Result<bool> {
        if matches!(self.live, LivePhase::New) {
            // Native117b0/117c4/117d0 publish the active boundary pair and
            // valid byte before calling the clock. Fallible services preserve
            // this already-applied prefix while leaving the copy unacknowledged.
            let active = &mut progress.entries[progress.active];
            active.save_position = boundary;
            active.replay_target = boundary;
            memory.valid = true;
            let [first, second] = metadata.saved();
            self.live = LivePhase::Clock(format!("{first} {second}"));
        }
        if let LivePhase::Clock(text) = &self.live {
            let time = services.packed_time()?;
            memory.capture(progress, time);
            self.live = LivePhase::Publish(text.clone());
        }
        if let LivePhase::Publish(text) = &self.live {
            services.captured(memory)?;
            self.live = LivePhase::Notify(text.clone());
        }
        if let LivePhase::Notify(text) = &self.live {
            services.notify(text)?;
            self.live = LivePhase::Complete;
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests;
