//! Native 4a9e0 consumes captured contacts before button edges and held D-pad pan.
use super::{
    cg_gallery::CgRow,
    cg_viewer::{CgViewerNavigation, ViewerControls},
};
use crate::{
    adv::input_area::{InputArea, InputAreaTarget, PointerFrame},
    app::AppAction,
};
use enum_map::EnumMap;
use glam::Vec2;
use shin_input::{ActionState, RawInputState, inputs::GamepadAxis};

struct ViewerTarget<'a> {
    model: &'a mut CgViewerNavigation,
    row: &'a CgRow,
}
impl InputAreaTarget for ViewerTarget<'_> {
    fn probe(&mut self, _: Vec2) -> bool {
        self.model.pointer_probe()
    }
    fn reset(&mut self) {} // 4bbb0.
    fn press(&mut self, _: Vec2) {} // 4bbd0.
    fn release(&mut self, _: Vec2) {
        self.model.pointer_release(self.row);
    }
    fn long_press(&mut self, _: Vec2) {} // 4bc90.
    fn drag(&mut self, previous: Vec2, current: Vec2) {
        self.model.pointer_drag(current - previous);
    }
    fn gesture(&mut self, velocity: Vec2) {
        self.model.pointer_gesture(velocity);
    }
    fn cancel(&mut self) {} // 4bdb0.
}
pub(super) struct CgViewerInput {
    area: InputArea,
}
impl CgViewerInput {
    pub fn axes(raw: &RawInputState) -> Vec2 {
        Vec2::new(
            raw.gamepads.unified.axes[GamepadAxis::LeftStickX],
            -raw.gamepads.unified.axes[GamepadAxis::LeftStickY],
        )
    }
    pub fn new() -> Self {
        Self {
            area: InputArea::canvas(),
        }
    }
    pub fn resize(&mut self, model: &mut CgViewerNavigation, row: &CgRow, extent: (u16, u16)) {
        self.area.disable(&mut ViewerTarget { model, row });
        self.area = InputArea::new(extent.0, extent.1);
    }
    /// Axes are the native left-stick coordinates: X right-positive, Y down-positive.
    pub fn input_frame(
        &mut self,
        model: &mut CgViewerNavigation,
        row: &CgRow,
        enabled: bool,
        frame: &PointerFrame<'_>,
        state: &EnumMap<AppAction, ActionState>,
        axes: Vec2,
    ) {
        if !enabled || model.closing() {
            self.area.disable(&mut ViewerTarget { model, row });
            return;
        }
        if self.area.update(&mut ViewerTarget { model, row }, frame) {
            return;
        }
        let mut pan = Vec2::new(-axes.x * 32.0, axes.y * 32.0);
        if state[AppAction::ViewerPanUp].is_held {
            pan.y = -16.0;
        } else if state[AppAction::ViewerPanDown].is_held {
            pan.y = 16.0;
        }
        if state[AppAction::ViewerPanLeft].is_held {
            pan.x = 16.0;
        } else if state[AppAction::ViewerPanRight].is_held {
            pan.x = -16.0;
        }
        model.controller(
            row,
            ViewerControls {
                cancel: state[AppAction::Cancel].is_clicked,
                confirm: state[AppAction::Enter].is_clicked,
                fit_toggle: state[AppAction::CharacterVariant].is_clicked,
                indicator_toggle: state[AppAction::CharacterState].is_clicked,
                pan,
            },
        );
    }
}

#[cfg(test)]
mod tests;
