//! Shared contact capture wins before native Bustup controller39e80.
use crate::title::{
    bustup::catalog::BustupCatalog,
    bustup::menu::{BustupMenu, Controls},
};
use crate::{
    adv::input_area::{InputArea, InputAreaTarget, PointerFrame},
    app::AppAction,
};
use enum_map::EnumMap;
use glam::Vec2;
use shin_input::ActionState;

struct Target<'a> {
    menu: &'a mut BustupMenu,
    catalog: &'a BustupCatalog,
}
impl InputAreaTarget for Target<'_> {
    fn probe(&mut self, point: Vec2) -> bool {
        self.menu.pointer_probe(self.catalog, point)
    }
    fn reset(&mut self) {
        self.menu.pointer_reset();
    }
    fn press(&mut self, _: Vec2) {
        self.menu.pointer_press();
    }
    fn release(&mut self, _: Vec2) {
        self.menu.pointer_release(self.catalog);
    }
    fn long_press(&mut self, _: Vec2) {}
    fn drag(&mut self, _: Vec2, _: Vec2) {
        self.menu.pointer_cancel();
    }
    fn gesture(&mut self, _: Vec2) {}
    fn cancel(&mut self) {
        self.menu.pointer_cancel();
    }
}
pub(crate) struct BustupInput {
    area: InputArea,
}
impl BustupInput {
    pub fn new() -> Self {
        Self {
            area: InputArea::canvas(),
        }
    }
    pub fn resize(&mut self, menu: &mut BustupMenu, catalog: &BustupCatalog, extent: (u16, u16)) {
        self.area.disable(&mut Target { menu, catalog });
        self.area = InputArea::new(extent.0, extent.1);
    }
    /// Caller polls transition tasks first and passes eligibility after newest
    /// blocking tasks, including resource loads and their completion frame.
    pub fn input_frame(
        &mut self,
        menu: &mut BustupMenu,
        catalog: &BustupCatalog,
        eligible: &mut bool,
        frame: &PointerFrame<'_>,
        state: &EnumMap<AppAction, ActionState>,
    ) -> bool {
        let mut target = Target { menu, catalog };
        let consumed = if *eligible && target.menu.input_alive() {
            self.area.update(&mut target, frame)
        } else {
            self.area.disable(&mut target);
            false
        };
        target.menu.controller(
            catalog,
            eligible,
            consumed,
            Controls {
                cancel: state[AppAction::Cancel].is_clicked,
                confirm: state[AppAction::Enter].is_clicked,
                up: state[AppAction::Up].is_clicked_or_repeated,
                down: state[AppAction::AnyDown].is_clicked_or_repeated,
                left: state[AppAction::Left].is_clicked_or_repeated,
                right: state[AppAction::Right].is_clicked_or_repeated,
                page_up: state[AppAction::CharacterPageUp].is_clicked_or_repeated,
                page_down: state[AppAction::CharacterPageDown].is_clicked_or_repeated,
            },
        )
    }
}
#[cfg(test)]
mod tests;
