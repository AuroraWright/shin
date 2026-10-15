//! Original1155c/11664 forward memory searches. Their first pass uses replay
//! polls; the second uses command virtual+18, including live variable effects
//! in a temporary VM whose commit gate stays disabled.
use super::{
    checkpoints::CheckpointMemory, progress::ProgressMemory, read_history::MessageReadHistory,
    replay::ReplaySession,
};
use anyhow::{Result, ensure};
use shin_core::format::scenario::higurashi::HigurashiScenario;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Native1155c: stop at the first command disallowed by read policy or an
    /// interactive/chapter/EXIT barrier, then require a strictly later target.
    ReadAhead,
    /// Native11664: available only inside an event; stop after normal flags
    /// return, or at an interactive/EXIT barrier, without the later-target test.
    EventEnd,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    CommandBudget,
    Complete,
    Cancelled,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Replay,
    Forward,
}

pub struct ForwardSearch {
    scenario: Arc<HigurashiScenario>,
    pub(super) temporary: ReplaySession,
    source_boundary: u32,
    kind: Kind,
    read_only: bool,
    phase: Phase,
    check_event_flags: bool,
    status: Status,
    fault: Option<String>,
    result: Option<ReplaySession>,
}
impl ForwardSearch {
    pub fn begin(
        scenario: Arc<HigurashiScenario>,
        memory: &CheckpointMemory,
        progress: &ProgressMemory,
        source_boundary: u32,
        source_flags: u32,
        kind: Kind,
        read_only: bool,
    ) -> Result<Option<Self>> {
        if kind == Kind::EventEnd && source_flags & 0x8000_0000 != 0 {
            return Ok(None);
        }
        let temporary = ReplaySession::new(
            scenario.clone(),
            memory.clone_for_search(source_boundary, progress)?,
        )?;
        // These helpers always decode/poll in their first pass, even if the
        // initial boundary already satisfies18db0's separate entry shortcut.
        Ok(Some(Self {
            scenario,
            temporary,
            source_boundary,
            kind,
            read_only,
            phase: Phase::Replay,
            check_event_flags: false,
            status: Status::CommandBudget,
            fault: None,
            result: None,
        }))
    }
    pub fn status(&self) -> Status {
        self.status
    }
    pub fn fault(&self) -> Option<&str> {
        self.fault.as_deref()
    }
    /// A result is a fresh112cc fork, not the advanced temporary VM. Replay it
    /// to its saved boundary before giving it to reconstruct_replay.
    pub fn take_result(&mut self) -> Result<Option<ReplaySession>> {
        ensure!(
            self.status == Status::Complete && self.fault.is_none(),
            "Forward search is incomplete"
        );
        Ok(self.result.take())
    }
    /// Fairness/cancellation are additional host scheduling boundaries around
    /// the native synchronous algorithm. Cancellation discards the temporary
    /// result; it cannot publish queued writes or alter the source VM/history.
    pub fn pump(
        &mut self,
        history: &MessageReadHistory,
        max_commands: usize,
        max_instructions: usize,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Status> {
        ensure!(
            max_commands > 0 && max_instructions > 0,
            "Forward search budgets must be positive"
        );
        ensure!(
            self.fault.is_none(),
            "Forward search is faulted: {:?}",
            self.fault
        );
        if self.status != Status::CommandBudget {
            return Ok(self.status);
        }
        for _ in 0..max_commands {
            if cancelled() {
                self.result = None;
                self.status = Status::Cancelled;
                return Ok(self.status);
            }
            if let Err(error) = self.step(history, max_instructions) {
                self.fault = Some(format!("{error:#}"));
                return Err(error);
            }
            if self.status == Status::Complete {
                return Ok(self.status);
            }
        }
        Ok(Status::CommandBudget)
    }
    fn finish(&mut self) -> Result<()> {
        if self.kind == Kind::ReadAhead {
            let target = self.temporary.read.timeline().replay_target;
            self.temporary.read.timeline_mut().boundary = target;
            if target <= self.source_boundary {
                self.status = Status::Complete;
                return Ok(());
            }
        }
        let target = self.temporary.read.timeline().boundary;
        let fork = self.temporary.memory.fork_for_restore(
            target,
            self.source_boundary,
            &self.temporary.progress,
        )?;
        self.result = Some(ReplaySession::new(self.scenario.clone(), fork)?);
        self.status = Status::Complete;
        Ok(())
    }
    fn step(&mut self, history: &MessageReadHistory, max_instructions: usize) -> Result<()> {
        if self.phase == Phase::Forward
            && self.check_event_flags
            && self.kind == Kind::EventEnd
            && self.temporary.read.state().flags & 0x8000_0000 != 0
        {
            return self.finish();
        }
        self.temporary.prepare_command(history, max_instructions)?;
        let forward = if self.phase == Phase::Forward {
            Some(self.read_only)
        } else {
            None
        };
        if self.temporary.poll_command(forward)? {
            self.temporary.retire_command();
            if self.phase == Phase::Forward {
                self.check_event_flags = true;
            }
        } else if self.phase == Phase::Replay {
            // The retained command receives its+18 callback first; event flags
            // are checked only after that command successfully retires.
            self.phase = Phase::Forward;
            self.check_event_flags = false;
        } else {
            self.finish()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
