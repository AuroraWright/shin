//! Native 19CE0 publication/fade owner and 59460..596AC snapshot clock.
use anyhow::Result;
use shin_core::time::Ticks;

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
    /// Native597A4 uses FMSUB: preserve its one-rounding endpoint arithmetic.
    pub fn mask_weights(&self) -> [f32; 2] {
        let inverse = 1.0 / self.softness;
        let span = inverse + 1.0;
        let progress = self.elapsed / self.duration;
        if self.flags & 1 == 0 {
            let first = (-span).mul_add(progress, 1.0);
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
