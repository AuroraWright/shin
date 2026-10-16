//! Native 49230 dispatches shared contact capture before gated controller input.
use super::{CgControls, CgGalleryList};
use crate::{
    adv::input_area::{InputArea, InputAreaTarget, PointerFrame},
    app::AppAction,
};
use enum_map::EnumMap;
use glam::Vec2;
use shin_input::ActionState;

impl InputAreaTarget for CgGalleryList {
    fn probe(&mut self, point: Vec2) -> bool {
        self.pointer_probe(point)
    }
    fn reset(&mut self) {
        self.pointer_reset();
    }
    fn press(&mut self, _: Vec2) {
        self.pointer_press();
    }
    fn release(&mut self, _: Vec2) {
        self.pointer_release();
    }
    fn long_press(&mut self, _: Vec2) {} // 4a070 is a no-op.
    fn drag(&mut self, previous: Vec2, current: Vec2) {
        self.pointer_drag(current.y - previous.y);
    }
    fn gesture(&mut self, velocity: Vec2) {
        self.pointer_gesture(velocity);
    }
    fn cancel(&mut self) {
        self.pointer_cancel();
    }
}
pub(crate) struct CgGalleryInput {
    area: InputArea,
}
impl CgGalleryInput {
    pub fn new() -> Self {
        Self {
            area: InputArea::canvas(),
        }
    }
    pub fn resize(&mut self, gallery: &mut CgGalleryList, extent: (u16, u16)) {
        self.area.disable(gallery);
        self.area = InputArea::new(extent.0, extent.1);
    }
    pub fn input_frame(
        &mut self,
        gallery: &mut CgGalleryList,
        enabled: bool,
        frame: &PointerFrame<'_>,
        state: &EnumMap<AppAction, ActionState>,
        axis: f32,
    ) {
        gallery.prepare_frame();
        if !enabled || gallery.closing() {
            self.area.disable(gallery);
            return;
        }
        if self.area.update(gallery, frame) {
            return;
        }
        gallery.controller(
            CgControls {
                confirm: state[AppAction::Enter].is_clicked,
                cancel: state[AppAction::Cancel].is_clicked,
                left: state[AppAction::Left].is_clicked_or_repeated,
                right: state[AppAction::Right].is_clicked_or_repeated,
                up: state[AppAction::Up].is_clicked_or_repeated,
                down: state[AppAction::AnyDown].is_clicked_or_repeated,
                page_up: state[AppAction::CharacterPageUp].is_clicked_or_repeated,
                page_down: state[AppAction::CharacterPageDown].is_clicked_or_repeated,
            },
            axis,
        );
    }
}

#[cfg(test)]
mod tests;
