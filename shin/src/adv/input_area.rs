//! Native InputArea6afc0/6b110/6b900 contact capture and frame dispatch.
use enum_map::EnumMap;
use glam::{IVec2, Vec2};
use shin_input::ActionState;
use std::collections::VecDeque;

use super::{manual::ManualModel, manual::ManualPhase, system_menu::SystemMenuModel};
use crate::app::AppAction;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum PointerKind {
    Down = 1,
    Move = 2,
    Up = 3,
    Cancel = 4,
}
/// Native events carry integer physical coordinates. Up/cancel coordinates are ignored.
#[derive(Debug, Clone, Copy)]
pub struct PointerEvent {
    pub kind: PointerKind,
    pub id: i32,
    pub position: IVec2,
}
pub struct PointerFrame<'a> {
    pub number: u32,
    pub time_seconds: f64,
    pub events: &'a [PointerEvent],
}
/// Desktop mouse motion sampled over the final 80ms of a released drag.
/// Native touch frames continue to use InputArea's original release calculation.
#[derive(Clone, Copy)]
pub(crate) struct MouseRelease {
    position: Vec2,
    velocity: Vec2,
}
/// Reuse the story-menu mouse policy across desktop gesture targets. Native
/// touch callbacks keep their original velocity and direction checks.
pub(crate) fn desktop_gesture_vector(displacement: Vec2, velocity: Vec2) -> Vec2 {
    if displacement.length() >= 64.0 {
        displacement
    } else {
        velocity
    }
}
impl MouseRelease {
    pub(crate) fn scaled(self, scale: Vec2) -> Self {
        Self {
            position: self.position * scale,
            velocity: self.velocity * scale,
        }
    }
    pub(crate) fn position(self) -> Vec2 {
        self.position
    }
    pub(crate) fn velocity(self) -> Vec2 {
        self.velocity
    }
}
/// Convert desktop pointer samples into native contact events without synthesizing
/// a new Down when a modal opens underneath an already held mouse button.
#[derive(Default)]
pub struct PointerSampler {
    held: bool,
    pending_cancel: bool,
    ignore_until_release: bool,
    previous: IVec2,
    frame: u32,
    time_seconds: f64,
    events: Vec<PointerEvent>,
    mouse_motion: VecDeque<(f64, Vec2)>,
    mouse_release: Option<MouseRelease>,
}
impl PointerSampler {
    /// Retire a scene's capture after a desktop shutdown notice. Its next
    /// sample delivers Cancel, and an inherited hold cannot start a fresh tap.
    pub fn cancel_contact(&mut self, held: bool) {
        self.pending_cancel = true;
        self.ignore_until_release |= held || self.held;
        self.held = false;
        self.mouse_motion.clear();
        self.mouse_release = None;
    }
    pub fn sample(&mut self, position: Vec2, held: bool, elapsed_seconds: f64) {
        self.frame = self.frame.wrapping_add(1);
        if elapsed_seconds.is_finite() && elapsed_seconds > 0.0 {
            self.time_seconds += elapsed_seconds;
        }
        self.events.clear();
        self.mouse_release = None;
        let position = position.as_ivec2();
        if std::mem::take(&mut self.pending_cancel) {
            self.events.push(PointerEvent {
                kind: PointerKind::Cancel,
                id: 0,
                position,
            });
        }
        if self.ignore_until_release {
            self.ignore_until_release = held;
            self.previous = position;
            return;
        }
        if held && !self.held {
            self.mouse_motion.clear();
        }
        if held || self.held {
            self.mouse_motion
                .push_back((self.time_seconds, position.as_vec2()));
            while self.mouse_motion.len() > 64
                || (self.mouse_motion.len() > 2
                    && self.mouse_motion[0].0 < self.time_seconds - 0.08)
            {
                self.mouse_motion.pop_front();
            }
            if !held {
                let (time, origin) = self.mouse_motion[0];
                let seconds = self.time_seconds - time;
                self.mouse_release = Some(MouseRelease {
                    position: position.as_vec2(),
                    velocity: if seconds > 0.0 {
                        (position.as_vec2() - origin) * (0.016683333333333335 / seconds) as f32
                    } else {
                        Vec2::ZERO
                    },
                });
                self.mouse_motion.clear();
            }
        }
        let kind = match (self.held, held) {
            (false, true) => Some(PointerKind::Down),
            (true, false) => Some(PointerKind::Up),
            (true, true) if position != self.previous => Some(PointerKind::Move),
            _ => None,
        };
        if let Some(kind) = kind {
            self.events.push(PointerEvent {
                kind,
                id: 0,
                position,
            });
        }
        self.held = held;
        self.previous = position;
    }
    pub fn frame(&self) -> PointerFrame<'_> {
        PointerFrame {
            number: self.frame,
            time_seconds: self.time_seconds,
            events: &self.events,
        }
    }
    pub(crate) fn mouse_release(&self) -> Option<MouseRelease> {
        self.mouse_release
    }
}

/// Nonvisual full-canvas tap recognition. Uses the same InputArea contact
/// classification as TouchEffect's ripple, independently of the effect setting.
pub(crate) struct TapInput {
    area: InputArea,
    target: TapTarget,
    extent: (u16, u16),
}
#[derive(Default)]
struct TapTarget(bool);
impl InputAreaTarget for TapTarget {
    fn probe(&mut self, _: Vec2) -> bool {
        true
    }
    fn reset(&mut self) {}
    fn press(&mut self, _: Vec2) {}
    fn release(&mut self, _: Vec2) {
        self.0 = true;
    }
    fn long_press(&mut self, _: Vec2) {}
    fn drag(&mut self, _: Vec2, _: Vec2) {}
    fn gesture(&mut self, _: Vec2) {}
    fn cancel(&mut self) {
        self.0 = false;
    }
}
impl Default for TapInput {
    fn default() -> Self {
        Self {
            area: InputArea::canvas(),
            target: TapTarget::default(),
            extent: (1920, 1080),
        }
    }
}
impl TapInput {
    pub fn cancel(&mut self) {
        self.area.disable(&mut self.target);
        self.target.0 = false;
    }
    pub fn update(&mut self, sampler: &PointerSampler, extent: (u16, u16)) -> bool {
        let position = sampler.previous;
        if extent.0 == 0
            || extent.1 == 0
            || position.x < 0
            || position.y < 0
            || position.x >= extent.0 as i32
            || position.y >= extent.1 as i32
        {
            self.cancel();
            return false;
        }
        if extent != self.extent {
            self.cancel();
            self.area = InputArea::new(extent.0, extent.1);
            self.extent = extent;
        }
        self.area.update(&mut self.target, &sampler.frame());
        std::mem::take(&mut self.target.0)
    }
}

pub trait InputAreaTarget {
    fn probe(&mut self, position: Vec2) -> bool;
    fn reset(&mut self);
    fn press(&mut self, position: Vec2);
    fn release(&mut self, position: Vec2);
    fn long_press(&mut self, position: Vec2);
    fn drag(&mut self, previous: Vec2, current: Vec2);
    fn gesture(&mut self, velocity: Vec2);
    /// Desktop drag release can use total displacement; native touch remains
    /// subject to the original per-frame flick velocity calculation.
    fn mouse_gesture(&mut self, _displacement: Vec2, velocity: Vec2) {
        self.gesture(velocity);
    }
    fn cancel(&mut self);
}
pub trait ModalInputTarget: InputAreaTarget {
    fn accepts_pointer_input(&self) -> bool;
    fn controller_input(&mut self, state: &EnumMap<AppAction, ActionState>);
}
#[derive(Debug, Clone)]
pub struct InputArea {
    scale: Vec2,
    physical_width: f32,
    contacts: Vec<(i32, IVec2)>,
    cached: Option<(u32, bool)>,
    long_press_enabled: bool,
    tap: bool,
    pressed: bool,
    dragged: bool,
    start_time: f64,
    last_time: f64,
    origin: Vec2,
    previous: Vec2,
    last_delta: Vec2,
}
impl InputArea {
    pub fn new(physical_width: u16, physical_height: u16) -> Self {
        assert!(physical_width > 0 && physical_height > 0);
        Self {
            scale: Vec2::new(
                1920.0 / physical_width as f32,
                1080.0 / physical_height as f32,
            ),
            physical_width: physical_width as f32,
            contacts: Vec::new(),
            cached: None,
            long_press_enabled: false,
            tap: false,
            pressed: false,
            dragged: false,
            start_time: 0.0,
            last_time: 0.0,
            origin: Vec2::ZERO,
            previous: Vec2::ZERO,
            last_delta: Vec2::ZERO,
        }
    }
    /// For screen hosts that already map window coordinates into the1920x1080 canvas.
    pub fn canvas() -> Self {
        Self::new(1920, 1080)
    }
    pub fn set_long_press_enabled(&mut self, enabled: bool) {
        self.long_press_enabled = enabled;
    }
    /// Native6b0a0 cancels then resets a live capture when its owner loses input.
    pub fn disable(&mut self, target: &mut impl InputAreaTarget) -> bool {
        if self.contacts.is_empty() {
            return false;
        }
        target.cancel();
        target.reset();
        self.contacts.clear();
        true
    }
    /// Native menu1d2d4 and manual86698 give pointer capture priority over controls.
    pub fn input_frame(
        &mut self,
        target: &mut impl ModalInputTarget,
        frame: &PointerFrame<'_>,
        state: &EnumMap<AppAction, ActionState>,
    ) {
        self.input_frame_with_mouse_release(target, frame, state, None);
    }
    pub(crate) fn input_frame_with_mouse_release(
        &mut self,
        target: &mut impl ModalInputTarget,
        frame: &PointerFrame<'_>,
        state: &EnumMap<AppAction, ActionState>,
        mouse_release: Option<MouseRelease>,
    ) {
        if !target.accepts_pointer_input() {
            self.disable(target);
        } else if !self.update_with_mouse_release(target, frame, mouse_release) {
            target.controller_input(state);
        }
    }
    /// Native6b900 caches both callbacks and consumed status once per frame.
    pub fn update(&mut self, target: &mut impl InputAreaTarget, frame: &PointerFrame<'_>) -> bool {
        self.update_with_mouse_release(target, frame, None)
    }
    pub(crate) fn update_with_mouse_release(
        &mut self,
        target: &mut impl InputAreaTarget,
        frame: &PointerFrame<'_>,
        mouse_release: Option<MouseRelease>,
    ) -> bool {
        if let Some((number, consumed)) = self.cached {
            if number == frame.number {
                return consumed;
            }
        }
        let consumed = self.dispatch(target, frame, mouse_release);
        self.cached = Some((frame.number, consumed));
        consumed
    }
    fn dispatch(
        &mut self,
        target: &mut impl InputAreaTarget,
        frame: &PointerFrame<'_>,
        mouse_release: Option<MouseRelease>,
    ) -> bool {
        let now = frame.time_seconds;
        let mut structural_change = false;
        for event in frame.events {
            match event.kind {
                PointerKind::Down => {
                    if self.contacts.is_empty() {
                        let position = event.position.as_vec2();
                        self.start_time = now;
                        self.last_time = now;
                        self.origin = position;
                        self.previous = position;
                        self.last_delta = Vec2::ZERO;
                        self.tap = true;
                        self.pressed = false;
                        self.dragged = false;
                        if !target.probe(position * self.scale) {
                            continue;
                        }
                    } else {
                        self.tap = false;
                    }
                    self.contacts.push((event.id, event.position));
                    structural_change = true;
                }
                PointerKind::Move => {
                    if let Some(contact) = self.contacts.iter_mut().find(|c| c.0 == event.id) {
                        contact.1 = event.position;
                    }
                }
                PointerKind::Up | PointerKind::Cancel => {
                    let Some(index) = self.contacts.iter().position(|c| c.0 == event.id) else {
                        continue;
                    };
                    self.contacts.remove(index);
                    structural_change = true;
                    if self.contacts.is_empty() {
                        let desktop = mouse_release.filter(|_| event.kind == PointerKind::Up);
                        if let Some(release) = desktop {
                            if (release.position - self.origin).length()
                                > self.physical_width * 8.0 / 960.0
                            {
                                self.tap = false;
                                self.dragged = true;
                                if release.position != self.previous {
                                    target.drag(
                                        self.previous * self.scale,
                                        release.position * self.scale,
                                    );
                                    self.previous = release.position;
                                }
                            }
                        }
                        if event.kind == PointerKind::Cancel {
                            target.cancel();
                        } else if self.tap {
                            target.release(self.previous * self.scale);
                        } else if self.dragged && desktop.is_some() {
                            let release = desktop.unwrap();
                            target.mouse_gesture(
                                (release.position - self.origin) * self.scale,
                                release.velocity * self.scale,
                            );
                        } else if self.dragged && now > self.last_time {
                            // Native f64 division/multiplication followed by f32 conversion.
                            let factor = 0.016683333333333335 / (now - self.last_time);
                            let velocity = Vec2::new(
                                (self.last_delta.x as f64 * factor) as f32,
                                (self.last_delta.y as f64 * factor) as f32,
                            );
                            if velocity.length() > self.physical_width * 10.0 / 960.0 {
                                target.gesture(-velocity * self.scale);
                            }
                        }
                        target.reset();
                    }
                }
            }
        }
        if self.contacts.is_empty() {
            return structural_change;
        }
        let current = self
            .contacts
            .iter()
            .fold(Vec2::ZERO, |sum, c| sum + c.1.as_vec2())
            / self.contacts.len() as f32;
        if structural_change {
            self.previous = current;
            self.last_delta = Vec2::ZERO;
            self.last_time = now;
            return true;
        }
        let delta = self.previous - current;
        let drag = if self.tap {
            if (current - self.origin).length() > self.physical_width * 8.0 / 960.0 {
                self.tap = false;
                true
            } else {
                if !self.pressed && now - self.start_time >= 0.15 {
                    target.press(current * self.scale);
                    self.pressed = true;
                }
                if self.long_press_enabled && now - self.start_time >= 1.15 {
                    target.long_press(current * self.scale);
                    self.tap = false;
                }
                false
            }
        } else {
            delta.length() > 1.0
        };
        if drag {
            target.drag(self.previous * self.scale, current * self.scale);
            self.previous = current;
            self.dragged = true;
        }
        self.last_delta = delta;
        self.last_time = now;
        true
    }
}
impl InputAreaTarget for ManualModel {
    fn probe(&mut self, position: Vec2) -> bool {
        self.pointer_probe(position)
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
        self.pointer_drag(current.x - previous.x);
    }
    fn gesture(&mut self, velocity: Vec2) {
        self.pointer_gesture(velocity);
    }
    fn cancel(&mut self) {
        self.pointer_cancel();
    }
}
impl ModalInputTarget for ManualModel {
    fn accepts_pointer_input(&self) -> bool {
        self.phase() == ManualPhase::Active
    }
    fn controller_input(&mut self, state: &EnumMap<AppAction, ActionState>) {
        self.input(state);
    }
}
impl InputAreaTarget for SystemMenuModel {
    fn probe(&mut self, position: Vec2) -> bool {
        self.pointer_probe(position)
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
    fn drag(&mut self, _: Vec2, _: Vec2) {
        self.pointer_leave();
    }
    fn gesture(&mut self, velocity: Vec2) {
        self.pointer_gesture(velocity);
    }
    fn mouse_gesture(&mut self, displacement: Vec2, velocity: Vec2) {
        let candidate = desktop_gesture_vector(displacement, velocity);
        let angle = candidate.y.atan2(candidate.x) / f32::from_bits(0x40490fd8) * 180.0;
        self.pointer_gesture(
            if candidate.length() >= 64.0 && (angle > 150.0 || angle < -150.0) {
                candidate
            } else {
                velocity
            },
        );
    }
    fn cancel(&mut self) {
        self.pointer_leave();
    }
}
impl ModalInputTarget for SystemMenuModel {
    fn accepts_pointer_input(&self) -> bool {
        self.accepts_input()
    }
    fn controller_input(&mut self, state: &EnumMap<AppAction, ActionState>) {
        self.input(state);
    }
}

#[cfg(test)]
mod tests;
