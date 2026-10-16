//! Updated e3258/e3bec/e4260 title QuizMenu. All eight choices start authored
//! ADV scenarios5..12; the selected scenario's script supplies QUIZ operands.
use crate::{
    adv::{
        character_view::CharacterTextures,
        higurashi::input::Buttons,
        input_area::{InputArea, InputAreaTarget, PointerFrame},
    },
    app::higurashi::kakera_menu::view::Packet,
    layer::higurashi_properties::HigurashiTweener,
};
use anyhow::{Result, ensure};
use glam::{Mat4, Vec2};
use shin_core::time::Ticks;
use shin_render::render_pass::RenderPass;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Destination {
    Top,
    Story(i32),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Cue(&'static str),
    Destination(Destination),
}
struct Row {
    id: i32,
    opacity: HigurashiTweener,
    flash: HigurashiTweener,
}
struct State {
    phase: i32,
    column: i32,
    line: i32,
    pointer: i32,
    cancelled: bool,
    rows: Vec<Row>,
    actions: Vec<Action>,
}
fn control(clock: &mut HigurashiTweener, value: f32, ticks: f32, flags: u32) {
    clock
        .control(value, Ticks::from_f32(ticks), flags)
        .expect("Native quiz menu tween");
}
fn rate(clock: &mut HigurashiTweener, value: f32) {
    clock.fast_forward_to(clock.value());
    control(clock, value, f32::from_bits(0x3e4ccccd), 0x40);
}
impl State {
    fn new() -> Self {
        Self {
            phase: 0,
            column: 0,
            line: 0,
            pointer: -1,
            cancelled: false,
            rows: (0..8)
                .map(|id| Row {
                    id,
                    opacity: HigurashiTweener::new(0.0),
                    flash: HigurashiTweener::new(0.0),
                })
                .collect(),
            actions: Vec::new(),
        }
    }
    fn selected(&self) -> usize {
        (self.column + 2 * self.line) as usize
    }
    fn prepare(&mut self) -> bool {
        if self.phase == 0 {
            for (i, row) in self.rows.iter_mut().enumerate() {
                control(&mut row.opacity, 0.0, (i / 2) as f32 * 6.0, 0x80);
                control(&mut row.opacity, 1.0, 30.0, 0);
            }
            self.phase = 1;
        }
        if self.phase == 1 && self.rows.iter().all(|r| r.opacity.is_idle()) {
            self.phase = 2;
        }
        if self.phase == 3 && self.rows.iter().all(|r| r.opacity.is_idle()) {
            self.actions.push(Action::Destination(if self.cancelled {
                Destination::Top
            } else {
                Destination::Story(5 + self.column + 2 * self.line)
            }));
            self.phase = -1;
        }
        self.phase == 2
    }
    fn confirm(&mut self) {
        if self.phase != 2 {
            return;
        }
        let selected = self.selected();
        rate(&mut self.rows[selected].flash, 0.7);
        control(
            &mut self.rows[selected].flash,
            0.0,
            f32::from_bits(0x3e4ccccd),
            0x40,
        );
        self.actions.push(Action::Cue("quiz_call"));
        for (i, row) in self.rows.iter_mut().enumerate() {
            if i == selected {
                control(&mut row.opacity, 0.0, 60.0, 0x80);
                control(&mut row.opacity, 0.0, 40.0, 0);
            } else {
                control(&mut row.opacity, 0.0, (i / 2) as f32 * 6.0, 0x80);
                control(&mut row.opacity, 0.0, 30.0, 0);
            }
        }
        self.phase = 3;
    }
    fn retreat(&mut self) {
        if self.phase != 2 {
            return;
        }
        self.cancelled = true;
        self.actions.push(Action::Cue("cancel"));
        for (i, row) in self.rows.iter_mut().enumerate() {
            control(&mut row.opacity, 0.0, (i / 2) as f32 * 6.0, 0x80);
            control(&mut row.opacity, 0.0, 30.0, 0);
        }
        self.phase = 3;
    }
    fn poll_controls(&mut self, captured: bool, buttons: Buttons) {
        if captured || self.phase != 2 {
            return;
        }
        if buttons.pressed & (1 << 24) != 0 {
            self.confirm();
        } else if buttons.pressed & (1 << 25) != 0 {
            self.retreat();
        } else {
            let old = (self.column, self.line);
            if buttons.repeat & (1 << 7) != 0 {
                self.column = if self.column <= 0 { 1 } else { self.column - 1 };
            } else if buttons.repeat & (1 << 5) != 0 {
                self.column = if self.column >= 1 { 0 } else { self.column + 1 };
            }
            if buttons.repeat & (1 << 6) != 0 {
                self.line = if self.line + 1 >= 4 { 0 } else { self.line + 1 };
            } else if buttons.repeat & (1 << 4) != 0 {
                self.line = if self.line <= 0 { 3 } else { self.line - 1 };
            }
            if old != (self.column, self.line) {
                self.actions.push(Action::Cue("move"));
            }
        }
    }
    fn advance(&mut self, delta: Ticks) {
        for row in &mut self.rows {
            row.opacity.update(delta);
            row.flash.update(delta);
        }
    }
    fn row_at(&self, point: Vec2) -> i32 {
        self.rows
            .iter()
            .enumerate()
            .position(|(i, _)| {
                let x = 110.0 + (i % 2) as f32 * 456.0;
                let y = 420.0 + (i / 2) as f32 * 130.0;
                point.x >= x && point.x < x + 440.0 && point.y >= y && point.y < y + 130.0
            })
            .map_or(-1, |i| i as i32)
    }
    fn select_pointer(&mut self) {
        self.column = self.pointer & 1;
        self.line = self.pointer / 2;
    }
}
impl InputAreaTarget for State {
    fn probe(&mut self, point: Vec2) -> bool {
        self.pointer = self.row_at(point);
        true
    }
    fn reset(&mut self) {
        self.pointer = -1;
    }
    fn press(&mut self, _: Vec2) {
        if self.pointer >= 0 {
            rate(&mut self.rows[self.pointer as usize].flash, 0.7);
            self.select_pointer();
        }
    }
    fn release(&mut self, _: Vec2) {
        if self.pointer >= 0 {
            self.select_pointer();
            self.confirm();
        } else {
            self.retreat();
        }
    }
    fn cancel(&mut self) {
        if self.pointer >= 0 {
            rate(&mut self.rows[self.pointer as usize].flash, 0.0);
            self.pointer = -1;
        }
    }
    fn drag(&mut self, _: Vec2, _: Vec2) {
        self.cancel();
    }
    fn gesture(&mut self, _: Vec2) {}
    fn long_press(&mut self, _: Vec2) {}
}
pub(super) struct QuizMenu {
    state: State,
    area: InputArea,
}
impl QuizMenu {
    pub fn new(extent: (u16, u16)) -> Self {
        Self {
            state: State::new(),
            area: InputArea::new(extent.0, extent.1),
        }
    }
    pub fn complete(&self) -> bool {
        self.state.phase < 0
    }
    #[cfg(test)]
    pub(super) fn reference_state(&self) -> (i32, usize) {
        (self.state.phase, self.state.selected())
    }
    pub fn resize(&mut self, extent: (u16, u16)) {
        self.area.disable(&mut self.state);
        self.area = InputArea::new(extent.0, extent.1);
    }
    pub fn cancel_input(&mut self) {
        self.area.disable(&mut self.state);
    }
    pub fn poll(
        &mut self,
        delta: Ticks,
        permission: bool,
        buttons: Buttons,
        pointer: &PointerFrame<'_>,
    ) -> Result<Vec<Action>> {
        ensure!(
            delta.as_f32().is_finite() && delta.as_f32() >= 0.0,
            "Invalid quiz title-menu delta"
        );
        if self.state.prepare() {
            if permission {
                let captured = self.area.update(&mut self.state, pointer);
                self.state.poll_controls(captured, buttons);
            } else {
                self.area.disable(&mut self.state);
            }
        }
        self.state.advance(delta);
        Ok(std::mem::take(&mut self.state.actions))
    }
    fn packets(&self, plus: bool) -> Vec<Packet> {
        let matrix = Mat4::from_cols_array(&[
            2.0 / 1920.0,
            0.0,
            0.0,
            0.0,
            0.0,
            -2.0 / 1080.0,
            0.0,
            0.0,
            0.0,
            0.0,
            -1.0,
            0.0,
            -1.0,
            1.0,
            -0.0,
            1.0,
        ]);
        self.state
            .rows
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let x = 110.0 + (i % 2) as f32 * 456.0;
                let y = 420.0 + (i / 2) as f32 * 130.0;
                let u = if i == self.state.selected() { 0.5 } else { 0.0 };
                let v = (row.id * 130) as f32 / 1040.0;
                Packet {
                    texture: if plus { "q_menu_plus" } else { "q_menu" }.into(),
                    matrix,
                    rect: [x, y, x + 440.0, y + 130.0],
                    uv: [u, v, u + 0.5, v + 0.125],
                    color: 0x00ffffff
                        | u32::from((row.opacity.value().max(0.0) * 255.0) as u8) << 24,
                    opaque: false,
                    flash: row.flash.value(),
                }
            })
            .collect()
    }
    pub fn render(&self, pass: &mut RenderPass, atlas: &CharacterTextures, plus: bool) {
        for p in self.packets(plus && atlas.contains("q_menu_plus")) {
            p.render(pass, Some(atlas));
        }
    }
}
#[cfg(test)]
mod tests;
