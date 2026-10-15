//! Updated425a4/44148/44224/45060 character-selection layer clocks.
//! The scene slot, asynchronous atlas producer and input wrapper are separate owners.
use super::input::{Buttons, OverlayInput};
use crate::layer::higurashi_properties::HigurashiTweener;
use anyhow::{Result, ensure};
use glam::Vec2;
use shin_core::time::Ticks;

pub mod command;
pub mod wrapper;

#[derive(Clone)]
pub struct Row {
    pub id: usize,
    pub original: usize,
    pub x: i32,
    pub alpha: HigurashiTweener,
    pub flash: HigurashiTweener,
}
#[derive(Clone)]
pub struct Model {
    pub rows: Vec<Row>,
    pub clocks: [HigurashiTweener; 8],
    pub top: i32,
    pub selected: i32,
    pub phase: u32,
    pub result: Option<i32>,
    cues: Vec<&'static str>,
}
fn control(t: &mut HigurashiTweener, value: f32, time: f32, flags: u32) {
    t.control(value, Ticks::from_f32(time), flags)
        .expect("Audited character selection tween");
}
impl Model {
    /// Original434fc copies rows/private0..5 and phase/selection, but leaves
    /// clocks6/7 at their newly constructed zero values with empty queues.
    pub(crate) fn native_copy(&self) -> Self {
        let mut model = self.clone();
        model.clocks[6] = HigurashiTweener::new(0.);
        model.clocks[7] = HigurashiTweener::new(0.);
        model.cues.clear();
        model.result = None;
        model
    }
    pub fn new(mask: u32) -> Result<Self> {
        let originals = (0..4).filter(|i| mask & (1 << i) != 0).collect::<Vec<_>>();
        ensure!(
            !originals.is_empty(),
            "Native character selection requires a visible character"
        );
        let spacing = 1920 / (originals.len() as i32 + 1);
        let rows = originals
            .into_iter()
            .enumerate()
            .map(|(id, original)| Row {
                id,
                original,
                x: spacing * (id as i32 + 1),
                alpha: HigurashiTweener::new(0.0),
                flash: HigurashiTweener::new(0.0),
            })
            .collect::<Vec<_>>();
        Ok(Self {
            top: 678 - 75 * rows.len() as i32,
            rows,
            clocks: std::array::from_fn(|i| {
                HigurashiTweener::new(if matches!(i, 4 | 5) { 1.0 } else { 0.0 })
            }),
            selected: 0,
            phase: 0,
            result: None,
            cues: Vec::new(),
        })
    }
    pub fn permissions(&self) -> OverlayInput {
        OverlayInput {
            menus: self.phase == 2,
            hide_window: false,
        }
    }
    pub fn take_cues(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.cues)
    }
    fn focus(&mut self) {
        self.clocks[2].fast_forward_to(300.0);
        control(&mut self.clocks[2], 0.0, 30.0, 2);
        self.clocks[3].fast_forward_to(0.0);
        control(&mut self.clocks[3], 1.0, 30.0, 0);
        self.clocks[4].fast_forward_to(0.0);
        control(&mut self.clocks[4], 1.0, 30.0, 0);
        self.clocks[6].fast_forward_to(1.0);
        control(&mut self.clocks[6], 0.0, 10.0, 2);
        self.clocks[7].fast_forward_to(0.0);
        control(&mut self.clocks[7], 1.0, 10.0, 0);
    }
    fn close(&mut self) {
        let flash = &mut self.rows[self.selected as usize].flash;
        flash.fast_forward_to(flash.value());
        control(flash, f32::from_bits(0x3f333333), 0.2, 0x40);
        control(flash, 0.0, 0.2, 0x40);
        let mut delay = 0.0;
        for row in &mut self.rows {
            if row.id as i32 != self.selected {
                control(&mut row.alpha, 0.0, delay, 0x80);
                control(&mut row.alpha, 0.0, 60.0, 0);
                delay += 10.0;
            }
        }
        delay += 60.0;
        let row = &mut self.rows[self.selected as usize];
        control(&mut row.alpha, 0.0, delay, 0x80);
        control(&mut row.alpha, 0.0, 60.0, 0);
        for i in [1, 0, 5] {
            control(&mut self.clocks[i], 0.0, delay, 0x80);
            control(&mut self.clocks[i], 0.0, 60.0, 0);
        }
        self.phase = 3;
    }
    /// Wrapper45ddc executes45060 before the screen's user-layer clocks tick.
    pub fn poll_phase(&mut self) {
        if self.phase == 0 {
            for (i, row) in self.rows.iter_mut().enumerate() {
                control(&mut row.alpha, 0.0, i as f32 * 10.0, 0x80);
                control(&mut row.alpha, 1.0, 60.0, 0);
            }
            self.clocks[0].fast_forward_to(0.0);
            control(&mut self.clocks[0], 0.0, 60.0, 2);
            self.clocks[1].fast_forward_to(0.0);
            control(&mut self.clocks[1], 1.0, 60.0, 0);
            self.focus();
            self.cues.push("select");
            self.phase = 1;
        }
        if matches!(self.phase, 1 | 3)
            && self.clocks[0].is_idle()
            && self.clocks[1].is_idle()
            && self.rows.iter().all(|r| r.alpha.is_idle())
        {
            self.phase += 1;
        }
        if self.phase == 4 && self.result.is_none() {
            self.result = Some(self.rows[self.selected as usize].original as i32);
        }
    }
    pub fn advance_layer(&mut self, delta: Ticks) {
        self.clocks[0].update(delta);
        self.clocks[1].update(delta);
        for row in &mut self.rows {
            row.alpha.update(delta);
            row.flash.update(delta);
        }
        for c in &mut self.clocks[2..] {
            c.update(delta);
        }
    }
    pub fn fast_forward(&mut self) {
        for c in &mut self.clocks[..6] {
            c.fast_forward_unless_prohibited();
        }
        for row in &mut self.rows {
            row.alpha.fast_forward_unless_prohibited();
            row.flash.fast_forward_unless_prohibited();
        }
    }
    pub fn input(&mut self, permission: bool, buttons: Buttons) {
        if self.phase != 2 || !permission {
            return;
        }
        if buttons.pressed & (1 << 24) != 0 {
            self.cues.push("decide");
            self.close();
            return;
        }
        let old = self.selected;
        let next = if buttons.repeat & (1 << 4) != 0 {
            if old > 0 {
                old - 1
            } else {
                self.rows.len() as i32 - 1
            }
        } else if buttons.repeat & (1 << 6) != 0 {
            if old + 1 >= self.rows.len() as i32 {
                0
            } else {
                old + 1
            }
        } else {
            old
        };
        if next != old {
            self.cues.push("move");
            self.selected = next;
            self.focus();
        }
    }
    fn hit(&self, p: Vec2) -> Option<usize> {
        if self.phase != 2 || !(p.x >= 432.0 && p.x < 1044.0) {
            return None;
        }
        self.rows.iter().position(|r| {
            let top = (self.top + r.id as i32 * 150 + 3) as f32;
            p.y >= top && p.y < top + 144.0
        })
    }
    pub fn probe(&self, p: Vec2) -> bool {
        self.hit(p).is_some()
    }
    pub fn press(&mut self, p: Vec2) {
        if let Some(id) = self.hit(p) {
            self.selected = id as i32;
            self.focus();
        }
    }
    pub fn release(&mut self, p: Vec2) {
        if let Some(id) = self.hit(p) {
            if self.selected != id as i32 {
                self.selected = id as i32;
                self.focus();
            }
            self.cues.push("decide");
            self.close();
        }
    }
}

#[cfg(test)]
mod tests;

pub mod view;
