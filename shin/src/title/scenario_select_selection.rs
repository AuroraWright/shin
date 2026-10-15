//! Ready-state selection and task gates in native ScenarioSelect.
//! The scene owner handles opening/closing, resource captures and destinations.
use super::{scenario_select::ScenarioSelectSetup, scenario_select_view::ChapterFeedback};
use crate::app::AppAction;
use enum_map::EnumMap;
use glam::Vec2;
use shin_core::time::{Ticks, Tween, Tweener};
use shin_input::ActionState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SelectionEvent {
    CaptureDetails,
    MoveSound,
    ConfirmSound,
    DeniedSound,
    BackSound,
    BeginBack,
    ConfirmDialog,
    UnlockDialog,
    UnlockScenarios,
    LaunchScene(u8),
    /// Native tab probe returns metadata IDs as vector ordinals. Report an
    /// invalid ordinal explicitly instead of emulating its out-of-bounds read.
    InvalidNativeTabIndex(usize),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PointerTarget {
    None,
    Blank,
    Close,
    Tab(usize),
    Pane(Option<usize>),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Gate {
    Active,
    Flash { group: usize, entry: usize },
    Dialog,
    UnlockDialog,
    Leaving,
}
struct EntryClocks {
    focus: Tweener,
    flash: Tweener,
}
struct GroupClocks {
    focus: Tweener,
    flash: Tweener,
    entries: Vec<EntryClocks>,
    remembered_entry: usize,
    remembered_scroll: i32,
}
pub(super) struct ScenarioSelection {
    setup: ScenarioSelectSetup,
    group: usize,
    entry: usize,
    groups: Vec<GroupClocks>,
    scroll: Tweener,
    details_fade: Tweener,
    gate: Gate,
    events: Vec<SelectionEvent>,
    pointer: PointerTarget,
    close_flash: Tweener,
    velocity: f32,
    unlock_available: bool,
}
fn rate(clock: &mut Tweener, value: f32, speed: f32) {
    clock.enqueue_now(
        value,
        Tween::linear(Ticks::from_f32((value - clock.value()).abs() / speed)),
    );
}
impl ScenarioSelection {
    /// Called when the native opening task hands control to df330.
    pub fn new_active(setup: ScenarioSelectSetup) -> Self {
        assert!(!setup.groups.is_empty());
        let groups = setup
            .groups
            .iter()
            .enumerate()
            .map(|(group, data)| GroupClocks {
                focus: Tweener::new(if group == 0 { 1.0 } else { 0.0 }),
                flash: Tweener::new(0.0),
                entries: data
                    .entries
                    .iter()
                    .enumerate()
                    .map(|(entry, _)| EntryClocks {
                        focus: Tweener::new(if entry == 0 { 1.0 } else { 0.0 }),
                        flash: Tweener::new(0.0),
                    })
                    .collect(),
                remembered_entry: 0,
                remembered_scroll: 0,
            })
            .collect();
        Self {
            setup,
            group: 0,
            entry: 0,
            groups,
            scroll: Tweener::new(0.0),
            details_fade: Tweener::new(0.0),
            gate: Gate::Active,
            events: Vec::new(),
            pointer: PointerTarget::None,
            close_flash: Tweener::new(0.0),
            velocity: 0.0,
            unlock_available: false,
        }
    }
    pub fn selected(&self) -> (usize, usize) {
        (self.group, self.entry)
    }
    pub fn accepts_input(&self) -> bool {
        self.gate == Gate::Active
    }
    pub fn set_unlock_available(&mut self, available: bool) {
        self.unlock_available = available;
    }
    pub fn setup(&self) -> &ScenarioSelectSetup {
        &self.setup
    }
    pub fn scroll(&self) -> f32 {
        self.scroll.value()
    }
    pub fn details_fade(&self) -> f32 {
        self.details_fade.value()
    }
    pub fn chapter_feedback(&self) -> Vec<ChapterFeedback> {
        self.groups[self.group]
            .entries
            .iter()
            .map(|entry| ChapterFeedback {
                focus: entry.focus.value(),
                flash: entry.flash.value(),
            })
            .collect()
    }
    pub fn tab_focus(&self) -> Vec<f32> {
        self.groups
            .iter()
            .map(|group| group.focus.value())
            .collect()
    }
    pub fn take_events(&mut self) -> Vec<SelectionEvent> {
        std::mem::take(&mut self.events)
    }
    pub fn close_flash_idle(&self) -> bool {
        self.close_flash.is_idle()
    }
    pub fn close_flash(&self) -> f32 {
        self.close_flash.value()
    }
    pub fn tab_flash(&self) -> Vec<f32> {
        self.groups
            .iter()
            .map(|group| group.flash.value())
            .collect()
    }
    fn capture_details(&mut self) {
        self.events.push(SelectionEvent::CaptureDetails);
        self.details_fade = Tweener::new(1.0);
        self.details_fade
            .enqueue(0.0, Tween::linear(Ticks::from_f32(10.0)));
    }
    pub fn select_group(&mut self, group: usize) -> bool {
        if !self.accepts_input() || group == self.group {
            return false;
        }
        assert!(group < self.groups.len());
        let old = &mut self.groups[self.group];
        old.remembered_entry = self.entry;
        // Native fcvtzs stores the evaluated value, dropping fractional scroll.
        old.remembered_scroll = self.scroll.value() as i32;
        rate(&mut old.focus, 0.0, 0.25);
        rate(&mut self.groups[group].focus, 1.0, 0.25);
        self.group = group;
        self.velocity = 0.0;
        self.entry = self.groups[group].remembered_entry;
        self.scroll = Tweener::new(self.groups[group].remembered_scroll as f32);
        self.capture_details();
        true
    }
    pub fn select_entry(&mut self, entry: usize, scroll_into_view: bool) -> bool {
        if !self.accepts_input() {
            return false;
        }
        self.change_entry(entry, scroll_into_view)
    }
    fn change_entry(&mut self, entry: usize, scroll_into_view: bool) -> bool {
        if entry == self.entry {
            return false;
        }
        assert!(entry < self.groups[self.group].entries.len());
        rate(
            &mut self.groups[self.group].entries[self.entry].focus,
            0.0,
            0.1,
        );
        rate(&mut self.groups[self.group].entries[entry].focus, 1.0, 0.25);
        self.entry = entry;
        self.capture_details();
        if scroll_into_view {
            self.velocity = 0.0;
            let data = &self.setup.groups[self.group];
            let top = data.entries[entry].y as f32;
            let current = self.scroll.value();
            let target = if top < current {
                top.max(0.0)
            } else if top - current + 110.0 > 670.0 {
                (top + 110.0 - 670.0).min((data.content_height - 670) as f32)
            } else {
                current
            };
            // Freeze the current scroll even when no adjustment is needed.
            self.scroll
                .enqueue_now(target, Tween::linear(Ticks::from_f32(4.0)));
        }
        true
    }
    pub fn controller_input(&mut self, state: &EnumMap<AppAction, ActionState>) {
        if !self.accepts_input() {
            return;
        }
        let group_count = self.groups.len();
        if group_count >= 2 {
            let group = if state[AppAction::ScenarioTabPrevious].is_clicked_or_repeated {
                Some((self.group + group_count - 1) % group_count)
            } else if state[AppAction::ScenarioTabNext].is_clicked_or_repeated {
                Some((self.group + 1) % group_count)
            } else {
                None
            };
            if group.is_some_and(|group| self.select_group(group)) {
                self.events.push(SelectionEvent::MoveSound);
                return;
            }
        }
        let entry = if state[AppAction::Up].is_clicked_or_repeated {
            Some(self.entry.saturating_sub(1))
        } else if state[AppAction::AnyDown].is_clicked_or_repeated {
            Some((self.entry + 1).min(self.groups[self.group].entries.len() - 1))
        } else {
            None
        };
        if entry.is_some_and(|entry| self.select_entry(entry, true)) {
            self.events.push(SelectionEvent::MoveSound);
            return;
        }
        if state[AppAction::Cancel].is_clicked {
            self.velocity = 0.0;
            self.events
                .extend([SelectionEvent::BackSound, SelectionEvent::BeginBack]);
            self.gate = Gate::Leaving;
        } else if state[AppAction::Enter].is_clicked {
            self.confirm();
        } else if state[AppAction::UnlockScenarios].is_clicked && self.unlock_available {
            // Updated df568 checks Cancel, Confirm, then Plus. No chapter flash.
            self.gate = Gate::UnlockDialog;
            self.events
                .extend([SelectionEvent::ConfirmSound, SelectionEvent::UnlockDialog]);
        }
    }
    /// Native e05d4..e06c4 moves the pane by the pointer Y difference, then
    /// keeps the selected entry wholly within the visible interval. The
    /// Pointer callbacks separately handle cancellation and release velocity.
    pub fn drag_scroll(&mut self, delta_y: f32) {
        if !self.accepts_input() {
            return;
        }
        let data = &self.setup.groups[self.group];
        let scroll = (self.scroll.value() - delta_y).clamp(0.0, (data.content_height - 670) as f32);
        // Native bb2e0 cancels any scroll queue and snaps to the drag value.
        self.apply_scroll(scroll);
    }
    fn apply_scroll(&mut self, scroll: f32) {
        let data = &self.setup.groups[self.group];
        let mut entry = self.entry;
        loop {
            let top = data.entries[entry].y as f32;
            if top < scroll {
                entry += 1;
            } else if top + 110.0 > scroll + 670.0 {
                entry -= 1;
            } else {
                break;
            }
        }
        self.change_entry(entry, false);
        self.scroll = Tweener::new(scroll);
    }
    pub fn pointer_probe(&mut self, point: Vec2) -> bool {
        self.pointer = PointerTarget::None;
        if !self.accepts_input() {
            return false;
        }
        if (1764.0..1848.0).contains(&point.x) && (32.0..116.0).contains(&point.y) {
            self.pointer = PointerTarget::Close;
            return true;
        }
        if !(160.0..980.0).contains(&point.y) {
            self.pointer = PointerTarget::Blank;
            return true;
        }
        if self.groups.len() >= 2 {
            let left = (1920.0 - self.groups.len() as f32 * 300.0) * 0.5;
            for (index, group) in self.setup.groups.iter().enumerate() {
                // Native dff60 compares the original metadata ID with the
                // selected visible ordinal, then returns that original ID.
                if group.id as usize == self.group {
                    continue;
                }
                let x = left + index as f32 * 300.0;
                if point.x >= x && point.x < x + 300.0 && (144.0..211.5).contains(&point.y) {
                    self.pointer = PointerTarget::Tab(group.id as usize);
                    return true;
                }
            }
        }
        if !(86.0..526.0).contains(&point.x) || !(246.0..916.0).contains(&point.y) {
            return false;
        }
        let y = (self.scroll.value() - 246.0) + point.y;
        let entry = self.setup.groups[self.group]
            .entries
            .iter()
            .position(|entry| y >= entry.y as f32 && y < entry.y as f32 + 110.0);
        if self.velocity.abs() >= f32::EPSILON {
            self.velocity = 0.0;
            self.pointer = PointerTarget::Pane(None);
        } else {
            self.pointer = PointerTarget::Pane(entry);
        }
        true
    }
    pub fn pointer_reset(&mut self) {
        self.pointer = PointerTarget::None;
    }
    fn pointer_flash(&mut self) -> Option<&mut Tweener> {
        match self.pointer {
            PointerTarget::Close => Some(&mut self.close_flash),
            PointerTarget::Tab(index) => self.groups.get_mut(index).map(|group| &mut group.flash),
            PointerTarget::Pane(Some(index)) => {
                Some(&mut self.groups[self.group].entries[index].flash)
            }
            _ => None,
        }
    }
    pub fn pointer_press(&mut self) {
        if let Some(flash) = self.pointer_flash() {
            rate(flash, 0.7, 0.2);
        }
        if let PointerTarget::Pane(Some(index)) = self.pointer {
            self.select_entry(index, true);
        }
    }
    pub fn pointer_release(&mut self) {
        if !self.accepts_input() {
            return;
        }
        match self.pointer {
            PointerTarget::Close => {
                rate(&mut self.close_flash, 0.7, 0.2);
                self.close_flash
                    .enqueue(0.0, Tween::linear(Ticks::from_f32(3.5)));
                self.events
                    .extend([SelectionEvent::ConfirmSound, SelectionEvent::BeginBack]);
                self.gate = Gate::Leaving;
            }
            PointerTarget::Tab(index) => {
                if let Some(group) = self.groups.get_mut(index) {
                    rate(&mut group.flash, 0.7, 0.2);
                    group
                        .flash
                        .enqueue(0.0, Tween::linear(Ticks::from_f32(3.5)));
                    self.events.push(SelectionEvent::ConfirmSound);
                    self.select_group(index);
                } else {
                    self.events
                        .push(SelectionEvent::InvalidNativeTabIndex(index));
                }
            }
            PointerTarget::Pane(Some(index)) => {
                self.select_entry(index, true);
                self.confirm();
            }
            _ => {}
        }
    }
    pub fn pointer_drag(&mut self, delta_y: f32) {
        if self.pointer == PointerTarget::Blank {
            rate(&mut self.close_flash, 0.0, 0.2);
        }
        if let Some(flash) = self.pointer_flash() {
            rate(flash, 0.0, 0.2);
        }
        match self.pointer {
            PointerTarget::Close | PointerTarget::Blank => self.pointer = PointerTarget::Blank,
            PointerTarget::Pane(_) => {
                self.pointer = PointerTarget::Pane(None);
                self.drag_scroll(delta_y);
            }
            _ => {}
        }
    }
    pub fn pointer_cancel(&mut self) {
        if self.pointer == PointerTarget::Blank {
            rate(&mut self.close_flash, 0.0, 0.2);
        }
        if let Some(flash) = self.pointer_flash() {
            rate(flash, 0.0, 0.2);
        }
        if matches!(self.pointer, PointerTarget::Close | PointerTarget::Blank) {
            self.pointer = PointerTarget::Blank;
        }
    }
    pub fn pointer_gesture(&mut self, velocity: Vec2) {
        if matches!(self.pointer, PointerTarget::Pane(_)) {
            self.velocity = velocity.y;
        }
    }
    pub fn confirm(&mut self) {
        if !self.accepts_input() {
            return;
        }
        let state = self.setup.groups[self.group].entries[self.entry].state;
        let flash = &mut self.groups[self.group].entries[self.entry].flash;
        rate(flash, 0.7, 0.2);
        flash.enqueue(0.0, Tween::linear(Ticks::from_f32(0.7 / 0.2)));
        if state == 0 {
            self.events.push(SelectionEvent::DeniedSound);
        } else {
            self.events.push(SelectionEvent::ConfirmSound);
            if state <= 2 {
                flash.enqueue(0.0, Tween::linear(Ticks::from_f32(30.0)));
            }
            self.gate = Gate::Flash {
                group: self.group,
                entry: self.entry,
            };
        }
    }
    /// Native input/task polling precedes each frame's clock advancement.
    pub fn prepare_frame(&mut self) {
        if let Gate::Flash { group, entry } = self.gate {
            if !self.groups[group].entries[entry].flash.is_idle() {
                return;
            }
            if self.setup.groups[group].entries[entry].state >= 3 {
                self.gate = Gate::Dialog;
                self.events.push(SelectionEvent::ConfirmDialog);
            } else {
                self.launch();
            }
        }
    }
    pub fn answer_dialog(&mut self, accepted: bool) {
        if self.gate != Gate::Dialog {
            return;
        }
        if accepted {
            self.launch();
        } else {
            self.gate = Gate::Active;
        }
    }
    pub fn answer_unlock_dialog(&mut self, accepted: bool) {
        if self.gate != Gate::UnlockDialog {
            return;
        }
        if accepted {
            self.gate = Gate::Leaving;
            self.events.push(SelectionEvent::UnlockScenarios);
        } else {
            self.gate = Gate::Active;
        }
    }
    fn launch(&mut self) {
        self.gate = Gate::Leaving;
        self.events.push(SelectionEvent::LaunchScene(
            self.setup.groups[self.group].entries[self.entry].scenario_id,
        ));
    }
    pub fn advance_clocks(&mut self, delta: Ticks) {
        // Native ddba0 runs tasks/input, then inertia, then feedback clocks.
        if self.velocity != 0.0 {
            let current = self.scroll.value();
            let scroll = (current - self.velocity * delta.as_f32()).clamp(
                0.0,
                (self.setup.groups[self.group].content_height - 670) as f32,
            );
            if (current - scroll).abs() < f32::EPSILON {
                self.velocity = 0.0;
            } else {
                self.apply_scroll(scroll);
                self.velocity *= 0.95f32.powf(delta.as_f32());
                if self.velocity.abs() < 0.1 {
                    self.velocity = 0.0;
                }
            }
        }
        self.scroll.update(delta);
        self.details_fade.update(delta);
        self.close_flash.update(delta);
        for group in &mut self.groups {
            group.focus.update(delta);
            group.flash.update(delta);
            for entry in &mut group.entries {
                entry.focus.update(delta);
                entry.flash.update(delta);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shin_core::format::save::SaveData;
    fn model() -> ScenarioSelection {
        let mut save = SaveData::default();
        save.persist_data.set(25, 1);
        save.persist_data.set(48, 3);
        ScenarioSelection::new_active(ScenarioSelectSetup::from_save(&save))
    }
    fn input(actions: &[AppAction]) -> EnumMap<AppAction, ActionState> {
        let mut state = EnumMap::default();
        for &action in actions {
            state[action] = ActionState {
                is_clicked: true,
                is_clicked_or_repeated: true,
                ..Default::default()
            };
        }
        state
    }
    fn advance(model: &mut ScenarioSelection, ticks: f32) {
        model.advance_clocks(Ticks::from_f32(ticks));
    }
    #[test]
    fn start_h_j_change_tabs_arrows_only_move_entries() {
        use shin_input::{Action, RawInputState};
        use winit::keyboard::KeyCode;
        let mut m = model();
        for (key, expected) in [
            (KeyCode::ArrowRight, (0, 0)),
            (KeyCode::KeyJ, (1, 0)),
            (KeyCode::ArrowDown, (1, 1)),
            (KeyCode::KeyH, (0, 0)),
            (KeyCode::ArrowLeft, (0, 0)),
        ] {
            let mut raw = RawInputState::new();
            raw.keyboard.insert(key);
            let state = AppAction::lower(&raw).map(|_, held| ActionState {
                is_clicked: held,
                is_clicked_or_repeated: held,
                ..Default::default()
            });
            m.controller_input(&state);
            assert_eq!(m.selected(), expected, "{key:?}");
            advance(&mut m, 100.0);
        }
    }

    #[test]
    fn start_controller_shoulders_switch_tabs_and_directions_do_not() {
        use shin_input::{Action, RawInputState, inputs::GamepadButton};
        let mut m = model();
        for (button, expected) in [
            (GamepadButton::Right, (0, 0)),
            (GamepadButton::R, (1, 0)),
            (GamepadButton::Down, (1, 1)),
            (GamepadButton::L, (0, 0)),
            (GamepadButton::Left, (0, 0)),
        ] {
            let mut raw = RawInputState::new();
            raw.gamepads.unified.buttons[button] = true;
            let state = AppAction::lower(&raw).map(|_, held| ActionState {
                is_clicked: held,
                is_clicked_or_repeated: held,
                ..Default::default()
            });
            m.controller_input(&state);
            assert_eq!(m.selected(), expected, "{button:?}");
            advance(&mut m, 100.0);
        }
    }

    #[test]
    fn start_controller_shoulders_change_tabs_and_dpad_stick_horizontal_do_not() {
        use shin_input::{
            Action, RawInputState,
            inputs::{GamepadButton, VirtualGamepadButton},
        };
        let mut model = model();
        let dispatch = |model: &mut ScenarioSelection, raw: &RawInputState| {
            model.controller_input(&AppAction::lower(raw).map(|_, held| ActionState {
                is_clicked: held,
                is_clicked_or_repeated: held,
                ..Default::default()
            }));
            advance(model, 100.0);
        };
        for button in [GamepadButton::Left, GamepadButton::Right] {
            let mut raw = RawInputState::new();
            raw.gamepads.unified.buttons[button] = true;
            dispatch(&mut model, &raw);
            assert_eq!(model.selected(), (0, 0));
        }
        for direction in [
            VirtualGamepadButton::StickLLeft,
            VirtualGamepadButton::StickLRight,
        ] {
            let mut raw = RawInputState::new();
            raw.gamepads.unified.virtual_keys[direction] = true;
            dispatch(&mut model, &raw);
            assert_eq!(model.selected(), (0, 0));
        }
        for (button, tab) in [(GamepadButton::R, 1), (GamepadButton::L, 0)] {
            let mut raw = RawInputState::new();
            raw.gamepads.unified.buttons[button] = true;
            dispatch(&mut model, &raw);
            assert_eq!(model.selected(), (tab, 0));
        }
    }

    #[test]
    fn movement_priorities_and_bounds_preserve_native_action_fallthrough() {
        let mut m = model();
        m.controller_input(&input(&[
            AppAction::ScenarioTabPrevious,
            AppAction::ScenarioTabNext,
            AppAction::Up,
            AppAction::Cancel,
            AppAction::Enter,
        ]));
        assert_eq!(m.selected(), (2, 0));
        assert!(m.accepts_input());
        assert_eq!(
            m.take_events(),
            [SelectionEvent::CaptureDetails, SelectionEvent::MoveSound]
        );
        m.controller_input(&input(&[
            AppAction::Up,
            AppAction::AnyDown,
            AppAction::Enter,
        ]));
        // At upper bound Up wins over Down, but confirmation still falls through.
        assert_eq!(m.selected(), (2, 0));
        assert!(m.accepts_input());
        assert_eq!(m.take_events(), [SelectionEvent::DeniedSound]);
        m.controller_input(&input(&[AppAction::AnyDown, AppAction::Enter]));
        assert_eq!(m.selected(), (2, 1));
        assert!(m.accepts_input());
        m.take_events();
        m.controller_input(&input(&[
            AppAction::AnyDown,
            AppAction::Cancel,
            AppAction::Enter,
        ]));
        assert!(!m.accepts_input());
        assert_eq!(
            m.take_events(),
            [SelectionEvent::BackSound, SelectionEvent::BeginBack]
        );
    }
    #[test]
    fn each_group_restores_its_entry_and_truncated_evaluated_scroll() {
        let mut m = model();
        m.select_group(1);
        m.select_entry(19, true);
        advance(&mut m, 1.0);
        assert_eq!(m.scroll(), 392.0);
        m.scroll.enqueue_now(392.75, Tween::IMMEDIATE);
        m.select_group(0);
        assert_eq!(m.selected(), (0, 0));
        assert_eq!(m.scroll(), 0.0);
        m.select_entry(7, true);
        advance(&mut m, 4.0);
        assert_eq!(m.scroll(), 224.0);
        m.select_group(1);
        assert_eq!(m.selected(), (1, 19));
        assert_eq!(m.scroll(), 392.0);
        m.select_group(0);
        assert_eq!(m.selected(), (0, 7));
        assert_eq!(m.scroll(), 224.0);
    }
    #[test]
    fn focus_and_details_clocks_continue_for_inactive_groups() {
        let mut m = model();
        m.select_entry(1, false);
        advance(&mut m, 2.0);
        let frame = m.chapter_feedback();
        assert!((frame[0].focus - 0.8).abs() < 1e-6);
        assert_eq!(frame[1].focus, 0.5);
        assert!((m.details_fade() - 0.8).abs() < 1e-6);
        m.select_group(1);
        advance(&mut m, 8.0);
        m.select_group(0);
        let frame = m.chapter_feedback();
        assert_eq!(frame[0].focus, 0.0);
        assert_eq!(frame[1].focus, 1.0);
        assert_eq!(m.tab_focus(), [0.0, 1.0, 0.0]);
    }
    #[test]
    fn first_chapter_flash_and_hold_dispatch_only_on_next_task_poll() {
        let mut m = model();
        m.confirm();
        assert!(!m.accepts_input());
        m.take_events();
        advance(&mut m, 7.0);
        m.prepare_frame();
        assert!(m.take_events().is_empty());
        advance(&mut m, 30.0);
        assert!(m.take_events().is_empty());
        m.prepare_frame();
        assert_eq!(m.take_events(), [SelectionEvent::LaunchScene(0)]);
        m.prepare_frame();
        assert!(m.take_events().is_empty());
    }
    #[test]
    fn drag_clamps_scroll_and_moves_focus_to_a_wholly_visible_entry_without_sound() {
        let mut m = model();
        m.select_group(1);
        m.take_events();
        m.drag_scroll(-10000.0);
        assert_eq!(m.scroll(), 1568.0);
        assert_eq!(m.selected(), (1, 14));
        assert_eq!(m.take_events(), [SelectionEvent::CaptureDetails]);
        m.select_entry(19, false);
        m.take_events();
        m.drag_scroll(1368.0);
        assert_eq!(m.scroll(), 200.0);
        assert_eq!(m.selected(), (1, 6));
        assert_eq!(m.take_events(), [SelectionEvent::CaptureDetails]);
        m.drag_scroll(10000.0);
        assert_eq!(m.scroll(), 0.0);
        assert_eq!(m.selected(), (1, 5));
        m.select_group(2);
        m.drag_scroll(-10000.0);
        assert_eq!(m.scroll(), 0.0);
    }
    #[test]
    fn completed_chapter_dialog_rearms_input_on_no_and_launches_on_yes() {
        let mut m = model();
        m.select_group(2);
        m.select_entry(1, false);
        m.take_events();
        m.confirm();
        advance(&mut m, 7.0);
        m.prepare_frame();
        assert_eq!(
            m.take_events(),
            [SelectionEvent::ConfirmSound, SelectionEvent::ConfirmDialog]
        );
        m.answer_dialog(false);
        assert!(m.accepts_input());
        m.confirm();
        advance(&mut m, 7.0);
        m.prepare_frame();
        m.take_events();
        m.answer_dialog(true);
        assert_eq!(m.take_events(), [SelectionEvent::LaunchScene(47)]);
        assert!(!m.accepts_input());
    }
    #[test]
    fn negative_state_is_denied_by_details_but_accepted_by_confirmation() {
        let mut save = SaveData::default();
        save.persist_data.set(2, -1);
        let mut m = ScenarioSelection::new_active(ScenarioSelectSetup::from_save(&save));
        m.select_entry(1, false);
        assert_eq!(m.setup().groups[0].entries[1].details_name(), "details_48");
        m.take_events();
        m.confirm();
        assert_eq!(m.take_events(), [SelectionEvent::ConfirmSound]);
        advance(&mut m, 37.0);
        m.prepare_frame();
        assert_eq!(m.take_events(), [SelectionEvent::LaunchScene(1)]);
    }

    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_start_selection_priorities_memory_and_confirmation_gates() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let word = |address: usize| {
            u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap())
        };
        for (address, expected) in [
            (0xdf4c8, 0x3750014a),
            (0xdf4cc, 0x3658020a),
            (0xdf510, 0x37c0034a),
            (0xdf514, 0x36c803aa),
            (0xdf57c, 0x0aa87d15),
            (0xdf58c, 0x37e801c8),
            (0xdf590, 0x36e002a8),
            (0xdfa90, 0xb900e928),
            (0xdfa98, 0x1e380008),
            (0xdfaa8, 0xb900ed28),
            (0xdfae4, 0xb940e909),
            (0xdfaec, 0xbd40ed00),
            (0xdfd0c, 0xbd457101),
            (0xdfd44, 0x1e2a1001),
            (0xdfdd0, 0x1e249001),
            (0xdfe90, 0x1e221001),
            (0xdf7b4, 0x34000708),
            (0xdf7d8, 0x7100091f),
            (0xdf7dc, 0x540000ec),
            (0xdf7e4, 0x1e27d001),
            (0xe2a5c, 0x97ff63e5),
            (0xe2a80, 0x71000d1f),
            (0xe2a84, 0x5400034b),
            (0xe2af0, 0x97fff390),
            (0xe2d64, 0x7100051f),
            (0xe2d74, 0x34000068),
            (0xe2d78, 0x17fff16e),
            (0xe2d80, 0x17fff2ec),
            (0xe0038, 0xf9000109),
            (0xe008c, 0xb9019260),
            (0xe00cc, 0xb9019269),
            (0xe01d8, 0x1e20c000),
            (0xe01e4, 0xf900ce7f),
            (0xe01ec, 0x12800008),
            (0xe020c, 0xb9019a68),
            (0xdff68, 0x6b09011f),
            (0xdfff8, 0xb94002b4),
            (0xe0338, 0x97ff6db2),
            (0xe0348, 0x320003e2),
            (0xe0478, 0x71000501),
            (0xe049c, 0x17fffca5),
            (0xe05dc, 0x1e213808),
            (0xe06c4, 0x97ff6b07),
            (0xe0708, 0xbd019c01),
            (0xddbf4, 0x1e280821),
            (0xddbf8, 0x1e213809),
            (0xddc44, 0x54000724),
            (0xddd08, 0x9400fbce),
            (0xddd0c, 0x1e200920),
            (0xddd18, 0xbd019e60),
            (0xddd24, 0x54000045),
            (0xddd28, 0xb9019e7f),
        ] {
            assert_eq!(word(address), expected, "native{address:x}");
        }
        for (address, expected) in [
            (0x159570, 0.1f32),
            (0x159640, 670.0),
            (0x1596b4, -670.0),
            (0x1595a4, 0.7),
            (0x159974, 0.2),
            (0x159a7c, 144.0),
            (0x15941c, 67.5),
            (0x159730, 300.0),
            (0x15937c, 160.0),
            (0x15940c, 980.0),
            (0x159480, 1764.0),
            (0x15971c, 1848.0),
            (0x159694, 32.0),
            (0x159658, 116.0),
            (0x159670, 86.0),
            (0x159918, 526.0),
            (0x1593cc, 246.0),
            (0x1599d8, 916.0),
            (0x159ab8, f32::EPSILON),
            (0x1595d8, 0.95),
        ] {
            assert_eq!(f32::from_bits(word(address)), expected);
        }
        let message = "このシナリオを開始します。@rよろしいですか？";
        assert_eq!(
            &exe[0x1635cc + 256..0x1635cc + 256 + message.len()],
            message.as_bytes()
        );
        let relocations: std::collections::HashMap<_, _> = (0x11e040..0x152eb8)
            .step_by(24)
            .map(|offset| {
                (
                    u64::from_le_bytes(exe[offset + 256..offset + 264].try_into().unwrap()),
                    u64::from_le_bytes(exe[offset + 272..offset + 280].try_into().unwrap()),
                )
            })
            .collect();
        assert_eq!(relocations[&0x21f860], 0xe2a40);
        assert_eq!(relocations[&0x21f8d0], 0xe2d60);
    }
}
