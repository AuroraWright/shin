//! Native update 0x78820..0x78918: phases are copied with a layer, never
//! wrapped, and advance independently of the property-animation enable flag.
#[derive(Debug, Clone, Copy, Default)]
pub struct EffectPhases(pub [f32; 3]);
impl EffectPhases {
    pub fn advance(&mut self, delta: f32, periods: [f32; 3]) {
        for (phase, period) in self.0.iter_mut().zip(periods) {
            if period != 0.0 {
                *phase += (1.0 / period) * delta;
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn periods_freeze_reverse_and_change_without_resetting_or_wrapping() {
        let mut phases = EffectPhases::default();
        phases.advance(25.0, [100.0, -100.0, 0.0]);
        assert_eq!(phases.0, [0.25, -0.25, 0.0]);
        phases.advance(200.0, [100.0, -100.0, 0.0]);
        assert_eq!(phases.0, [2.25, -2.25, 0.0]);
        phases.advance(50.0, [200.0, 0.0, 100.0]);
        assert_eq!(phases.0, [2.5, -2.25, 0.5]);
        let cloned = phases;
        phases.advance(50.0, [200.0, 0.0, 100.0]);
        assert_eq!(cloned.0, [2.5, -2.25, 0.5]);
        assert_eq!(phases.0, [2.75, -2.25, 1.0]);
    }
}
