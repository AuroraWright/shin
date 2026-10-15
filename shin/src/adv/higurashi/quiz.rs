//! Updated3d48/fcac/fd34/fd3c QUIZ:checkpoint,score injection and owned scene.
use super::{
    autosave::{AutosaveMemory},
    progress::ProgressMemory,
};
use anyhow::{Result, bail, ensure};
use shin_core::{
    format::scenario::instruction_elements::Register,
    vm::higurashi::command::HigurashiRuntimeCommand as Cmd,
};

#[derive(Debug, Clone)]
pub struct Request {
    pub destination: Register,
    pub args: [i32; 3],
}
pub trait Services {
    fn packed_time(&mut self) -> Result<u64>;
    fn queue_scene(&mut self, args: [i32; 3], source: &ProgressMemory) -> Result<()>;
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    New,
    Clock,
    Queue,
}
#[derive(Debug)]
pub struct QuizCommand {
    request: Request,
    replay_stop: i32,
    result: Option<i32>,
    phase: Phase,
    response_started: bool,
}
impl QuizCommand {
    pub fn new(command: &Cmd) -> Result<Self> {
        let Cmd::QUIZ { dest, args } = command else {
            bail!("Quiz owner requires QUIZ")
        };
        Ok(Self {
            request: Request {
                destination: *dest,
                args: *args,
            },
            replay_stop: -1,
            result: None,
            phase: Phase::New,
            response_started: false,
        })
    }
    pub fn request(&self) -> &Request {
        &self.request
    }
    pub fn replay_stop(&self) -> i32 {
        self.replay_stop
    }
    pub fn result(&self) -> Option<i32> {
        self.result
    }
    /// fcac does not consume an old response before save_position. At/after
    /// that boundary it records the current ADV28 score and retains stop0.
    pub fn poll_replay(
        &mut self,
        progress: &mut ProgressMemory,
        boundary: u32,
        score: i32,
        mut publish: impl FnMut(Register, i32) -> Result<()>,
    ) -> Result<bool> {
        ensure!(self.phase == Phase::New, "QUIZ live copy is still pending");
        if boundary < progress.progress().active_entry().save_position {
            return Ok(true);
        }
        if !self.response_started {
            progress.choice_response(false, score)?;
            self.result = Some(score);
            self.response_started = true;
        }
        publish(self.request.destination, self.result.unwrap())?;
        self.replay_stop = 0;
        self.response_started = false;
        Ok(false)
    }
    /// fd34 always blocks forward search,including a consumed saved stop.
    pub fn forward_complete(&self) -> bool {
        false
    }
    pub fn poll_live(
        &mut self,
        progress: &mut ProgressMemory,
        memory: &mut AutosaveMemory,
        boundary: u32,
        services: &mut impl Services,
    ) -> Result<bool> {
        if self.replay_stop >= 0 {
            return Ok(true);
        }
        if self.phase == Phase::New {
            let active = progress.progress().active_index();
            let entry = &mut progress.progress_mut().entries[active];
            entry.save_position = boundary;
            entry.replay_target = boundary;
            memory.valid = true;
            self.phase = Phase::Clock;
        }
        if self.phase == Phase::Clock {
            let time = services.packed_time()?;
            memory.capture(progress.progress(), time);
            self.phase = Phase::Queue;
        }
        services.queue_scene(self.request.args, progress)?;
        self.phase = Phase::New;
        Ok(false)
    }
}

#[cfg(test)]
mod tests;
