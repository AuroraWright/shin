//! Memory reconstruction using original command constructors and replay polls.
//! Live GPU/audio/UI owners are hydrated only after this state reaches Ready.
use super::{
    layers::Gerokasu2Layers,
    responses,
    save_commands::{AutosaveCommand, SaveSession, StateCheckpoint},
};
use anyhow::{Context, Result, ensure};
use shin_core::{
    format::{save::index_set::IndexSet, scenario::gerokasu2::Gerokasu2Scenario},
    vm::{
        command::{CommandResult, RuntimeCommand},
        gerokasu2::{Gerokasu2RuntimeCommand as Cmd, Gerokasu2Scripter},
    },
};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayStatus {
    CommandBudget,
    Ready,
    Cancelled,
    Published,
}
pub struct RetainedCommand {
    pub command: Cmd,
    pub interactive_first_check: bool,
    pub interactive_result: Option<i32>,
    pub replay_stop: i32,
    pub autosave: Option<AutosaveCommand>,
    pub quiz_bank: Option<shin_core::vm::command::types::LayerbankId>,
}
pub struct ReplaySession {
    scenario: Arc<Gerokasu2Scenario>,
    pub vm: Gerokasu2Scripter,
    pub layers: Box<Gerokasu2Layers>,
    pub save: SaveSession,
    pub pending: Option<RetainedCommand>,
    response: CommandResult,
    status: ReplayStatus,
    fault: Option<String>,
    pub completed_commands: u64,
    pub reforks: u64,
}
impl ReplaySession {
    pub fn check_publication(&self, scenario: &Gerokasu2Scenario) -> Result<()> {
        ensure!(
            self.status == ReplayStatus::Ready && self.fault.is_none(),
            "Ger replay is not ready for publication"
        );
        ensure!(
            self.vm.matches_scenario(scenario),
            "Ger replay publication scenario mismatch"
        );
        Ok(())
    }
    pub(crate) fn take_live_parts(
        &mut self,
    ) -> (
        Gerokasu2Scripter,
        Box<Gerokasu2Layers>,
        SaveSession,
        Option<RetainedCommand>,
        CommandResult,
    ) {
        assert_eq!(self.status, ReplayStatus::Ready);
        self.status = ReplayStatus::Published;
        let fresh = SaveSession::fresh(0, 0);
        (
            self.vm.clone(),
            std::mem::take(&mut self.layers),
            std::mem::replace(&mut self.save, fresh),
            self.pending.take(),
            std::mem::replace(&mut self.response, CommandResult::None),
        )
    }
    pub fn new(
        scenario: Arc<Gerokasu2Scenario>,
        save: SaveSession,
        restored: Option<Arc<StateCheckpoint>>,
    ) -> Result<Self> {
        let (vm, layers) = if let Some(state) = restored {
            ensure!(
                state.vm.matches_scenario(&scenario),
                "Ger replay checkpoint scenario mismatch"
            );
            (state.vm.scripter(), state.layers.clone())
        } else {
            let entry = save.progress.active_entry();
            (
                Gerokasu2Scripter::new(scenario.clone(), entry.scenario_id, entry.random_seed),
                Box::default(),
            )
        };
        // Original1B1A0 checks once on entering a new/replaced VM. Successful
        // replay polls continue across the saved boundary until a poll blocks.
        let status = if save.timeline.boundary >= save.progress.active_entry().save_position {
            ReplayStatus::Ready
        } else {
            ReplayStatus::CommandBudget
        };
        Ok(Self {
            scenario,
            vm,
            layers,
            save,
            pending: None,
            response: CommandResult::None,
            status,
            fault: None,
            completed_commands: 0,
            reforks: 0,
        })
    }
    pub fn from_progress(
        scenario: Arc<Gerokasu2Scenario>,
        progress: shin_core::format::save::gerokasu2::Progress,
    ) -> Result<Self> {
        let save = SaveSession::new(
            progress,
            Default::default(),
            Default::default(),
            Default::default(),
        );
        Self::new(scenario, save, None)
    }
    pub fn status(&self) -> ReplayStatus {
        self.status
    }
    /// Run the desktop load worker in bounded chunks. Cancellation remains
    /// observable between chunks and completed commands. Numeric execution
    /// retains its independent instruction budget.
    pub(crate) fn pump_to_ready(
        &mut self,
        seen: &IndexSet,
        cancelled: impl Fn() -> bool,
    ) -> Result<()> {
        loop {
            ensure!(!cancelled(), "Ger save reconstruction cancelled");
            match self.pump(seen, 4096, 100000, &cancelled)? {
                ReplayStatus::Ready => return Ok(()),
                ReplayStatus::CommandBudget => {}
                ReplayStatus::Cancelled => anyhow::bail!("Ger save reconstruction cancelled"),
                ReplayStatus::Published => {
                    anyhow::bail!("Ger save reconstruction already published")
                }
            }
        }
    }
    pub fn fault(&self) -> Option<&str> {
        self.fault.as_deref()
    }
    fn reached(&self) -> bool {
        self.save.timeline.boundary >= self.save.progress.active_entry().save_position
    }
    pub fn pump(
        &mut self,
        seen: &IndexSet,
        max_commands: usize,
        max_instructions: usize,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<ReplayStatus> {
        ensure!(
            max_commands > 0 && max_instructions > 0,
            "Ger replay budgets must be positive"
        );
        ensure!(
            self.fault.is_none(),
            "Ger replay is faulted: {:?}",
            self.fault
        );
        ensure!(
            self.status != ReplayStatus::Published,
            "Ger replay was already published"
        );
        if self.status == ReplayStatus::Cancelled {
            return Ok(self.status);
        }
        if self.status == ReplayStatus::Ready {
            if cancelled() {
                self.status = ReplayStatus::Cancelled;
            }
            return Ok(self.status);
        }
        for _ in 0..max_commands {
            let completed = self.completed_commands;
            if let Err(error) = self.step(seen, max_instructions) {
                self.fault = Some(format!("{error:#}"));
                return Err(error);
            }
            // Native checks cancellation after successful polls and before
            // resource hydration; a blocked refork re-enters directly.
            if (completed != self.completed_commands || self.status == ReplayStatus::Ready)
                && cancelled()
            {
                self.status = ReplayStatus::Cancelled;
                return Ok(self.status);
            }
            if self.status == ReplayStatus::Ready {
                return Ok(self.status);
            }
        }
        Ok(ReplayStatus::CommandBudget)
    }
    fn prepare(&mut self, seen: &IndexSet, max_instructions: usize) -> Result<()> {
        if self.pending.is_some() {
            return Ok(());
        }
        let command = self.vm.run(
            std::mem::replace(&mut self.response, CommandResult::None),
            max_instructions,
        )?;
        ensure!(
            supported(&command),
            "Ger replay constructor is not implemented: {command:?}"
        );
        let has_text = self.save.descriptors.message.text.is_some();
        let interactive_first_check = !has_text;
        let mut autosave = None;
        let mut quiz_bank = None;
        match &command {
            Cmd::MessageSet { .. } => self.save.message(&self.vm, &self.layers)?,
            Cmd::Selection { .. } => self.save.interactive(&self.vm, &self.layers, has_text)?,
            Cmd::Shared(RuntimeCommand::QUIZ(c)) => {
                self.save.interactive(&self.vm, &self.layers, has_text)?;
                quiz_bank = u16::try_from(c.arg)
                    .ok()
                    .and_then(shin_core::vm::command::types::LayerId::try_new)
                    .and_then(|id| self.layers.target(id))
                    .map(|target| target.layerbank);
            }
            Cmd::Shared(RuntimeCommand::EVBEGIN(c)) => {
                self.save.event_begin(&self.vm, &self.layers, c.arg)?
            }
            Cmd::Shared(RuntimeCommand::EVEND(_)) => self.save.event_end(&self.vm, &self.layers)?,
            Cmd::Shared(RuntimeCommand::RESUMESET(_)) => {
                self.save.resume_set(&self.vm, &self.layers)?
            }
            Cmd::Shared(RuntimeCommand::AUTOSAVE(_)) => {
                autosave = Some(self.save.begin_autosave(&self.vm, &self.layers)?)
            }
            Cmd::Shared(RuntimeCommand::SAVEINFO(c)) => self.save.metadata.set(c.level, &c.info)?,
            _ => {}
        }
        let was_read = if let Cmd::MessageSet { msg_id, .. } = &command {
            (msg_id.0 as i32) < 0 || seen.contains(msg_id.0 as usize)
        } else {
            self.save.descriptors.current_was_read != 0
        };
        self.save.descriptors.prepare(&command, was_read)?;
        // Message properties live in their descriptor at9104, outside the
        // earlier isolated integer LayerState object.
        if !matches!(command, Cmd::LayerCtrl { layer_id: -5, .. }) {
            self.layers.apply(&command)?;
        }
        self.pending = Some(RetainedCommand {
            command,
            interactive_first_check,
            interactive_result: None,
            replay_stop: -1,
            autosave,
            quiz_bank,
        });
        Ok(())
    }
    fn poll(&mut self) -> Result<bool> {
        let reached = self.reached();
        let p = self.pending.as_mut().unwrap();
        match &p.command {
            Cmd::Shared(RuntimeCommand::SGET(_)) => {
                let value = self.save.persistent_replay();
                let destination = self
                    .vm
                    .pending_destination()
                    .context("Ger replay SGET destination missing")?;
                self.vm.write_pending_result(destination, value)?;
                self.response = CommandResult::WriteMemory(destination, value);
                Ok(true)
            }
            Cmd::Shared(RuntimeCommand::SSET(c)) => {
                self.save.variables.set_replay(
                    c.slot_number,
                    c.value,
                    self.save.timeline.boundary,
                    self.save.progress.active_entry().replay_target,
                );
                Ok(true)
            }
            Cmd::Selection { dest, .. } => {
                if p.interactive_first_check && reached {
                    p.replay_stop = 0;
                    return Ok(false);
                }
                let value = responses::read_selection(
                    &mut self.save.progress.active_entry_mut().responses,
                    &mut self.save.response_bit_position,
                );
                self.vm.write_pending_result(*dest, value)?;
                p.interactive_result = Some(value);
                self.response = CommandResult::WriteMemory(*dest, value);
                self.save.timeline.boundary = self.save.timeline.boundary.wrapping_add(1);
                let reached =
                    self.save.timeline.boundary >= self.save.progress.active_entry().save_position;
                if reached {
                    p.replay_stop = 1;
                }
                Ok(!reached)
            }
            Cmd::Shared(RuntimeCommand::QUIZ(_)) => {
                if p.interactive_first_check && reached {
                    p.replay_stop = 0;
                    return Ok(false);
                }
                let bank = p
                    .quiz_bank
                    .context("Ger replay QUIZ has no valid native bank")?;
                let value = responses::read_quiz(
                    &mut self.save.progress.active_entry_mut().responses,
                    &mut self.save.response_bit_position,
                );
                let destination = self
                    .vm
                    .pending_destination()
                    .context("Ger replay QUIZ destination missing")?;
                self.vm.write_pending_result(destination, value)?;
                self.layers.complete_interaction(bank, value);
                p.interactive_result = Some(value);
                self.response = CommandResult::WriteMemory(destination, value);
                self.save.timeline.boundary = self.save.timeline.boundary.wrapping_add(1);
                let reached =
                    self.save.timeline.boundary >= self.save.progress.active_entry().save_position;
                if reached {
                    p.replay_stop = 1;
                }
                Ok(!reached)
            }
            Cmd::Shared(RuntimeCommand::AUTOSAVE(_)) => p
                .autosave
                .as_mut()
                .unwrap()
                .poll_replay(&self.save.progress, self.save.timeline.boundary),
            Cmd::MessageSet { .. }
            | Cmd::Shared(
                RuntimeCommand::EVBEGIN(_)
                | RuntimeCommand::EVEND(_)
                | RuntimeCommand::RESUMESET(_),
            ) => Ok(!reached),
            Cmd::Shared(RuntimeCommand::EXIT(c)) => Ok(c.arg1 != 0),
            Cmd::Shared(RuntimeCommand::RESUME(_)) => Ok(false),
            _ => Ok(true),
        }
    }
    fn step(&mut self, seen: &IndexSet, max_instructions: usize) -> Result<()> {
        self.prepare(seen, max_instructions)?;
        if self.poll()? {
            self.pending = None;
            self.completed_commands += 1;
        } else if self.save.timeline.event >= 0 || self.save.descriptors.restart_required != 0 {
            let target = self.save.timeline.last_skip;
            let (save, state) = self.save.fork_for_restore(target, target);
            let mut replacement = Self::new(self.scenario.clone(), save, state)?;
            replacement.completed_commands = self.completed_commands;
            replacement.reforks = self.reforks + 1;
            *self = replacement;
        } else {
            self.status = ReplayStatus::Ready;
        }
        Ok(())
    }
}
fn supported(command: &Cmd) -> bool {
    matches!(
        command,
        Cmd::Audio(_)
            | Cmd::Wait { .. }
            | Cmd::MessageInit { .. }
            | Cmd::MessageSet { .. }
            | Cmd::MessageFace { .. }
            | Cmd::GerMessageFace { .. }
            | Cmd::VoicePlay { .. }
            | Cmd::VoiceWait { .. }
            | Cmd::Selection { .. }
            | Cmd::LayerLoad { .. }
            | Cmd::LayerInit { .. }
            | Cmd::LayerUnload { .. }
            | Cmd::LayerCtrl { .. }
            | Cmd::LayerWait { .. }
            | Cmd::LayerSelect { .. }
            | Cmd::MovieWait { .. }
            | Cmd::TransWait { .. }
            | Cmd::Wipe { .. }
            | Cmd::TransitionSet { .. }
            | Cmd::PlaneSelect { .. }
            | Cmd::MaskLoad { .. }
            | Cmd::Shared(
                RuntimeCommand::EXIT(_)
                    | RuntimeCommand::SGET(_)
                    | RuntimeCommand::SSET(_)
                    | RuntimeCommand::MSGWAIT(_)
                    | RuntimeCommand::MSGSIGNAL(_)
                    | RuntimeCommand::MSGSYNC(_)
                    | RuntimeCommand::MSGCLOSE(_)
                    | RuntimeCommand::VOICESTOP(_)
                    | RuntimeCommand::SAVEINFO(_)
                    | RuntimeCommand::AUTOSAVE(_)
                    | RuntimeCommand::EVBEGIN(_)
                    | RuntimeCommand::EVEND(_)
                    | RuntimeCommand::RESUMESET(_)
                    | RuntimeCommand::RESUME(_)
                    | RuntimeCommand::SYSCALL(_)
                    | RuntimeCommand::TROPHY(_)
                    | RuntimeCommand::UNLOCK(_)
                    | RuntimeCommand::PAGEBACK(_)
                    | RuntimeCommand::PLANECLEAR(_)
                    | RuntimeCommand::LAYERSWAP(_)
                    | RuntimeCommand::MASKUNLOAD(_)
                    | RuntimeCommand::WIPEWAIT(_)
                    | RuntimeCommand::QUIZ(_)
                    | RuntimeCommand::DEBUGOUT(_)
            )
    )
}
pub mod chapter_skip;
#[cfg(test)]
mod native_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod worker_tests;
