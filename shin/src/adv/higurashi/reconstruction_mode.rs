//! Updated19578 phase owner and55380..55524 overlay clock. Snapshot drawing
//! and menu/indicator services remain explicit platform owners.
use anyhow::Result;
use shin_core::time::Ticks;

pub mod overlay;

#[derive(Clone, Debug)]
pub struct OverlayClock {
    pub active: bool,
    pub elapsed: f32,
    pub duration: f32,
    pub source_present: bool,
    pub mask_present: bool,
    pub softness: f32,
    pub flags: u32,
}
impl OverlayClock {
    pub fn captured(source_present: bool) -> Self {
        Self {
            active: false,
            elapsed: 0.0,
            duration: 1.0,
            source_present,
            mask_present: false,
            softness: 0.0,
            flags: 0,
        }
    }
    pub fn busy(&self) -> bool {
        if self.active {
            self.elapsed < self.duration
        } else {
            self.source_present
        }
    }
    pub fn visible(&self) -> bool {
        self.source_present && self.elapsed < self.duration
    }
    pub fn advance(&mut self, delta: Ticks) {
        if self.active {
            self.elapsed += delta.as_f32();
            if self.elapsed > self.duration {
                self.elapsed = self.duration;
            }
        }
    }
    fn start(&mut self, duration: u32) {
        self.active = true;
        self.elapsed = if duration == 0 { 1.0 } else { 0.0 };
        self.duration = if duration == 0 { 1.0 } else { duration as f32 };
    }
    pub fn clear(&mut self, duration: u32) {
        self.start(duration);
        self.mask_present = false;
    }
    pub fn masked(&mut self, duration: u32, softness: f32, flags: u32, present: bool) {
        self.start(duration);
        self.softness = if softness == 0.0 {
            1.0 / 256.0
        } else {
            softness.abs()
        };
        self.flags = flags;
        self.mask_present = present;
    }
    /// Native555f4..55668, before shader saturation. Keep the original operation
    /// order; the two endpoint weights may be outside0..1.
    pub fn mask_weights(&self) -> [f32; 2] {
        let inverse = 1.0 / self.softness;
        let span = inverse + 1.0;
        let progress = self.elapsed / self.duration;
        if self.flags & 1 == 0 {
            let first = 1.0 - span * progress;
            [first, inverse + first]
        } else {
            let first = span * (1.0 - progress);
            [first, first - inverse]
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Preparing,
    Fading,
    Complete,
}

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
