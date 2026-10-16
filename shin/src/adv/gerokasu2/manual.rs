//! Ger Manual79024/79350/79FD0 and private opening/close tasks.
//! Pure CPU state; asset transport, pointer capture and parent ownership live in the host.
use std::collections::VecDeque;
#[path = "manual/artwork.rs"]
pub mod artwork;
#[cfg(test)]
#[path = "manual/tests.rs"]
mod tests;
pub const ATLAS: &str = "/manual.txa";
pub const TEXTURES: &[&str] = &[
    "bg",
    "button_shadow",
    "caption",
    "close",
    "top",
    "help_manual",
    "key",
];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Loading,
    Opening,
    Active,
    CloseFlash,
    Closing,
    Closed,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub left: bool,
    pub right: bool,
    pub previous: bool,
    pub next: bool,
    pub close: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    ReturnSource(u32),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    First,
    Open,
    Input,
    CloseWait,
    Close,
}
#[derive(Clone, Copy, Debug)]
pub struct Task {
    pub kind: Kind,
    pub done: bool,
}
#[derive(Clone, Debug)]
struct Motion {
    from: f32,
    to: f32,
    elapsed: f32,
    duration: f32,
    sine: bool,
    queue: VecDeque<(f32, f32, bool)>,
}
impl Motion {
    fn new(v: f32) -> Self {
        Self {
            from: v,
            to: v,
            elapsed: 1.,
            duration: 1.,
            sine: false,
            queue: VecDeque::new(),
        }
    }
    fn value(&self) -> f32 {
        let t = self.elapsed / self.duration;
        let t = if self.sine {
            (f32::from_bits(0x3fc90fdb) * t).sin()
        } else {
            t
        };
        (self.to - self.from).mul_add(t, self.from)
    }
    fn busy(&self) -> bool {
        self.elapsed < self.duration || !self.queue.is_empty()
    }
    fn snap(&mut self, v: f32) {
        *self = Self::new(v);
    }
    fn start(&mut self, v: f32, speed: f32, sine: bool) {
        self.from = self.value();
        self.to = v;
        self.elapsed = 0.;
        self.duration = ((v - self.from) / speed).abs();
        self.sine = sine;
        if self.duration == 0. {
            self.elapsed = 1.;
            self.duration = 1.;
        }
    }
    fn target(&mut self, v: f32, speed: f32, sine: bool) {
        self.snap(self.value());
        self.start(v, speed, sine);
    }
    fn enqueue(&mut self, v: f32, speed: f32, sine: bool) {
        self.queue.push_back((v, speed, sine));
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
            if let Some((v, s, e)) = self.queue.pop_front() {
                self.start(v, s, e);
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
pub struct Controller {
    pub phase: Phase,
    pub progress: f32,
    pub count: usize,
    pub selected: i32,
    pub contact: i32,
    pub swipe: bool,
    pub closing: bool,
    pub complete: bool,
    pub tasks: Vec<Task>,
    ready: bool,
    available: usize,
    scroll: Motion,
    flash: Motion,
    sounds: Vec<&'static str>,
    actions: Vec<Action>,
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
            phase: Phase::Loading,
            progress: 0.,
            count: 0,
            selected: 0,
            contact: -1,
            swipe: false,
            closing: false,
            complete: false,
            tasks: vec![Task {
                kind: Kind::First,
                done: false,
            }],
            ready: false,
            available: 0,
            scroll: Motion::new(0.),
            flash: Motion::new(0.),
            sounds: vec![],
            actions: vec![],
            disabled: 0,
        }
    }
    pub fn assets_ready(&mut self, count: usize) {
        assert!(count <= i32::MAX as usize);
        self.available = count;
        self.ready = true;
    }
    pub fn resources_ready(&self) -> bool {
        self.ready
    }
    pub fn scroll(&self) -> f32 {
        self.scroll.value()
    }
    pub fn scroll_busy(&self) -> bool {
        self.scroll.busy()
    }
    pub fn close_flash(&self) -> f32 {
        self.flash.value()
    }
    pub fn flash_busy(&self) -> bool {
        self.flash.busy()
    }
    pub fn accepts_input(&self) -> bool {
        self.phase == Phase::Active
    }
    pub fn take_sounds(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.sounds)
    }
    pub fn take_actions(&mut self) -> Vec<Action> {
        std::mem::take(&mut self.actions)
    }
    fn retarget(&mut self) {
        self.scroll
            .target(self.selected.wrapping_mul(1500) as f32, 100., true);
    }
    fn close(&mut self) {
        self.closing = true;
        self.phase = Phase::CloseFlash;
        self.flash.target(0.7, 0.2, false);
        self.flash.enqueue(0., 0.2, false);
        self.tasks.push(Task {
            kind: Kind::CloseWait,
            done: false,
        });
    }
    /// Actual79FD0 consumes permission even while capture or scroll blocks controls.
    /// Direction priority is Left26, Right27, L10, R11, then B29 if the page stays put.
    pub fn poll_controls(&mut self, permission: bool, captured: bool, input: Input) -> bool {
        if !permission {
            self.disabled += 1;
        } else if !captured && !self.scroll.busy() {
            let next = if input.left {
                self.selected.wrapping_sub(1)
            } else if input.right {
                self.selected.wrapping_add(1)
            } else if input.previous {
                self.selected.wrapping_sub(1)
            } else {
                self.selected.wrapping_add(i32::from(input.next))
            };
            let page = if next < 0 {
                0
            } else {
                next.min(self.count as i32 - 1)
            };
            if page != self.selected {
                self.selected = page;
                self.retarget();
                self.sounds.push("move");
            } else if input.close {
                self.sounds.push("cancel");
                self.close();
            }
        }
        !self.closing
    }
    /// Native scope prunes completed tasks before polling a fixed task snapshot;
    /// newly appended Open/Input/Close tasks start on the next update. Clocks follow.
    pub fn update(&mut self, dt: f32, permission: bool, captured: bool, input: Input) {
        if !dt.is_finite() || dt < 0. {
            return;
        }
        self.tasks.retain(|t| !t.done);
        let count = self.tasks.len();
        let mut permission = permission;
        for i in (0..count).rev() {
            let done = match self.tasks[i].kind {
                Kind::First => {
                    if self.ready {
                        self.count = self.available;
                        self.phase = Phase::Opening;
                        self.tasks.push(Task {
                            kind: Kind::Open,
                            done: false,
                        });
                        true
                    } else {
                        false
                    }
                }
                Kind::Open => {
                    self.progress = dt.mul_add(0.1, self.progress);
                    if self.progress >= 1. {
                        self.progress = 1.;
                        self.phase = Phase::Active;
                        self.tasks.push(Task {
                            kind: Kind::Input,
                            done: false,
                        });
                        true
                    } else {
                        false
                    }
                }
                Kind::Input => {
                    let keep = self.poll_controls(permission, captured, input);
                    permission = false;
                    !keep
                }
                Kind::CloseWait => {
                    permission = false;
                    if !self.flash.busy() {
                        self.actions.push(Action::ReturnSource(0));
                        self.phase = Phase::Closing;
                        self.tasks.push(Task {
                            kind: Kind::Close,
                            done: false,
                        });
                        true
                    } else {
                        false
                    }
                }
                Kind::Close => {
                    self.progress = dt.mul_add(-0.1, self.progress);
                    if self.progress <= 0. {
                        self.progress = 0.;
                        self.actions.push(Action::ReturnSource(1));
                        self.complete = true;
                        self.phase = Phase::Closed;
                        true
                    } else {
                        false
                    }
                }
            };
            self.tasks[i].done = done;
        }
        self.scroll.update(dt);
        self.flash.update(dt);
    }
    /// Ger close is108x108 at1766,46; page eligibility is Y-only,162..980.
    pub fn probe(&mut self, x: f32, y: f32) -> bool {
        self.swipe = false;
        self.contact = if (46.0..154.0).contains(&y) && (1766.0..1874.0).contains(&x) {
            1
        } else if (162.0..980.0).contains(&y) {
            -1
        } else {
            0
        };
        true
    }
    pub fn reset(&mut self) {
        if self.contact < 0 && !self.swipe {
            self.selected = (self.scroll() / 1500. + 0.5) as i32;
            self.retarget();
        }
        self.contact = -1;
    }
    pub fn press(&mut self) {
        if self.contact == 1 {
            self.flash.target(0.7, 0.2, false);
        }
    }
    pub fn release(&mut self) {
        if self.contact == 1 {
            self.sounds.push("decide");
            self.close();
        }
    }
    pub fn cancel(&mut self) {
        if self.contact >= 0 {
            self.flash.target(0., 0.2, false);
            self.contact = 0;
        }
    }
    pub fn drag(&mut self, previous_x: f32, current_x: f32) {
        if self.contact >= 0 {
            self.cancel();
            return;
        }
        let next = self.scroll() - (current_x - previous_x);
        let max = (self.count as i32 - 1).wrapping_mul(1500) as f32;
        self.scroll.snap(if next < 0. {
            0.
        } else if next > max {
            (self.count as i32 - 1) as f32 * 1500.
        } else {
            next
        });
    }
    pub fn gesture(&mut self, x: f32, y: f32) {
        if self.contact >= 0 {
            return;
        }
        let norm = x.mul_add(x, y * y).sqrt();
        if norm <= 20. || norm.is_nan() {
            return;
        }
        let angle = y.atan2(x) / f32::from_bits(0x40490fd8) * 180.;
        if angle > 150. || angle < -150. {
            self.selected = self.selected.wrapping_add(1).min(self.count as i32 - 1);
        } else if angle < 30. && angle > -30. {
            self.selected = self.selected.saturating_sub(1).max(0);
        } else {
            return;
        }
        self.retarget();
        self.swipe = true;
    }
}
