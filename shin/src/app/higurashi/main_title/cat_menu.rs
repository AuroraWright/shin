//! Updated e17dc/e21d4/e2958 Title.CatMenu: story and gallery categories.
use super::{control, rate};
use crate::{
    adv::{
        characters::view::CharacterTextures,
        higurashi::input::Buttons,
        input_area::{InputArea, InputAreaTarget, PointerFrame},
    },
    layer::higurashi_properties::HigurashiTweener,
};
use anyhow::{Result, ensure};
use glam::{Vec2, Vec4, vec2, vec3, vec4};
use shin_core::{format::save::higurashi::GameData, time::Ticks};
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    quad_vertices::build_quad_vertices,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, vertices::PosColTexVertex},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::app::higurashi) enum SceneRequest {
    Story { data: GameData, flags: u32 },
    Cg,
    Music,
    Movie,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Destination {
    Top(i32),
    Story(i32),
    Cg,
    Music,
    Movie,
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
    variant: u32,
    enabled: u32,
    phase: i32,
    selected: i32,
    base_y: i32,
    pending: i32,
    pointer: i32,
    rows: Vec<Row>,
    actions: Vec<Action>,
}
impl State {
    fn new(variant: u32, unlock_word: i16, selected: i32) -> Self {
        assert!(variant <= 1, "Unknown Higurashi category variant");
        let ids = if variant == 0 { 0..5 } else { 5..8 };
        let rows = ids
            .map(|id| Row {
                id,
                opacity: HigurashiTweener::new(0.0),
                flash: HigurashiTweener::new(0.0),
            })
            .collect::<Vec<_>>();
        let selected = rows.iter().position(|row| row.id == selected).unwrap_or(0) as i32;
        Self {
            variant,
            enabled: if variant == 0 {
                i32::from(unlock_word | 1) as u32
            } else {
                7
            },
            phase: 0,
            selected,
            base_y: 360 + (600 - rows.len() as i32 * 120) / 2,
            pending: -1,
            pointer: -1,
            rows,
            actions: Vec::new(),
        }
    }
    fn confirm(&mut self) {
        if self.phase != 2 {
            return;
        }
        let at = self.selected as usize;
        rate(&mut self.rows[at].flash, 0.7);
        control(
            &mut self.rows[at].flash,
            0.0,
            f32::from_bits(0x3e4ccccd),
            0x40,
        );
        if self.enabled & (1 << at) == 0 {
            self.actions.push(Action::Cue("error"));
        } else {
            self.actions.push(Action::Cue(if self.variant == 0 {
                "start"
            } else {
                "decide"
            }));
            self.pending = self.selected;
            self.phase = 3;
        }
    }
    fn retreat(&mut self) {
        if self.phase == 2 {
            self.actions.push(Action::Cue("cancel"));
            self.pending = -1;
            self.phase = 3;
        }
    }
    fn prepare(&mut self) -> bool {
        if self.phase == 0 {
            for (index, row) in self.rows.iter_mut().enumerate() {
                control(&mut row.opacity, 0.0, index as f32 * 6.0, 0x80);
                control(&mut row.opacity, 1.0, 30.0, 0);
            }
            self.phase = 1;
        }
        if self.phase == 1 && self.rows.iter().all(|row| row.opacity.is_idle()) {
            self.phase = 2;
        }
        if self.phase == 3 {
            let hold = 30.0 + self.rows.len() as f32 * 6.0;
            for (index, row) in self.rows.iter_mut().enumerate() {
                if index as i32 == self.pending {
                    control(&mut row.opacity, 0.0, hold, 0x80);
                    control(&mut row.opacity, 0.0, 40.0, 0);
                } else {
                    control(&mut row.opacity, 0.0, index as f32 * 6.0, 0x80);
                    control(&mut row.opacity, 0.0, 30.0, 0);
                }
            }
            self.phase = 4;
        }
        if self.phase == 4 && self.rows.iter().all(|row| row.opacity.is_idle()) {
            let destination = if self.pending < 0 {
                Destination::Top(if self.variant == 0 { 0 } else { 5 })
            } else {
                match self.rows[self.pending as usize].id {
                    id @ 0..=4 => Destination::Story(id),
                    5 => Destination::Cg,
                    6 => Destination::Music,
                    7 => Destination::Movie,
                    _ => unreachable!(),
                }
            };
            self.actions.push(Action::Destination(destination));
            self.phase = -1;
        }
        self.phase == 2
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
            let old = self.selected;
            let count = self.rows.len() as i32;
            if buttons.repeat & (1 << 6) != 0 {
                self.selected = if old + 1 >= count { 0 } else { old + 1 };
            } else if buttons.repeat & (1 << 4) != 0 {
                self.selected = if old <= 0 { count - 1 } else { old - 1 };
            }
            if self.selected != old {
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
        if self.phase != 2 || point.x < 248.0 || point.x >= 848.0 {
            return -1;
        }
        self.rows
            .iter()
            .enumerate()
            .position(|(index, _)| {
                let y = (self.base_y + index as i32 * 120) as f32;
                point.y >= y && point.y < y + 120.0
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
            rate(&mut self.rows[self.pointer as usize].flash, 0.7);
            self.selected = self.pointer;
        }
    }
    fn release(&mut self, _: Vec2) {
        if self.pointer >= 0 {
            self.selected = self.pointer;
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
pub(super) struct CatMenu {
    state: State,
    area: InputArea,
}
impl CatMenu {
    pub fn new(variant: u32, unlock_word: i16, selected: i32, extent: (u16, u16)) -> Self {
        Self {
            state: State::new(variant, unlock_word, selected),
            area: InputArea::new(extent.0, extent.1),
        }
    }
    pub fn complete(&self) -> bool {
        self.state.phase < 0
    }
    pub fn resize(&mut self, extent: (u16, u16)) {
        self.area.disable(&mut self.state);
        self.area = InputArea::new(extent.0, extent.1);
    }
    pub fn cancel_input(&mut self) {
        self.area.disable(&mut self.state);
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    #[allow(
        dead_code,
        reason = "Inspection hook shared by native tests and optional window probes."
    )]
    pub(super) fn reference_phase(&self) -> i32 {
        self.state.phase
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    #[allow(
        dead_code,
        reason = "Inspection hook shared by native tests and optional window probes."
    )]
    pub(super) fn reference_selection(&self) -> (u32, i32, i32) {
        (
            self.state.variant,
            self.state.rows[self.state.selected as usize].id,
            self.state.phase,
        )
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
            "Invalid category-menu delta"
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
    pub fn render(&self, pass: &mut RenderPass, atlas: &CharacterTextures, plus: bool) {
        for (index, row) in self.state.rows.iter().enumerate() {
            let locked = self.state.enabled & (1 << index) == 0;
            let name = match (locked, plus) {
                (false, false) => "s_menu",
                (false, true) => "s_menu_plus",
                (true, false) => "s_menu_lock",
                (true, true) => "s_menu_lock_plus",
            };
            let name = if atlas.contains(name) {
                name
            } else {
                if locked { "s_menu_lock" } else { "s_menu" }
            };
            let y = (self.state.base_y + index as i32 * 120) as f32;
            let u = if index as i32 == self.state.selected {
                0.5
            } else {
                0.0
            };
            let v = (row.id * 120) as f32 / 960.0;
            draw(
                pass,
                atlas,
                name,
                vec4(248.0, y, 600.0, 120.0),
                vec4(u, v, u + 0.5, v + 0.125),
                row.opacity.value(),
                row.flash.value(),
            );
            if locked {
                draw(
                    pass,
                    atlas,
                    "lock",
                    vec4(224.0, y, 108.0, 108.0),
                    vec4(0.0, 0.0, 1.0, 1.0),
                    row.opacity.value(),
                    0.0,
                );
            }
        }
    }
}
fn draw(
    pass: &mut RenderPass,
    atlas: &CharacterTextures,
    name: &str,
    rect: Vec4,
    uv: Vec4,
    opacity: f32,
    flash: f32,
) {
    let alpha = (opacity.max(0.0) * 255.0) as u8;
    let vertices = build_quad_vertices(|corner| PosColTexVertex {
        position: vec3(rect.x + rect.z * corner.x, rect.y + rect.w * corner.y, 0.0),
        texture_position: vec2(
            uv.x + (uv.z - uv.x) * corner.x,
            uv.y + (uv.w - uv.y) * corner.y,
        ),
        color: shin_core::primitives::color::UnormColor::from_rgba(255, 255, 255, alpha),
    });
    let texture = atlas.get(name).as_source();
    let transform = crate::render::top_left_projection_matrix();
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
    pass.run(
        RenderRequestBuilder::new()
            .color_blend_type(ColorBlendType::Layer1)
            .build(program, DrawPrimitive::TrianglesStrip),
    );
}

#[cfg(test)]
mod tests;
