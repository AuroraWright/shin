//! Memory replay for the updated18db0 reconstruction worker. Arithmetic,
//! constructor snapshots, integer layers and response owners advance together;
//! live GPU/audio/UI callbacks are deferred until publication.
use super::{
    autosave::{AutosaveCommand, Execution},
    checkpoints::{CaptureServices, CheckpointMemory, RestoredMemory},
    choice::ChoiceCommand,
    layers::LayerBanks,
    progress::ProgressMemory,
    read_history::MessageReadHistory,
    read_state::{Checkpoint, CommandReadState, ReadServices},
};
use anyhow::{Context, Result, bail, ensure};
use shin_core::{
    format::scenario::higurashi::HigurashiScenario,
    vm::{
        command::CommandResult,
        higurashi::{HigurashiScripter, command::HigurashiRuntimeCommand as Cmd},
    },
};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    CommandBudget,
    Ready,
    Cancelled,
    Published,
}

pub(super) struct RetainedCommand {
    pub command: Cmd,
    pub autosave: Option<AutosaveCommand>,
    pub choice: Option<ChoiceCommand>,
    pub character: Option<super::character_selection::command::CharacterCommand>,
    pub tips_get: Option<super::tips_get::TipsGetCommand>,
    pub notification: Option<super::notification_command::NotificationCommand>,
    pub kakera: Option<super::kakera::KakeraCommand>,
    pub scenario_select: Option<super::scenario_select::ScenarioSelectCommand>,
    pub quiz: Option<super::quiz::QuizCommand>,
}

pub struct ReplaySession {
    scenario: Arc<HigurashiScenario>,
    pub(super) vm: Option<HigurashiScripter>,
    pub read: CommandReadState,
    pub layers: Box<LayerBanks>,
    pub memory: CheckpointMemory,
    pub progress: ProgressMemory,
    pub(super) pending: Option<RetainedCommand>,
    pub(super) response: CommandResult,
    status: Status,
    fault: Option<String>,
    pub completed_commands: u64,
    pub exit_forks: u64,
    quiz_score: i32,
}

struct Services<'a> {
    memory: &'a mut CheckpointMemory,
    progress: &'a mut ProgressMemory,
    history: &'a MessageReadHistory,
}
impl ReadServices for Services<'_> {
    fn checkpoint_owner(&mut self) -> Option<(&mut CheckpointMemory, &mut ProgressMemory)> {
        Some((self.memory, self.progress))
    }
    fn checkpoint(&mut self, _: Checkpoint, _: super::input::ReadState) -> Result<()> {
        bail!("Replay capture requires the current constructor context")
    }
    fn message_is_read(&self, id: u32) -> bool {
        self.history.contains(id)
    }
    fn message_completed(&mut self, _: u32) -> Result<()> {
        bail!("Memory replay cannot publish live message completion")
    }
}

impl ReplaySession {
    /// The fork owns current progress/globals and the pruned checkpoint archive.
    /// An empty archive starts with the active scenario ID/seed, as109bc does.
    pub fn new(scenario: Arc<HigurashiScenario>, fork: RestoredMemory) -> Result<Self> {
        let (vm, read, layers) = if let Some(saved) = fork.restored {
            ensure!(
                saved.vm.matches_scenario(&scenario),
                "Higurashi replay scenario mismatch"
            );
            (
                saved.vm.scripter(),
                saved.read.clone(),
                saved.layers.clone(),
            )
        } else {
            let active = fork.progress.progress().active_entry();
            (
                HigurashiScripter::new(&scenario, active.scenario_id, active.random_seed)?,
                CommandReadState::new(),
                Box::new(LayerBanks::default()),
            )
        };
        // Native18dd4 checks once on entering a new/replaced VM. Crossing a
        // boundary during successful polls does not itself end the loop.
        let status =
            if read.timeline().boundary >= fork.progress.progress().active_entry().save_position {
                Status::Ready
            } else {
                Status::CommandBudget
            };
        Ok(Self {
            scenario,
            vm: Some(vm),
            read,
            layers,
            memory: fork.memory,
            progress: fork.progress,
            pending: None,
            response: CommandResult::None,
            status,
            fault: None,
            completed_commands: 0,
            exit_forks: 0,
            quiz_score: 0,
        })
    }
    pub fn status(&self) -> Status {
        self.status
    }
    pub(crate) fn set_quiz_score(&mut self, score: i32) {
        self.quiz_score = score;
    }
    pub fn pending_command(&self) -> Option<&Cmd> {
        self.pending.as_ref().map(|p| &p.command)
    }
    pub fn fault(&self) -> Option<&str> {
        self.fault.as_deref()
    }
    pub(super) fn check_reconstruction(&self, scenario: &HigurashiScenario) -> Result<()> {
        ensure!(
            self.scenario.raw() == scenario.raw(),
            "Higurashi reconstruction scenario mismatch"
        );
        ensure!(
            self.vm.is_some()
                && self.fault.is_none()
                && matches!(self.status, Status::CommandBudget | Status::Ready),
            "Higurashi reconstruction requires a usable replay fork"
        );
        Ok(())
    }
    pub(super) fn check_publication(&self, scenario: &HigurashiScenario) -> Result<()> {
        ensure!(
            self.status == Status::Ready && self.fault.is_none(),
            "Replay has not reached publication"
        );
        ensure!(
            self.vm
                .as_ref()
                .context("Replay VM already published")?
                .matches_scenario(scenario),
            "Higurashi reconstruction scenario mismatch"
        );
        Ok(())
    }
    pub(super) fn mark_published(&mut self) {
        self.status = Status::Published;
    }

    /// Caller-selected command slices make cancellation/fairness bounded. A VM
    /// or constructor error faults this session rather than replaying a partial
    /// response/capture. Reconstruct a new fork to retry such a failure.
    pub fn pump(
        &mut self,
        history: &MessageReadHistory,
        max_commands: usize,
        max_instructions: usize,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Status> {
        ensure!(
            max_commands > 0 && max_instructions > 0,
            "Replay budgets must be positive"
        );
        ensure!(
            self.fault.is_none(),
            "Higurashi replay is faulted: {:?}",
            self.fault
        );
        if self.status == Status::Published || self.status == Status::Cancelled {
            return Ok(self.status);
        }
        if self.status == Status::Ready {
            if cancelled() {
                self.status = Status::Cancelled;
            }
            return Ok(self.status);
        }
        for _ in 0..max_commands {
            let completed = self.completed_commands;
            if let Err(error) = self.step(history, max_instructions) {
                self.fault = Some(format!("{error:#}"));
                return Err(error);
            }
            // Native18e04 checks after a completed poll,18e64 before assets.
            // A blocked EXIT refork goes directly back to decoding when its
            // new VM has not reached the saved boundary yet.
            if (self.completed_commands != completed || self.status == Status::Ready) && cancelled()
            {
                self.status = Status::Cancelled;
                return Ok(self.status);
            }
            if self.status == Status::Ready {
                return Ok(self.status);
            }
        }
        Ok(Status::CommandBudget)
    }

    pub(super) fn prepare_command(
        &mut self,
        history: &MessageReadHistory,
        max_instructions: usize,
    ) -> Result<()> {
        if self.pending.is_none() {
            let command = self.vm.as_mut().unwrap().run(
                std::mem::replace(&mut self.response, CommandResult::None),
                max_instructions,
            )?;
            // These constructors have audited memory owners. Interactive/menu
            // commands with additional state remain explicit until implemented.
            ensure!(
                supported(&command),
                "Higurashi replay constructor is not implemented: {command:?}"
            );
            let choice = if matches!(command, Cmd::SELECT { .. }) {
                Some(ChoiceCommand::new(
                    &command,
                    self.read.descriptors().message.text.is_none(),
                )?)
            } else {
                None
            };
            let before = self.read.constructor_capture(&command);
            self.read.prepare(
                &command,
                &mut CaptureServices {
                    services: &mut Services {
                        memory: &mut self.memory,
                        progress: &mut self.progress,
                        history,
                    },
                    vm: self.vm.as_ref().unwrap(),
                    read: &before,
                    layers: &self.layers,
                },
            )?;
            match &command {
                Cmd::LAYERSELECT { args: [from, to] } => self.layers.select(*from, *to),
                Cmd::LAYERBACK { argument } => {
                    self.layers.start_back(*argument)?;
                }
                Cmd::LAYERINIT { id } => {
                    self.layers.initialize(*id)?;
                }
                Cmd::LAYERLOAD {
                    args: [id, kind, flags],
                    params,
                } => {
                    self.layers.load(*id, *kind, *flags as u32, *params)?;
                }
                // Delay/tween/wipe clocks live outside the integer checkpoint.
                Cmd::LAYERUNLOAD { args: [id, _] } => {
                    self.layers.unload(*id)?;
                }
                Cmd::LAYERCTRL {
                    args: [id, property],
                    params,
                } => {
                    self.layers
                        .control(*id, *property, params.0, params.2 as u32)?;
                }
                Cmd::WIPE { .. } => {
                    self.layers.commit_back();
                }
                _ => {}
            }
            let character = if matches!(command, Cmd::CHARSELECT { .. }) {
                Some(super::character_selection::command::CharacterCommand::new(
                    &command,
                    &mut self.layers,
                )?)
            } else {
                None
            };
            let tips_get = if matches!(command, Cmd::TIPSGET { .. }) {
                Some(super::tips_get::TipsGetCommand::new(
                    &command,
                    &mut self.layers,
                )?)
            } else {
                None
            };
            let notification = if matches!(command, Cmd::OTSUGET { .. } | Cmd::KAKERAGET { .. }) {
                Some(super::notification_command::NotificationCommand::new(
                    &command,
                    &mut self.layers,
                )?)
            } else {
                None
            };
            let autosave = matches!(command, Cmd::AUTOSAVE { .. }).then(AutosaveCommand::new);
            let kakera = matches!(command, Cmd::KAKERA {}).then(super::kakera::KakeraCommand::new);
            let scenario_select = if matches!(command, Cmd::SNRSEL { .. }) {
                Some(super::scenario_select::ScenarioSelectCommand::new(
                    &command,
                )?)
            } else {
                None
            };
            let quiz = if matches!(command, Cmd::QUIZ { .. }) {
                Some(super::quiz::QuizCommand::new(&command)?)
            } else {
                None
            };
            self.pending = Some(RetainedCommand {
                command,
                choice,
                autosave,
                character,
                tips_get,
                notification,
                kakera,
                scenario_select,
                quiz,
            });
        }
        Ok(())
    }

    pub(super) fn poll_command(&mut self, forward_read_only: Option<bool>) -> Result<bool> {
        let pending = self.pending.as_mut().unwrap();
        let complete = if let Some(response) = self.progress.dispatch_variables(
            &pending.command,
            if forward_read_only.is_some() {
                Execution::Live
            } else {
                Execution::Replay
            },
            self.read.timeline().boundary,
        )? {
            if let CommandResult::WriteMemory(dest, value) = response {
                // Native getter polls publish before10fc4 retires the command,
                // including when cancellation follows before the next decode.
                self.vm
                    .as_mut()
                    .unwrap()
                    .write_pending_result(dest, value)?;
            }
            self.response = response;
            true
        } else if let Some(choice) = pending.choice.as_mut() {
            let complete = if forward_read_only.is_some() {
                choice.replay_stop() > 0
            } else {
                choice.poll_replay(
                    &mut self.progress,
                    &mut self.read.timeline_mut().boundary,
                    |dest, value| self.vm.as_mut().unwrap().write_pending_result(dest, value),
                )?
            };
            if complete {
                self.response = CommandResult::WriteMemory(
                    choice.request().destination,
                    choice.result().context("Replayed choice has no response")?,
                );
            }
            complete
        } else if let Some(character) = pending.character.as_mut() {
            let complete = if forward_read_only.is_some() {
                character.replay_stop() > 0
            } else {
                character.poll_replay(
                    &mut self.progress,
                    &mut self.read.timeline_mut().boundary,
                    |dest, value| self.vm.as_mut().unwrap().write_pending_result(dest, value),
                )?
            };
            if complete {
                self.response = CommandResult::WriteMemory(
                    character.request().destination,
                    character
                        .result()
                        .context("Replayed character command has no response")?,
                );
            }
            complete
        } else if let Some(notification) = pending.notification.as_mut() {
            if forward_read_only.is_some() {
                notification.forward_complete()
            } else {
                notification.poll_replay()
            }
        } else if let Some(tips) = pending.tips_get.as_mut() {
            if forward_read_only.is_some() {
                tips.forward_complete()
            } else {
                tips.poll_replay(
                    &self.progress,
                    &mut self.read.timeline_mut().boundary,
                    self.quiz_score as u32,
                )?
            }
        } else if let Some(quiz) = pending.quiz.as_mut() {
            if forward_read_only.is_some() {
                quiz.forward_complete()
            } else {
                let complete = quiz.poll_replay(
                    &mut self.progress,
                    self.read.timeline().boundary,
                    self.quiz_score,
                    |dest, value| self.vm.as_mut().unwrap().write_pending_result(dest, value),
                )?;
                // The earlier native branch does not change the register. The
                // VM feedback bridge retires it with its current value.
                let dest = quiz.request().destination;
                let value = quiz
                    .result()
                    .unwrap_or_else(|| self.vm.as_ref().unwrap().read_register(dest));
                self.response = CommandResult::WriteMemory(dest, value);
                complete
            }
        } else if let Some(scenario_select) = pending.scenario_select.as_mut() {
            if forward_read_only.is_some() {
                scenario_select.forward_complete()
            } else {
                scenario_select
                    .poll_replay(self.progress.progress(), self.read.timeline().boundary)?
            }
        } else if let Some(kakera) = pending.kakera.as_mut() {
            if forward_read_only.is_some() {
                false
            } else {
                kakera.poll_replay(self.progress.progress(), self.read.timeline().boundary)?
            }
        } else if let Some(autosave) = pending.autosave.as_mut() {
            if forward_read_only.is_some() {
                autosave.flags()[1] != 0
            } else {
                autosave.poll_replay(self.progress.progress(), self.read.timeline().boundary)?
            }
        } else if let Some(read_only) = forward_read_only {
            match &pending.command {
                Cmd::EXIT { arg1: 0, .. } => false,
                Cmd::MSGSET { .. } => {
                    self.read.state().flags & 0x8000_0000 == 0
                        || !read_only
                        || self.read.state().is_read != 0
                }
                _ => true,
            }
        } else {
            match &pending.command {
                Cmd::EXIT { arg1: 0, .. } => false,
                Cmd::MSGSET { .. } | Cmd::EVBEGIN { .. } | Cmd::EVEND { .. } => {
                    self.read.timeline().boundary
                        < self.progress.progress().active_entry().save_position
                }
                _ => true,
            }
        };
        Ok(complete)
    }

    pub(super) fn retire_command(&mut self) {
        if let Some(character) = self.pending.as_mut().and_then(|p| p.character.as_mut()) {
            character.release(&mut self.layers);
        }
        if let Some(tips) = self.pending.as_mut().and_then(|p| p.tips_get.as_mut()) {
            tips.release(&mut self.layers);
        }
        if let Some(notification) = self.pending.as_mut().and_then(|p| p.notification.as_mut()) {
            notification.release(&mut self.layers);
        }
        self.pending = None;
        self.completed_commands += 1;
    }

    fn step(&mut self, history: &MessageReadHistory, max_instructions: usize) -> Result<()> {
        self.prepare_command(history, max_instructions)?;
        if self.poll_command(None)? {
            self.retire_command();
        } else if self.read.state().fast_forward_blocked != 0 {
            // Native18e24 forks the current memory to replay_target after EXIT,
            // then rechecks the entry boundary before decoding any command.
            let target = self.read.timeline().replay_target;
            let fork = self
                .memory
                .fork_for_restore(target, target, &self.progress)?;
            let mut replacement = Self::new(self.scenario.clone(), fork)?;
            replacement.quiz_score = self.quiz_score;
            replacement.completed_commands = self.completed_commands;
            replacement.exit_forks = self.exit_forks + 1;
            *self = replacement;
        } else {
            self.status = Status::Ready;
        }
        Ok(())
    }
}

fn supported(command: &Cmd) -> bool {
    matches!(
        command,
        Cmd::EXIT { .. }
            | Cmd::SGET { .. }
            | Cmd::SSET { .. }
            | Cmd::OpcodeDA { .. }
            | Cmd::OpcodeDB { .. }
            | Cmd::WAIT { .. }
            | Cmd::MSGINIT { .. }
            | Cmd::MSGSET { .. }
            | Cmd::MSGWAIT { .. }
            | Cmd::MSGSIGNAL { .. }
            | Cmd::MSGSYNC { .. }
            | Cmd::MSGCLOSE { .. }
            | Cmd::MSGFACE { .. }
            | Cmd::LOGSET { .. }
            | Cmd::SELECT { .. }
            | Cmd::CHARSELECT { .. }
            | Cmd::TIPSGET { .. }
            | Cmd::OTSUGET { .. }
            | Cmd::KAKERAGET { .. }
            | Cmd::KAKERA { .. }
            | Cmd::SNRSEL { .. }
            | Cmd::QUIZ { .. }
            | Cmd::WIPE { .. }
            | Cmd::WIPEWAIT { .. }
            | Cmd::BGMPLAY { .. }
            | Cmd::BGMSTOP { .. }
            | Cmd::BGMVOL { .. }
            | Cmd::BGMWAIT { .. }
            | Cmd::BGMSYNC { .. }
            | Cmd::SEPLAY { .. }
            | Cmd::SESTOP { .. }
            | Cmd::SESTOPALL { .. }
            | Cmd::SEVOL { .. }
            | Cmd::SEPAN { .. }
            | Cmd::SEWAIT { .. }
            | Cmd::SEONCE { .. }
            | Cmd::VOICEPLAY { .. }
            | Cmd::VOICESTOP { .. }
            | Cmd::VOICEWAIT { .. }
            | Cmd::SAVEINFO { .. }
            | Cmd::CHART { .. }
            | Cmd::AUTOSAVE { .. }
            | Cmd::EVBEGIN { .. }
            | Cmd::EVEND { .. }
            | Cmd::TROPHY { .. }
            | Cmd::UNLOCK { .. }
            | Cmd::FAKESELECT { .. }
            | Cmd::LAYERINIT { .. }
            | Cmd::LAYERLOAD { .. }
            | Cmd::LAYERUNLOAD { .. }
            | Cmd::LAYERCTRL { .. }
            | Cmd::LAYERWAIT { .. }
            | Cmd::LAYERBACK { .. }
            | Cmd::LAYERSELECT { .. }
            | Cmd::MOVIEWAIT { .. }
            | Cmd::DEBUGOUT { .. }
    )
}

#[cfg(test)]
pub(super) mod tests;
