//! Snapshot-overlay fade clock shared by the Higurashi and Gerokasu2
//! reconstruction/menu owners (Higurashi 55380..55524, Gerokasu2 59460..596AC).
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
    /// Higurashi 555f4..55668, before shader saturation. Keep the original
    /// operation order; the two endpoint weights may be outside0..1. Gerokasu2
    /// rounds the first endpoint once (`overlay::fused_mask_weights`).
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
