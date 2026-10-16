//! Native SystemMenu opening/closing and controller state (1c100/1d000).
//! ADV owns the modal scopes and backdrop; child factories live in the menu host.
pub(super) mod modal;
pub mod screen;
pub mod view;
use crate::app::AppAction;
use glam::{Vec2, Vec4, vec4};
use shin_core::time::{Easing, Ticks, Tween, Tweener};

/// Native1c1b8..1c224 and1cb90 retain route-local menu data at open.
/// The owner supplies the app-global play clock separately on each update.
#[derive(Debug, Clone)]
pub struct SystemMenuSnapshot {
    pub entry: shin_core::format::save::GameData,
    pub information: super::information::InformationState,
    pub scene_title: String,
}
impl SystemMenuSnapshot {
    pub fn from_vm(state: &super::VmState) -> Self {
        Self {
            entry: state.snapshot_game_data(),
            information: state.information.clone(),
            scene_title: state.scene_title.clone(),
        }
    }
    pub fn model(&self) -> SystemMenuModel {
        SystemMenuModel::new(
            self.information.character_grid,
            !self.information.acquired_tips.is_empty(),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemMenuPhase {
    Opening,
    Active,
    /// Native task-group gate blocks input while rows retreat beneath the child.
    Child,
    Closing,
    Closed,
}
#[derive(Debug, Clone)]
pub struct SystemMenuModel {
    items: Vec<u8>,
    selected: usize,
    phase: SystemMenuPhase,
    slides: Vec<Tweener>,
    flashes: Vec<Tweener>,
    detail_slide: Tweener,
    detail_fade: Tweener,
    confirmation: bool,
    ready_item: Option<u8>,
    sounds: Vec<&'static str>,
    pointer_row: Option<usize>,
}
impl SystemMenuModel {
    /// Constructor1c570 omits item3 when characters ID is negative;
    ///1c694 omits item4 when the copied information vector is empty.
    pub fn new(characters_id: i32, has_information: bool) -> Self {
        let items: Vec<_> = (0..7)
            .filter(|&id| (id != 3 || characters_id >= 0) && (id != 4 || has_information))
            .collect();
        let slides = items
            .iter()
            .enumerate()
            .map(|(row, _)| {
                let mut slide = Tweener::new(0.0);
                slide.enqueue(0.0, Tween::linear(Ticks::from_f32(row as f32 * 2.0)));
                slide.enqueue(
                    1.0,
                    Tween {
                        duration: Ticks::from_f32(10.0),
                        easing: Easing::SineOut,
                    },
                );
                slide
            })
            .collect();
        let mut detail_slide = Tweener::new(1.0);
        detail_slide.enqueue(
            0.0,
            Tween {
                duration: Ticks::from_f32(10.0),
                easing: Easing::SineOut,
            },
        );
        let mut detail_fade = Tweener::new(0.0);
        detail_fade.enqueue(1.0, Tween::linear(Ticks::from_f32(10.0)));
        Self {
            flashes: items.iter().map(|_| Tweener::new(0.0)).collect(),
            items,
            selected: 0,
            phase: SystemMenuPhase::Opening,
            slides,
            detail_slide,
            detail_fade,
            confirmation: false,
            ready_item: None,
            sounds: Vec::new(),
            pointer_row: None,
        }
    }
    pub fn items(&self) -> &[u8] {
        &self.items
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn phase(&self) -> SystemMenuPhase {
        self.phase
    }
    pub fn feedback(&self, row: usize) -> Option<(f32, f32)> {
        Some((
            self.slides.get(row)?.value(),
            self.flashes.get(row)?.value(),
        ))
    }
    /// Native hit rectangles stay at x24; rendering slides in from -480.
    pub fn hit_rect(&self, row: usize) -> Option<Vec4> {
        (row < self.items.len()).then(|| vec4(24.0, 72.0 + row as f32 * 128.0, 396.0, 120.0))
    }
    pub fn render_rect(&self, row: usize) -> Option<Vec4> {
        let mut rect = self.hit_rect(row)?;
        rect.x += (self.slides[row].value() - 1.0) * 504.0;
        Some(rect)
    }
    /// Native sysmenu atlas has two396px columns and seven120px rows.
    pub fn atlas_rect(&self, row: usize) -> Option<Vec4> {
        let item = *self.items.get(row)?;
        Some(vec4(
            if row == self.selected { 0.0 } else { 0.5 },
            item as f32 * 120.0 / 840.0,
            0.5,
            1.0 / 7.0,
        ))
    }
    pub fn detail_feedback(&self) -> (f32, f32) {
        (self.detail_slide.value(), self.detail_fade.value())
    }
    pub fn update(&mut self, ticks: f32) {
        if !ticks.is_finite() || ticks < 0.0 {
            return;
        }
        self.prepare_frame();
        self.advance_clocks(ticks);
    }
    pub(super) fn prepare_frame(&mut self) {
        // Native1d050/1d278 checks clocks before advancing them.
        let idle = self.slides.iter().all(Tweener::is_idle)
            && self.detail_slide.is_idle()
            && self.detail_fade.is_idle();
        if idle {
            match self.phase {
                SystemMenuPhase::Opening => self.phase = SystemMenuPhase::Active,
                SystemMenuPhase::Closing => self.phase = SystemMenuPhase::Closed,
                _ => {}
            }
        }
        // Task1f900 waits on the current selected row, then calls1e100.
        if self.confirmation && self.flashes[self.selected].is_idle() {
            self.ready_item = Some(self.items[self.selected]);
            self.confirmation = false;
        }
    }
    pub(super) fn advance_clocks(&mut self, ticks: f32) {
        for clock in self
            .slides
            .iter_mut()
            .chain(self.flashes.iter_mut())
            .chain([&mut self.detail_slide, &mut self.detail_fade])
        {
            clock.update(Ticks::from_f32(ticks));
        }
    }
    pub fn input(&mut self, state: &enum_map::EnumMap<AppAction, shin_input::ActionState>) {
        if !self.accepts_input() {
            return;
        }
        // Native1d2e8: B/Y (close) precede A, then Up/Down/Left/Right.
        if state[AppAction::Cancel].is_clicked || state[AppAction::CharacterState].is_clicked {
            self.close();
            return;
        }
        if state[AppAction::Enter].is_clicked {
            self.confirm();
            return;
        }
        let count = self.items.len();
        let selected = if state[AppAction::Up].is_clicked_or_repeated {
            (self.selected + count - 1) % count
        } else if state[AppAction::AnyDown].is_clicked_or_repeated {
            (self.selected + 1) % count
        } else if state[AppAction::Left].is_clicked_or_repeated {
            0
        } else if state[AppAction::Right].is_clicked_or_repeated {
            count - 1
        } else {
            self.selected
        };
        if selected != self.selected {
            self.selected = selected;
            self.sounds.push("move");
        }
    }
    pub(super) fn accepts_input(&self) -> bool {
        self.phase == SystemMenuPhase::Active && !self.confirmation && self.ready_item.is_none()
    }
    /// Native1e640 uses half-open static canvas rectangles. The host is responsible
    /// for mapping platform pointer coordinates through the native input transform.
    pub fn row_at(&self, position: Vec2) -> Option<usize> {
        (0..self.items.len()).find(|&row| {
            let rect = self.hit_rect(row).unwrap();
            position.x >= rect.x
                && position.x < rect.x + rect.z
                && position.y >= rect.y
                && position.y < rect.y + rect.w
        })
    }
    /// Native1e780 captures the entire input area, including its blank background.
    /// InputArea probes on first down and retains this B4 target until release/reset.
    pub fn pointer_probe(&mut self, position: Vec2) -> bool {
        if !self.accepts_input() {
            return false;
        }
        self.pointer_row = self.row_at(position);
        true
    }
    /// Native1e7e0 resets eligibility without changing row feedback.
    pub fn pointer_reset(&mut self) {
        self.pointer_row = None;
    }
    pub fn pointer_press(&mut self) {
        if !self.accepts_input() {
            return;
        }
        if let Some(row) = self.pointer_row {
            self.retarget_flash(row, 0.7);
            self.selected = row;
        }
    }
    pub fn pointer_release(&mut self) {
        if !self.accepts_input() {
            return;
        }
        if let Some(row) = self.pointer_row {
            self.selected = row;
            self.confirm();
        } else {
            self.close();
        }
    }
    /// Native1e9e0/1ec70 cancel the captured row's flash at0.2/tick.
    pub fn pointer_leave(&mut self) {
        if let Some(row) = self.pointer_row.take() {
            self.retarget_flash(row, 0.0);
        }
    }
    fn retarget_flash(&mut self, row: usize, target: f32) {
        let flash = &mut self.flashes[row];
        let duration = (target - flash.value()).abs() / 0.2;
        flash.enqueue_now(target, Tween::linear(Ticks::from_f32(duration)));
    }
    /// Native1eaa0: velocity magnitude strictly above20, angle outside[-150,150].
    /// Use the executable's PI constant for its atan2 radians-to-degrees conversion.
    pub fn pointer_gesture(&mut self, velocity: Vec2) {
        if !self.accepts_input() {
            return;
        }
        let magnitude = (velocity.x * velocity.x + velocity.y * velocity.y).sqrt();
        let angle = velocity.y.atan2(velocity.x) / 3.141592025756836_f32 * 180.0;
        if magnitude > 20.0 && (angle > 150.0 || angle < -150.0) {
            self.close();
        }
    }
    pub fn confirm(&mut self) {
        if !self.accepts_input() {
            return;
        }
        let flash = &mut self.flashes[self.selected];
        flash.enqueue_now(
            0.7,
            Tween::linear(Ticks::from_f32((0.7 - flash.value()).abs() / 0.2)),
        );
        flash.enqueue(0.0, Tween::linear(Ticks::from_f32(3.5)));
        self.confirmation = true;
        self.sounds.push("decide");
    }
    pub fn close(&mut self) {
        if !self.accepts_input() {
            return;
        }
        self.retreat_rows();
        self.phase = SystemMenuPhase::Closing;
        self.sounds.push("cancel");
    }
    /// Dispatch1e100 starts the child and queues1e010 without releasing ADV.
    /// Taking the request transfers responsibility for delivering its callback.
    pub fn take_item(&mut self) -> Option<u8> {
        let item = self.ready_item.take()?;
        self.retreat_rows();
        self.phase = SystemMenuPhase::Child;
        self.pointer_row = None;
        Some(item)
    }
    /// All ordinary child callbacks reopen only for response1 (manual1fe90).
    /// Other responses retain the gate for the host's scene/title transition.
    pub fn child_response(&mut self, response: i32) {
        if self.phase != SystemMenuPhase::Child || response != 1 {
            return;
        }
        for (row, slide) in self.slides.iter_mut().enumerate() {
            slide.enqueue(0.0, Tween::linear(Ticks::from_f32(row as f32 * 2.0)));
            slide.enqueue(
                1.0,
                Tween {
                    duration: Ticks::from_f32(10.0),
                    easing: Easing::SineOut,
                },
            );
        }
        self.detail_slide.enqueue(
            0.0,
            Tween {
                duration: Ticks::from_f32(10.0),
                easing: Easing::SineOut,
            },
        );
        self.detail_fade
            .enqueue(1.0, Tween::linear(Ticks::from_f32(10.0)));
        self.phase = SystemMenuPhase::Opening;
    }
    /// Native1e010 and closing share motion, but only closing releases the host.
    fn retreat_rows(&mut self) {
        for (row, slide) in self.slides.iter_mut().enumerate() {
            slide.enqueue(
                slide.value(),
                Tween::linear(Ticks::from_f32(row as f32 * 2.0)),
            );
            slide.enqueue(
                0.0,
                Tween {
                    duration: Ticks::from_f32(10.0),
                    easing: Easing::SineIn,
                },
            );
        }
        self.detail_slide.enqueue(
            1.0,
            Tween {
                duration: Ticks::from_f32(10.0),
                easing: Easing::SineIn,
            },
        );
        self.detail_fade
            .enqueue(0.0, Tween::linear(Ticks::from_f32(10.0)));
    }
    pub fn take_sounds(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.sounds)
    }
}

#[cfg(test)]
mod tests;
