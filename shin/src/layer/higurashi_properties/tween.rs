//! Older tween storage evaluates endpoints on every read. Its queue retains
//! raw relative/speed controls until activation, unlike later cached values.
use shin_core::time::{Easing, Ticks, Tween};
use std::collections::VecDeque;

#[derive(Debug, Clone, Copy)]
struct Control {
    value: f32,
    time: Ticks,
    flags: u32,
    easing: Easing,
}

#[derive(Debug, Clone)]
pub struct HigurashiTweener {
    from: f32,
    to: f32,
    elapsed: Ticks,
    tween: Tween,
    protected: bool,
    queue: VecDeque<Control>,
}

impl HigurashiTweener {
    pub fn new(value: f32) -> Self {
        Self {
            from: value,
            to: value,
            elapsed: Ticks::from_f32(1.0),
            tween: Tween::linear(Ticks::from_f32(1.0)),
            protected: false,
            queue: VecDeque::new(),
        }
    }
    pub fn value(&self) -> f32 {
        self.from + (self.to - self.from) * self.tween.value(self.elapsed)
    }
    pub fn target_value(&self) -> f32 {
        self.queue.iter().fold(self.to, |value, control| {
            if control.flags & 0x80 != 0 {
                value + control.value
            } else {
                control.value
            }
        })
    }
    pub fn is_waiting(&self) -> bool {
        self.elapsed < self.tween.duration || !self.queue.is_empty()
    }
    pub fn is_idle(&self) -> bool {
        !self.is_waiting()
    }
    pub fn fast_forward_to(&mut self, value: f32) {
        self.from = value;
        self.to = value;
        self.elapsed = Ticks::from_f32(1.0);
        self.tween = Tween::linear(Ticks::from_f32(1.0));
        self.queue.clear();
    }
    fn activate(&mut self, control: Control) {
        self.from = self.to;
        self.to = if control.flags & 0x80 != 0 {
            self.from + control.value
        } else {
            control.value
        };
        let duration = if control.flags & 0x40 != 0 {
            if control.time > Ticks::ZERO && self.from != self.to {
                Ticks::from_f32(((self.to - self.from) / control.time.as_f32()).abs())
            } else {
                Ticks::ZERO
            }
        } else {
            control.time
        };
        (self.elapsed, self.tween.duration) = if duration <= Ticks::ZERO {
            (Ticks::from_f32(1.0), Ticks::from_f32(1.0))
        } else {
            (Ticks::ZERO, duration)
        };
        self.tween.easing = control.easing;
        self.protected = control.flags & 0x200 != 0;
    }
    pub fn control(&mut self, value: f32, time: Ticks, flags: u32) -> anyhow::Result<()> {
        let easing = match flags & 0x3f {
            0 => Easing::Linear,
            1 => Easing::SineIn,
            2 => Easing::SineOut,
            3 => Easing::SineInOut,
            mode => anyhow::bail!("Unsupported Higurashi easing {mode}"),
        };
        let mut control = Control {
            value,
            time,
            flags,
            easing,
        };
        if flags & 0x100 != 0 {
            if flags & 0x80 != 0 {
                control.value = self.target_value() + value;
                control.flags &= !0x80;
            }
            self.fast_forward_to(self.value());
        }
        if self.is_waiting() {
            self.queue.push_back(control);
        } else {
            self.activate(control);
        }
        Ok(())
    }
    pub fn fast_forward_unless_prohibited(&mut self) {
        loop {
            if self.protected && self.elapsed < self.tween.duration {
                return;
            }
            self.elapsed = self.tween.duration;
            let Some(control) = self.queue.pop_front() else {
                return;
            };
            self.activate(control);
        }
    }
    pub fn update(&mut self, delta: Ticks) {
        let delta = delta.as_f32();
        if delta <= 0.0 || delta.is_nan() {
            return;
        }
        let mut consumed = 0.0f32;
        loop {
            let old_elapsed = self.elapsed.as_f32();
            let duration = self.tween.duration.as_f32();
            let elapsed = (delta - consumed) + old_elapsed;
            if elapsed < duration {
                self.elapsed = Ticks::from_f32(elapsed);
                return;
            }
            self.elapsed = self.tween.duration;
            let Some(control) = self.queue.pop_front() else {
                return;
            };
            self.activate(control);
            consumed += duration - old_elapsed;
            if consumed >= delta {
                return;
            }
        }
    }
}
