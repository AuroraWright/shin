//! Updated1a8bc/1b628 SystemMenu. Child modes, modal backdrop/audio scope and
//! text/GPU resources are supplied by their owners; this owns rows and input.
use crate::adv::input_area::{
    InputArea, InputAreaTarget, MouseRelease, PointerFrame, desktop_gesture_vector,
};
use crate::layer::higurashi_properties::HigurashiTweener;
use glam::Vec2;
use shin_core::time::Ticks;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cue {
    Move,
    Cancel,
    Confirm,
}

pub trait MenuServices {
    ///15038 captures the backdrop, acquires counted conceal/pause and disables
    /// scene animation.158e0 releases those owners when closing finishes.
    fn enter_modal(&mut self);
    fn exit_modal(&mut self);
    fn cue(&mut self, cue: Cue);
    fn push_confirmation(&mut self, callback: Confirmation);
    fn push_layer_reloader(&mut self);
    ///1c804 creates the selected child. The atlas item is read when the flash
    /// finishes, rather than captured at the instant Confirm is pressed.
    fn dispatch_item(&mut self, item: u32);
}

#[derive(Debug, Clone)]
pub struct Row {
    pub item: u32,
    pub slide: HigurashiTweener,
    pub flash: HigurashiTweener,
}

pub struct SystemMenu {
    state: State,
    area: InputArea,
}
struct State {
    phase: i32,
    selected: usize,
    pointer_row: Option<usize>,
    vertical_offset: i32,
    layer_reload: bool,
    extended_scenario: bool,
    initial_setting: u8,
    rows: Vec<Row>,
    detail_slide: HigurashiTweener,
    detail_fade: HigurashiTweener,
    play_time: String,
}

fn control(tween: &mut HigurashiTweener, value: f32, time: f32, flags: u32) {
    tween
        .control(value, Ticks::from_f32(time), flags)
        .expect("Native SystemMenu uses supported tween flags");
}
fn clear(tween: &mut HigurashiTweener) {
    tween.fast_forward_to(tween.value());
}

impl SystemMenu {
    ///1ae94..1aea0 stores scenarioID>10000 in Menu+90;1c96c reads that
    ///flag to choose the full or reduced Config schema.
    pub(crate) fn config_flags(&self) -> u32 {
        1 + u32::from(self.state.extended_scenario)
    }
    pub fn new(scenario_id: i32, setting: u8, services: &mut impl MenuServices) -> Self {
        services.enter_modal();
        let id = scenario_id as u32;
        let items = if id.wrapping_sub(5) <= 7 {
            vec![2, 3, 5]
        } else {
            let mut items = vec![0, 1, 2];
            if id.wrapping_sub(2000) < 1000 || id.wrapping_sub(12000) <= 999 {
                items.extend([3, 7]);
            } else if id.wrapping_sub(1000) < 1000 || id.wrapping_sub(11000) <= 999 {
                items.extend([3, 6]);
            } else {
                if scenario_id <= 9999 {
                    items.push(4);
                }
                items.push(3);
            }
            items.push(5);
            items
        };
        Self {
            area: InputArea::new(1280, 720),
            state: State {
                phase: 0,
                selected: 0,
                pointer_row: None,
                vertical_offset: 72,
                layer_reload: false,
                extended_scenario: scenario_id > 10000,
                initial_setting: setting,
                rows: items
                    .into_iter()
                    .map(|item| Row {
                        item,
                        slide: HigurashiTweener::new(0.0),
                        flash: HigurashiTweener::new(0.0),
                    })
                    .collect(),
                detail_slide: HigurashiTweener::new(1.0),
                detail_fade: HigurashiTweener::new(0.0),
                play_time: String::new(),
            },
        }
    }
    pub fn rows(&self) -> &[Row] {
        &self.state.rows
    }
    pub fn selected(&self) -> usize {
        self.state.selected
    }
    pub(crate) fn input_ready(&self) -> bool {
        self.state.phase == 1 && !self.state.busy()
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    pub(crate) fn reference_input_ready(&self) -> bool {
        self.input_ready()
    }
    pub fn phase(&self) -> i32 {
        self.state.phase
    }
    pub fn completion_word(&self) -> u32 {
        (self.state.phase as u32) >> 31
    }
    pub fn detail_slide(&self) -> f32 {
        self.state.detail_slide.value()
    }
    pub fn detail_fade(&self) -> f32 {
        self.state.detail_fade.value()
    }
    pub fn play_time(&self) -> &str {
        &self.state.play_time
    }
    pub fn cancel_input(&mut self, services: &mut impl MenuServices) {
        self.area.disable(&mut Contact {
            state: &mut self.state,
            setting: 0,
            services,
        });
    }
    pub fn layer_reload_finished(&mut self) {
        self.state.layer_reload = false;
    }
    ///1c804 hides the menu behind its child without entering closing phase2.
    pub fn child_started(&mut self) {
        self.state.animate(false);
    }
    ///1ba34 reopens rows/details after the child's return callback.
    pub fn child_returned(&mut self, services: &mut impl MenuServices) {
        self.state.animate(true);
        // Native5eb0c flushes contact ownership on reopening.
        self.area.disable(&mut Contact {
            state: &mut self.state,
            setting: 0,
            services,
        });
    }

    /// Input eligibility/closing completion are checked before clocks advance.
    /// Every phase advances row/detail clocks and formats the global play clock.
    /// Returns0 like native1b628, consuming the lower mode's input permission.
    pub fn poll(
        &mut self,
        delta: Ticks,
        permission: u32,
        pressed: u32,
        repeated: u32,
        frame: &PointerFrame<'_>,
        setting: u8,
        play_ticks: f64,
        services: &mut impl MenuServices,
    ) -> u32 {
        self.poll_with_mouse_release(
            delta, permission, pressed, repeated, frame, setting, play_ticks, services, None,
        )
    }
    pub(crate) fn poll_with_mouse_release(
        &mut self,
        delta: Ticks,
        permission: u32,
        pressed: u32,
        repeated: u32,
        frame: &PointerFrame<'_>,
        setting: u8,
        play_ticks: f64,
        services: &mut impl MenuServices,
        mouse_release: Option<MouseRelease>,
    ) -> u32 {
        let state = &mut self.state;
        if state.phase == 0 {
            state.animate(true);
            self.area.disable(&mut Contact {
                state,
                setting,
                services,
            });
            state.phase = state.phase.wrapping_add(1);
        }
        if state.phase == 1 {
            if permission & 1 == 0 || state.busy() {
                self.area.disable(&mut Contact {
                    state,
                    setting,
                    services,
                });
            } else if !self.area.update_with_mouse_release(
                &mut Contact {
                    state,
                    setting,
                    services,
                },
                frame,
                mouse_release,
            ) {
                if pressed & 0x0200_8000 != 0 {
                    state.close(setting, services);
                } else if pressed & 0x0100_0000 != 0 {
                    state.confirm(services);
                } else {
                    let previous = state.selected;
                    let count = state.rows.len();
                    let selected = if repeated & (1 << 4) != 0 {
                        if previous > 0 {
                            previous - 1
                        } else {
                            count - 1
                        }
                    } else if repeated & (1 << 6) != 0 {
                        if previous + 1 >= count {
                            0
                        } else {
                            previous + 1
                        }
                    } else if repeated & (1 << 7) != 0 {
                        0
                    } else if repeated & (1 << 5) != 0 {
                        count - 1
                    } else {
                        previous
                    };
                    if selected != previous {
                        services.cue(Cue::Move);
                        state.selected = selected;
                    }
                }
            }
        } else if state.phase == 2 && !state.busy() && !state.layer_reload {
            services.exit_modal();
            state.phase = -1;
        }
        for row in &mut state.rows {
            row.slide.update(delta);
            row.flash.update(delta);
        }
        state.detail_slide.update(delta);
        state.detail_fade.update(delta);
        state.play_time = play_time_text(play_ticks);
        0
    }
}

impl State {
    fn busy(&self) -> bool {
        self.rows.iter().any(|row| row.slide.is_waiting())
            || self.detail_slide.is_waiting()
            || self.detail_fade.is_waiting()
    }
    fn animate(&mut self, opening: bool) {
        for (index, row) in self.rows.iter_mut().enumerate() {
            control(&mut row.slide, 0.0, index as f32 * 2.0, 0x80);
            control(
                &mut row.slide,
                f32::from(u8::from(opening)),
                10.0,
                if opening { 2 } else { 1 },
            );
        }
        control(
            &mut self.detail_slide,
            f32::from(u8::from(!opening)),
            10.0,
            if opening { 2 } else { 1 },
        );
        control(&mut self.detail_fade, f32::from(u8::from(opening)), 10.0, 0);
    }
    fn close(&mut self, setting: u8, services: &mut impl MenuServices) {
        services.cue(Cue::Cancel);
        if self.extended_scenario && setting != self.initial_setting {
            self.layer_reload = true;
            services.push_layer_reloader();
        }
        self.animate(false);
        self.phase = self.phase.wrapping_add(1);
    }
    fn confirm(&mut self, services: &mut impl MenuServices) {
        services.cue(Cue::Confirm);
        let flash = &mut self.rows[self.selected].flash;
        clear(flash);
        control(flash, 0.7, 0.2, 0x40);
        control(flash, 0.0, 0.2, 0x40);
        services.push_confirmation(Confirmation::default());
    }
    fn row_at(&self, position: Vec2) -> Option<usize> {
        //1d4f0 uses x16 with the constructor's y72 offset, independently of
        // the animated drawing x24 position.
        // The native lower bounds are inclusive; upper bounds are exclusive.
        (0..self.rows.len()).find(|&index| {
            let y = (index as u64 * 128).wrapping_add(self.vertical_offset as i64 as u64) as f32;
            position.x >= 16.0 && position.x < 412.0 && position.y >= y && position.y < y + 120.0
        })
    }
    fn cancel_pointer(&mut self) {
        if let Some(index) = self.pointer_row.take() {
            let flash = &mut self.rows[index].flash;
            clear(flash);
            control(flash, 0.0, 0.2, 0x40);
        }
    }
}
struct Contact<'a, S> {
    state: &'a mut State,
    setting: u8,
    services: &'a mut S,
}
impl<S: MenuServices> InputAreaTarget for Contact<'_, S> {
    fn probe(&mut self, position: Vec2) -> bool {
        self.state.pointer_row = self.state.row_at(position);
        true
    }
    fn reset(&mut self) {
        self.state.pointer_row = None;
    }
    fn press(&mut self, _: Vec2) {
        if let Some(index) = self.state.pointer_row {
            let flash = &mut self.state.rows[index].flash;
            clear(flash);
            control(flash, 0.7, 0.2, 0x40);
            self.state.selected = index;
        }
    }
    fn release(&mut self, _: Vec2) {
        if let Some(index) = self.state.pointer_row {
            self.state.selected = index;
            self.state.confirm(self.services);
        } else {
            self.state.close(self.setting, self.services);
        }
    }
    fn long_press(&mut self, _: Vec2) {}
    fn drag(&mut self, _: Vec2, _: Vec2) {
        self.state.cancel_pointer();
    }
    fn gesture(&mut self, _: Vec2) {}
    fn mouse_gesture(&mut self, displacement: Vec2, velocity: Vec2) {
        // Original1d86c/1d870 are RET. Keep the native callback unchanged;
        // desktop mouse closing deliberately reuses the story-menu policy.
        let candidate = desktop_gesture_vector(displacement, velocity);
        let left = |vector: Vec2| {
            let angle = vector.y.atan2(vector.x) / f32::from_bits(0x40490fd8) * 180.0;
            angle > 150.0 || angle < -150.0
        };
        if (candidate.length() >= 64.0 && left(candidate))
            || (velocity.length() > 20.0 && left(velocity))
        {
            self.state.close(self.setting, self.services);
        }
    }
    fn cancel(&mut self) {
        self.state.cancel_pointer();
    }
}

/// Native1ffa0 task mode/203b0 callback. The actual stack excludes a newly
/// appended task from the current poll snapshot and prunes it one poll after
/// completion. It blocks input even on the frame dispatch_item is called.
pub struct Confirmation {
    waiting: bool,
}
impl Default for Confirmation {
    fn default() -> Self {
        Self { waiting: true }
    }
}
impl Confirmation {
    pub fn completion_word(&self) -> u32 {
        u32::from(!self.waiting)
    }
    pub fn poll(&mut self, menu: &mut SystemMenu, services: &mut impl MenuServices) -> u32 {
        if self.waiting && menu.state.rows[menu.state.selected].flash.is_idle() {
            services.dispatch_item(menu.state.rows[menu.state.selected].item);
            self.waiting = false;
        }
        0
    }
}

/// Original fcvtzu32 saturates negative/NaN to0 and overflow to u32::MAX.
pub fn play_time_text(ticks: f64) -> String {
    let minutes = ((ticks / 3600.0) as u32).min(59_999);
    if minutes < 60 {
        format!("{minutes}分")
    } else {
        format!("{}時間 {}分", minutes / 60, minutes % 60)
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod desktop_tests;
pub mod view;
