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
pub(super) struct TapInput {
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
mod tests {
    #[test]
    fn tap_input_matches_ripple_contact_rules_and_cancels_inherited_holds() {
        for width in [960u16, 1920, 3840] {
            let extent = (width, width * 9 / 16);
            let scale = width as f32 / 1920.0;
            let start = Vec2::new(800.0, 500.0) * scale;
            let mut sampler = PointerSampler::default();
            let mut tap = TapInput::default();
            let sample = |tap: &mut TapInput, sampler: &mut PointerSampler, p, held, seconds| {
                sampler.sample(p, held, seconds);
                tap.update(sampler, extent)
            };
            assert!(!sample(&mut tap, &mut sampler, start, true, 0.016));
            assert!(
                !sample(&mut tap, &mut sampler, start, true, 1.5),
                "Holding never taps, including long hold"
            );
            assert!(sample(&mut tap, &mut sampler, start, false, 0.016));
            assert!(
                !tap.update(&sampler, extent),
                "Same release frame cannot tap twice"
            );
            assert!(!sample(&mut tap, &mut sampler, start, true, 0.016));
            assert!(!sample(
                &mut tap,
                &mut sampler,
                start + Vec2::new(20.0, 0.0) * scale,
                true,
                0.016
            ));
            assert!(!sample(&mut tap, &mut sampler, start, true, 0.016));
            assert!(
                !sample(&mut tap, &mut sampler, start, false, 0.016),
                "Returning a dragged pointer cannot tap"
            );
            assert!(!sample(&mut tap, &mut sampler, start, true, 0.016));
            sampler.cancel_contact(true);
            assert!(
                !sample(&mut tap, &mut sampler, start, false, 0.016),
                "Focus cancel never taps"
            );
            assert!(!sample(&mut tap, &mut sampler, start, true, 0.016));
            tap.cancel();
            assert!(
                !sample(&mut tap, &mut sampler, start, false, 0.016),
                "Inherited held release never taps"
            );
            assert!(!sample(
                &mut tap,
                &mut sampler,
                Vec2::new(-32.0, 100.0),
                true,
                0.016
            ));
            assert!(
                !sample(&mut tap, &mut sampler, start, false, 0.016),
                "Out-of-canvas press never taps"
            );
        }
    }

    #[test]
    fn desktop_notice_cancels_capture_without_releasing_or_restarting_held_contact() {
        use super::*;
        let mut sampler = PointerSampler::default();
        sampler.sample(Vec2::new(100.0, 100.0), true, 0.0);
        assert!(matches!(sampler.frame().events[0].kind, PointerKind::Down));
        sampler.cancel_contact(true);
        sampler.sample(Vec2::new(200.0, 100.0), true, 0.1);
        assert_eq!(sampler.frame().events.len(), 1);
        assert!(matches!(
            sampler.frame().events[0].kind,
            PointerKind::Cancel
        ));
        sampler.sample(Vec2::ZERO, true, 0.1);
        assert!(sampler.frame().events.is_empty());
        sampler.sample(Vec2::ZERO, false, 0.1);
        assert!(sampler.frame().events.is_empty());
        sampler.sample(Vec2::ZERO, true, 0.1);
        assert!(matches!(sampler.frame().events[0].kind, PointerKind::Down));
    }

    use super::*;
    use glam::ivec2;
    #[derive(Debug, PartialEq)]
    enum Call {
        Probe(Vec2),
        Press(Vec2),
        Release(Vec2),
        Long(Vec2),
        Drag(Vec2, Vec2),
        Gesture(Vec2),
        Cancel,
        Reset,
    }
    #[derive(Default)]
    struct Recorder {
        reject: bool,
        calls: Vec<Call>,
    }
    impl InputAreaTarget for Recorder {
        fn probe(&mut self, p: Vec2) -> bool {
            self.calls.push(Call::Probe(p));
            !self.reject
        }
        fn reset(&mut self) {
            self.calls.push(Call::Reset);
        }
        fn press(&mut self, p: Vec2) {
            self.calls.push(Call::Press(p));
        }
        fn release(&mut self, p: Vec2) {
            self.calls.push(Call::Release(p));
        }
        fn long_press(&mut self, p: Vec2) {
            self.calls.push(Call::Long(p));
        }
        fn drag(&mut self, p: Vec2, q: Vec2) {
            self.calls.push(Call::Drag(p, q));
        }
        fn gesture(&mut self, p: Vec2) {
            self.calls.push(Call::Gesture(p));
        }
        fn cancel(&mut self) {
            self.calls.push(Call::Cancel);
        }
    }
    fn event(kind: PointerKind, id: i32, x: i32, y: i32) -> PointerEvent {
        PointerEvent {
            kind,
            id,
            position: ivec2(x, y),
        }
    }
    fn step(
        area: &mut InputArea,
        target: &mut impl InputAreaTarget,
        number: u32,
        time_seconds: f64,
        events: &[PointerEvent],
    ) -> bool {
        area.update(
            target,
            &PointerFrame {
                number,
                time_seconds,
                events,
            },
        )
    }
    #[test]
    fn delayed_press_frame_cache_and_release_keep_first_capture() {
        let mut a = InputArea::new(960, 540);
        let mut r = Recorder::default();
        assert!(step(
            &mut a,
            &mut r,
            0,
            0.0,
            &[event(PointerKind::Down, 4, 100, 50)]
        ));
        assert_eq!(r.calls, [Call::Probe(Vec2::new(200.0, 100.0))]);
        // Same frame ignores new events/time and never invokes callbacks twice.
        assert!(step(
            &mut a,
            &mut r,
            0,
            0.5,
            &[event(PointerKind::Cancel, 4, 0, 0)]
        ));
        assert!(step(&mut a, &mut r, 1, 0.149, &[]));
        assert_eq!(r.calls.len(), 1);
        assert!(step(
            &mut a,
            &mut r,
            2,
            0.15,
            &[event(PointerKind::Move, 4, 108, 50)]
        ));
        assert_eq!(r.calls[1], Call::Press(Vec2::new(216.0, 100.0)));
        assert!(step(
            &mut a,
            &mut r,
            3,
            0.2,
            &[event(PointerKind::Up, 4, 900, 400)]
        ));
        // Up coordinates and sub-threshold movement do not change release baseline.
        assert_eq!(
            r.calls[2..],
            [Call::Release(Vec2::new(200.0, 100.0)), Call::Reset]
        );
        assert!(!step(&mut a, &mut r, 4, 0.3, &[]));
    }
    #[test]
    fn quick_tap_releases_without_press_and_rejected_or_unknown_contacts_pass_through() {
        let mut a = InputArea::canvas();
        let mut r = Recorder::default();
        assert!(step(
            &mut a,
            &mut r,
            1,
            0.0,
            &[
                event(PointerKind::Down, 1, 20, 30),
                event(PointerKind::Up, 1, 90, 90)
            ]
        ));
        assert_eq!(
            r.calls,
            [
                Call::Probe(Vec2::new(20.0, 30.0)),
                Call::Release(Vec2::new(20.0, 30.0)),
                Call::Reset
            ]
        );
        r.reject = true;
        assert!(!step(
            &mut a,
            &mut r,
            2,
            0.1,
            &[event(PointerKind::Down, 1, 20, 30)]
        ));
        assert!(!step(
            &mut a,
            &mut r,
            3,
            0.2,
            &[
                event(PointerKind::Move, 1, 80, 80),
                event(PointerKind::Up, 1, 80, 80),
                event(PointerKind::Cancel, 99, 0, 0)
            ]
        ));
        assert_eq!(r.calls.len(), 4);
    }
    #[test]
    fn drag_threshold_endpoints_velocity_and_stationary_release() {
        for stationary in [false, true] {
            let mut a = InputArea::new(960, 540);
            let mut r = Recorder::default();
            step(
                &mut a,
                &mut r,
                0,
                0.0,
                &[event(PointerKind::Down, 1, 500, 200)],
            );
            step(
                &mut a,
                &mut r,
                1,
                0.01,
                &[event(PointerKind::Move, 1, 492, 200)],
            );
            assert_eq!(r.calls.len(), 1); // Exactly8px still a tap.
            step(
                &mut a,
                &mut r,
                2,
                0.02,
                &[event(PointerKind::Move, 1, 491, 200)],
            );
            assert_eq!(
                r.calls[1],
                Call::Drag(Vec2::new(1000.0, 400.0), Vec2::new(982.0, 400.0))
            );
            step(
                &mut a,
                &mut r,
                3,
                0.03,
                &[event(PointerKind::Move, 1, 441, 200)],
            );
            assert_eq!(
                r.calls[2],
                Call::Drag(Vec2::new(982.0, 400.0), Vec2::new(882.0, 400.0))
            );
            if stationary {
                step(&mut a, &mut r, 4, 0.04, &[]);
            }
            step(&mut a, &mut r, 5, 0.05, &[event(PointerKind::Up, 1, 0, 0)]);
            if stationary {
                assert_eq!(r.calls[3..], [Call::Reset]);
            } else {
                let Call::Gesture(v) = r.calls[3] else {
                    panic!("missing gesture: {:?}", r.calls);
                };
                assert!((v.x + 83.416666).abs() < 0.00002 && v.y == 0.0);
                assert_eq!(r.calls[4], Call::Reset);
            }
        }
    }
    #[test]
    fn extra_contacts_disable_tap_and_reset_centroid_before_motion() {
        let mut a = InputArea::canvas();
        let mut r = Recorder::default();
        step(
            &mut a,
            &mut r,
            0,
            0.0,
            &[
                event(PointerKind::Down, 1, 100, 200),
                event(PointerKind::Down, 2, 300, 200),
            ],
        );
        assert_eq!(r.calls, [Call::Probe(Vec2::new(100.0, 200.0))]);
        step(
            &mut a,
            &mut r,
            1,
            0.2,
            &[event(PointerKind::Move, 1, 102, 200)],
        );
        assert_eq!(r.calls.len(), 1); // Centroid moves exactly1px; no drag.
        step(
            &mut a,
            &mut r,
            2,
            0.3,
            &[event(PointerKind::Move, 2, 304, 200)],
        );
        assert_eq!(
            r.calls[1],
            Call::Drag(Vec2::new(200.0, 200.0), Vec2::new(203.0, 200.0))
        );
        step(
            &mut a,
            &mut r,
            3,
            0.4,
            &[event(PointerKind::Cancel, 1, 0, 0)],
        );
        assert_eq!(r.calls.len(), 2); // Removing one contact is not owner cancellation.
        step(&mut a, &mut r, 4, 0.5, &[event(PointerKind::Up, 2, 0, 0)]);
        assert_eq!(r.calls[2..], [Call::Reset]); // Structural change cleared swipe delta.
    }
    #[test]
    fn cancel_and_disable_order_and_optional_long_press() {
        let mut a = InputArea::canvas();
        let mut r = Recorder::default();
        a.set_long_press_enabled(true);
        step(
            &mut a,
            &mut r,
            0,
            0.0,
            &[event(PointerKind::Down, 1, 100, 200)],
        );
        step(&mut a, &mut r, 1, 1.149, &[]);
        assert_eq!(r.calls.len(), 2);
        step(&mut a, &mut r, 2, 1.15, &[]);
        assert_eq!(r.calls[2], Call::Long(Vec2::new(100.0, 200.0)));
        step(&mut a, &mut r, 3, 1.2, &[event(PointerKind::Up, 1, 0, 0)]);
        assert_eq!(r.calls[3..], [Call::Reset]); // Long press suppresses tap release.
        step(
            &mut a,
            &mut r,
            4,
            1.3,
            &[event(PointerKind::Down, 2, 100, 200)],
        );
        step(
            &mut a,
            &mut r,
            5,
            1.4,
            &[event(PointerKind::Cancel, 2, 0, 0)],
        );
        assert_eq!(r.calls[5..], [Call::Cancel, Call::Reset]);
        step(
            &mut a,
            &mut r,
            6,
            1.5,
            &[event(PointerKind::Down, 3, 100, 200)],
        );
        assert!(a.disable(&mut r));
        assert!(!a.disable(&mut r));
        assert_eq!(r.calls[8..], [Call::Cancel, Call::Reset]);
    }
    fn controls() -> EnumMap<AppAction, ActionState> {
        EnumMap::default()
    }
    #[test]
    fn manual_dispatch_blocks_cancel_during_capture_and_swipes_silently() {
        let mut m = ManualModel::new(24).unwrap();
        m.update(10.0);
        let mut a = InputArea::canvas();
        let mut input = controls();
        input[AppAction::Cancel].is_clicked = true;
        a.input_frame(
            &mut m,
            &PointerFrame {
                number: 0,
                time_seconds: 0.0,
                events: &[event(PointerKind::Down, 1, 1000, 500)],
            },
            &input,
        );
        assert_eq!(m.phase(), ManualPhase::Active);
        a.input_frame(
            &mut m,
            &PointerFrame {
                number: 1,
                time_seconds: 0.1,
                events: &[event(PointerKind::Move, 1, 200, 500)],
            },
            &input,
        );
        assert_eq!(m.scroll(), 800.0);
        a.input_frame(
            &mut m,
            &PointerFrame {
                number: 2,
                time_seconds: 0.11668333333333334,
                events: &[event(PointerKind::Up, 1, 1800, 80)],
            },
            &input,
        );
        assert_eq!(m.selected(), 1);
        assert_eq!(m.phase(), ManualPhase::Active);
        assert!(m.take_sounds().is_empty());
        a.input_frame(
            &mut m,
            &PointerFrame {
                number: 3,
                time_seconds: 0.2,
                events: &[],
            },
            &input,
        );
        assert_eq!(m.phase(), ManualPhase::Active); // Controller still gated by page tween.
        m.update(20.0);
        a.input_frame(
            &mut m,
            &PointerFrame {
                number: 4,
                time_seconds: 0.3,
                events: &[],
            },
            &input,
        );
        assert_eq!(m.phase(), ManualPhase::CloseFlash);
        assert_eq!(m.take_sounds(), ["cancel"]);
    }
    #[test]
    fn menu_dispatch_selects_original_capture_and_drag_cancels_confirmation() {
        for drag in [false, true] {
            let mut m = SystemMenuModel::new(0, true);
            m.update(22.0);
            m.update(0.0);
            let mut a = InputArea::canvas();
            let mut input = controls();
            input[AppAction::Cancel].is_clicked = true;
            a.input_frame(
                &mut m,
                &PointerFrame {
                    number: 0,
                    time_seconds: 0.0,
                    events: &[event(PointerKind::Down, 1, 30, 72 + 5 * 128)],
                },
                &input,
            );
            a.input_frame(
                &mut m,
                &PointerFrame {
                    number: 1,
                    time_seconds: 0.15,
                    events: &[],
                },
                &input,
            );
            assert_eq!(m.selected(), 5);
            if drag {
                a.input_frame(
                    &mut m,
                    &PointerFrame {
                        number: 2,
                        time_seconds: 0.2,
                        events: &[event(PointerKind::Move, 1, 200, 72 + 5 * 128)],
                    },
                    &input,
                );
            }
            a.input_frame(
                &mut m,
                &PointerFrame {
                    number: 3,
                    time_seconds: 0.4,
                    events: &[event(PointerKind::Up, 1, 30, 80)],
                },
                &input,
            );
            m.update(7.0);
            m.update(0.0);
            assert_eq!(m.take_item(), if drag { None } else { Some(5) });
            assert_eq!(m.take_sounds(), if drag { vec![] } else { vec!["decide"] });
        }
    }

    #[test]
    fn desktop_sampler_keeps_physical_contacts_and_monotonic_time_before_modal_open() {
        let mut sampler = PointerSampler::default();
        sampler.sample(Vec2::new(20.9, 30.7), true, 0.1);
        assert_eq!(sampler.frame().events[0].kind, PointerKind::Down);
        assert_eq!(sampler.frame().events[0].position, IVec2::new(20, 30));
        // Opening a fresh area under an already held pointer cannot capture it.
        sampler.sample(Vec2::new(21.0, 30.0), true, 0.2);
        let mut area = InputArea::new(960, 540);
        let mut r = Recorder::default();
        assert!(!area.update(&mut r, &sampler.frame()));
        assert_eq!(sampler.frame().events[0].kind, PointerKind::Move);
        sampler.sample(Vec2::new(21.0, 30.0), false, 0.1);
        assert!(!area.update(&mut r, &sampler.frame()));
        sampler.sample(Vec2::new(21.0, 30.0), true, 0.1);
        assert!(area.update(&mut r, &sampler.frame()));
        assert_eq!(r.calls, [Call::Probe(Vec2::new(42.0, 60.0))]);
        sampler.sample(Vec2::new(21.0, 30.0), true, 0.1);
        assert!(sampler.frame().events.is_empty());
        assert!((sampler.frame().time_seconds - 0.6).abs() < 1e-12);
    }
    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_input_area_contacts_timing_centroid_swipe_and_priority() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0x6b27c, 0x51000508),
            (0x6b280, 0x71000d1f),
            (0x6b298, 0x10000089),
            (0x6b29c, 0x3868696a),
            (0x6b2a0, 0x8b0a0929),
            (0x6b2b4, 0x3900e27f),
            (0x6b3b4, 0x2900d11c),
            (0x6b3f8, 0x6d042669),
            (0x6b3fc, 0x79007268),
            (0x6b404, 0x2d0b0a61),
            (0x6b408, 0x2d0a0a61),
            (0x6b418, 0xf9400908),
            (0x6b41c, 0x1e210800),
            (0x6b420, 0x1e220861),
            (0x6b43c, 0x36000ee0),
            (0x6b580, 0x3940e268),
            (0x6b590, 0xf9401508),
            (0x6b594, 0x2d4b0660),
            (0x6b59c, 0x1e220800),
            (0x6b5a0, 0x1e230821),
            (0x6b1a0, 0xf9402508),
            (0x6b1b4, 0xf9400d08),
            (0x6b1d4, 0x3940ea68),
            (0x6b1e0, 0x1e603920),
            (0x6b1e8, 0x54001e2d),
            (0x6b1ec, 0x1e601940),
            (0x6b1f4, 0x1e22c021),
            (0x6b1fc, 0x1e62402c),
            (0x6b230, 0x12003c08),
            (0x6b234, 0x0b202908),
            (0x6b238, 0x531f7908),
            (0x6b248, 0x54001b2d),
            (0x6b254, 0xf9402108),
            (0x6b25c, 0x1e2c8800),
            (0x6b260, 0x1e2d8821),
            (0x6b6c4, 0x1e22182d),
            (0x6b6c8, 0x1e22180c),
            (0x6b6d0, 0x2d0b326d),
            (0x6b6d4, 0xf900327f),
            (0x6b6dc, 0xfd002669),
            (0x6b6f0, 0x1e2c382a),
            (0x6b6f4, 0x1e2d380b),
            (0x6b734, 0x531d7108),
            (0x6b744, 0x1e211800),
            (0x6b74c, 0x540001ad),
            (0x6b750, 0x3900e27f),
            (0x6b778, 0x5400056c),
            (0x6b790, 0xfd464901),
            (0x6b79c, 0x5400016b),
            (0x6b7a8, 0xf9401108),
            (0x6b7c4, 0x3900e668),
            (0x6b7cc, 0x34000488),
            (0x6b7d8, 0xfd46a501),
            (0x6b7e4, 0x540003cb),
            (0x6b7f0, 0xf9401908),
            (0x6b808, 0x3900e27f),
            (0x6b82c, 0xf9401d08),
            (0x6b830, 0x2d4b0660),
            (0x6b840, 0x1e2209a2),
            (0x6b844, 0x1e230983),
            (0x6b854, 0x2d0b326d),
            (0x6b858, 0x3900ea68),
            (0x6b85c, 0x2d0c2a6b),
            (0x6b864, 0xfd002669),
            (0x6b924, 0x350000c8),
            (0x6b92c, 0x6b14011f),
            (0x6b930, 0x54000061),
            (0x6b934, 0x39404668),
            (0x6b948, 0x39004668),
            (0x6b950, 0xb9001674),
            (0x1d2d8, 0x9401398a),
            (0x1d2dc, 0x3707ede0),
            (0x86698, 0x97ff949a),
            (0x8669c, 0x37000220),
            (0x866a8, 0x9400d4d2),
            (0x866ac, 0x370001a0),
            (0x866dc, 0x97ff9271),
            (0x86b94, 0x1e293901),
            (0x86b98, 0x1e213800),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "instruction at{address:x}"
            );
        }
        assert_eq!(&exe[0x16fb04 + 256..0x16fb08 + 256], &[0, 55, 17, 36]);
        for (address, expected) in [
            (0x164c90, 0.15_f64),
            (0x164d48, 1.15),
            (0x164d08, 0.016683333333333335),
        ] {
            assert_eq!(
                f64::from_le_bytes(exe[address + 256..address + 264].try_into().unwrap()),
                expected
            );
        }
        // Manual and menu vtable+10..48: probe/reset/press/release/no-op/drag/gesture/cancel.
        for (slot, expected) in [
            (0x1d8700, 0x86840_i64),
            (0x1d8708, 0x86960),
            (0x1d8710, 0x86a30),
            (0x1d8718, 0x86ac0),
            (0x1d8720, 0x86b20),
            (0x1d8728, 0x86be0),
            (0x1d8730, 0x86dd0),
            (0x1d8738, 0x86e30),
            (0x1d2628, 0x1e7b0),
            (0x1d2630, 0x1e7f0),
            (0x1d2638, 0x1e870),
            (0x1d2640, 0x1e950),
            (0x1d2648, 0x1e9d0),
            (0x1d2650, 0x1ea40),
            (0x1d2658, 0x1eb80),
            (0x1d2660, 0x1ecd0),
        ] {
            let value = (0x11e040..0x152eb8).step_by(24).find_map(|offset| {
                let address =
                    u64::from_le_bytes(exe[offset + 256..offset + 264].try_into().unwrap());
                (address == slot).then(|| {
                    i64::from_le_bytes(exe[offset + 272..offset + 280].try_into().unwrap())
                })
            });
            assert_eq!(value, Some(expected));
        }
    }
}
