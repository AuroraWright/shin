//! Ger13764/14194/14210 counted modal, conceal and voice/effect scopes.
use super::cg_gallery::motion::Motion;
use anyhow::Result;
pub mod backdrop;

pub trait Services {
    fn hide_message(&mut self, hidden: bool);
    fn hide_overlay(&mut self, hidden: bool);
    fn hide_indicator(&mut self, hidden: bool);
    fn hide_menu(&mut self, hidden: bool);
    fn pause_voice(&mut self, paused: bool);
    fn pause_effects(&mut self, paused: bool);
}

pub struct Modal {
    pub pause_depth: u32,
    pub conceal_depth: u32,
    pub animations_enabled: bool,
    pub fade: Motion,
}
impl Default for Modal {
    fn default() -> Self {
        Self {
            pause_depth: 0,
            conceal_depth: 0,
            animations_enabled: true,
            fade: Motion::new(0.0),
        }
    }
}
impl Modal {
    pub fn enter(
        &mut self,
        capture: impl FnOnce() -> Result<()>,
        services: &mut impl Services,
    ) -> Result<()> {
        // Original capture13800 precedes *all* modal mutations. Desktop
        // preparation errors must leave the still-owned scene usable.
        capture()?;
        self.fade.target(1.0, 0.1, 0, true);
        self.conceal(true, services);
        if self.pause_depth == 0 {
            services.pause_voice(true);
            services.pause_effects(true);
        }
        self.pause_depth = self.pause_depth.wrapping_add(1);
        self.animations_enabled = false;
        Ok(())
    }
    pub fn exit(&mut self, services: &mut impl Services) {
        self.animations_enabled = true;
        self.pause_depth = self.pause_depth.wrapping_sub(1);
        if self.pause_depth == 0 {
            services.pause_voice(false);
            services.pause_effects(false);
        }
        self.conceal(false, services);
        self.fade.target(0.0, 0.1, 0, true);
    }
    pub fn conceal(&mut self, hide: bool, services: &mut impl Services) {
        if hide {
            if self.conceal_depth == 0 {
                services.hide_message(true);
                services.hide_overlay(true);
                services.hide_indicator(true);
                services.hide_menu(true);
            }
            self.conceal_depth = self.conceal_depth.wrapping_add(1);
        } else {
            self.conceal_depth = self.conceal_depth.wrapping_sub(1);
            if self.conceal_depth == 0 {
                services.hide_menu(false);
                services.hide_indicator(false);
                services.hide_overlay(false);
                services.hide_message(false);
            }
        }
    }
    /// Advanced by the parent independently of the paused VM/layer clocks.
    pub fn advance(&mut self, delta: f32) -> bool {
        self.fade.update(delta);
        self.fade.value() == 0.0 && !self.fade.busy()
    }
}

#[cfg(test)]
mod render_tests;
#[cfg(test)]
mod tests;
