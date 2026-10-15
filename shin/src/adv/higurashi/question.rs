//! Updated22168/23644 question layout, queued animation and contact ownership.
//! Glyph/atlas preparation and callback effects belong to the retained host.
pub mod view;
use crate::{
    adv::input_area::{InputArea, InputAreaTarget, PointerFrame},
    layer::higurashi_properties::HigurashiTweener,
};
use anyhow::{Result, ensure};
use glam::{Vec2, Vec4, vec2, vec4};
use shin_core::{
    layout::{HigurashiTextLayouter, LayoutParams, MessageTextLayouterDefaults, font::FontMetrics},
    time::Ticks,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cue {
    Move,
    Cancel,
    Confirm,
    NoSelection,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Callback {
    pub stage: u32,
    pub button: i32,
}
pub trait Services {
    fn cue(&mut self, cue: Cue);
    fn callback(&mut self, callback: Callback);
}
pub struct Button {
    pub rect: Vec4,
    pub text_size: Vec2,
    pub background: Vec4,
    pub fill: Vec4,
    pub outline: Vec4,
    pub focus: HigurashiTweener,
    pub flash: HigurashiTweener,
}
pub struct Question {
    area: InputArea,
    state: State,
}
struct State {
    phase: i32,
    selected: i32,
    cancellable: bool,
    cooldown: f32,
    dragging: bool,
    pointer: i32,
    velocity: Vec2,
    origin: Vec2,
    inner: Vec2,
    outer: Vec2,
    vertical: bool,
    scale: HigurashiTweener,
    opacity: HigurashiTweener,
    buttons: Vec<Button>,
}
fn control(t: &mut HigurashiTweener, value: f32, time: f32, flags: u32) {
    t.control(value, Ticks::from_f32(time), flags)
        .expect("Native question tween flags");
}
fn clear(t: &mut HigurashiTweener) {
    t.fast_forward_to(t.value());
}
fn focus(t: &mut HigurashiTweener, value: f32, speed: f32) {
    clear(t);
    control(t, value, speed, 0x40);
}
fn flash(t: &mut HigurashiTweener, released: bool) {
    clear(t);
    control(t, 0.7, 0.2, 0x40);
    if released {
        control(t, 0.0, 0.2, 0x40);
    }
}

/// SimpleText's outline subtracts four pixels from its1440-pixel layout width.
pub fn text_params(prompt: bool) -> LayoutParams {
    LayoutParams {
        layout_width: 1436.0,
        text_size: if prompt { 54.0 } else { 44.0 },
        line_padding_between: 3.0,
        line_padding_above: 4.0,
        line_padding_below: 4.0,
        perform_soft_breaks: prompt,
        ..Default::default()
    }
}
pub fn measure(font: impl FontMetrics + Clone, text: &str, prompt: bool) -> Vec2 {
    let layout = HigurashiTextLayouter::new(
        font,
        text_params(prompt),
        MessageTextLayouterDefaults {
            color: 0xfff,
            draw_speed: 50,
            fade: 200,
        },
    );
    let (_, _, size) = if prompt {
        layout.parse(text)
    } else {
        layout.parse_plain(text, true)
    };
    size + Vec2::splat(4.0)
}
impl Question {
    pub fn new(prompt: Vec2, answers: &[Vec2], selected: i32, cancellable: bool) -> Result<Self> {
        ensure!(
            answers.len() <= i32::MAX as usize,
            "Too many question buttons"
        );
        ensure!(
            std::iter::once(&prompt)
                .chain(answers)
                .all(|s| s.is_finite() && s.min_element() >= 0.0),
            "Invalid question text extent"
        );
        let mut inner = vec2(prompt.x.max(600.0), prompt.y);
        let mut rects: Vec<_> = answers
            .iter()
            .map(|s| vec4(0.0, 0.0, s.x + 80.0, s.y + 24.0))
            .collect();
        let mut vertical = false;
        if !rects.is_empty() {
            let width = rects.iter().map(|r| r.z.max(180.0)).fold(0.0, f32::max);
            let height = rects.iter().map(|r| r.w).fold(0.0, f32::max);
            let total = width * rects.len() as f32;
            let available = inner.x * 0.75;
            let mut y = prompt.y + 8.0;
            vertical = total > available;
            if vertical {
                let width = available.max(width);
                inner.x = inner.x.max(width);
                let x = (inner.x - width) * 0.5;
                for r in &mut rects {
                    r.x = x;
                    r.y = y;
                    r.z = width;
                    y += r.w + 8.0;
                }
                inner.y = y - 8.0;
            } else {
                let gap = if rects.len() > 1 {
                    ((available - total) / (rects.len() - 1) as f32).min(30.0)
                } else {
                    0.0
                };
                let mut x = (inner.x - (total + gap * (rects.len() - 1) as f32)) * 0.5;
                for r in &mut rects {
                    r.x = x;
                    r.y = y + (height - r.w) * 0.5;
                    r.z = width;
                    x += width + gap;
                }
                inner.y = y + height;
            }
        }
        let selected = if rects.is_empty() {
            -1
        } else if selected < 0 || selected as usize >= rects.len() {
            0
        } else {
            selected
        };
        let buttons = rects
            .into_iter()
            .enumerate()
            .map(|(i, rect)| Button {
                rect,
                text_size: answers[i],
                background: Vec4::ONE,
                fill: Vec4::ONE,
                outline: vec4(0.0, 0.0, 0.0, 240.0 * f32::from_bits(0x3b808081)),
                focus: HigurashiTweener::new(if i as i32 == selected { 1.0 } else { 0.0 }),
                flash: HigurashiTweener::new(0.0),
            })
            .collect();
        Ok(Self {
            area: InputArea::new(1280, 720),
            state: State {
                phase: 0,
                selected,
                cancellable,
                cooldown: 0.0,
                dragging: false,
                pointer: -1,
                velocity: Vec2::ZERO,
                origin: (vec2(1920.0, 1080.0) - inner) * 0.5 - Vec2::splat(30.0),
                inner,
                outer: inner + Vec2::splat(76.0),
                vertical,
                scale: HigurashiTweener::new(1.4),
                opacity: HigurashiTweener::new(0.0),
                buttons,
            },
        })
    }
    pub fn from_text(
        font: impl FontMetrics + Clone,
        prompt: &str,
        answers: &[&str],
        selected: i32,
        cancellable: bool,
    ) -> Result<Self> {
        let sizes: Vec<_> = answers
            .iter()
            .map(|s| measure(font.clone(), s, false))
            .collect();
        Self::new(measure(font, prompt, true), &sizes, selected, cancellable)
    }
    pub fn phase(&self) -> i32 {
        self.state.phase
    }
    pub fn complete(&self) -> bool {
        self.state.phase < 0
    }
    pub fn resize_input(&mut self, extent: (u16, u16), services: &mut impl Services) {
        self.area.disable(&mut Target { state: &mut self.state, services });
        self.area = InputArea::new(extent.0, extent.1);
    }
    /// Platform focus/scene boundaries cancel contact without polling clocks,
    /// confirming an answer or emitting a controller callback.
    pub fn cancel_input(&mut self, services: &mut impl Services) {
        self.area.disable(&mut Target { state: &mut self.state, services });
    }
    pub fn selected(&self) -> i32 {
        self.state.selected
    }
    pub fn origin(&self) -> Vec2 {
        self.state.origin
    }
    pub fn inner(&self) -> Vec2 {
        self.state.inner
    }
    pub fn outer(&self) -> Vec2 {
        self.state.outer
    }
    pub fn scale(&self) -> f32 {
        self.state.scale.value()
    }
    pub fn opacity(&self) -> f32 {
        self.state.opacity.value()
    }
    pub fn buttons(&self) -> &[Button] {
        &self.state.buttons
    }
    /// Predicate-driven notification dismissal uses button−2 and no cue.
    /// Physical contacts default to1280×720. Scene hosts can set their viewport
    /// with resize_input;InputArea then maps directly into the logical canvas.
    pub fn poll(
        &mut self,
        delta: Ticks,
        permission: u32,
        pressed: u32,
        repeated: u32,
        frame: &PointerFrame<'_>,
        dismiss: bool,
        services: &mut impl Services,
    ) -> u32 {
        let s = &mut self.state;
        if s.phase == 0 {
            control(&mut s.scale, 1.0, 12.0, 2);
            control(&mut s.opacity, 1.0, 12.0, 0);
            s.phase = 1;
        }
        if s.phase == 1 && !s.scale.is_waiting() && !s.opacity.is_waiting() {
            s.phase = 2;
        }
        if s.phase == 2 {
            if dismiss {
                if s.selected >= 0 {
                    focus(&mut s.buttons[s.selected as usize].focus, 0.0, 0.1);
                }
                s.selected = -2;
                s.phase = 3;
            } else {
                let mut target = Target { state: s, services };
                if permission & 1 == 0 || target.state.buttons.is_empty() {
                    self.area.disable(&mut target);
                } else if !self.area.update(&mut target, frame) {
                    target.controller(pressed, repeated);
                }
            }
        // Native switch arms return to ticking after input; closing begins on
        // the next poll even if the new feedback clock is already idle.
        } else if s.phase == 3 {
            let waiting = s.selected >= 0 && s.buttons[s.selected as usize].flash.is_waiting();
            if !waiting {
                control(&mut s.scale, 0.5, 12.0, 1);
                control(&mut s.opacity, 0.0, 12.0, 0);
                services.callback(Callback {
                    stage: 0,
                    button: s.selected,
                });
                s.cooldown = 5.0;
                s.phase = 4;
            }
        }
        if s.phase == 4 && !s.scale.is_waiting() && !s.opacity.is_waiting() {
            s.cooldown -= delta.as_f32();
            if s.cooldown <= 0.0 {
                services.callback(Callback {
                    stage: 1,
                    button: s.selected,
                });
                s.phase = -1;
            }
        }
        s.opacity.update(delta);
        s.scale.update(delta);
        for b in &mut s.buttons {
            b.focus.update(delta);
            b.flash.update(delta);
            let weight = b.focus.value();
            //4cc68 computes from+(to−from)*weight with separate operations.
            let lerp = |from: Vec4, to: Vec4| from + (to - from) * weight;
            b.fill = lerp(vec4(1.0, 1.0, 1.0, 0.7), Vec4::ONE);
            b.background = lerp(vec4(0.0, 0.0, 0.0, 0.5), Vec4::splat(0.7));
        }
        s.origin += s.velocity;
        s.velocity *= 0.9f32.powf(delta.as_f32());
        if !s.dragging {
            for (position, size, extent) in [
                (&mut s.origin.x, s.outer.x, 1920.0),
                (&mut s.origin.y, s.outer.y, 1080.0),
            ] {
                if *position < 0.0 {
                    *position *= 0.8;
                } else if *position + size > extent {
                    *position = ((*position + size - extent) * 0.8 + extent) - size;
                }
            }
        }
        0
    }
}
struct Target<'a, S> {
    state: &'a mut State,
    services: &'a mut S,
}
impl<S: Services> Target<'_, S> {
    fn select(&mut self, selected: i32) {
        let s = &mut self.state;
        if s.selected == selected {
            return;
        }
        if s.selected >= 0 {
            focus(&mut s.buttons[s.selected as usize].focus, 0.0, 0.1);
        }
        if selected >= 0 {
            focus(&mut s.buttons[selected as usize].focus, 1.0, 1.0);
        }
        s.selected = selected;
    }
    fn controller(&mut self, pressed: u32, repeated: u32) {
        if pressed & (1 << 24) != 0 {
            if self.state.selected < 0 {
                self.services.cue(Cue::NoSelection);
            } else {
                self.services.cue(Cue::Confirm);
                flash(
                    &mut self.state.buttons[self.state.selected as usize].flash,
                    true,
                );
                self.state.phase = 3;
            }
            return;
        }
        if pressed & (1 << 25) != 0 && self.state.cancellable {
            self.services.cue(Cue::Cancel);
            self.select(-1);
            self.state.phase = 3;
            return;
        }
        let (dec, inc, first, last, primary) = if self.state.vertical {
            (4, 6, 7, 5, 0x50)
        } else {
            (7, 5, 4, 6, 0xa0)
        };
        let old = self.state.selected;
        let count = self.state.buttons.len() as i32;
        let next = if repeated & (1 << dec) != 0 {
            if old < 1 { count - 1 } else { old - 1 }
        } else if repeated & (1 << inc) != 0 {
            if old + 1 >= count { 0 } else { old + 1 }
        } else if repeated & (1 << first) != 0 {
            if pressed & primary == 0 { 0 } else { old }
        } else if repeated & (1 << last) != 0 {
            if pressed & primary == 0 {
                count - 1
            } else {
                old
            }
        } else {
            old
        };
        if next != old {
            self.select(next);
            self.services.cue(Cue::Move);
        }
    }
    fn leave(&mut self) {
        if self.state.pointer >= 0 {
            focus(
                &mut self.state.buttons[self.state.pointer as usize].flash,
                0.0,
                0.2,
            );
        }
        self.state.pointer = -1;
    }
}
fn inside(point: Vec2, rect: Vec4) -> bool {
    point.x >= rect.x && point.y >= rect.y && point.x < rect.x + rect.z && point.y < rect.y + rect.w
}
/// Contact cancellation only changes local feedback; these services must never
/// acknowledge an answer or synthesize a sound at a platform boundary.
pub(crate) struct CancelOnly;
impl Services for CancelOnly {
    fn cue(&mut self, _: Cue) { unreachable!("Contact cancellation has no cue") }
    fn callback(&mut self, _: Callback) { unreachable!("Contact cancellation has no answer") }
}
impl<S: Services> InputAreaTarget for Target<'_, S> {
    fn probe(&mut self, position: Vec2) -> bool {
        let s = &mut self.state;
        s.dragging = false;
        let local = position - (s.origin + vec2(30.0, 46.0));
        s.pointer = s
            .buttons
            .iter()
            .position(|b| inside(local, b.rect))
            .map_or(-1, |i| i as i32);
        if s.pointer < 0 && inside(position, vec4(s.origin.x, s.origin.y, s.outer.x, s.outer.y)) {
            s.dragging = true;
            s.velocity = Vec2::ZERO;
        }
        true
    }
    fn reset(&mut self) {
        self.state.pointer = -1;
        self.state.dragging = false;
    }
    fn press(&mut self, _: Vec2) {
        let p = self.state.pointer;
        if p >= 0 {
            flash(&mut self.state.buttons[p as usize].flash, false);
            self.select(p);
        }
    }
    fn release(&mut self, _: Vec2) {
        let p = self.state.pointer;
        if p >= 0 {
            self.services.cue(Cue::Confirm);
            self.select(p);
            flash(&mut self.state.buttons[p as usize].flash, true);
            self.state.phase = 3;
        }
    }
    fn long_press(&mut self, _: Vec2) {}
    fn drag(&mut self, previous: Vec2, current: Vec2) {
        self.leave();
        if self.state.dragging {
            self.state.origin += current - previous;
        }
    }
    fn gesture(&mut self, velocity: Vec2) {
        if self.state.dragging {
            self.state.velocity = velocity;
        }
    }
    fn cancel(&mut self) {
        self.leave();
    }
}

#[cfg(test)]
mod tests;
