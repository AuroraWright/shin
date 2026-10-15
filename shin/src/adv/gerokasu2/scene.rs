//! Retained WIPE phases and explicit Screen/Page services.
use super::layer_load::{LoadServices, LoadTransition};
use anyhow::Result;

pub trait SceneServices: LoadServices {
    fn observe_screen(&mut self) -> Result<()>;
    fn capture_previous_scene(&mut self) -> Result<()>;
    fn pageback(&mut self, start_anew: bool) -> Result<()>;
    fn promote_pending_page(&mut self, with_transition: bool) -> Result<()>;
    fn scene_wiping(&mut self) -> Result<bool>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WipeStage {
    Start,
    Promotion,
    Wipes,
    Complete,
}
pub struct WipeController {
    settings: LoadTransition,
    stage: WipeStage,
    started: bool,
    wait_attempted: bool,
    published: bool,
}
impl WipeController {
    pub fn new(had_pageback: bool, settings: LoadTransition) -> Self {
        Self {
            settings,
            stage: if had_pageback {
                WipeStage::Start
            } else {
                WipeStage::Complete
            },
            started: false,
            wait_attempted: false,
            published: false,
        }
    }
    pub fn stage(&self) -> WipeStage {
        self.stage
    }
    pub fn poll(&mut self, services: &mut dyn SceneServices) -> Result<bool> {
        // Native62d0 obtains the Screen before dispatching even a finished phase.
        services.observe_screen()?;
        if self.stage == WipeStage::Complete {
            return Ok(true);
        }
        if self.stage == WipeStage::Start {
            if self.settings.duration >= 1 {
                if !self.started {
                    services.start_transition(self.settings)?;
                    self.started = true;
                    if self.settings.flags & 1 == 0 {
                        services.set_animations_allowed(false);
                    }
                }
                if !self.wait_attempted {
                    self.wait_attempted = true;
                    if services.transition_present() && services.frame_elapsed_us() >> 4 <= 624 {
                        services.wait_transition(5000)?;
                    }
                }
            }
            self.stage = WipeStage::Promotion;
        }
        if self.stage == WipeStage::Promotion {
            if !self.published {
                if services.skip_requested() {
                    services.clear_transition();
                }
                if services.transition_present() && !services.transition_ready()? {
                    return Ok(false);
                }
                services.capture_previous_scene()?;
                services.promote_pending_page(services.transition_present())?;
                self.published = true;
            }
            services.set_animations_allowed(true);
            self.stage = WipeStage::Wipes;
        }
        // Unlike LAYERLOAD this reads the retained command's flags.
        if self.settings.flags & 2 == 0 && services.scene_wiping()? {
            return Ok(false);
        }
        services.command_finished()?;
        self.stage = WipeStage::Complete;
        Ok(true)
    }
}

#[cfg(test)]
mod tests;
