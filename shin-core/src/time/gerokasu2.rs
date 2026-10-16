//! Gerokasu2 property queues, verified against native 98680..99048.
//!
//! Raw relative and speed controls stay queued until installation. Unlike the
//! older policy, interpolation uses FMA and completed clocks retain both ends.
use super::{Easing, Tween, TweenOptions};
use std::{
    collections::VecDeque,
    sync::atomic::{AtomicU32, Ordering},
};

#[derive(Debug, Clone, Copy)]
pub(crate) struct Control {
    pub value: f32,
    pub duration: f32,
    pub flags: u32,
    pub easing: Easing,
    pub ignore_wait: bool,
}
#[derive(Debug)]
struct CachedValue(AtomicU32);
impl Clone for CachedValue {
    fn clone(&self) -> Self {
        Self(AtomicU32::new(self.0.load(Ordering::Relaxed)))
    }
}
impl CachedValue {
    fn new(value: f32) -> Self {
        Self(AtomicU32::new(value.to_bits()))
    }
    fn get(&self) -> Option<f32> {
        let value = f32::from_bits(self.0.load(Ordering::Relaxed));
        (!value.is_nan()).then_some(value)
    }
    fn set(&self, value: Option<f32>) {
        self.0
            .store(value.unwrap_or(f32::NAN).to_bits(), Ordering::Relaxed);
    }
}
#[derive(Debug, Clone)]
pub(crate) struct GerTweener {
    from: f32,
    to: f32,
    easing: Easing,
    elapsed: f32,
    total: f32,
    prohibit: bool,
    ignore_wait: bool,
    cached: CachedValue,
    queue: VecDeque<Control>,
}
impl GerTweener {
    pub(crate) fn new(value: f32) -> Self {
        Self {
            from: value,
            to: value,
            easing: Easing::Linear,
            elapsed: 1.0,
            total: 1.0,
            prohibit: false,
            ignore_wait: false,
            cached: CachedValue::new(value),
            queue: VecDeque::new(),
        }
    }
    pub(crate) fn value(&self) -> f32 {
        self.cached.get().unwrap_or_else(|| {
            let value = (self.to - self.from)
                .mul_add(self.easing.apply(self.elapsed / self.total), self.from);
            if !value.is_nan() {
                self.cached.set(Some(value));
            }
            value
        })
    }
    pub(crate) fn target_value(&self) -> f32 {
        self.queue.iter().fold(self.to, |base, c| {
            if c.flags & 128 != 0 {
                c.value + base
            } else {
                -0.0 + c.value
            }
        })
    }
    pub(crate) fn running(&self) -> bool {
        self.elapsed < self.total
    }
    pub(crate) fn waiting(&self) -> bool {
        self.running() && !self.ignore_wait
    }
    pub(crate) fn reset(&mut self, value: f32) {
        *self = Self::new(value);
    }
    fn install(&mut self, c: Control) {
        self.from = self.to;
        self.to = if c.flags & 128 != 0 {
            self.from + c.value
        } else {
            c.value
        };
        self.easing = c.easing;
        self.prohibit = c.flags & 4096 != 0;
        self.ignore_wait = c.ignore_wait;
        if c.duration > 0.0 && (c.flags & 64 == 0 || self.from != self.to) {
            self.total = if c.flags & 64 != 0 {
                ((self.to - self.from) / c.duration).abs()
            } else {
                c.duration
            };
            self.elapsed = 0.0;
        } else {
            self.elapsed = 1.0;
            self.total = 1.0;
        }
    }
    pub(crate) fn control(&mut self, mut c: Control) {
        match c.flags & 0x300 {
            0x100 => {
                if c.flags & 128 != 0 {
                    c.value += self.target_value();
                    c.flags &= !128;
                }
                let value = self.value();
                self.reset(value);
            }
            0x200 => {
                let value = self.target_value();
                self.reset(value);
            }
            _ => {}
        }
        if self.running() {
            self.queue.push_back(c);
        } else {
            self.install(c);
            self.cached.set(None);
        }
    }
    pub(crate) fn enqueue(&mut self, value: f32, tween: Tween, options: TweenOptions) {
        self.control(Control {
            value,
            duration: tween.duration.as_f32(),
            flags: if options.prohibit_fast_forward {
                4096
            } else {
                0
            },
            easing: tween.easing,
            ignore_wait: options.ignore_wait,
        });
    }
    pub(crate) fn advance(&mut self, delta: f32) {
        if delta <= 0.0 || delta.is_nan() {
            return;
        }
        let mut consumed = 0.0;
        loop {
            let next = (delta - consumed) + self.elapsed;
            if next < self.total {
                self.elapsed = next;
                consumed = delta;
                break;
            }
            consumed += self.total - self.elapsed;
            self.elapsed = self.total;
            let Some(c) = self.queue.pop_front() else {
                break;
            };
            self.install(c);
            if self.running() && !(consumed < delta) {
                break;
            }
        }
        if consumed > 0.0 {
            self.cached.set(None);
        }
    }
    pub(crate) fn forward(&mut self) {
        let value = self.target_value();
        self.reset(value);
    }
    pub(crate) fn forward_unless_prohibited(&mut self) {
        loop {
            if self.prohibit && self.running() {
                return;
            }
            self.elapsed = self.total;
            self.cached.set(None);
            let Some(c) = self.queue.pop_front() else {
                return;
            };
            self.install(c);
        }
    }
}

#[cfg(test)]
mod tests;
