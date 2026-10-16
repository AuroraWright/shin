//! Native CardSelect phase/tween state; resources and scene input bind separately.
use anyhow::{Result, ensure};
use shin_core::time::{Easing, Ticks, Tweener};
pub mod draw;
pub mod gerokasu;
pub mod gerokasu_draw;

/// The QUIZ-bound interactive drawable as seen by the scene and app input.
pub trait CardInteraction: Send {
    /// First listener query (QUIZ bind).
    fn attach_listener(&mut self);
    fn ready(&self) -> bool;
    fn result(&self) -> Option<i32>;
    fn restore_result(&mut self, result: i32);
    fn select(&mut self, index: usize) -> bool;
    fn confirm(&mut self) -> bool;
    fn navigate(&mut self, direction: i32) -> bool;
    /// B; `false` leaves the button to the ADV.
    fn cancel_input(&mut self) -> bool {
        false
    }
    fn pointer_move(&mut self, position: glam::Vec2) -> bool;
    fn pointer_press(&mut self) -> bool;
    fn pointer_release(&mut self) -> bool;
    fn pointer_cancel(&mut self);
    /// Sounds and voice requests since the last drain.
    fn take_events(&mut self) -> Vec<gerokasu::CardEvent> {
        Vec::new()
    }
    fn set_voice_playing(&mut self, _playing: bool) {}
}
impl CardInteraction for CardSelectModel {
    fn attach_listener(&mut self) {
        self.listener = true;
    }
    fn ready(&self) -> bool {
        CardSelectModel::ready(self)
    }
    fn result(&self) -> Option<i32> {
        CardSelectModel::result(self)
    }
    fn restore_result(&mut self, result: i32) {
        CardSelectModel::restore_result(self, result);
    }
    fn select(&mut self, index: usize) -> bool {
        CardSelectModel::select(self, index)
    }
    fn confirm(&mut self) -> bool {
        CardSelectModel::confirm(self)
    }
    fn navigate(&mut self, direction: i32) -> bool {
        CardSelectModel::navigate(self, direction)
    }
    fn pointer_move(&mut self, position: glam::Vec2) -> bool {
        CardSelectModel::pointer_move(self, position)
    }
    fn pointer_press(&mut self) -> bool {
        CardSelectModel::pointer_press(self)
    }
    fn pointer_release(&mut self) -> bool {
        CardSelectModel::pointer_release(self)
    }
    fn pointer_cancel(&mut self) {
        CardSelectModel::pointer_cancel(self);
    }
}
impl CardInteraction for gerokasu::GerokasuCardModel {
    fn attach_listener(&mut self) {
        gerokasu::GerokasuCardModel::attach_listener(self);
    }
    fn ready(&self) -> bool {
        gerokasu::GerokasuCardModel::ready(self)
    }
    fn result(&self) -> Option<i32> {
        gerokasu::GerokasuCardModel::result(self)
    }
    fn restore_result(&mut self, result: i32) {
        gerokasu::GerokasuCardModel::restore_result(self, result);
    }
    fn select(&mut self, index: usize) -> bool {
        self.ready() && self.phase == 1 && self.select_card(index as i32)
    }
    fn confirm(&mut self) -> bool {
        self.ready() && gerokasu::GerokasuCardModel::confirm(self)
    }
    fn navigate(&mut self, direction: i32) -> bool {
        self.ready() && gerokasu::GerokasuCardModel::navigate(self, direction)
    }
    fn cancel_input(&mut self) -> bool {
        self.ready() && self.cancel()
    }
    fn pointer_move(&mut self, position: glam::Vec2) -> bool {
        gerokasu::GerokasuCardModel::pointer_move(self, position)
    }
    fn pointer_press(&mut self) -> bool {
        gerokasu::GerokasuCardModel::pointer_press(self)
    }
    fn pointer_release(&mut self) -> bool {
        gerokasu::GerokasuCardModel::pointer_release(self)
    }
    fn pointer_cancel(&mut self) {
        gerokasu::GerokasuCardModel::pointer_cancel(self);
    }
    fn take_events(&mut self) -> Vec<gerokasu::CardEvent> {
        std::mem::take(&mut self.events)
    }
    fn set_voice_playing(&mut self, playing: bool) {
        self.voice_playing = playing;
    }
}

#[derive(Clone, Debug)]
pub struct Card {
    pub face: u32,
    pub delay: f32,
    /// Native+8,+60,+b8,+110,+168,+1c0,+218: focus,press,X,Y,turn,scale,alpha.
    pub tweens: [Tweener; 7],
}
#[derive(Clone, Debug)]
pub struct CardSelectModel {
    pub cards: Vec<Card>,
    pub phase: i32,
    pub selected: usize,
    pub hover: Option<usize>,
    pub automatic: bool,
    pub countdown: i32,
    pub elapsed: f32,
    pub stagger: f32,
    pub caption_y: Tweener,
    pub caption_alpha: Tweener,
    pub listener: bool,
    pub cues: u32,
}
fn control(t: &mut Tweener, v: f32, d: f32, flags: u32) {
    let easing = match flags & 63 {
        0 => Easing::Linear,
        1 => Easing::SineIn,
        2 => Easing::SineOut,
        _ => unreachable!(),
    };
    t.gerokasu2_control(v, Ticks::from_f32(d), flags, easing, false);
}
fn reset_current(t: &mut Tweener) {
    let value = t.value();
    t.fast_forward_to(value);
}
fn minstd(state: &mut u32) -> u32 {
    *state = (u64::from(*state) * 48271 % 2147483647) as u32;
    *state
}
fn uniform(state: &mut u32, last: usize) -> usize {
    if last == 0 {
        return 0;
    }
    let range = last + 1;
    let bits = usize::BITS - (range - 1).leading_zeros();
    let mask = (1u32 << bits) - 1;
    let threshold = (2147483646u32 >> bits) << bits;
    loop {
        let v = minstd(state) - 1;
        if v < threshold {
            let v = (v & mask) as usize;
            if v < range {
                return v;
            }
        }
    }
}
impl CardSelectModel {
    pub fn new(raw_mask: i32, clock_seed: u32) -> Result<Self> {
        let automatic = raw_mask & 0x1000 != 0;
        let (mask, selected) = if automatic {
            match raw_mask & 15 {
                1 => (11, 1),
                2 => (9, 0),
                _ => (15, 2),
            }
        } else {
            (raw_mask & 15, 0)
        };
        ensure!(mask != 0, "Empty Ger CardSelect mask");
        let count = mask.count_ones() as usize;
        let spacing = 316.0f32;
        let mut x = (1280.0 - (count as f32 * spacing - 40.0)).mul_add(0.5, 320.0);
        if !automatic {
            x += 320.0;
        }
        x += 138.0;
        let mut cards = Vec::new();
        for face in 0..4 {
            if mask & (1 << face) != 0 {
                let values = [0.0, 0.0, x, 996.0, 0.0, 1.0, 0.0];
                cards.push(Card {
                    face,
                    delay: 0.0,
                    tweens: values.map(Tweener::new_gerokasu2),
                });
                x += spacing;
            }
        }
        let mut order: Vec<usize> = (0..count).collect();
        let mut seed = (clock_seed % 2147483647).max(1);
        for i in 0..count - 1 {
            let j = uniform(&mut seed, count - i - 1);
            if j != 0 {
                order.swap(i, i + j);
            }
        }
        let step = if automatic { 10.0 } else { 20.0 };
        for (i, &index) in order.iter().enumerate() {
            cards[index].delay = i as f32 * step;
        }
        Ok(Self {
            cards,
            phase: 0,
            selected,
            hover: None,
            automatic,
            countdown: 30,
            elapsed: 0.0,
            stagger: count as f32 * step,
            caption_y: Tweener::new_gerokasu2(-100.0),
            caption_alpha: Tweener::new_gerokasu2(0.0),
            listener: false,
            cues: 0,
        })
    }
    pub fn render_clone_model(&self) -> Self {
        let mut copy = self.clone();
        // Native2f774 copies values/queues/phase but leaves listener130 nil.
        copy.listener = false;
        copy
    }
    pub fn ready(&self) -> bool {
        self.phase == 2 && !self.automatic
    }
    pub fn result(&self) -> Option<i32> {
        (self.phase < 0).then(|| self.cards[self.selected].face as i32)
    }
    pub fn navigate(&mut self, direction: i32) -> bool {
        if !self.ready() {
            return false;
        }
        let next = (self.selected as i32 + direction.signum()).rem_euclid(self.cards.len() as i32)
            as usize;
        if next != self.selected {
            self.cues += 1;
            self.select(next);
        }
        true
    }
    /// Native32220 ignores visual scale/turn and uses left/top inclusive,
    /// right/bottom exclusive card rectangles in widget canvas coordinates.
    pub fn pointer_move(&mut self, position: glam::Vec2) -> bool {
        if !self.ready() || !position.is_finite() {
            return false;
        }
        self.hover = self.cards.iter().position(|c| {
            let x = c.tweens[2].value() - 138.0;
            let y = c.tweens[3].value() - 196.0;
            position.x >= x && position.x < x + 276.0 && position.y >= y && position.y < y + 392.0
        });
        self.hover.is_some()
    }
    pub fn pointer_press(&mut self) -> bool {
        if !self.ready() {
            return false;
        }
        let Some(i) = self.hover else {
            return false;
        };
        reset_current(&mut self.cards[i].tweens[1]);
        control(&mut self.cards[i].tweens[1], 0.7, 0.2, 64);
        self.select(i);
        true
    }
    pub fn pointer_release(&mut self) -> bool {
        if !self.ready() {
            return false;
        }
        let Some(i) = self.hover else {
            return false;
        };
        self.select(i);
        self.confirm();
        self.hover = None;
        true
    }
    pub fn pointer_cancel(&mut self) {
        if let Some(i) = self.hover {
            reset_current(&mut self.cards[i].tweens[1]);
            control(&mut self.cards[i].tweens[1], 0.0, 0.2, 64);
        }
        self.hover = None;
    }
    pub fn pointer_leave(&mut self) {
        self.hover = None;
    }
    fn busy(&self) -> bool {
        self.caption_y.is_idle() == false
            || self.caption_alpha.is_idle() == false
            || self
                .cards
                .iter()
                .any(|c| c.tweens.iter().any(|t| !t.is_idle()))
    }
    pub fn update(&mut self, delta: Ticks) {
        match self.phase {
            0 => {
                for card in &mut self.cards {
                    for index in [3, 4, 6] {
                        control(&mut card.tweens[index], 0.0, card.delay, 128);
                    }
                    control(&mut card.tweens[3], -300.0, 60.0, 130);
                    control(&mut card.tweens[4], 2.0, 90.0, 2);
                    control(&mut card.tweens[6], 1.0, 60.0, 0);
                }
                if !self.automatic {
                    control(
                        &mut self.cards[self.selected].tweens[0],
                        0.0,
                        self.stagger + 60.0,
                        128,
                    );
                    control(&mut self.cards[self.selected].tweens[0], 1.0, 4.0, 0);
                    control(
                        &mut self.cards[self.selected].tweens[5],
                        0.0,
                        self.stagger + 60.0,
                        128,
                    );
                    control(
                        &mut self.cards[self.selected].tweens[5],
                        f32::from_bits(0x3f866666),
                        4.0,
                        0,
                    );
                }
                control(&mut self.caption_y, 0.0, self.stagger + 60.0, 128);
                control(&mut self.caption_y, 0.0, 30.0, 2);
                control(&mut self.caption_alpha, 0.0, self.stagger + 60.0, 128);
                control(&mut self.caption_alpha, 1.0, 30.0, 0);
                self.phase = 1;
                self.poll_entrance();
            }
            1 => self.poll_entrance(),
            2 => {
                if self.automatic {
                    self.countdown -= 1;
                    if self.countdown <= 0 {
                        self.cues += 1;
                        self.confirm_inner();
                    }
                }
            }
            3 => {
                if self.cards[self.selected].tweens[1].is_idle() && !self.busy() {
                    self.phase = -1;
                }
            }
            _ => {}
        }
        self.caption_y.update(delta);
        self.caption_alpha.update(delta);
        for card in &mut self.cards {
            for tween in &mut card.tweens {
                tween.update(delta);
            }
        }
        let value = self.elapsed + delta.as_f32() / 60.0;
        self.elapsed = value - value.trunc();
    }
    fn poll_entrance(&mut self) {
        if !self.busy() && self.listener {
            self.phase = 2;
            if self.automatic {
                self.countdown -= 1;
                if self.countdown <= 0 {
                    self.cues += 1;
                    self.confirm_inner();
                }
            }
        }
    }
    pub fn fast_forward(&mut self) {
        if self.phase == 2 && self.automatic {
            self.countdown = 0;
        }
        self.caption_y.fast_forward();
        self.caption_alpha.fast_forward();
        for card in &mut self.cards {
            for tween in &mut card.tweens {
                tween.fast_forward();
            }
        }
    }
    pub fn select(&mut self, index: usize) -> bool {
        if !self.ready() || index >= self.cards.len() || index == self.selected {
            return false;
        }
        let old = &mut self.cards[self.selected];
        reset_current(&mut old.tweens[0]);
        control(&mut old.tweens[0], 0.0, f32::from_bits(0x3d4ccccd), 64);
        reset_current(&mut old.tweens[5]);
        control(&mut old.tweens[5], 1.0, f32::from_bits(0x3b23d70a), 64);
        self.selected = index;
        let card = &mut self.cards[index];
        reset_current(&mut card.tweens[0]);
        control(&mut card.tweens[0], 1.0, 0.25, 64);
        reset_current(&mut card.tweens[5]);
        control(&mut card.tweens[5], f32::from_bits(0x3f866666), 0.125, 64);
        true
    }
    pub fn confirm(&mut self) -> bool {
        if !self.ready() {
            return false;
        }
        self.cues += 1;
        self.confirm_inner();
        true
    }
    fn confirm_inner(&mut self) {
        let c = &mut self.cards[self.selected];
        reset_current(&mut c.tweens[1]);
        control(&mut c.tweens[1], f32::from_bits(0x3f333333), 0.2, 64);
        control(&mut c.tweens[1], 0.0, 0.2, 64);
        control(&mut c.tweens[0], 0.0, 30.0, 0);
        control(&mut c.tweens[2], 0.0, 30.0, 128);
        control(
            &mut c.tweens[2],
            if self.automatic { 960.0 } else { 1280.0 },
            30.0,
            0,
        );
        control(&mut c.tweens[3], 0.0, 30.0, 128);
        control(&mut c.tweens[3], 540.0, 30.0, 0);
        control(&mut c.tweens[5], 0.0, 30.0, 128);
        control(&mut c.tweens[5], 1.5, 30.0, 0);
        control(&mut self.caption_alpha, 0.0, 30.0, 0);
        let mut delay = 0.0;
        for (i, c) in self.cards.iter_mut().enumerate() {
            if i != self.selected {
                control(&mut c.tweens[6], 0.0, delay, 128);
                control(&mut c.tweens[6], 0.0, 30.0, 0);
                control(&mut c.tweens[3], 0.0, delay, 128);
                control(&mut c.tweens[3], 100.0, 30.0, 129);
                delay += 10.0;
            }
        }
        self.phase = 3;
    }
    pub fn cancel(&mut self) {
        self.phase = -1;
    }
    /// Original30FA8 (InteractiveLayer result setter) ignores its argument and
    /// marks the phase complete, retaining selected face, tweens and listener.
    pub fn restore_result(&mut self, _result: i32) {
        self.phase = -1;
    }
}

#[cfg(test)]
mod tests;
