//! Native 19CE0 publication/fade owner and 59460..596AC snapshot clock.
use anyhow::Result;

pub use crate::adv::overlay_clock::{OverlayClock, Phase};

pub trait Services {
    fn publish_resources(&mut self) -> Result<()>;
    fn reset_blackout_and_indicators(&mut self);
    fn clear_message(&mut self);
    /// The new VM is visible to ADV before the old VM's destructor runs.
    fn install_and_retire_source_vm(&mut self);
}
pub struct ReconstructionMode {
    pub mode: u32,
    phase: Phase,
}
impl ReconstructionMode {
    pub fn new(mode: u32) -> Self {
        Self {
            mode,
            phase: Phase::Preparing,
        }
    }
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn poll(
        &mut self,
        worker_status: u32,
        overlay: &mut OverlayClock,
        services: &mut impl Services,
    ) -> Result<Phase> {
        if self.phase == Phase::Preparing && worker_status == 1 {
            services.publish_resources()?;
            // Mode3 branches at1AD20, before the blackout/indicator reset.
            if self.mode != 3 {
                services.reset_blackout_and_indicators();
            }
            match self.mode {
                0 => {
                    services.clear_message();
                    overlay.masked(30, f32::from_bits(0x3e99_999a), 0, overlay.mask_present);
                }
                1 => overlay.clear(4),
                2 | 3 => overlay.clear(0),
                _ => {} // Native unsigned >3 leaves the existing fade intact.
            }
            services.install_and_retire_source_vm();
            self.phase = Phase::Fading;
        }
        if self.phase == Phase::Fading && !overlay.busy() {
            self.phase = Phase::Complete;
        }
        Ok(self.phase)
    }
}

/// Original RESUME77A8 queues once and always blocks its old continuation.
#[derive(Default)]
pub struct ResumeCommand {
    entered: bool,
}
impl ResumeCommand {
    pub fn poll(&mut self, request: impl FnOnce() -> Result<()>) -> Result<bool> {
        if !self.entered {
            request()?;
            self.entered = true;
        }
        Ok(false)
    }
}

pub mod overlay;
#[cfg(test)]
mod tests;
