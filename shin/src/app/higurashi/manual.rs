//! Updated Manual71fb4/72e58/72f88 and private73630/7399c/73c3c/73e14.
//! Atlas transport, InputArea ownership and parent callbacks belong to the host.
use crate::adv::{higurashi::input::Buttons, input_area::InputAreaTarget};
use crate::layer::higurashi_properties::HigurashiTweener;
use anyhow::{Result, ensure};
use glam::Vec2;
use shin_core::time::Ticks;
pub(super) mod owner;
pub(super) mod view;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Cue(&'static str),
    DisableInput,
    ReturnSource(u32),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    First,
    Open,
    Input,
    CloseWait,
    Close,
}
pub(super) struct Task {
    pub kind: Kind,
    pub done: bool,
}
#[derive(Clone, Copy, Debug)]
pub(super) struct ViewState {
    pub ready: bool,
    pub count: usize,
    pub selected: i32,
    pub progress: f32,
    pub scroll: f32,
    pub close_flash: f32,
}
pub(super) struct Model {
    pub ready: bool,
    pub count: usize,
    pub selected: i32,
    pub progress: f32,
    pub contact: i32,
    pub swipe: bool,
    pub closing: bool,
    pub complete: bool,
    pub scroll: HigurashiTweener,
    pub close_flash: HigurashiTweener,
    available_count: usize,
    pending: Vec<Kind>,
    actions: Vec<Action>,
}
fn control(t: &mut HigurashiTweener, value: f32, speed: f32, flags: u32) {
    t.control(value, Ticks::from_f32(speed), flags)
        .expect("Native Manual tween flags");
}
impl Model {
    pub fn new() -> Self {
        Self {
            ready: false,
            count: 0,
            selected: 0,
            progress: 0.,
            contact: -1,
            swipe: false,
            closing: false,
            complete: false,
            scroll: HigurashiTweener::new(0.),
            close_flash: HigurashiTweener::new(0.),
            available_count: 0,
            pending: vec![Kind::First],
            actions: vec![],
        }
    }
    /// Resource publication precedes First's next poll and page discovery.
    /// A concrete owner should reject an empty archive before accepting input;
    /// the original discovery function itself permits a zero page count.
    pub fn publish_resource(&mut self, numbered_count: usize) -> Result<()> {
        ensure!(
            numbered_count <= i32::MAX as usize,
            "Manual numbered atlas count exceeds its native signed page field"
        );
        self.available_count = numbered_count;
        self.ready = true;
        Ok(())
    }
    pub fn take_actions(&mut self) -> Vec<Action> {
        std::mem::take(&mut self.actions)
    }
    pub fn take_pending_tasks(&mut self) -> Vec<Task> {
        std::mem::take(&mut self.pending)
            .into_iter()
            .map(|kind| Task { kind, done: false })
            .collect()
    }
    pub fn view_state(&self) -> ViewState {
        ViewState {
            ready: self.ready,
            count: self.count,
            selected: self.selected,
            progress: self.progress,
            scroll: self.scroll.value(),
            close_flash: self.close_flash.value(),
        }
    }
    pub fn advance_clocks(&mut self, delta: Ticks) {
        self.scroll.update(delta);
        self.close_flash.update(delta);
    }
    fn retarget_page(&mut self) {
        self.scroll.fast_forward_to(self.scroll.value());
        control(
            &mut self.scroll,
            self.selected.wrapping_mul(1500) as f32,
            100.,
            0x42,
        );
    }
    fn clear_flash(&mut self) {
        self.close_flash.fast_forward_to(self.close_flash.value());
        control(&mut self.close_flash, 0., f32::from_bits(0x3e4ccccd), 0x40);
    }
    fn close(&mut self) {
        self.closing = true;
        self.close_flash.fast_forward_to(self.close_flash.value());
        control(
            &mut self.close_flash,
            f32::from_bits(0x3f333333),
            f32::from_bits(0x3e4ccccd),
            0x40,
        );
        control(&mut self.close_flash, 0., f32::from_bits(0x3e4ccccd), 0x40);
        self.pending.push(Kind::CloseWait);
    }
    /// Original72e58 consumes permission regardless of capture/scroll readiness.
    /// Direction/shoulder repeat priority is Left7,Right5,L10,R11,then B25.
    pub fn poll_controls(&mut self, permission: bool, captured: bool, buttons: Buttons) -> bool {
        if !permission {
            self.actions.push(Action::DisableInput);
        } else if !captured && self.scroll.is_idle() {
            let b = buttons.repeat;
            let next = if b & (1 << 7) != 0 {
                self.selected.wrapping_sub(1)
            } else if b & (1 << 5) != 0 {
                self.selected.wrapping_add(1)
            } else if b & (1 << 10) != 0 {
                self.selected.wrapping_sub(1)
            } else {
                self.selected.wrapping_add(((b >> 11) & 1) as i32)
            };
            let page = if next < 0 {
                0
            } else {
                next.min(self.count as i32 - 1)
            };
            if page != self.selected {
                self.actions.push(Action::Cue("move"));
                self.selected = page;
                self.retarget_page();
            } else if buttons.pressed & (1 << 25) != 0 {
                self.actions.push(Action::Cue("cancel"));
                self.close();
            }
        }
        !self.closing
    }
    pub fn probe_contact(&mut self, p: Vec2) -> bool {
        self.swipe = false;
        self.contact = if p.y >= 30. && p.y < 114. && p.x >= 1764. && p.x < 1848. {
            1
        } else if p.y >= 162. && p.y < 980. {
            -1
        } else {
            0
        };
        true
    }
    pub fn reset_contact(&mut self) {
        if self.contact < 0 && !self.swipe {
            self.selected = (self.scroll.value() / 1500. + 0.5) as i32;
            self.retarget_page();
        }
        self.contact = -1;
    }
    pub fn press_contact(&mut self) {
        if self.contact == 1 {
            self.close_flash.fast_forward_to(self.close_flash.value());
            control(
                &mut self.close_flash,
                f32::from_bits(0x3f333333),
                f32::from_bits(0x3e4ccccd),
                0x40,
            );
        }
    }
    pub fn release_contact(&mut self) {
        if self.contact == 1 {
            self.actions.push(Action::Cue("decide"));
            self.close();
        }
    }
    pub fn cancel_contact(&mut self) {
        if self.contact >= 0 {
            self.clear_flash();
            self.contact = 0;
        }
    }
    pub fn drag_contact(&mut self, previous: Vec2, current: Vec2) {
        if self.contact >= 0 {
            self.clear_flash();
            self.contact = 0;
            return;
        }
        let mut next = self.scroll.value() - (current.x - previous.x);
        if next < 0. {
            next = 0.;
        } else if next > ((self.count as i32 - 1).wrapping_mul(1500)) as f32 {
            next = (self.count as i32 - 1) as f32 * 1500.;
        }
        self.scroll.fast_forward_to(next);
    }
    pub fn gesture_contact(&mut self, speed: Vec2) {
        if self.contact >= 0 {
            return;
        }
        let norm = (speed.x * speed.x + speed.y * speed.y).sqrt();
        if norm <= 20. || norm.is_nan() {
            return;
        }
        let angle = speed.y.atan2(speed.x) / f32::from_bits(0x40490fd8) * 180.;
        if angle > 150. || angle < -150. {
            self.selected = self.selected.wrapping_add(1).min(self.count as i32 - 1);
        } else if angle < 30. && angle > -30. {
            self.selected = self.selected.saturating_sub(1).max(0);
        } else {
            return;
        }
        self.retarget_page();
        self.swipe = true;
    }
}
impl InputAreaTarget for Model {
    fn probe(&mut self, p: Vec2) -> bool {
        self.probe_contact(p)
    }
    fn reset(&mut self) {
        self.reset_contact();
    }
    fn press(&mut self, _: Vec2) {
        self.press_contact();
    }
    fn release(&mut self, _: Vec2) {
        self.release_contact();
    }
    fn long_press(&mut self, _: Vec2) {} // Original732bc/732c0 literalRET; default disabled.
    fn drag(&mut self, previous: Vec2, current: Vec2) {
        self.drag_contact(previous, current);
    }
    fn gesture(&mut self, speed: Vec2) {
        self.gesture_contact(speed);
    }
    fn cancel(&mut self) {
        self.cancel_contact();
    }
}
pub(super) trait Services {
    fn input(&mut self, model: &mut Model, permission: bool);
    fn event(&mut self, action: Action) -> Result<()>;
}
impl Task {
    pub fn poll_native(
        &mut self,
        model: &mut Model,
        status: u32,
        delta: Ticks,
        permission: bool,
        services: &mut dyn Services,
    ) -> Result<Vec<Task>> {
        let mut spawned = vec![];
        match self.kind {
            Kind::First => {
                if status == 2 {
                    if model.count == 0 {
                        model.count = model.available_count;
                    }
                    model.closing = false;
                    spawned.push(Task {
                        kind: Kind::Open,
                        done: false,
                    });
                    self.done = true;
                }
            }
            Kind::Open => {
                model.progress += delta.as_f32() * f32::from_bits(0x3dcccccd);
                if model.progress >= 1. {
                    model.progress = 1.;
                    spawned.push(Task {
                        kind: Kind::Input,
                        done: false,
                    });
                    self.done = true;
                }
            }
            Kind::Input => {
                services.input(model, permission);
                self.done = model.closing;
            }
            Kind::CloseWait => {
                if model.close_flash.is_idle() {
                    model.actions.push(Action::ReturnSource(0));
                    spawned.push(Task {
                        kind: Kind::Close,
                        done: false,
                    });
                    self.done = true;
                }
            }
            Kind::Close => {
                model.progress += delta.as_f32() * f32::from_bits(0xbdcccccd);
                if model.progress <= 0. {
                    model.progress = 0.;
                    model.actions.push(Action::ReturnSource(1));
                    model.complete = true;
                    self.done = true;
                }
            }
        }
        spawned.extend(model.take_pending_tasks());
        for action in model.take_actions() {
            services.event(action)?;
        }
        Ok(spawned)
    }
}

#[cfg(test)]
mod model_tests;
