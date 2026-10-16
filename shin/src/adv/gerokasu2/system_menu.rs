//! Original Ger1BCC8 in-game SystemMenu. Host owns modal backdrop/audio scopes,
//! child assets and text GPU preparation. This model owns native row/task order.
use std::collections::VecDeque;
#[path = "system_menu/artwork.rs"]
pub mod artwork;
#[cfg(test)]
#[path = "system_menu/tests.rs"]
mod tests;
pub const ATLAS: &str = "/adv.txa";
pub const TEXTURES: &[&str] = &["sysmenu_normal", "sysmenu_focus", "sysmenu_shadow", "info"];
pub const TITLE_QUESTION: &str = "ゲームを終了して、タイトル画面に戻ります。@rよろしいですか？";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Child {
    Save,
    Load,
    Config,
    Manual,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    EnterModal,
    LeaveModal,
    OpenChild(Child),
    ConfirmTitle,
    ReturnTitle,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub cancel: bool,
    pub confirm: bool,
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
}
/// Ger98680/989C0/98E50 queues: linear/cosine/sine, speed and delayed hold.
#[derive(Clone, Debug)]
pub struct Motion {
    from: f32,
    to: f32,
    elapsed: f32,
    duration: f32,
    curve: u8,
    direct: bool,
    queue: VecDeque<(Option<f32>, f32, u8, bool)>,
}
impl Motion {
    pub fn new(value: f32) -> Self {
        Self {
            from: value,
            to: value,
            elapsed: 1.,
            duration: 1.,
            curve: 0,
            direct: true,
            queue: VecDeque::new(),
        }
    }
    pub fn value(&self) -> f32 {
        if self.direct {
            return self.to;
        }
        let t = self.elapsed / self.duration;
        let t = match self.curve {
            1 => 1. - (f32::from_bits(0x3fc90fdb) * t).cos(),
            2 => (f32::from_bits(0x3fc90fdb) * t).sin(),
            _ => t,
        };
        (self.to - self.from).mul_add(t, self.from)
    }
    pub fn busy(&self) -> bool {
        self.elapsed < self.duration || !self.queue.is_empty()
    }
    pub fn snap(&mut self, value: f32) {
        *self = Self::new(value);
    }
    fn start(&mut self, value: Option<f32>, time: f32, curve: u8, speed: bool) {
        self.from = self.value();
        self.to = value.unwrap_or(self.from);
        self.direct = false;
        self.elapsed = 0.;
        self.curve = curve;
        self.duration = if speed {
            ((self.to - self.from) / time).abs()
        } else {
            time
        };
        if self.duration == 0. {
            self.duration = 1.;
            self.elapsed = 1.;
        }
    }
    fn add(&mut self, value: Option<f32>, time: f32, curve: u8, speed: bool) {
        if self.busy() {
            self.queue.push_back((value, time, curve, speed));
        } else {
            self.start(value, time, curve, speed);
        }
    }
    fn target(&mut self, value: f32, time: f32, curve: u8, speed: bool) {
        self.snap(self.value());
        self.start(Some(value), time, curve, speed);
    }
    pub fn update(&mut self, dt: f32) {
        if dt <= 0. {
            return;
        }
        let mut left = dt;
        loop {
            let remaining = self.duration - self.elapsed;
            if left < remaining {
                self.elapsed += left;
                break;
            }
            self.elapsed = self.duration;
            left -= remaining;
            if let Some((v, t, c, s)) = self.queue.pop_front() {
                self.start(v, t, c, s);
            } else {
                break;
            }
            if left <= 0. {
                break;
            }
        }
    }
}
#[derive(Clone, Debug)]
pub struct Row {
    pub item: u8,
    pub slide: Motion,
    pub flash: Motion,
}
#[derive(Clone, Debug)]
pub struct Controller {
    pub phase: i32,
    pub selected: usize,
    pub pointer_row: i32,
    pub rows: [Row; 5],
    pub detail_slide: Motion,
    pub detail_fade: Motion,
    pub play_time: String,
    pub child: bool,
    pending_decide: bool,
    events: Vec<Event>,
    sounds: Vec<&'static str>,
    pub disabled: u32,
}
impl Default for Controller {
    fn default() -> Self {
        Self::new()
    }
}
impl Controller {
    pub fn new() -> Self {
        Self {
            phase: 0,
            selected: 0,
            pointer_row: -1,
            rows: std::array::from_fn(|i| Row {
                item: i as u8,
                slide: Motion::new(0.),
                flash: Motion::new(0.),
            }),
            detail_slide: Motion::new(1.),
            detail_fade: Motion::new(0.),
            play_time: String::new(),
            child: false,
            pending_decide: false,
            events: vec![Event::EnterModal],
            sounds: vec![],
            disabled: 0,
        }
    }
    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }
    pub fn take_sounds(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.sounds)
    }
    pub fn busy(&self) -> bool {
        self.rows.iter().any(|r| r.slide.busy())
            || self.detail_slide.busy()
            || self.detail_fade.busy()
    }
    pub fn accepts_input(&self) -> bool {
        self.phase == 1 && !self.child && !self.pending_decide && !self.busy()
    }
    fn animate(&mut self, opening: bool) {
        for (i, r) in self.rows.iter_mut().enumerate() {
            r.slide.add(None, i as f32 * 2., 0, false);
            r.slide.add(
                Some(if opening { 1. } else { 0. }),
                10.,
                if opening { 2 } else { 1 },
                false,
            );
        }
        self.detail_slide.add(
            Some(if opening { 0. } else { 1. }),
            10.,
            if opening { 2 } else { 1 },
            false,
        );
        self.detail_fade
            .add(Some(if opening { 1. } else { 0. }), 10., 0, false);
        if opening {
            self.disabled += 1;
            self.reset();
        }
    }
    fn close(&mut self) {
        self.animate(false);
        self.phase += 1;
    }
    fn decide(&mut self) {
        self.sounds.push("decide");
        let f = &mut self.rows[self.selected].flash;
        f.target(0.7, 0.2, 0, true);
        f.add(Some(0.), 0.2, 0, true);
        self.pending_decide = true;
    }
    fn dispatch(&mut self) {
        self.events.push(match self.rows[self.selected].item {
            0 => Event::OpenChild(Child::Save),
            1 => Event::OpenChild(Child::Load),
            2 => Event::OpenChild(Child::Config),
            3 => Event::OpenChild(Child::Manual),
            _ => Event::ConfirmTitle,
        });
        self.child = true;
        self.animate(false);
    }
    /// Native child callbacks reopen only reason1. Other completion words do not
    /// invent a resume; the retained owner decides when its child has retired.
    pub fn child_returned(&mut self, reason: i32) {
        if reason == 1 {
            self.child = false;
            self.animate(true);
        }
    }
    pub fn title_answer(&mut self, reason: i32, choice: i32) {
        if reason == 1 && choice == 0 {
            self.events.push(Event::ReturnTitle);
        } else if reason == 0 && choice != 0 {
            self.child = false;
            self.animate(true);
        }
    }
    /// Explicit desktop preparation failure recovery, not native unwind behavior.
    pub fn recover_child(&mut self) {
        self.child = false;
        self.pending_decide = false;
        self.animate(true);
        self.reset();
    }
    pub fn update(
        &mut self,
        dt: f32,
        permission: bool,
        captured: bool,
        input: Input,
        play_ticks: f64,
    ) {
        if !dt.is_finite() || dt < 0. {
            return;
        }
        let mut allowed = permission && !self.child && !self.pending_decide;
        if self.pending_decide && !self.rows[self.selected].flash.busy() {
            self.pending_decide = false;
            self.dispatch();
            allowed = false;
        }
        if self.phase == 0 {
            self.animate(true);
            self.phase = 1;
        }
        if self.phase == 1 {
            if !allowed || self.busy() {
                self.disabled += 1;
            } else if !captured {
                if input.cancel {
                    self.sounds.push("cancel");
                    self.close();
                } else if input.confirm {
                    self.decide();
                } else {
                    let next = if input.up {
                        if self.selected == 0 {
                            4
                        } else {
                            self.selected - 1
                        }
                    } else if input.down {
                        (self.selected + 1) % 5
                    } else if input.left {
                        0
                    } else if input.right {
                        4
                    } else {
                        self.selected
                    };
                    if next != self.selected {
                        self.sounds.push("move");
                        self.selected = next;
                    }
                }
            }
        } else if self.phase == 2 && !self.busy() {
            self.events.push(Event::LeaveModal);
            self.phase = -1;
        }
        for r in &mut self.rows {
            r.slide.update(dt);
            r.flash.update(dt);
        }
        self.detail_slide.update(dt);
        self.detail_fade.update(dt);
        let minutes = (play_ticks / 3600.) as u32;
        let (h, m) = if minutes >= 60_000 {
            (999, 59)
        } else {
            (minutes / 60, minutes % 60)
        };
        self.play_time = if h == 0 {
            format!("{m}分")
        } else {
            format!("{h}時間 {m}分")
        };
    }
    pub fn gesture(&mut self, velocity: [f32; 2]) {
        let [x, y] = velocity;
        let magnitude = x.mul_add(x, y * y).sqrt();
        let degrees = (y.atan2(x) / f32::from_bits(0x40490fd8)) * 180.;
        if magnitude > 20. && (degrees > 150. || degrees < -150.) {
            self.sounds.push("cancel");
            self.close();
        }
    }
    pub fn hit(&self, p: [f32; 2]) -> i32 {
        for i in 0..5 {
            if p[0] >= 24.
                && p[0] < 488.
                && p[1] >= 72. + i as f32 * 120.
                && p[1] < 184. + i as f32 * 120.
            {
                return i as i32;
            }
        }
        -1
    }
    pub fn probe(&mut self, p: [f32; 2]) -> bool {
        self.pointer_row = self.hit(p);
        true
    }
    pub fn reset(&mut self) {
        self.pointer_row = -1;
    }
    pub fn press(&mut self) {
        if self.pointer_row >= 0 {
            let i = self.pointer_row as usize;
            self.rows[i].flash.target(0.7, 0.2, 0, true);
            self.selected = i;
        }
    }
    pub fn release(&mut self) {
        if self.pointer_row >= 0 {
            self.selected = self.pointer_row as usize;
            self.decide();
        } else {
            self.sounds.push("cancel");
            self.close();
        }
    }
    pub fn cancel(&mut self) {
        if self.pointer_row >= 0 {
            self.rows[self.pointer_row as usize]
                .flash
                .target(0., 0.2, 0, true);
            self.pointer_row = -1;
        }
    }
}
