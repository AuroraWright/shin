//! Updated1a60c/1a7f4 HiddenWindow and15958 counted ADV concealment.
//! Reuses the later engine's contact owner with Higurashi's logical buttons.
use crate::adv::input_area::{InputArea, InputAreaTarget, PointerFrame};
use glam::Vec2;

pub trait ConcealServices {
    fn hide_message(&mut self, hidden: bool);
    fn hide_overlay(&mut self, hidden: bool);
    fn hide_indicator(&mut self, hidden: bool);
    fn hide_menu(&mut self, hidden: bool);
}

#[derive(Default)]
pub struct ConcealState {
    pub depth: u32,
}
impl ConcealState {
    ///15958 uses bit0 and wrapping32-bit counter arithmetic. Only zero/nonzero
    /// edges change the owners, in different orders on acquire and release.
    pub fn control(&mut self, raw: u32, services: &mut impl ConcealServices) {
        if raw & 1 != 0 {
            if self.depth == 0 {
                services.hide_message(true);
                services.hide_overlay(true);
                services.hide_indicator(true);
                services.hide_menu(true);
            }
            self.depth = self.depth.wrapping_add(1);
        } else {
            self.depth = self.depth.wrapping_sub(1);
            if self.depth == 0 {
                services.hide_menu(false);
                services.hide_indicator(false);
                services.hide_overlay(false);
                services.hide_message(false);
            }
        }
    }
}

pub struct HiddenWindow {
    pub completed: u8,
    area: InputArea,
}
impl Default for HiddenWindow {
    fn default() -> Self {
        Self {
            completed: 0,
            // Native cc9c0/cc9d0 give the touch coordinate extent1280x720,
            // independent of authored1920x1080 and desktop output resolution.
            area: InputArea::new(1280, 720),
        }
    }
}
struct Contact<'a>(&'a mut u8);
impl InputAreaTarget for Contact<'_> {
    fn probe(&mut self, _: Vec2) -> bool {
        true
    }
    fn reset(&mut self) {}
    fn press(&mut self, _: Vec2) {}
    fn release(&mut self, _: Vec2) {
        *self.0 = 1;
        tracing::debug!("Higurashi hidden textbox restored by tap");
    }
    fn long_press(&mut self, _: Vec2) {}
    fn drag(&mut self, _: Vec2, _: Vec2) {}
    fn gesture(&mut self, _: Vec2) {}
    fn cancel(&mut self) {}
}
impl HiddenWindow {
    pub fn completion_word(&self) -> u32 {
        u32::from(self.completed)
    }
    pub fn cancel_input(&mut self) {
        self.area.disable(&mut Contact(&mut self.completed));
    }
    /// Permission/completion gates flush pointer capture before any logical
    /// snapshot is examined. A consumed contact frame suppresses buttons.
    pub fn poll(&mut self, permission: u32, pressed: u32, frame: &PointerFrame<'_>) -> u32 {
        let mut contact = Contact(&mut self.completed);
        if permission & 1 == 0 || *contact.0 != 0 {
            self.area.disable(&mut contact);
        } else if !self.area.update(&mut contact, frame) && pressed & 0x0300_9000 != 0 {
            *contact.0 = 1;
        }
        0
    }
}

#[cfg(test)]
mod tests;
