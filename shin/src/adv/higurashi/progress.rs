//! A single live progress/response binding, matching native10b14..10b2c.
//! The response cursor and pending variable queues belong to this VM session;
//! the active progress entry owns its response bytes. Disk state is separate.
use super::{
    autosave::{Execution, SaveProgress},
    chart_progress::ChartUnlocks,
    variables::{self, HigurashiVariables},
};
use anyhow::Result;
use shin_core::{
    format::save::PersistData,
    vm::{command::CommandResult, higurashi::command::HigurashiRuntimeCommand as Cmd},
};

#[derive(Clone, Debug)]
pub struct ProgressMemory {
    progress: SaveProgress,
    variables: HigurashiVariables,
    /// Native GameData+68 vector. Preserve its bytes across forks/replacement;
    /// command/UI mutation semantics are separate from the+80 variable bank.
    trailing_bytes: Vec<u8>,
    /// Native settings+d0, outside the per-VM checkpoint and commit queues.
    pub chart_unlocks: ChartUnlocks,
}
impl ProgressMemory {
    /// Explicit loaded progress/settings/GameData+80 values. Native10b28 binds
    /// the active entry's vector with cursor zero and empty per-VM write queues.
    pub fn new(persistent: PersistData, progress: SaveProgress, bytes: Vec<u8>) -> Self {
        Self {
            progress,
            variables: HigurashiVariables::new(persistent, bytes),
            trailing_bytes: Vec::new(),
            chart_unlocks: ChartUnlocks::from_saved_words(Vec::new()),
        }
    }
    pub fn with_chart_unlocks(mut self, unlocks: ChartUnlocks) -> Self {
        self.chart_unlocks = unlocks;
        self
    }
    pub fn with_trailing_bytes(mut self, bytes: Vec<u8>) -> Self {
        self.trailing_bytes = bytes;
        self
    }
    pub fn trailing_bytes(&self) -> &[u8] { &self.trailing_bytes }
    /// Native GameData+68 stores u16 values,including values above255.
    pub fn with_trailing_values(self, values: Vec<u16>) -> Self {
        self.with_trailing_bytes(values.into_iter().flat_map(u16::to_le_bytes).collect())
    }
    pub fn trailing_values(&self) -> Result<Vec<u16>> {
        anyhow::ensure!(self.trailing_bytes.len() % 2 == 0, "Incomplete native u16 trailing vector");
        Ok(self.trailing_bytes.chunks_exact(2).map(|v| u16::from_le_bytes([v[0],v[1]])).collect())
    }
    pub fn progress(&self) -> &SaveProgress {
        &self.progress
    }
    pub(crate) fn progress_mut(&mut self) -> &mut SaveProgress {
        &mut self.progress
    }
    pub fn persistent(&self) -> &PersistData {
        self.variables.persistent()
    }
    pub fn bytes(&self) -> &[u8] {
        self.variables.bytes()
    }
    pub fn pending_persistent(&self) -> &[(u32, i32)] {
        self.variables.pending_persistent()
    }
    pub fn pending_bytes(&self) -> &[(u32, i32)] {
        self.variables.pending_bytes()
    }
    pub fn responses(&self) -> (&[u8], usize) {
        (
            &self.progress.active_entry().response_data,
            self.variables.route().1,
        )
    }
    /// SELECT347c4/347d0 shares the same active vector and cursor with variables.
    pub(super) fn choice_response(&mut self, replay: bool, value: i32) -> Result<i32> {
        let active = self.progress.active_index();
        self.variables.choice_in_progress(&mut self.progress.entries[active].response_data, replay, value)
    }
    /// Native335d4 restores the cursor without truncating/replacing response
    /// bytes or rolling back global settings/GameData byte variables.
    pub fn restore_response_cursor(&mut self, position: usize) -> Result<()> {
        self.variables
            .restore_progress_cursor(&self.progress.active_entry().response_data, position)
    }
    pub fn dispatch_variables(
        &mut self,
        command: &Cmd,
        execution: Execution,
        boundary: u32,
    ) -> Result<Option<CommandResult>> {
        let active = self.progress.active_index();
        let execution = match execution {
            Execution::Live => variables::Execution::Live,
            Execution::Replay => variables::Execution::Replay {
                boundary,
                target: self.progress.active_entry().replay_target,
            },
        };
        self.variables.dispatch_in_progress(
            &mut self.progress.entries[active].response_data,
            command,
            execution,
        )
    }
    /// Native1185c/11a94 commit both ordered queues before capture decisions.
    pub fn commit_pending(&mut self, enabled: bool) -> Result<()> {
        self.variables.commit_pending(enabled)
    }
    /// Native109bc creates a new VM from current GameData: response vectors
    /// remain complete, queued writes start empty, and112cc sets the active pair.
    pub(super) fn fork_for_replay(&self, saved_position: u32, replay_target: u32, cursor: usize) -> Result<Self> {
        let mut progress = self.progress.clone();
        let active = progress.active_index();
        progress.entries[active].save_position = saved_position;
        progress.entries[active].replay_target = replay_target;
        let mut fork = Self::new(self.persistent().clone(), progress, self.bytes().to_vec())
            .with_trailing_bytes(self.trailing_bytes.clone())
            .with_chart_unlocks(self.chart_unlocks.clone());
        fork.variables.restore_checkpoint_cursor(cursor);
        Ok(fork)
    }
}

#[cfg(test)]
mod tests;
