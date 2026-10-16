//! Ger Viewer4D0D4/4D304/4DC30/4DB3C. Authored locked variants are skipped.
//! Capture/publication is split at the renderer boundary; no ready picture is fabricated.
use super::{Entry, motion::Motion};
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub confirm: bool,
    pub cancel: bool,
    pub fit: bool,
    pub info: bool,
    pub left: bool,
    pub right: bool,
    pub up: bool,
    pub down: bool,
    pub axes: [f32; 2],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Load(u16),
    CaptureForPublication,
    CaptureForClose,
}
#[derive(Clone, Debug)]
pub struct Viewer {
    pub entry: Entry,
    pub variant: i32,
    pub closing: bool,
    pub finished: bool,
    pub show_info: bool,
    pub fit: bool,
    pub velocity: [f32; 2],
    pub offset: [f32; 2],
    pub extent: Option<[f32; 2]>,
    pub placement: [Motion; 4],
    pub capture_elapsed: f32,
    loading: bool,
    pending: Vec<Event>,
    sounds: Vec<&'static str>,
    waiting: bool,
    capture_blocking: bool,
}
impl Viewer {
    pub fn new(entry: Entry) -> Result<Self, &'static str> {
        if entry.locked()
            || entry.pictures.is_empty()
            || entry.pictures.len() != entry.unlocked.len()
        {
            return Err("Locked or malformed CG viewer group");
        }
        let mut s = Self {
            entry,
            variant: -1,
            closing: false,
            finished: false,
            show_info: true,
            fit: false,
            velocity: [0.; 2],
            offset: [0.; 2],
            extent: None,
            placement: [
                Motion::new(0.),
                Motion::new(0.),
                Motion::new(1000.),
                Motion::new(1000.),
            ],
            capture_elapsed: 30.,
            loading: false,
            pending: vec![],
            sounds: vec![],
            waiting: false,
            capture_blocking: false,
        };
        s.next();
        Ok(s)
    }
    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.pending)
    }
    pub fn take_sounds(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.sounds)
    }
    pub fn accepts_input(&self) -> bool {
        !self.loading && !self.waiting && !self.capture_blocking && !self.closing && !self.finished
    }
    pub fn has_picture(&self) -> bool {
        self.extent.is_some()
    }
    fn next(&mut self) {
        self.velocity = [0.; 2];
        let mut next = self.variant + 1;
        while (next as usize) < self.entry.pictures.len() && !self.entry.unlocked[next as usize] {
            next += 1;
        }
        if next as usize >= self.entry.pictures.len() {
            self.variant = -1;
            self.close();
        } else {
            self.variant = next;
            self.loading = true;
            self.pending
                .push(Event::Load(self.entry.pictures[next as usize]));
        }
    }
    fn close(&mut self) {
        self.closing = true;
        self.show_info = false;
        self.waiting = true;
        self.pending.push(Event::CaptureForClose);
    }
    /// Original Ready hides badges only during composed-parent capture, then installs
    /// the real Picture and centers its property10/11,20/21 bounds. Header origins cancel.
    pub fn picture_ready(&mut self) {
        self.show_info = false;
        self.waiting = true;
        self.pending.push(Event::CaptureForPublication);
    }
    pub fn publish(&mut self, extent: [f32; 2]) {
        self.extent = Some(extent);
        self.apply_bounds(false);
        self.show_info = true;
        self.loading = false;
        self.waiting = false;
        self.capture_elapsed = 0.;
        self.capture_blocking = true;
    }
    pub fn publish_close(&mut self) {
        self.extent = None;
        self.waiting = false;
        self.capture_elapsed = 0.;
        self.capture_blocking = true;
    }
    pub fn transform(&self) -> [f32; 4] {
        std::array::from_fn(|i| self.placement[i].value())
    }
    pub fn capture_opacity(&self) -> f32 {
        (1. - self.capture_elapsed / 30.).max(0.)
    }
    pub fn apply_bounds(&mut self, animated: bool) {
        let Some([width, height]) = self.extent else {
            return;
        };
        let values = if self.fit {
            let ratio = (width / 1920.).max(height / 1080.);
            [0., 0., 1000. / ratio, 1000. / ratio]
        } else {
            let half_x = width * (-0.5);
            let half_y = height * (-0.5);
            self.offset[0] = self.offset[0].clamp(
                (half_x + 960.).min(0.),
                ((width + half_x) + (-960.)).max(0.),
            );
            self.offset[1] = self.offset[1].clamp(
                (half_y + 540.).min(0.),
                ((height + half_y) + (-540.)).max(0.),
            );
            [self.offset[0], -self.offset[1], 1000., 1000.]
        };
        for (i, v) in values.into_iter().enumerate() {
            if animated {
                if self.placement[i].busy() {
                    self.placement[i].enqueue(v, 10., 0, false);
                } else {
                    self.placement[i].target(v, 10., 0, false);
                }
            } else {
                self.placement[i].snap(v);
            }
        }
    }
    fn pan_allowed(&self) -> bool {
        self.extent.is_some() && !self.fit && !self.placement[2].busy()
    }
    pub fn controls(&mut self, permission: bool, captured: bool, input: Input) {
        if !permission || captured {
            return;
        }
        if input.cancel {
            self.sounds.push("cancel");
            self.close();
        } else if input.confirm {
            self.sounds.push("decide");
            self.next();
        } else if input.fit {
            if self.extent.is_some() {
                self.fit = !self.fit;
                self.apply_bounds(true);
            }
            self.velocity = [0.; 2];
        } else {
            if input.info {
                self.show_info = !self.show_info;
            }
            if self.pan_allowed() {
                let dx = if input.left {
                    16.
                } else if input.right {
                    -16.
                } else {
                    input.axes[0] * (-32.)
                };
                let dy = if input.up {
                    -16.
                } else if input.down {
                    16.
                } else {
                    input.axes[1] * 32.
                };
                if dx != 0. || dy != 0. {
                    self.velocity = [0.; 2];
                    self.offset[0] += dx;
                    self.offset[1] += dy;
                }
                self.apply_bounds(false);
            }
        }
    }
    /// Original inertia takes a position step even when delta0, then pow(.95,delta).
    pub fn update(&mut self, dt: f32) {
        if !dt.is_finite() || dt < 0. {
            return;
        }
        if self.velocity != [0.; 2] {
            if self.pan_allowed() {
                self.offset[0] += self.velocity[0];
                self.offset[1] -= self.velocity[1];
                let decay = f32::from_bits(0x3f733333).powf(dt);
                self.velocity[0] *= decay;
                self.velocity[1] *= decay;
                self.apply_bounds(false);
            } else {
                self.velocity = [0.; 2];
            }
        }
        for p in &mut self.placement {
            p.update(dt);
        }
        let was_done = self.capture_elapsed >= 30.;
        self.capture_elapsed = (self.capture_elapsed + dt).min(30.);
        if self.capture_blocking && was_done {
            self.capture_blocking = false;
        }
        if self.closing && !self.waiting && was_done {
            self.finished = true;
        }
    }
    pub fn probe(&mut self) {
        self.velocity = [0.; 2];
    }
    pub fn release(&mut self) {
        self.sounds.push("decide");
        self.next();
    }
    pub fn drag(&mut self, old: [f32; 2], new: [f32; 2]) {
        if self.has_picture() && !self.fit {
            self.offset[0] += new[0] - old[0];
            self.offset[1] -= (new[1] - old[1]) * 0.5625;
            self.apply_bounds(false);
        }
    }
    pub fn gesture(&mut self, v: [f32; 2]) {
        if self.has_picture() && !self.fit {
            self.velocity = [v[0], v[1] * 0.5625];
        }
    }
}
