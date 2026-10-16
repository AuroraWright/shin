//! Desktop contacts enter the same InputArea callbacks as native browser input.
use super::{Resources, model, view};
use crate::adv::input_area::{InputAreaTarget, desktop_gesture_vector};
use glam::Vec2;
use view::pointer::{Context, Event, Point, Pointer};
pub(super) struct Contact<'a> {
    pointer: &'a mut Pointer,
    controls: &'a model::Controller,
    resources: &'a Resources,
    events: &'a mut Vec<Event>,
    rows: Vec<view::Row>,
}
impl<'a> Contact<'a> {
    pub fn new(
        pointer: &'a mut Pointer,
        controls: &'a model::Controller,
        resources: &'a Resources,
        events: &'a mut Vec<Event>,
    ) -> Self {
        Self {
            pointer,
            controls,
            resources,
            events,
            rows: resources.rows(&[]),
        }
    }
}
fn point(p: Vec2) -> Point {
    Point { x: p.x, y: p.y }
}
impl InputAreaTarget for Contact<'_> {
    fn probe(&mut self, p: Vec2) -> bool {
        if !p.is_finite() || p.min_element() < 0. || p.x >= 1920. || p.y >= 1080. {
            return false;
        }
        let ctx = Context {
            rows: &self.rows,
            selected: self.controls.selected(),
            scroll: self.controls.scroll(),
            minimum: self.resources.geometry.minimum,
            bottom: self.resources.geometry.bottom,
        };
        self.pointer.hover(point(p), &ctx).captured
    }
    fn reset(&mut self) {
        self.pointer.cancel();
    }
    fn press(&mut self, p: Vec2) {
        let ctx = Context {
            rows: &self.rows,
            selected: self.controls.selected(),
            scroll: self.controls.scroll(),
            minimum: self.resources.geometry.minimum,
            bottom: self.resources.geometry.bottom,
        };
        self.events.extend(self.pointer.press(point(p), &ctx));
    }
    fn release(&mut self, p: Vec2) {
        let ctx = Context {
            rows: &self.rows,
            selected: self.controls.selected(),
            scroll: self.controls.scroll(),
            minimum: self.resources.geometry.minimum,
            bottom: self.resources.geometry.bottom,
        };
        self.events.extend(self.pointer.release(point(p), &ctx));
    }
    fn long_press(&mut self, _: Vec2) {}
    fn drag(&mut self, previous: Vec2, current: Vec2) {
        let ctx = Context {
            rows: &self.rows,
            selected: self.controls.selected(),
            scroll: self.controls.scroll(),
            minimum: self.resources.geometry.minimum,
            bottom: self.resources.geometry.bottom,
        };
        self.events
            .extend(self.pointer.drag(point(previous), point(current), &ctx));
    }
    fn gesture(&mut self, v: Vec2) {
        self.events.extend(self.pointer.fling(point(v)));
    }
    fn mouse_gesture(&mut self, displacement: Vec2, velocity: Vec2) {
        self.gesture(desktop_gesture_vector(displacement, velocity));
    }
    fn cancel(&mut self) {
        if let Some(row) = self.pointer.row() {
            self.events.push(Event::VoiceFlash { row, value: 0. });
        }
        self.events.push(Event::CloseFlash { value: 0. });
        self.pointer.cancel();
    }
}
