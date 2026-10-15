//! Updated15038/158e0 counted modal scope and independently advanced ADV140.
use crate::layer::higurashi_properties::HigurashiTweener;
use anyhow::Result;

pub mod capture;
pub mod backdrop;
use shin_core::time::Ticks;

pub trait ModalServices {
    fn conceal(&mut self, raw: u32);
    fn pause_voice(&mut self, paused: bool);
    fn pause_effects(&mut self, paused: bool);
}

pub struct ModalState {
    pub pause_depth: u32,
    pub animations_enabled: bool,
    pub fade: HigurashiTweener,
}
impl Default for ModalState {
    fn default() -> Self {
        Self {
            pause_depth: 0,
            animations_enabled: true,
            fade: HigurashiTweener::new(0.0),
        }
    }
}
impl ModalState {
    pub fn enter(
        &mut self,
        capture: impl FnOnce() -> Result<()>,
        services: &mut impl ModalServices,
    ) -> Result<()> {
        // Capture precedes every owner mutation. A failed desktop preparation
        // leaves the scope unchanged and available for an explicit retry.
        capture()?;
        self.target(1.0);
        services.conceal(1);
        if self.pause_depth == 0 {
            services.pause_voice(true);
            services.pause_effects(true);
        }
        self.pause_depth = self.pause_depth.wrapping_add(1);
        self.animations_enabled = false;
        Ok(())
    }
    pub fn exit(&mut self, services: &mut impl ModalServices) {
        // Native enables animation even while another pause owner remains.
        self.animations_enabled = true;
        self.pause_depth = self.pause_depth.wrapping_sub(1);
        if self.pause_depth == 0 {
            services.pause_voice(false);
            services.pause_effects(false);
        }
        services.conceal(0);
        self.target(0.0);
    }
    fn target(&mut self, value: f32) {
        self.fade.fast_forward_to(self.fade.value());
        self.fade
            .control(value, Ticks::from_f32(0.1), 0x40)
            .expect("Native modal fade uses supported tween flags");
    }
    ///12e14 runs outside VM/animation gates, before audio/UI. The retained
    /// texture is released only when the fade is zero and its queue is idle.
    pub fn advance(&mut self, delta: Ticks) -> bool {
        self.fade.update(delta);
        self.fade.value() == 0.0 && !self.fade.is_waiting()
    }
}

#[cfg(test)]
mod tests;
