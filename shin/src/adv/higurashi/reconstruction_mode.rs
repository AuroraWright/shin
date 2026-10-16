//! Updated19578 phase owner and55380..55524 overlay clock. Snapshot drawing
//! and menu/indicator services remain explicit platform owners.
use anyhow::Result;

pub mod overlay;

pub use crate::adv::overlay_clock::{OverlayClock, Phase};

/// Callbacks correspond to the native publication prefix and platform owners.
/// Errors preserve the current phase for desktop retry; native exceptions are
/// outside this model. Source retirement must synchronously retire its workers.
pub trait Services {
    fn publish_resources(&mut self) -> Result<()>;
    fn reset_blackout_and_indicators(&mut self);
    fn clear_message(&mut self);
    fn retire_source_vm(&mut self);
    fn enable_animations(&mut self);
    fn install_replayed_vm(&mut self);
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
    /// Native only accepts worker status exactly1. Fade completion is polled
    /// in the publication call itself; reaching its endpoint during update
    /// installs the VM at the next poll, independently of visible source state.
    pub fn poll(
        &mut self,
        worker_status: u32,
        mask_present: bool,
        overlay: &mut OverlayClock,
        services: &mut impl Services,
    ) -> Result<Phase> {
        if self.phase == Phase::Preparing && worker_status == 1 {
            services.publish_resources()?;
            services.reset_blackout_and_indicators();
            if mask_present {
                services.clear_message();
                overlay.masked(30, f32::from_bits(0x3e99_999a), 0, true);
            } else {
                overlay.clear(4);
            }
            services.retire_source_vm();
            services.enable_animations();
            self.phase = Phase::Fading;
        }
        if self.phase == Phase::Fading && !overlay.busy() {
            services.install_replayed_vm();
            self.phase = Phase::Complete;
        }
        Ok(self.phase)
    }
}

#[cfg(test)]
mod tests;
