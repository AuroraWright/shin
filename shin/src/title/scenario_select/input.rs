//! Native Start df480 routes the shared InputArea before controller actions.
use super::selection::ScenarioSelection;
use crate::{
    adv::input_area::{InputArea, InputAreaTarget, ModalInputTarget, PointerFrame},
    app::AppAction,
};
use enum_map::EnumMap;
use glam::Vec2;
use shin_core::time::Ticks;
use shin_input::ActionState;

impl InputAreaTarget for ScenarioSelection {
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
    fn long_press(&mut self, _: Vec2) {}
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        adv::input_area::{PointerEvent, PointerKind},
        title::{scenario_select::ScenarioSelectSetup, scenario_select::selection::SelectionEvent},
    };
    use shin_core::format::save::SaveData;
    fn model(prefix: bool) -> ScenarioSelection {
        let mut save = SaveData::default();
        if prefix {
            for slot in 1..=24 {
                save.persist_data.set(slot, 3);
            }
        }
        ScenarioSelection::new_active(ScenarioSelectSetup::from_save(&save))
    }
    fn event(kind: PointerKind, x: i32, y: i32) -> PointerEvent {
        PointerEvent {
            kind,
            id: 0,
            position: glam::ivec2(x, y),
        }
    }
    fn frame(
        input: &mut ScenarioSelectInput,
        m: &mut ScenarioSelection,
        n: u32,
        time: f64,
        events: &[PointerEvent],
        state: &EnumMap<AppAction, ActionState>,
    ) {
        input.update_frame(
            m,
            Ticks::ZERO,
            &PointerFrame {
                number: n,
                time_seconds: time,
                events,
            },
            state,
        );
    }
    #[test]
    fn blank_capture_and_rejected_center_route_controller_differently() {
        let mut m = model(false);
        let mut input = ScenarioSelectInput::new();
        let mut state = EnumMap::default();
        state[AppAction::Enter] = ActionState {
            is_clicked: true,
            ..Default::default()
        };
        frame(
            &mut input,
            &mut m,
            0,
            0.0,
            &[event(PointerKind::Down, 10, 10)],
            &state,
        );
        assert!(m.accepts_input());
        assert!(m.take_events().is_empty());
        frame(
            &mut input,
            &mut m,
            1,
            0.05,
            &[event(PointerKind::Up, 10, 10)],
            &state,
        );
        assert!(m.accepts_input());
        frame(
            &mut input,
            &mut m,
            2,
            0.1,
            &[event(PointerKind::Down, 1000, 500)],
            &state,
        );
        assert!(!m.accepts_input());
        assert_eq!(m.take_events(), [SelectionEvent::ConfirmSound]);
    }
    #[test]
    fn short_entry_tap_selects_silently_and_rejects_exactly_zero() {
        let mut m = model(false);
        let mut input = ScenarioSelectInput::new();
        let state = EnumMap::default();
        frame(
            &mut input,
            &mut m,
            0,
            0.0,
            &[event(PointerKind::Down, 100, 370)],
            &state,
        );
        assert_eq!(m.selected(), (0, 0));
        frame(
            &mut input,
            &mut m,
            1,
            0.05,
            &[event(PointerKind::Up, 1900, 1000)],
            &state,
        );
        assert_eq!(m.selected(), (0, 1));
        assert!(m.accepts_input());
        assert_eq!(
            m.take_events(),
            [SelectionEvent::CaptureDetails, SelectionEvent::DeniedSound]
        );
    }
    #[test]
    fn delayed_entry_press_selects_and_drag_cancels_confirmation() {
        let mut m = model(false);
        let mut input = ScenarioSelectInput::new();
        let state = EnumMap::default();
        frame(
            &mut input,
            &mut m,
            0,
            0.0,
            &[event(PointerKind::Down, 100, 370)],
            &state,
        );
        frame(&mut input, &mut m, 1, 0.16, &[], &state);
        assert_eq!(m.selected(), (0, 1));
        m.advance_clocks(Ticks::from_f32(1.0));
        assert!((m.chapter_feedback()[1].flash - 0.2).abs() < 1e-6);
        frame(
            &mut input,
            &mut m,
            2,
            0.2,
            &[event(PointerKind::Move, 100, 330)],
            &state,
        );
        assert_eq!(m.scroll(), 40.0);
        frame(
            &mut input,
            &mut m,
            3,
            0.21,
            &[event(PointerKind::Up, 100, 330)],
            &state,
        );
        assert!(m.accepts_input());
        assert!(
            m.take_events()
                .iter()
                .all(|event| *event == SelectionEvent::CaptureDetails)
        );
        m.advance_clocks(Ticks::from_f32(2.0));
        assert_eq!(m.chapter_feedback()[1].flash, 0.0);
    }
    #[test]
    fn close_drag_and_resize_cancel_pressed_flash() {
        let mut m = model(false);
        let mut input = ScenarioSelectInput::new();
        let state = EnumMap::default();
        frame(
            &mut input,
            &mut m,
            0,
            0.0,
            &[event(PointerKind::Down, 1800, 60)],
            &state,
        );
        frame(&mut input, &mut m, 1, 0.16, &[], &state);
        m.advance_clocks(Ticks::from_f32(1.0));
        assert!((m.close_flash() - 0.2).abs() < 1e-6);
        input.resize(&mut m, (960, 540));
        m.advance_clocks(Ticks::from_f32(2.0));
        assert_eq!(m.close_flash(), 0.0);
        frame(
            &mut input,
            &mut m,
            2,
            0.2,
            &[event(PointerKind::Down, 900, 30)],
            &state,
        );
        frame(
            &mut input,
            &mut m,
            3,
            0.23,
            &[event(PointerKind::Move, 910, 30)],
            &state,
        );
        frame(
            &mut input,
            &mut m,
            4,
            0.25,
            &[event(PointerKind::Up, 910, 30)],
            &state,
        );
        assert!(m.accepts_input());
        assert!(m.take_events().is_empty());
    }
    #[test]
    fn tabs_use_static_bounds_select_on_release_and_keep_native_ordinal_errors_explicit() {
        let mut m = model(true);
        let mut input = ScenarioSelectInput::new();
        let state = EnumMap::default();
        assert!(!m.pointer_probe(Vec2::new(600.0, 180.0))); // selected tab
        assert!(!m.pointer_probe(Vec2::new(810.0, 211.5))); // half-open bottom
        frame(
            &mut input,
            &mut m,
            0,
            0.0,
            &[event(PointerKind::Down, 810, 180)],
            &state,
        );
        frame(&mut input, &mut m, 1, 0.16, &[], &state);
        assert_eq!(m.selected(), (0, 0));
        m.advance_clocks(Ticks::from_f32(1.0));
        assert!((m.tab_flash()[1] - 0.2).abs() < 1e-6);
        frame(
            &mut input,
            &mut m,
            2,
            0.2,
            &[event(PointerKind::Up, 810, 180)],
            &state,
        );
        assert_eq!(m.selected(), (1, 0));
        assert_eq!(
            m.take_events(),
            [SelectionEvent::ConfirmSound, SelectionEvent::CaptureDetails]
        );
        let mut save = SaveData::default();
        save.persist_data.set(25, 1);
        save.persist_data.set(48, 3);
        let mut sparse = ScenarioSelection::new_active(ScenarioSelectSetup::from_save(&save));
        assert!(sparse.pointer_probe(Vec2::new(900.0, 180.0)));
        sparse.pointer_release();
        assert_eq!(
            sparse.take_events(),
            [SelectionEvent::InvalidNativeTabIndex(3)]
        );
        assert_eq!(sparse.selected(), (0, 0));
    }
    #[test]
    fn list_gap_and_inertia_stop_do_not_confirm_a_chapter() {
        let mut m = model(false);
        assert!(m.pointer_probe(Vec2::new(100.0, 357.0)));
        m.pointer_release();
        assert!(m.take_events().is_empty());
        m.pointer_gesture(Vec2::new(0.0, -20.0));
        m.pointer_reset();
        m.advance_clocks(Ticks::from_f32(2.0));
        assert_eq!(m.scroll(), 40.0);
        assert_eq!(m.selected(), (0, 1));
        m.take_events();
        assert!(m.pointer_probe(Vec2::new(100.0, 370.0)));
        m.pointer_release();
        assert!(m.take_events().is_empty());
        m.advance_clocks(Ticks::from_f32(2.0));
        assert_eq!(m.scroll(), 40.0);
        // A new tap can confirm after the first tap stopped inertia.
        m.pointer_reset();
        assert!(m.pointer_probe(Vec2::new(100.0, 370.0)));
        m.pointer_release();
        assert_eq!(m.take_events(), [SelectionEvent::DeniedSound]);
    }
    #[test]
    fn inertia_continues_during_captured_flash_wait_and_launch_reads_live_selection() {
        let mut m = model(false);
        assert!(m.pointer_probe(Vec2::new(100.0, 357.0)));
        m.pointer_gesture(Vec2::new(0.0, -20.0));
        m.pointer_reset();
        m.confirm();
        m.take_events();
        m.advance_clocks(Ticks::from_f32(37.0));
        assert_eq!(m.selected(), (0, 2));
        assert_eq!(m.take_events(), [SelectionEvent::CaptureDetails]);
        // e2a40 checks the captured first chapter's state/flash. df930 then
        // reads the current chapter, which inertia may have changed.
        m.prepare_frame();
        assert_eq!(m.take_events(), [SelectionEvent::LaunchScene(2)]);
    }
}
impl ModalInputTarget for ScenarioSelection {
    fn accepts_pointer_input(&self) -> bool {
        self.accepts_input()
    }
    fn controller_input(&mut self, state: &EnumMap<AppAction, ActionState>) {
        ScenarioSelection::controller_input(self, state);
    }
}
pub(crate) struct ScenarioSelectInput {
    area: InputArea,
}
impl ScenarioSelectInput {
    pub fn new() -> Self {
        Self {
            area: InputArea::canvas(),
        }
    }
    pub fn resize(&mut self, model: &mut ScenarioSelection, extent: (u16, u16)) {
        self.area.disable(model);
        self.area = InputArea::new(extent.0, extent.1);
    }
    pub fn update_frame(
        &mut self,
        model: &mut ScenarioSelection,
        delta: Ticks,
        frame: &PointerFrame<'_>,
        state: &EnumMap<AppAction, ActionState>,
    ) {
        model.prepare_frame();
        self.area.input_frame(model, frame, state);
        model.advance_clocks(delta);
    }
}
