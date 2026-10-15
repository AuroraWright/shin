//! Updated df934/e0510/e0d2c top menu. Older two-column layout and mode requests.
use crate::{
    adv::{
        character_view::CharacterTextures,
        higurashi::input::Buttons,
        input_area::{InputArea, InputAreaTarget, PointerFrame},
    },
    layer::higurashi_properties::HigurashiTweener,
};
use anyhow::{Result, ensure};
use glam::{Vec2, Vec4, vec2, vec3, vec4};
use shin_core::{primitives::color::UnormColor, time::Ticks};
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    quad_vertices::build_quad_vertices,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, vertices::PosColTexVertex},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app::higurashi) enum Destination {
    Cat { variant: u32, selected: i32 },
    Save { return_id: i32 },
    Config { flags: u32, return_id: i32 },
    Chart { selected: i32, return_id: i32 },
    Manual { return_id: i32 },
    Tips { selected: i32, alternate: bool },
    Quiz,
    Origin { selected: i32 },
    IdleDemo,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Cue {
    Move,
    Confirm,
    Error,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Action {
    Logo(f32, f32),
    Cue(Cue),
    Destination(Destination),
}
struct Row {
    id: i32,
    rect: Vec4,
    opacity: HigurashiTweener,
    flash: HigurashiTweener,
}
struct State {
    phase: i32,
    idle: i32,
    enabled: u32,
    column: i32,
    line: i32,
    pointer: i32,
    idle_demo: bool,
    rows: Vec<Row>,
    actions: Vec<Action>,
}
fn control(clock: &mut HigurashiTweener, value: f32, ticks: f32, flags: u32) {
    clock
        .control(value, Ticks::from_f32(ticks), flags)
        .expect("Verified native top tween");
}
fn rate(clock: &mut HigurashiTweener, value: f32) {
    clock.fast_forward_to(clock.value());
    control(clock, value, f32::from_bits(0x3e4ccccd), 0x40);
}
impl State {
    fn new(plus: bool, auto_valid: bool, selected: i32) -> Self {
        let ids = if plus {
            vec![0, 1, 2, 3, 5, 6, 7, 4, 8]
        } else {
            vec![0, 1, 2, 3, 5, 6, 7, 4]
        };
        let rows = ids
            .into_iter()
            .enumerate()
            .map(|(index, id)| Row {
                id,
                rect: if id == 8 {
                    vec4(258.0, 856.0, 600.0, 112.0)
                } else {
                    vec4(
                        110.0 + (index % 2) as f32 * 456.0,
                        if plus {
                            410.0 + 114.0 * (index / 2) as f32
                        } else {
                            436.0 + 120.0 * (index / 2) as f32
                        },
                        440.0,
                        112.0,
                    )
                },
                opacity: HigurashiTweener::new(0.0),
                flash: HigurashiTweener::new(0.0),
            })
            .collect::<Vec<_>>();
        let selected = if selected < 0 {
            i32::from(auto_valid)
        } else {
            selected
        };
        let index = rows.iter().position(|row| row.id == selected).unwrap_or(0) as i32;
        Self {
            phase: 0,
            idle: 0,
            enabled: if plus { 0x1ff } else { 0xff },
            column: index % 2,
            line: index / 2,
            pointer: -1,
            idle_demo: false,
            rows,
            actions: Vec::new(),
        }
    }
    fn selected(&self) -> usize {
        (self.column + 2 * self.line) as usize
    }
    fn confirm(&mut self) {
        if self.phase != 2 {
            return;
        }
        let at = self.selected();
        rate(&mut self.rows[at].flash, 0.7);
        control(
            &mut self.rows[at].flash,
            0.0,
            f32::from_bits(0x3e4ccccd),
            0x40,
        );
        if self.enabled & (1 << at) == 0 {
            self.actions.push(Action::Cue(Cue::Error));
            return;
        }
        self.actions.push(Action::Cue(Cue::Confirm));
        for (index, row) in self.rows.iter_mut().enumerate() {
            if index != at {
                control(&mut row.opacity, 0.0, 6.0 * (index / 2) as f32, 0x80);
                control(&mut row.opacity, 0.0, 30.0, 0);
            }
        }
        control(&mut self.rows[at].opacity, 0.0, 60.0, 0x80);
        control(&mut self.rows[at].opacity, 0.0, 40.0, 0);
        self.phase = 3;
    }
    fn prepare(&mut self) -> bool {
        if self.phase == 0 {
            self.actions.push(Action::Logo(1.0, 30.0));
            for (index, row) in self.rows.iter_mut().enumerate() {
                control(&mut row.opacity, 0.0, 6.0 * (index / 2) as f32, 0x80);
                control(&mut row.opacity, 1.0, 30.0, 0);
            }
            self.phase = 1;
        }
        if self.phase == 1 && self.rows.iter().all(|row| row.opacity.is_idle()) {
            self.phase = 2;
        }
        self.phase == 2
    }
    fn begin_input(&mut self, permission: bool) -> bool {
        if self.phase != 2 {
            return false;
        }
        if !permission {
            self.idle = 0;
            return false;
        }
        let before = self.idle;
        self.idle = self.idle.wrapping_add(1);
        if before >= 3599 {
            self.idle_demo = true;
            self.actions.push(Action::Logo(0.0, 30.0));
            for row in &mut self.rows {
                control(&mut row.opacity, 0.0, 30.0, 0);
            }
            self.phase = 3;
            return false;
        }
        true
    }
    fn poll_controls(&mut self, captured: bool, buttons: Buttons) {
        if captured {
            self.idle = 0;
            return;
        }
        if self.phase != 2 {
            return;
        }
        if buttons.pressed & (1 << 24) != 0 {
            self.confirm();
            return;
        }
        let mut column = self.column;
        if buttons.repeat & (1 << 7) != 0 {
            column = if column == 0 { 1 } else { column - 1 };
        } else if buttons.repeat & (1 << 5) != 0 {
            column = if column > 0 { 0 } else { column + 1 };
        }
        let lines = (self.rows.len() as i32 + 1) / 2;
        let mut line = self.line;
        if buttons.repeat & (1 << 6) != 0 {
            line = if line + 1 >= lines { 0 } else { line + 1 };
        } else if buttons.repeat & (1 << 4) != 0 {
            line = if line < 1 { lines - 1 } else { line - 1 };
        }
        if column + 2 * line >= self.rows.len() as i32 {
            column -= 1;
        }
        if column != self.column || line != self.line {
            self.column = column;
            self.line = line;
            self.idle = 0;
            self.actions.push(Action::Cue(Cue::Move));
        }
    }
    fn finish(&mut self) {
        if self.phase != 3 || self.rows.iter().any(|row| row.opacity.is_waiting()) {
            return;
        }
        let id = self.rows[self.selected()].id;
        let destination = if self.idle_demo {
            Destination::IdleDemo
        } else {
            match id {
                0 => Destination::Cat {
                    variant: 0,
                    selected: -1,
                },
                1 => Destination::Save { return_id: 1 },
                2 => Destination::Config {
                    flags: 0,
                    return_id: 2,
                },
                3 => Destination::Chart {
                    selected: -1,
                    return_id: 3,
                },
                4 => Destination::Manual { return_id: 4 },
                5 => Destination::Cat {
                    variant: 1,
                    selected: -1,
                },
                6 => Destination::Tips {
                    selected: 0,
                    alternate: false,
                },
                7 => Destination::Quiz,
                8 => Destination::Origin { selected: 0 },
                _ => unreachable!(),
            }
        };
        self.actions.push(Action::Destination(destination));
        self.phase = -1;
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
            .position(|row| {
                point.x >= row.rect.x
                    && point.x < row.rect.x + row.rect.z
                    && point.y >= row.rect.y
                    && point.y < row.rect.y + row.rect.w
            })
            .map_or(-1, |index| index as i32)
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
            let at = self.pointer as usize;
            rate(&mut self.rows[at].flash, 0.7);
            self.column = self.pointer % 2;
            self.line = self.pointer / 2;
        }
    }
    fn release(&mut self, _: Vec2) {
        if self.pointer >= 0 {
            self.column = self.pointer % 2;
            self.line = self.pointer / 2;
            self.confirm();
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
pub(super) struct TopMenu {
    state: State,
    area: InputArea,
}
impl TopMenu {
    pub fn new(plus: bool, auto_valid: bool, selected: i32, extent: (u16, u16)) -> Self {
        Self {
            state: State::new(plus, auto_valid, selected),
            area: InputArea::new(extent.0, extent.1),
        }
    }
    pub fn complete(&self) -> bool {
        self.state.phase < 0
    }
    /// The diagnostic journey enters the real Load destination (native ID1).
    pub(super) fn touch_diagnostic_ready(&self) -> bool {
        self.state.phase == 2 && self.state.rows[self.state.selected()].id == 1
    }
    pub fn resize(&mut self, extent: (u16, u16)) {
        self.area.disable(&mut self.state);
        self.area = InputArea::new(extent.0, extent.1);
    }
    pub fn cancel_input(&mut self) {
        self.area.disable(&mut self.state);
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    #[allow(dead_code, reason = "Inspection hook shared by native tests and optional window probes.")]
    pub(super) fn reference_phase(&self) -> i32 {
        self.state.phase
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    #[allow(dead_code, reason = "Inspection hook shared by native tests and optional window probes.")]
    pub(super) fn reference_selection(&self) -> i32 {
        self.state.rows[self.state.selected()].id
    }
    #[cfg(test)]
    pub(super) fn reference_idle(&self) -> i32 {
        self.state.idle
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
            "Invalid top-menu delta"
        );
        let active = self.state.prepare();
        let input = self.state.begin_input(permission);
        let captured = if input {
            self.area.update(&mut self.state, pointer)
        } else {
            if active && !permission {
                self.area.disable(&mut self.state);
            }
            false
        };
        if input {
            self.state.poll_controls(captured, buttons);
        }
        self.state.finish();
        self.state.advance(delta);
        Ok(std::mem::take(&mut self.state.actions))
    }
    pub fn render(&self, pass: &mut RenderPass, atlas: &CharacterTextures) {
        let selected = self.state.selected();
        let transform = crate::render::top_left_projection_matrix();
        for (index, row) in self.state.rows.iter().enumerate() {
            let u = if index == selected { 0.5 } else { 0.0 };
            let (v, v2) = if row.id == 8 {
                (0.0, 0.875)
            } else {
                let v = (row.id * 128) as f32 / f32::from_bits(0x44808000);
                (v, v + f32::from_bits(0x3ddf20df))
            };
            let opacity = row.opacity.value()
                * if self.state.enabled & (1 << index) != 0 {
                    1.0
                } else {
                    0.5
                };
            let alpha = (opacity.max(0.0) * 255.0) as u8;
            let flash = row.flash.value();
            let vertices = build_quad_vertices(|corner| PosColTexVertex {
                position: vec3(
                    row.rect.x + row.rect.z * corner.x,
                    row.rect.y + row.rect.w * corner.y,
                    0.0,
                ),
                texture_position: vec2(u + 0.5 * corner.x, v + (v2 - v) * corner.y),
                color: UnormColor::from_rgba(255, 255, 255, alpha),
            });
            let texture = atlas
                .get(if row.id == 8 { "o_menu" } else { "t_menu" })
                .as_source();
            let builder = RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1);
            let program = if flash > 0.0 {
                RenderProgramWithArguments::Button {
                    vertices: VertexSource::VertexData {
                        vertices: &vertices,
                    },
                    texture,
                    transform,
                    flash1: vec4(1.0, 1.0, 1.0, flash),
                    flash2: Vec4::ZERO,
                }
            } else {
                RenderProgramWithArguments::Sprite {
                    vertices: VertexSource::VertexData {
                        vertices: &vertices,
                    },
                    sprite: texture,
                    transform,
                }
            };
            pass.run(builder.build(program, DrawPrimitive::TrianglesStrip));
        }
    }
}

#[cfg(test)]
mod tests;
