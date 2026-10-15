//! Updated KAKERA3b2c/ef40/ef9c:copy progress,then defer a separate menu scene.
//! This command has no UI layer,reservation,choice result or response bits.
use super::{
    autosave::{AutosaveMemory, SaveProgress},
    progress::ProgressMemory,
};
use anyhow::{Result, ensure};

pub trait KakeraServices {
    fn packed_time(&mut self) -> Result<u64>;
    ///656c0 copies the complete source GameData before275b0 queues its factory.
    /// Mode is0 for the command. The service owns date/global scene bindings.
    fn queue_scene(&mut self, mode: i32, source: &ProgressMemory) -> Result<()>;
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    New,
    Clock,
    Queue,
}
#[derive(Debug)]
pub struct KakeraCommand {
    replay_stop: i32,
    phase: Phase,
}
impl KakeraCommand {
    pub fn new() -> Self {
        Self {
            replay_stop: -1,
            phase: Phase::New,
        }
    }
    pub fn replay_stop(&self) -> i32 {
        self.replay_stop
    }
    /// ef40 does not consume any response. ef94 always blocks forward search.
    pub fn poll_replay(&mut self, progress: &SaveProgress, boundary: u32) -> Result<bool> {
        ensure!(
            self.phase == Phase::New,
            "KAKERA live progress copy is still pending"
        );
        if boundary >= progress.active_entry().save_position {
            self.replay_stop = 0;
            Ok(false)
        } else {
            Ok(true)
        }
    }
    /// ef9c's saved stop completes without a copy/factory. Fresh live execution
    /// copies11780 and queues mode0,returningfalse until source scene retirement.
    /// Successful calls can replace a previous deferred request,matching275b0.
    pub fn poll_live(
        &mut self,
        progress: &mut ProgressMemory,
        memory: &mut AutosaveMemory,
        boundary: u32,
        services: &mut impl KakeraServices,
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
        services.queue_scene(0, progress)?;
        self.phase = Phase::New;
        Ok(false)
    }
}

#[cfg(test)]
pub(super) mod tests;
