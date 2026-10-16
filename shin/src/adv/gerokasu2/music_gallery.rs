//! Ger BGMMode34C80/351A0/361CC. Catalog rows snapshot persistent BGM bits;
//! capture transport, audio players and outer mode publication belong to the app.
use std::collections::VecDeque;
#[path = "music_gallery/artwork.rs"]
pub mod artwork;
#[cfg(test)]
#[path = "music_gallery/tests.rs"]
mod tests;
pub const ATLAS: &str = "/bgmmode.txa";
pub const MASK: &str = "/change.msk";
pub const TEXTURES: &[&str] = &[
    "bg",
    "shadow",
    "plate",
    "cur",
    "flash",
    "top",
    "caption",
    "button_shadow",
    "close",
    "help_bgmmode",
    "key",
];
#[derive(Clone, Copy, Debug)]
pub struct CatalogItem {
    pub bgm: u16,
    pub name: u16,
    pub once: u16,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub confirm: bool,
    pub cancel: bool,
    pub stop: bool,
    pub left: bool,
    pub right: bool,
    pub up: bool,
    pub down: bool,
    pub previous: bool,
    pub next: bool,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    Cue(&'static str),
    Play {
        bgm: u16,
        once: u16,
    },
    Stop(u32),
    ///378D0 queues CreateTitle(9), without an explicit BGM stop.
    ReturnTitle {
        requested_id: i32,
    },
    ///E29A0(14) is a platform notification, never an extra save unlock.
    Achievement(u32),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Loading,
    Opening,
    Active,
    Closing,
    Closed,
}
#[derive(Clone, Debug)]
pub struct Motion {
    from: f32,
    to: f32,
    elapsed: f32,
    duration: f32,
    queue: VecDeque<(f32, f32, bool)>,
}
impl Motion {
    fn new(value: f32) -> Self {
        Self {
            from: value,
            to: value,
            elapsed: 1.,
            duration: 1.,
            queue: VecDeque::new(),
        }
    }
    pub fn value(&self) -> f32 {
        (self.to - self.from).mul_add(self.elapsed / self.duration, self.from)
    }
    pub fn busy(&self) -> bool {
        self.elapsed < self.duration || !self.queue.is_empty()
    }
    fn snap(&mut self, value: f32) {
        *self = Self::new(value);
    }
    fn start(&mut self, value: f32, time: f32, speed: bool) {
        self.from = self.value();
        self.to = value;
        self.elapsed = 0.;
        self.duration = if speed {
            ((value - self.from) / time).abs()
        } else {
            time
        };
        if self.duration == 0. {
            self.elapsed = 1.;
            self.duration = 1.;
        }
    }
    fn control(&mut self, value: f32, time: f32, speed: bool) {
        if self.busy() {
            self.queue.push_back((value, time, speed));
        } else {
            self.start(value, time, speed);
        }
    }
    fn clear(&mut self) {
        self.snap(self.value());
    }
    fn flash(&mut self, release: bool) {
        self.clear();
        self.control(0.7, 0.2, true);
        if release {
            self.control(0., 0.2, true);
        }
    }
    fn release(&mut self) {
        self.clear();
        self.control(0., 0.2, true);
    }
    fn update(&mut self, dt: f32) {
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
            if let Some((v, t, s)) = self.queue.pop_front() {
                self.start(v, t, s);
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
    pub item: CatalogItem,
    pub name: u16,
    pub unlocked: bool,
    pub flash: Motion,
}
#[derive(Clone, Debug)]
pub struct Controller {
    pub rows: Vec<Row>,
    pub selected: i32,
    pub playing: i32,
    pub closing: bool,
    pub region: i32,
    /// Native contact: -1 absent,0 scroll region,positive index+1.
    pub hover: i32,
    pub velocity: f32,
    pub document_height: i32,
    pub scroll: Motion,
    pub close_flash: Motion,
    pub phase: Phase,
    pub elapsed: f32,
    pub opening_elapsed: f32,
    pub scrollbar_position: i32,
    ready: bool,
    pub disabled: u32,
    actions: Vec<Action>,
}
impl Controller {
    ///34C80 walks all entries until BD028 returns null; no append filtering.
    pub fn new(items: &[CatalogItem], unlocked: &[u32]) -> Result<Self, &'static str> {
        if items.is_empty() || items.len() > i32::MAX as usize / 100 {
            return Err("Ger Music catalog is empty or too large");
        }
        let rows: Vec<_> = items
            .iter()
            .copied()
            .map(|item| {
                let available = unlocked
                    .get(usize::from(item.bgm) / 32)
                    .is_some_and(|word| word & (1 << (item.bgm % 32)) != 0);
                Row {
                    item,
                    name: if available { item.name } else { 0 },
                    unlocked: available,
                    flash: Motion::new(0.),
                }
            })
            .collect();
        let actions = if rows.iter().all(|r| r.unlocked) {
            vec![Action::Achievement(14)]
        } else {
            vec![]
        };
        Ok(Self {
            document_height: (rows.len().div_ceil(2) * 100) as i32,
            rows,
            selected: 0,
            playing: -1,
            closing: false,
            region: -1,
            hover: -1,
            velocity: 0.,
            scroll: Motion::new(0.),
            close_flash: Motion::new(0.),
            phase: Phase::Loading,
            elapsed: 0.,
            opening_elapsed: 0.,
            scrollbar_position: 0,
            ready: false,
            disabled: 0,
            actions,
        })
    }
    pub fn assets_ready(&mut self) {
        self.ready = true;
    }
    /// Desktop preparation failure at the outer publication boundary. Native
    /// C++ unwind behavior is not asserted; retain rows/scroll/current playback.
    pub fn recover_return(&mut self) {
        self.phase = Phase::Active;
        self.closing = false;
        self.close_flash.snap(0.);
        self.actions
            .retain(|action| !matches!(action, Action::ReturnTitle { .. }));
    }
    pub fn accepts_input(&self) -> bool {
        self.phase == Phase::Active
    }
    pub fn take_actions(&mut self) -> Vec<Action> {
        std::mem::take(&mut self.actions)
    }
    pub fn scroll(&self) -> f32 {
        self.scroll.value()
    }
    pub fn flash(&self) -> f32 {
        self.close_flash.value()
    }
    fn reveal(&mut self) {
        let top = ((self.selected / 2) * 100) as f32;
        let current = self.scroll();
        let target = if top - current < 0. {
            top.max(0.)
        } else if (top - current) + 100. > 800. {
            (top + 100. - 800.).min(self.document_height as f32)
        } else {
            current
        };
        self.scroll.control(target, 4., false);
    }
    fn correct_selection(&mut self, value: f32) {
        let mut selected = self.selected;
        while value > ((selected / 2) * 100) as f32 {
            selected += 2;
        }
        while value + 800. < ((selected / 2) * 100) as f32 + 100. {
            selected -= 2;
        }
        if selected >= 0 && (selected as usize) < self.rows.len() {
            self.selected = selected;
        }
    }
    fn scroll_to(&mut self, value: f32) {
        let value = value.min((self.document_height - 800) as f32).max(0.);
        self.correct_selection(value);
        self.scroll.snap(value);
    }
    pub fn play(&mut self) {
        let row = &mut self.rows[self.selected as usize];
        row.flash.flash(true);
        if row.unlocked {
            self.actions.push(Action::Play {
                bgm: row.item.bgm,
                once: row.item.once,
            });
            self.playing = self.selected;
            self.velocity = 0.;
        } else {
            self.actions.push(Action::Cue("error"));
        }
    }
    fn close(&mut self) {
        self.closing = true;
        self.phase = Phase::Closing;
        self.close_flash.flash(true);
        self.velocity = 0.;
    }
    ///361CC always consumes permission; captured input and scroll tween gate keys.
    pub fn poll_controls(&mut self, permission: bool, captured: bool, input: Input) -> bool {
        if !permission {
            self.disabled += 1;
        } else if !captured && !self.scroll.busy() {
            if input.cancel {
                self.actions.push(Action::Cue("cancel"));
                self.close();
            } else if input.confirm {
                self.play();
            } else if input.stop {
                if self.playing >= 0 {
                    self.actions.push(Action::Stop(30));
                    self.playing = -1;
                }
            } else {
                let dx = if input.left {
                    -1
                } else {
                    i32::from(input.right)
                };
                let dy = if input.up {
                    -1
                } else if input.down {
                    1
                } else if input.previous {
                    -8
                } else if input.next {
                    8
                } else {
                    0
                };
                if dx | dy != 0 {
                    let column = (self.selected % 2 + dx).clamp(0, 1);
                    let row =
                        (self.selected / 2 + dy).clamp(0, self.rows.len().div_ceil(2) as i32 - 1);
                    let selected = (row * 2 + column).min(self.rows.len() as i32 - 1);
                    if selected != self.selected {
                        self.actions.push(Action::Cue("move"));
                        self.selected = selected;
                        self.velocity = 0.;
                        self.reveal();
                    }
                }
            }
        }
        !self.closing
    }
    ///378D0 observes the previous frame's flash and queues Title9 directly.
    pub fn poll_close(&mut self) -> bool {
        if self.close_flash.busy() {
            return true;
        }
        self.actions.push(Action::ReturnTitle { requested_id: 9 });
        false
    }
    ///351A0: native inertia uses velocity*delta, sampled scrollbar precedes tween.
    pub fn advance_clocks(&mut self, dt: f32, bgm_playing: bool) {
        if self.velocity.abs() >= f32::EPSILON {
            let value = (-self.velocity)
                .mul_add(dt, self.scroll())
                .min((self.document_height - 800) as f32)
                .max(0.);
            if (self.scroll() - value).abs() >= f32::EPSILON {
                self.scroll_to(value);
                self.velocity *= 0.95f32.powf(dt);
                if self.velocity.abs() < 0.1 {
                    self.velocity = 0.;
                }
            } else {
                self.velocity = 0.;
            }
        }
        self.scrollbar_position = self.scroll() as i32;
        self.scroll.update(dt);
        if self.playing >= 0 && !bgm_playing {
            self.playing = -1;
        }
        self.close_flash.update(dt);
        for row in &mut self.rows {
            row.flash.update(dt);
        }
        self.elapsed = dt.mul_add(f32::from_bits(0x3c888889), self.elapsed);
    }
    /// Poll native private owners before the parent audio/status/tween tail.
    /// A concrete host dispatches actions between this and advance_clocks.
    pub fn poll_frame(&mut self, dt: f32, permission: bool, captured: bool, input: Input) {
        if !dt.is_finite() || dt < 0. {
            return;
        }
        match self.phase {
            Phase::Loading if self.ready => {
                self.phase = Phase::Opening;
                self.opening_elapsed = 0.;
                self.actions.push(Action::Stop(60));
            }
            Phase::Opening if self.opening_elapsed >= 60. => self.phase = Phase::Active,
            Phase::Active => {
                self.poll_controls(permission, captured, input);
            }
            Phase::Closing => {
                if !self.poll_close() {
                    self.phase = Phase::Closed;
                }
            }
            _ => {}
        }
        if self.phase == Phase::Opening {
            self.opening_elapsed = (self.opening_elapsed + dt).min(60.);
        }
    }
    pub fn update(
        &mut self,
        dt: f32,
        permission: bool,
        captured: bool,
        input: Input,
        bgm_playing: bool,
    ) {
        if !dt.is_finite() || dt < 0. {
            return;
        }
        self.poll_frame(dt, permission, captured, input);
        self.advance_clocks(dt, bgm_playing);
    }
    pub fn probe(&mut self, x: f32, y: f32) -> bool {
        self.hover = -1;
        self.region = if (46.0..154.0).contains(&y) && (1766.0..1874.0).contains(&x) {
            1
        } else if (160.0..992.0).contains(&y) {
            -1
        } else {
            0
        };
        if self.region < 0 {
            if y >= 976. || x < 160. || x >= 1760. || y < 176. {
                return false;
            }
            let column = if (204.0..916.0).contains(&x) {
                0
            } else if (1004.0..1716.0).contains(&x) {
                1
            } else {
                -1
            };
            self.hover = 0;
            if column >= 0 {
                let yy = ((self.scroll() + -176.) + y) as i32;
                let row = yy / 100;
                if (6..=93).contains(&(yy % 100)) {
                    let index = row * 2 + column;
                    if index >= -1 && (index as usize) < self.rows.len() {
                        self.hover = index + 1;
                    }
                }
            }
            if self.velocity != 0. {
                self.hover = 0;
                self.velocity = 0.;
            }
        }
        true
    }
    pub fn reset(&mut self) {
        self.region = -1;
        self.hover = -1;
    }
    pub fn press(&mut self) {
        if self.region == 1 {
            self.close_flash.flash(false);
        } else if self.region < 0 && self.hover > 0 && (self.hover as usize) <= self.rows.len() {
            self.selected = self.hover - 1;
            self.rows[self.selected as usize].flash.flash(false);
            self.scroll.clear();
            self.reveal();
            self.velocity = 0.;
        }
    }
    pub fn release(&mut self) {
        if self.region == 1 {
            self.actions.push(Action::Cue("decide"));
            self.close();
        } else if self.region < 0 && self.hover > 0 && (self.hover as usize) <= self.rows.len() {
            self.selected = self.hover - 1;
            self.scroll.clear();
            self.reveal();
            self.velocity = 0.;
            self.play();
        }
    }
    pub fn drag(&mut self, previous_y: f32, current_y: f32) {
        if self.region >= 0 {
            self.close_flash.release();
            self.region = 0;
        } else if self.hover >= 0 {
            if self.hover > 0 {
                self.rows[self.hover as usize - 1].flash.release();
                self.hover = 0;
            }
            self.scroll_to(self.scroll() - (current_y - previous_y));
        }
    }
    pub fn gesture(&mut self, velocity_y: f32) {
        if self.region < 0 && self.hover >= 0 {
            self.velocity = velocity_y;
        }
    }
    pub fn cancel(&mut self) {
        if self.region >= 0 {
            self.close_flash.release();
            self.region = 0;
        } else if self.hover > 0 && (self.hover as usize) <= self.rows.len() {
            self.rows[self.hover as usize - 1].flash.release();
        }
    }
}
