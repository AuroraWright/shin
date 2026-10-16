//! Ger98680/98714/98E10/98E50's gallery subset: linear, sine and cosine queues.
use std::collections::VecDeque;
#[derive(Clone, Debug)]
pub struct Motion {
    from: f32,
    to: f32,
    elapsed: f32,
    duration: f32,
    curve: u8,
    direct: bool,
    queue: VecDeque<(f32, f32, u8, bool)>,
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
    fn start(&mut self, value: f32, time: f32, curve: u8, speed: bool) {
        self.from = self.value();
        self.direct = false;
        self.to = value;
        self.elapsed = 0.;
        self.curve = curve;
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
    pub fn target(&mut self, value: f32, time: f32, curve: u8, speed: bool) {
        self.snap(self.value());
        self.start(value, time, curve, speed);
    }
    pub fn enqueue(&mut self, value: f32, time: f32, curve: u8, speed: bool) {
        self.queue.push_back((value, time, curve, speed));
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
pub fn flash(m: &mut Motion, release: bool) {
    m.target(0.7, 0.2, 0, true);
    if release {
        m.enqueue(0., 0.2, 0, true);
    }
}
