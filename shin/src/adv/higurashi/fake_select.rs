//! Updated automatic FakeSelectLayer; it has no direct choice input callbacks.
use super::voice_commands::VoiceCommands;
use crate::layer::higurashi_properties::HigurashiTweener;
use glam::Vec2;
use shin_core::time::Ticks;

pub(crate) mod command;
pub(crate) mod view;

#[cfg(test)]
mod tests;

pub(crate) const LABELS: [&str; 2] = [
    "沙都子を見殺しにして記憶を残す",
    "沙都子を救いに戻り記憶を捨てる",
];
pub(crate) const VOICES: [&str; 2] = ["S12/05/mina_hrika_0000", "S12/05/mina_hrika_0010"];

pub(crate) struct Row {
    pub height: f32,
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
pub(crate) struct Model {
    pub rows: [Row; 2],
    pub selected: i32,
    pub phase: u32,
    pub countdown: f32,
    pub remaining: u32,
    pub seed: u32,
    cues: Vec<&'static str>,
}
fn control(tween: &mut HigurashiTweener, value: f32, duration: f32, flags: u32) {
    tween
        .control(value, Ticks::from_f32(duration), flags)
        .expect("Audited FakeSelect tween");
}
impl Model {
    pub fn new(heights: [f32; 2], raw_seed: u32) -> Self {
        let total = heights[0] + heights[1] + 32.;
        let top = (1080. - total) * f32::from_bits(0x3eb33333);
        Self {
            rows: std::array::from_fn(|i| Row {
                height: heights[i],
                y: top + if i == 0 { 0. } else { heights[0] + 32. },
                x: HigurashiTweener::new(-300.),
                alpha: HigurashiTweener::new(0.),
                focus: HigurashiTweener::new(0.5),
            }),
            selected: -1,
            phase: 0,
            countdown: 60.,
            remaining: 3,
            seed: (raw_seed % 0x7fff_ffff).max(1),
            cues: Vec::new(),
        }
    }
    pub fn take_cues(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.cues)
    }
    pub fn complete(&self) -> bool {
        self.phase >= 4
    }
    fn select(&mut self, selected: i32) {
        if selected != self.selected {
            if self.selected >= 0 {
                control(
                    &mut self.rows[self.selected as usize].focus,
                    0.5,
                    16.,
                    0x140,
                );
            }
            control(&mut self.rows[selected as usize].focus, 1., 4., 0x140);
            self.selected = selected;
        }
        self.cues.push("move");
    }
    fn close(&mut self) {
        let mut delay = 0.;
        for (id, row) in self.rows.iter_mut().enumerate() {
            if id as i32 != self.selected {
                control(&mut row.x, 0., delay, 0x80);
                control(&mut row.x, 300., 20., 1);
                control(&mut row.alpha, 0., delay, 0x80);
                control(&mut row.alpha, 0., 20., 0);
            }
            delay += 4.;
        }
        let row = &mut self.rows[self.selected as usize];
        control(&mut row.alpha, 0., delay + 20., 0x80);
        control(&mut row.alpha, 0., 30., 0);
        self.phase += 1;
    }
    pub fn fast_forward(&mut self) {
        match self.phase {
            1 | 3 => {
                for row in &mut self.rows {
                    row.x.fast_forward_unless_prohibited();
                    row.alpha.fast_forward_unless_prohibited();
                    row.focus.fast_forward_unless_prohibited();
                }
            }
            2 => self.countdown = 0.,
            _ => {}
        }
    }
    pub fn advance(&mut self, delta: Ticks, voice: &mut dyn VoiceCommands) {
        if self.phase == 0 {
            let mut delay = 0.;
            for row in &mut self.rows {
                control(&mut row.x, 0., delay, 0x80);
                control(&mut row.x, 0., 20., 2);
                control(&mut row.alpha, 0., delay, 0x80);
                control(&mut row.alpha, 1., 20., 0);
                delay += 4.;
            }
            self.cues.push("select");
            self.phase = 1;
        }
        if self.phase == 1 && self.rows.iter().all(Row::idle) {
            self.phase = 2;
        }
        if self.phase == 2 && voice.status_bits() == 0 {
            self.countdown -= delta.as_f32();
            if !(self.countdown > 0.) {
                if self.remaining == 0 {
                    self.cues.push("seldecide");
                    self.close();
                } else {
                    let previous = self.remaining;
                    self.remaining -= 1;
                    match previous {
                        3 => {
                            voice.play_command(VOICES[0], 1., 0);
                            self.select(1);
                            self.countdown = 10.;
                        }
                        2 => {
                            voice.play_command(VOICES[1], 1., 0);
                            self.select(0);
                            self.countdown = 100.;
                        }
                        1 => {
                            self.select(1);
                            self.countdown = 5.;
                        }
                        _ => {}
                    }
                }
            }
        } else if self.phase == 3 && self.rows.iter().all(Row::idle) {
            self.phase += 1;
        }
        // Native56e38 always advances the private row clocks at its tail;
        // inherited65-property clocks are intentionally not advanced here.
        for row in &mut self.rows {
            row.x.update(delta);
            row.alpha.update(delta);
            row.focus.update(delta);
        }
    }
    /// Native57380 samples jitter during its transparent draw, not its update.
    /// Runtime presentations/captures use the already prepared ADV cache.
    pub fn sample_jitter(&mut self) -> Vec2 {
        if self.remaining != 1 || !(30. <= self.countdown && self.countdown < 100.) {
            return Vec2::ZERO;
        }
        let mut next = || {
            self.seed = ((u64::from(self.seed) * 48271) % 0x7fff_ffff) as u32;
            ((self.seed - 1) as f32 * f32::from_bits(0x30000000)) * 8. - 4.
        };
        Vec2::new(next(), next())
    }
}
