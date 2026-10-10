use std::collections::VecDeque;

use crate::time::{Ticks, Tween};

type Value = f32;

#[derive(Debug, Clone, Copy, Default)]
pub struct TweenOptions {
    pub prohibit_fast_forward: bool,
    pub ignore_wait: bool,
}

#[derive(Debug, Clone, Copy)]
enum State {
    Idle,
    Tweening {
        values: (Value, Value),
        time: Ticks,
        tween: Tween,
        options: TweenOptions,
    },
}

/// Holds a value and plays back tweens which smoothly
/// adjust that value.
#[derive(Debug, Clone)]
pub struct Tweener {
    tween_queue: VecDeque<(Value, Tween, TweenOptions)>,
    state: State,
    value: Value,
}

impl Tweener {
    pub fn new(value: Value) -> Self {
        Self {
            tween_queue: VecDeque::new(),
            state: State::Idle,
            value,
        }
    }

    pub fn value(&self) -> Value {
        self.value
    }

    pub fn target_value(&self) -> Value {
        match self.state {
            State::Idle => self.value,
            State::Tweening {
                values: (_, value), ..
            } => match self.tween_queue.back() {
                None => value,
                Some(&(value, _, _)) => value,
            },
        }
    }

    pub fn is_idle(&self) -> bool {
        matches!(self.state, State::Idle)
    }

    /// Enqueues a new value to tween to.
    pub fn enqueue(&mut self, value: Value, tween: Tween) {
        self.enqueue_with_options(value, tween, TweenOptions::default());
    }

    pub fn enqueue_with_options(&mut self, value: Value, tween: Tween, options: TweenOptions) {
        match self.state {
            State::Idle if tween.duration <= Ticks::ZERO => self.value = value,
            State::Idle => {
                self.state = State::Tweening {
                    values: (self.value, value),
                    time: Ticks::ZERO,
                    tween,
                    options,
                };
            }
            State::Tweening { .. } => self.tween_queue.push_back((value, tween, options)),
        }
    }

    pub fn is_waiting(&self) -> bool {
        matches!(self.state, State::Tweening { options, .. } if !options.ignore_wait)
    }

    /// Skip unprotected transitions, stopping at the first protected one.
    pub fn fast_forward_unless_prohibited(&mut self) {
        while let State::Tweening {
            values, options, ..
        } = self.state
        {
            if options.prohibit_fast_forward {
                break;
            }
            self.value = values.1;
            self.next(Ticks::ZERO);
        }
    }

    /// Returns an linearly interpolated value between `a` and `b`.
    ///
    /// An amount of `0.0` should yield `a`, an amount of `1.0` should
    /// yield `b`, and an amount of `0.5` should yield a value halfway
    /// between `a` and `b`.
    fn lerp(a: Value, b: Value, amount: f32) -> Value {
        a + (b - a) * amount
    }

    fn next(&mut self, time: Ticks) {
        while let Some((value, tween, options)) = self.tween_queue.pop_front() {
            if tween.duration <= Ticks::ZERO {
                self.value = value;
                continue;
            }
            self.state = State::Tweening {
                values: (self.value, value),
                time,
                tween,
                options,
            };
            return;
        }
        self.state = State::Idle;
    }

    pub fn update(&mut self, delta_time: Ticks) {
        // Native bba50 leaves the active tween and its queue unchanged when
        // elapsed time is nonpositive or unordered. Oscillators have separate rules.
        let ticks = delta_time.as_f32();
        if ticks <= 0.0 || ticks.is_nan() {
            return;
        }
        let mut elapsed = delta_time;
        loop {
            let State::Tweening {
                values,
                time,
                tween,
                ..
            } = &mut self.state
            else {
                break;
            };
            *time += elapsed;
            if *time < tween.duration {
                self.value = Self::lerp(values.0, values.1, tween.value(*time));
                break;
            }
            self.value = values.1;
            elapsed = *time - tween.duration;
            self.next(Ticks::ZERO);
        }
    }

    /// Fast-forwards the tweener to the last enqueue value.
    pub fn fast_forward(&mut self) {
        let last_queue_value = self.tween_queue.pop_back();
        self.tween_queue.clear();

        let value = match last_queue_value {
            Some((value, _, _)) => value,
            None => match self.state {
                State::Idle => self.value,
                State::Tweening { values, .. } => values.1,
            },
        };

        self.state = State::Idle;
        self.value = value;
    }

    /// Fast-forwards the tweener to the specified value.
    pub fn fast_forward_to(&mut self, value: Value) {
        self.tween_queue.clear();
        self.state = State::Idle;
        self.value = value;
    }

    /// Enqueue a transition from the current value to the specified value, ignoring the previous queue (it's cleared).
    pub fn enqueue_now(&mut self, value: Value, tween: Tween) {
        self.fast_forward_to(self.value);
        self.enqueue(value, tween);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::Easing;

    fn linear(ticks: f32) -> Tween {
        Tween {
            duration: Ticks::from_f32(ticks),
            easing: Easing::Linear,
        }
    }

    #[test]
    fn one_update_consumes_multiple_queued_transitions_and_preserves_remaining_time() {
        let mut tween = Tweener::new(0.0);
        for value in [100.0, 200.0, 300.0] {
            tween.enqueue(value, linear(10.0));
        }
        tween.update(Ticks::from_f32(25.0));
        assert_eq!(tween.value(), 250.0);
        assert!(tween.is_waiting());
        tween.update(Ticks::from_f32(5.0));
        assert_eq!(tween.value(), 300.0);
        assert!(tween.is_idle());
    }

    #[test]
    fn skip_stops_at_protected_motion_then_resumes_after_normal_time_completion() {
        let mut tween = Tweener::new(0.0);
        tween.enqueue(100.0, linear(10.0));
        tween.enqueue_with_options(
            200.0,
            linear(10.0),
            TweenOptions {
                prohibit_fast_forward: true,
                ignore_wait: false,
            },
        );
        tween.enqueue(300.0, linear(10.0));
        tween.fast_forward_unless_prohibited();
        assert_eq!(tween.value(), 100.0);
        assert!(tween.is_waiting());
        tween.update(Ticks::from_f32(5.0));
        assert_eq!(tween.value(), 150.0);
        tween.fast_forward_unless_prohibited();
        assert_eq!(tween.value(), 150.0);
        tween.update(Ticks::from_f32(5.0));
        tween.fast_forward_unless_prohibited();
        assert_eq!(tween.value(), 300.0);
        assert!(tween.is_idle());
    }

    #[test]
    fn ignored_wait_applies_to_active_motion_and_zero_duration_finishes_immediately() {
        let mut tween = Tweener::new(0.0);
        tween.enqueue_with_options(
            100.0,
            linear(10.0),
            TweenOptions {
                ignore_wait: true,
                prohibit_fast_forward: true,
            },
        );
        tween.enqueue(200.0, linear(10.0));
        assert!(!tween.is_waiting());
        assert!(!tween.is_idle());
        tween.update(Ticks::from_f32(15.0));
        assert_eq!(tween.value(), 150.0);
        assert!(tween.is_waiting());
        tween.fast_forward_to(0.0);
        tween.enqueue_with_options(
            100.0,
            linear(0.0),
            TweenOptions {
                ignore_wait: false,
                prohibit_fast_forward: true,
            },
        );
        assert_eq!(tween.value(), 100.0);
        assert!(tween.is_idle());
        tween.enqueue(200.0, linear(10.0));
        tween.enqueue_with_options(
            300.0,
            linear(-1.0),
            TweenOptions {
                ignore_wait: false,
                prohibit_fast_forward: true,
            },
        );
        tween.enqueue(400.0, linear(10.0));
        tween.update(Ticks::from_f32(15.0));
        assert_eq!(tween.value(), 350.0);
    }
}
