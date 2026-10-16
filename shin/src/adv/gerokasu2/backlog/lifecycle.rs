//! MessageBrowser Start/Close/Exit and delayed scene-jump lifecycle.
//! Begin D0 tasks, process Main/chapter/Question, then end the parent tweens.
use std::collections::VecDeque;
#[cfg(test)]
#[path = "lifecycle/tests.rs"]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Opening,
    Active,
    CloseWait,
    JumpWait,
    Question,
    Fading,
    Done,
    Retired,
    Published,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    MainInstalled,
    ShowJumpQuestion {
        row: usize,
    },
    ///88E10 mutates the frozen jump payload at final Yes, before Exit.
    JumpAccepted {
        row: usize,
    },
    CancelHistory,
    ParentReturn {
        reason: i32,
    },
    ///88C74 calls fresh ADV(mode0) then queues outer replacement.
    PublishJump {
        row: usize,
    },
    Done,
    /// Outer TaskGroup retires the done owner at its next frame's sweep.
    Retire,
}
#[derive(Clone)]
struct Flash {
    from: f32,
    to: f32,
    elapsed: f32,
    duration: f32,
    queue: VecDeque<f32>,
}
impl Flash {
    fn new() -> Self {
        Self {
            from: 0.,
            to: 0.,
            elapsed: 1.,
            duration: 1.,
            queue: VecDeque::new(),
        }
    }
    fn value(&self) -> f32 {
        (self.to - self.from).mul_add(self.elapsed / self.duration, self.from)
    }
    fn busy(&self) -> bool {
        self.elapsed < self.duration || !self.queue.is_empty()
    }
    fn start(&mut self, to: f32) {
        self.from = self.value();
        self.to = to;
        self.elapsed = 0.;
        self.duration = ((to - self.from) / 0.2).abs();
        if self.duration == 0. {
            self.elapsed = 1.;
            self.duration = 1.;
        }
    }
    fn set(&mut self, to: f32) {
        self.queue.clear();
        self.start(to);
    }
    fn pulse(&mut self) {
        self.set(0.7);
        self.queue.push_back(0.);
    }
    fn update(&mut self, delta: f32) {
        if delta <= 0. {
            return;
        }
        let mut left = delta;
        loop {
            let remaining = self.duration - self.elapsed;
            if left < remaining {
                self.elapsed += left;
                break;
            }
            self.elapsed = self.duration;
            left -= remaining;
            if let Some(to) = self.queue.pop_front() {
                self.start(to);
            } else {
                break;
            }
            if left <= 0. {
                break;
            }
        }
    }
}
pub struct Controller {
    phase: Phase,
    progress: f32,
    blocked: bool,
    cancelled: bool,
    jump: bool,
    selected: usize,
    cached: Vec<bool>,
    close: Flash,
    jump_flashes: Vec<Flash>,
    portrait: Vec<Flash>,
}
impl Controller {
    /// Call after First84374 has prepared assets/history and invokes Start8281C.
    pub fn new(row_count: usize) -> Self {
        Self {
            phase: Phase::Opening,
            progress: 0.,
            blocked: true,
            cancelled: false,
            jump: false,
            selected: 0,
            cached: vec![false; row_count],
            close: Flash::new(),
            jump_flashes: vec![Flash::new(); row_count],
            portrait: vec![Flash::new(); row_count],
        }
    }
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn progress(&self) -> f32 {
        self.progress
    }
    pub fn accepts_main(&self) -> bool {
        self.phase == Phase::Active && !self.blocked
    }
    pub fn close_flash(&self) -> f32 {
        self.close.value()
    }
    pub fn close_busy(&self) -> bool {
        self.close.busy()
    }
    pub fn history_cancelled(&self) -> bool {
        self.cancelled
    }
    pub fn portrait_flash(&self, row: usize) -> f32 {
        self.portrait.get(row).map_or(0., Flash::value)
    }
    pub fn jump_flash(&self, row: usize) -> f32 {
        self.jump_flashes.get(row).map_or(0., Flash::value)
    }
    ///887C0 destroys both row tweens; recreation starts fresh clocks.
    pub fn sync_cached(&mut self, rows: &[usize]) {
        for i in 0..self.cached.len() {
            let cached = rows.contains(&i);
            if self.cached[i] != cached {
                self.jump_flashes[i] = Flash::new();
                self.portrait[i] = Flash::new();
            }
            self.cached[i] = cached;
        }
    }
    pub fn set_close_flash(&mut self, value: f32) {
        self.close.set(value);
    }
    pub fn set_portrait_flash(&mut self, row: usize, value: f32) {
        if self.cached.get(row) == Some(&true) {
            self.portrait[row].set(value);
        }
    }
    pub fn pulse_portrait(&mut self, row: usize) {
        if self.cached.get(row) == Some(&true) {
            self.portrait[row].pulse();
        }
    }
    ///82940 has no audio operation; keyboard Back stops replay before calling it,
    /// while the native pointer close path leaves replay running through fade.
    pub fn request_close(&mut self) {
        if self.accepts_main() {
            self.close.pulse();
            self.phase = Phase::CloseWait;
        }
    }
    pub fn request_quick_load(&mut self, row: usize, cached: bool) -> Vec<Event> {
        if !self.accepts_main() || row >= self.cached.len() {
            return vec![];
        }
        self.selected = row;
        self.cached[row] = cached;
        if cached {
            self.jump_flashes[row].pulse();
        }
        self.phase = Phase::JumpWait;
        vec![]
    }
    fn exit(&mut self) -> Vec<Event> {
        self.cancelled = true;
        self.phase = Phase::Fading;
        let mut events = vec![Event::CancelHistory];
        if !self.jump {
            events.push(Event::ParentReturn { reason: 0 });
        }
        events
    }
    pub fn request_jump(&mut self, row: usize) -> Vec<Event> {
        if self.phase != Phase::Question || row >= self.cached.len() {
            return vec![];
        }
        self.selected = row;
        self.jump = true;
        let mut events = vec![Event::JumpAccepted { row }];
        events.extend(self.exit());
        events
    }
    /// Only final stage1 Yes0 changes source positions and initiates Exit.
    pub fn answer(&mut self, final_stage: bool, button: i32) -> Vec<Event> {
        if self.phase != Phase::Question || !final_stage {
            return vec![];
        }
        if button == 0 {
            self.request_jump(self.selected)
        } else {
            self.phase = Phase::Active;
            vec![]
        }
    }
    /// D0 snapshot runs before108/Main. A completing blocking task still denies
    /// controls that frame; a newly queued fade first runs on the next begin.
    pub fn begin_frame(&mut self, delta: f32) -> Vec<Event> {
        self.blocked = matches!(
            self.phase,
            Phase::Opening | Phase::CloseWait | Phase::JumpWait | Phase::Question | Phase::Fading
        );
        match self.phase {
            Phase::Opening => {
                self.progress = delta.mul_add(f32::from_bits(0x3dcccccd), self.progress);
                if self.progress >= 1. {
                    self.progress = 1.;
                    self.phase = Phase::Active;
                    vec![Event::MainInstalled]
                } else {
                    vec![]
                }
            }
            Phase::CloseWait if !self.close.busy() => self.exit(),
            Phase::JumpWait
                if !self.cached[self.selected] || !self.jump_flashes[self.selected].busy() =>
            {
                self.phase = Phase::Question;
                vec![Event::ShowJumpQuestion { row: self.selected }]
            }
            Phase::Fading => {
                self.progress = delta.mul_add(f32::from_bits(0xbdcccccd), self.progress);
                if self.progress > 0. {
                    vec![]
                } else {
                    self.progress = 0.;
                    if self.jump {
                        self.phase = Phase::Published;
                        vec![Event::PublishJump { row: self.selected }]
                    } else {
                        self.phase = Phase::Done;
                        vec![Event::ParentReturn { reason: 1 }, Event::Done]
                    }
                }
            }
            Phase::Done => {
                self.phase = Phase::Retired;
                vec![Event::Retire]
            }
            _ => vec![],
        }
    }
    ///80C54..80C88 advances native speed-mode .2 units/tick motions after child
    /// processing. Do not pass these clocks to a generic duration.2 tween.
    pub fn end_frame(&mut self, delta: f32) {
        self.close.update(delta);
        for i in 0..self.cached.len() {
            if self.cached[i] {
                self.jump_flashes[i].update(delta);
                self.portrait[i].update(delta);
            }
        }
    }
    /// Desktop-only failed async destination recovery; native has no unwind.
    pub fn recover_failed_jump(&mut self) {
        if self.phase == Phase::Published {
            self.phase = Phase::Active;
            self.progress = 1.;
            self.blocked = true;
            self.cancelled = false;
            self.jump = false;
            self.close = Flash::new();
            for f in &mut self.jump_flashes {
                *f = Flash::new();
            }
        }
    }
}
