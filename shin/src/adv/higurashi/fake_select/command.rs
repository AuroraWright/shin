//! Pure3e9c constructor; ff5c attaches once, copies11780 once and waits on VOID.
use crate::adv::higurashi::{
    autosave::{AutosaveMemory, AutosaveServices},
    progress::ProgressMemory,
};
use anyhow::Result;

pub(crate) trait Services: AutosaveServices {
    fn open(&mut self) -> Result<()>;
    fn callback_received(&self) -> bool;
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    New,
    Copy,
    Waiting,
    Complete,
}
#[derive(Debug)]
pub(crate) struct Command {
    phase: Phase,
}
impl Command {
    pub fn new() -> Self {
        Self { phase: Phase::New }
    }
    pub fn poll_live(
        &mut self,
        progress: &mut ProgressMemory,
        memory: &mut AutosaveMemory,
        boundary: u32,
        services: &mut impl Services,
    ) -> Result<bool> {
        if self.phase == Phase::New {
            services.open()?;
            self.phase = Phase::Copy;
        }
        if self.phase == Phase::Copy {
            memory.capture_message_start(progress, boundary, services)?;
            self.phase = Phase::Waiting;
        }
        if self.phase == Phase::Waiting && services.callback_received() {
            self.phase = Phase::Complete;
        }
        Ok(self.phase == Phase::Complete)
    }
}
