//! Updated d3ec/d810/d8cc command, separate from the drawn character layer.
use super::super::{
    autosave::AutosaveMemory,
    choice::ChoiceHistory,
    layers::{HigurashiLayers, LayerBanks, Target},
    progress::ProgressMemory,
};
use anyhow::{Result, bail, ensure};
use shin_core::{
    format::scenario::instruction_elements::Register,
    vm::higurashi::command::HigurashiRuntimeCommand as Cmd,
};

#[derive(Clone, Debug)]
pub struct CharacterRequest {
    pub index: i32,
    pub destination: Register,
    pub mask: u32,
}
pub trait CharacterServices {
    /// Start/retain one atlas producer, then publish the character scene layer
    /// at target and its non-drawing root input wrapper. False retains loading;
    /// true acknowledges both attachments, before the initial11780 copy.
    fn prepare(
        &mut self,
        request: &CharacterRequest,
        target: Target,
        layers: &mut HigurashiLayers,
    ) -> Result<bool>;
    /// The wrapper callback supplies the original character ID once it closes.
    fn poll(&mut self) -> Result<Option<i32>>;
    fn packed_time(&mut self) -> Result<u64>;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Loading,
    OpeningCopy,
    Waiting,
    Last,
    Response,
    FinalCopy,
    Complete,
}
#[derive(Debug)]
pub struct CharacterCommand {
    request: CharacterRequest,
    reservation: Option<Target>,
    replay_stop: i32,
    replay_polled: bool,
    result: Option<i32>,
    phase: Phase,
    copy_started: bool,
}
impl CharacterCommand {
    /// The caller first clears text/captures/advances the native boundary. This
    /// reserves the command's mapping, even when replay later stops before UI.
    pub(in crate::adv::higurashi) fn new(command: &Cmd, layers: &mut LayerBanks) -> Result<Self> {
        let Cmd::CHARSELECT {
            index,
            dest,
            argument,
        } = command
        else {
            bail!("Character owner requires CHARSELECT");
        };
        Ok(Self {
            request: CharacterRequest {
                index: i32::from(*index) - 1,
                destination: *dest,
                mask: *argument as u32,
            },
            reservation: Some(layers.reserve_temporary()?),
            replay_stop: -1,
            replay_polled: false,
            result: None,
            phase: Phase::Loading,
            copy_started: false,
        })
    }
    pub fn request(&self) -> &CharacterRequest {
        &self.request
    }
    pub fn target(&self) -> Option<Target> {
        self.reservation
    }
    pub fn replay_stop(&self) -> i32 {
        self.replay_stop
    }
    pub fn result(&self) -> Option<i32> {
        self.result
    }
    pub(in crate::adv::higurashi) fn release(&mut self, layers: &mut LayerBanks) {
        if let Some(target) = self.reservation.take() {
            layers.release_temporary(target);
        }
    }
    pub fn poll_replay(
        &mut self,
        progress: &mut ProgressMemory,
        boundary: &mut u32,
        mut publish: impl FnMut(Register, i32) -> Result<()>,
    ) -> Result<bool> {
        ensure!(
            self.phase == Phase::Loading,
            "CHARSELECT live effects must finish before replay"
        );
        if self.replay_polled {
            return Ok(self.replay_stop < 0);
        }
        let target = progress.progress().active_entry().save_position;
        if self.result.is_none() && *boundary >= target {
            self.replay_stop = 0;
            self.replay_polled = true;
            return Ok(false);
        }
        let value = if let Some(value) = self.result {
            value
        } else {
            let value = progress.choice_response(true, 0)?;
            self.result = Some(value);
            value
        };
        publish(self.request.destination, value)?;
        *boundary = boundary.wrapping_add(1);
        if *boundary >= target {
            self.replay_stop = 1;
        }
        self.replay_polled = true;
        Ok(self.replay_stop < 0)
    }
    fn copy(
        &mut self,
        progress: &mut ProgressMemory,
        memory: &mut AutosaveMemory,
        boundary: u32,
        services: &mut impl CharacterServices,
    ) -> Result<()> {
        if !self.copy_started {
            let active = progress.progress().active_index();
            let entry = &mut progress.progress_mut().entries[active];
            entry.save_position = boundary;
            entry.replay_target = boundary;
            memory.valid = true;
            self.copy_started = true;
        }
        let time = services.packed_time()?;
        memory.capture(progress.progress(), time);
        self.copy_started = false;
        Ok(())
    }
    pub fn poll_live(
        &mut self,
        progress: &mut ProgressMemory,
        memory: &mut AutosaveMemory,
        history: &mut ChoiceHistory,
        boundary: &mut u32,
        layers: &mut HigurashiLayers,
        services: &mut impl CharacterServices,
        mut publish: impl FnMut(Register, i32) -> Result<()>,
    ) -> Result<bool> {
        // d8cc completes a consumed replay stop without loading, UI or copies.
        if self.replay_stop > 0 {
            return Ok(true);
        }
        if self.phase == Phase::Loading {
            if !services.prepare(
                &self.request,
                self.reservation.expect("Retained CHARSELECT reservation"),
                layers,
            )? {
                return Ok(false);
            }
            self.phase = Phase::OpeningCopy;
        }
        if self.phase == Phase::OpeningCopy {
            self.copy(progress, memory, *boundary, services)?;
            self.phase = Phase::Waiting;
        }
        if self.phase == Phase::Waiting {
            let Some(value) = services.poll()?.filter(|value| *value >= 0) else {
                return Ok(false);
            };
            self.result = Some(value);
            self.phase = Phase::Last;
        }
        let value = self.result.expect("Completed character callback");
        if self.phase == Phase::Last {
            // d8cc shares last[index], but never marks SELECT's seen bitmap.
            history.record_last(self.request.index, value)?;
            publish(self.request.destination, value)?;
            self.phase = Phase::Response;
        }
        if self.phase == Phase::Response {
            progress.choice_response(false, value)?;
            *boundary = boundary.wrapping_add(1);
            self.phase = Phase::FinalCopy;
        }
        if self.phase == Phase::FinalCopy {
            self.copy(progress, memory, *boundary, services)?;
            self.phase = Phase::Complete;
        }
        Ok(self.phase == Phase::Complete)
    }
}

#[cfg(test)]
pub(in crate::adv::higurashi) mod tests;
