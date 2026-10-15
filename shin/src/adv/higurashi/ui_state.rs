//! Updated ADV410 hint tween,55c7c/55fd4 and96eb4/97380 indicator clocks.
//! Textures/menu rendering and physical modal input remain separate owners.
use crate::layer::higurashi_properties::HigurashiTweener;
use shin_core::time::Ticks;

#[derive(Debug, Clone)]
pub struct Indicator {
    pub hidden: bool,
    pub opacity: f32,
    pub transition: HigurashiTweener,
}
impl Default for Indicator {
    fn default() -> Self {
        Self {
            hidden: false,
            opacity: 1.0,
            transition: HigurashiTweener::new(0.0),
        }
    }
}
impl Indicator {
    pub fn set_hidden(&mut self, raw: u32) {
        self.hidden = raw & 1 != 0;
    }
    pub fn reset(&mut self) {
        self.hidden = false;
        self.transition.fast_forward_to(0.0);
        self.opacity = 1.0;
    }
    ///55f90 stops the existing notification tween without creating a notice.
    pub fn stop(&mut self) {
        if self.transition.is_waiting() {
            self.transition
                .control(0.0, Ticks::from_f32(f32::from_bits(0x3cf5c28f)), 0x140)
                .expect("Native notification stop uses supported tween flags");
        }
    }
    pub fn advance(&mut self, delta: Ticks) -> bool {
        let change = delta.as_f32() * 0.1;
        self.opacity = if self.hidden {
            self.opacity - change
        } else {
            self.opacity + change
        };
        if self.hidden && self.opacity < 0.0 {
            self.opacity = 0.0;
        }
        if !self.hidden && self.opacity > 1.0 {
            self.opacity = 1.0;
        }
        // Native b.pl/b.mi also admit an unordered opacity comparison.
        let update = !(self.opacity < 1.0);
        if update {
            self.transition.update(delta);
        }
        update
    }
    pub fn fast_forward(&mut self) {
        if !self.hidden {
            self.transition.fast_forward_unless_prohibited();
            self.opacity = 1.0;
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct MenuIndicator {
    pub indicator: Indicator,
    pub elapsed: f32,
}
impl MenuIndicator {
    pub fn reset(&mut self) {
        self.indicator.reset();
        self.elapsed = 0.0;
    }
    pub fn advance(&mut self, delta: Ticks) {
        if self.indicator.advance(delta) {
            self.elapsed += delta.as_f32();
        }
    }
    pub fn stop(&mut self) {
        if self.indicator.transition.is_waiting() {
            self.indicator
                .transition
                .control(0.0, Ticks::from_f32(2.0), 0x140)
                .expect("Native menu stop uses supported tween flags");
        }
    }
    pub fn fast_forward(&mut self) {
        self.indicator.transition.fast_forward_unless_prohibited();
    }
}

#[derive(Debug, Clone)]
pub struct HigurashiUiState {
    pub hint: HigurashiTweener,
    pub indicator: Indicator,
    pub menu: MenuIndicator,
}
impl Default for HigurashiUiState {
    fn default() -> Self {
        Self {
            hint: HigurashiTweener::new(0.0),
            indicator: Default::default(),
            menu: Default::default(),
        }
    }
}
impl HigurashiUiState {
    ///13140 queues1/32, relative0/120, then0/32 after resetting ADV410.
    pub fn pulse_hint(&mut self) {
        self.hint.fast_forward_to(0.0);
        for (value, time, flags) in [(1.0, 32.0, 0), (0.0, 120.0, 0x80), (0.0, 32.0, 0)] {
            self.hint
                .control(value, Ticks::from_f32(time), flags)
                .expect("Native hint pulse uses supported tween flags");
        }
    }
    ///12f24/12f34/12f40 occur outside the scene-animation and VM pause gates.
    pub fn advance(&mut self, delta: Ticks) {
        self.hint.update(delta);
        self.indicator.advance(delta);
        self.menu.advance(delta);
    }
}
impl super::runner::reconstruction_mode::ReconstructionUi for HigurashiUiState {
    fn reset_blackout_and_indicators(&mut self) {
        self.hint.fast_forward_to(0.0);
        self.indicator.reset();
        self.menu.reset();
    }
}

#[cfg(test)]
mod tests;
