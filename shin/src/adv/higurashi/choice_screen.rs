//! Updatedc0e30/c1f20/c2b18 choice overlay, preserving original script indexes.
use super::input::{Buttons, OverlayInput};
use crate::layer::higurashi_properties::HigurashiTweener;
use anyhow::{Result, ensure};
use glam::Vec2;
use shin_core::time::Ticks;

pub mod view;
#[derive(Clone, Debug)]
pub struct Spec {
    pub original: usize,
    pub label: String,
    pub badges: u8,
    pub seen: bool,
    pub height: f32,
}
pub struct Row {
    pub spec: Spec,
    /// c0e30 assigns this before shuffle; pointer callbacks retain that identity.
    pub id: usize,
    pub y: f32,
    pub x: HigurashiTweener,
    pub alpha: HigurashiTweener,
    pub focus: HigurashiTweener,
}
impl Row {
    fn idle(&self) -> bool {
        self.x.is_idle() && self.alpha.is_idle() && self.focus.is_idle()
    }
}
pub struct Model {
    pub rows: Vec<Row>,
    pub selected: i32,
    pub phase: u32,
    pub hidden: bool,
    pub opacity: f32,
    pub badge_slide: HigurashiTweener,
    pub badge_alpha: HigurashiTweener,
    pub result: Option<i32>,
    silent: bool,
    cues: Vec<&'static str>,
}
fn control(t: &mut HigurashiTweener, value: f32, time: f32, flags: u32) {
    t.control(value, Ticks::from_f32(time), flags)
        .expect("Audited choice tween");
}
pub fn split_badges(label: &str) -> (u8, &str) {
    let Some(rest) = label.strip_prefix('*') else {
        return (0, label);
    };
    let mut badges = 0;
    let mut end = label.len();
    for (i, ch) in rest.char_indices() {
        if ch == '.' {
            end = i + 2;
            break;
        }
        if ch.is_ascii_digit() && ch <= '5' {
            badges |= 1 << (ch as u8 - b'0');
        }
    }
    (badges, &label[end..])
}
impl Model {
    pub fn new(specs: Vec<Spec>, visibility: u32, shuffle_seed: Option<u32>) -> Result<Self> {
        ensure!(specs.len() <= 12, "Higurashi choice exceeds12 rows");
        ensure!(
            visibility & 0x8000 == 0 || !specs.is_empty(),
            "Native silent choice requires a visible row"
        );
        let mut rows = specs
            .into_iter()
            .enumerate()
            .map(|(id, spec)| Row {
                spec,
                id,
                y: 0.0,
                x: HigurashiTweener::new(-300.0),
                alpha: HigurashiTweener::new(0.0),
                focus: HigurashiTweener::new(0.5),
            })
            .collect::<Vec<_>>();
        if visibility & 0x1000 != 0 {
            let mut order = (0..rows.len()).collect::<Vec<_>>();
            crate::adv::choice::shuffle_choice_indices(
                &mut order,
                shuffle_seed.expect("Shuffle seed supplied once"),
            );
            let mut source = rows.into_iter().map(Some).collect::<Vec<_>>();
            rows = order
                .into_iter()
                .map(|i| source[i].take().unwrap())
                .collect();
        }
        let mut total = 0.0;
        for row in &mut rows {
            row.y = total;
            total += row.spec.height + 32.0;
        }
        total -= 32.0;
        let top = (1080.0 - total) * f32::from_bits(0x3eb33333);
        for row in &mut rows {
            row.y += top;
        }
        let silent = visibility & 0x8000 != 0;
        if silent {
            rows[0].focus.fast_forward_to(1.0);
        }
        Ok(Self {
            rows,
            selected: if silent { 0 } else { -1 },
            phase: 0,
            hidden: false,
            opacity: 1.0,
            badge_slide: HigurashiTweener::new(0.0),
            badge_alpha: HigurashiTweener::new(0.0),
            result: None,
            silent,
            cues: Vec::new(),
        })
    }
    pub fn permissions(&self) -> OverlayInput {
        OverlayInput {
            menus: self.phase == 2,
            hide_window: self.phase == 2,
        }
    }
    pub fn take_cues(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.cues)
    }
    pub fn select(&mut self, selected: i32) {
        if self.selected == selected {
            return;
        }
        if self.selected >= 0 {
            control(
                &mut self.rows[self.selected as usize].focus,
                0.5,
                16.0,
                0x140,
            );
        }
        if selected >= 0 {
            control(&mut self.rows[selected as usize].focus, 1.0, 4.0, 0x140);
        }
        self.selected = selected;
        self.badge_slide.fast_forward_to(1.0);
        control(&mut self.badge_slide, 0.0, 10.0, 2);
        self.badge_alpha.fast_forward_to(0.0);
        control(&mut self.badge_alpha, 1.0, 10.0, 0);
    }
    fn close(&mut self) {
        let mut delay = 0.0;
        for row in &mut self.rows {
            if row.id as i32 != self.selected {
                control(&mut row.x, 0.0, delay, 0x80);
                control(&mut row.x, 300.0, 20.0, 1);
                control(&mut row.alpha, 0.0, delay, 0x80);
                control(&mut row.alpha, 0.0, 20.0, 0);
            }
            delay += 4.0;
        }
        let row = &mut self.rows[self.selected as usize];
        control(&mut row.alpha, 0.0, delay + 20.0, 0x80);
        control(&mut row.alpha, 0.0, 30.0, 0);
        self.phase += 1;
    }
    pub fn input(&mut self, permission: bool, buttons: Buttons) {
        if self.phase != 2 || !permission || self.opacity < 1.0 {
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
        } else if buttons.repeat & (1 << 7) != 0 && buttons.held & 0x50 == 0 {
            0
        } else if buttons.repeat & (1 << 5) != 0 && buttons.held & 0x50 == 0 {
            self.rows.len() as i32 - 1
        } else {
            old
        };
        if next != old {
            self.cues.push("move");
            self.select(next);
        } else if buttons.pressed & (1 << 24) != 0 {
            if self.selected >= 0 && (self.selected as usize) < self.rows.len() {
                self.cues.push("seldecide");
                self.close();
            } else {
                self.cues.push("error");
            }
        }
    }
    fn hit(&self, position: Vec2) -> Option<usize> {
        if self.phase != 2 || !(position.x >= 460.0 && position.x < 1460.0) {
            return None;
        }
        self.rows
            .iter()
            .position(|row| row.y <= position.y && position.y < row.y + row.spec.height)
    }
    pub fn probe(&self, position: Vec2) -> bool {
        self.hit(position).is_some()
    }
    pub fn press(&mut self, position: Vec2) {
        if let Some(i) = self.hit(position) {
            self.select(self.rows[i].id as i32);
        }
    }
    pub fn release(&mut self, position: Vec2) {
        if let Some(i) = self.hit(position) {
            self.cues.push("seldecide");
            self.select(self.rows[i].id as i32);
            self.close();
        }
    }
    pub fn fast_forward(&mut self) {
        self.opacity = 1.0;
        if self.phase | 2 == 3 {
            for row in &mut self.rows {
                row.x.fast_forward_unless_prohibited();
                row.alpha.fast_forward_unless_prohibited();
                row.focus.fast_forward_unless_prohibited();
            }
        }
    }
    pub fn advance(&mut self, delta: Ticks) {
        let step = delta.as_f32() * 0.1;
        if self.hidden {
            self.opacity -= step;
            if self.opacity < 0.0 {
                self.opacity = 0.0;
            }
        } else {
            self.opacity += step;
            if self.opacity > 1.0 {
                self.opacity = 1.0;
            }
        }
        if self.opacity < 1.0 {
            return;
        }
        if self.phase == 0 {
            let mut delay = 0.0;
            for row in &mut self.rows {
                control(&mut row.x, 0.0, delay, 0x80);
                control(&mut row.x, 0.0, 20.0, 2);
                control(&mut row.alpha, 0.0, delay, 0x80);
                control(&mut row.alpha, 1.0, 20.0, 0);
                delay += 4.0;
            }
            if !self.silent {
                self.cues.push("select");
            }
            self.phase = 1;
        }
        if self.phase == 1 && self.rows.iter().all(Row::idle) {
            self.phase = 2;
        } else if self.phase == 3 && self.rows.iter().all(Row::idle) {
            self.phase = 4;
            self.result = Some(self.rows[self.selected as usize].spec.original as i32);
            return;
        }
        for row in &mut self.rows {
            row.x.update(delta);
            row.alpha.update(delta);
            row.focus.update(delta);
        }
        self.badge_slide.update(delta);
        self.badge_alpha.update(delta);
    }
}

#[cfg(test)]
mod tests;
