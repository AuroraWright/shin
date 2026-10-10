//! Native global elapsed clock2271a8;26ffc accumulates float ticks into a double.
#[derive(Debug, Clone, Default)]
pub struct PlayClock {
    ticks: f64,
    saved_seconds: u32,
}
impl PlayClock {
    pub fn from_saved_seconds(seconds: u32) -> Self {
        Self {
            ticks: seconds as f64 * 60.0,
            saved_seconds: seconds,
        }
    }
    pub fn ticks(&self) -> f64 {
        self.ticks
    }
    pub fn saved_seconds(&self) -> u32 {
        self.saved_seconds
    }
    pub fn advance(&mut self, delta: f32) {
        // Runtime deltas come from Duration; reject invalid synthetic inputs.
        if !delta.is_finite() || delta < 0.0 {
            return;
        }
        self.ticks += delta as f64;
        let seconds = self.ticks / 60.0;
        // Native27028 skips the saved uint32 assignment after overflow.
        if seconds <= u32::MAX as f64 {
            self.saved_seconds = seconds as u32;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_global_play_clock_accumulation_restore_and_overflow() {
        let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&path)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0x268d0, 0xd0000fe8),
            (0x268d4, 0xf943b908),
            (0x268d8, 0xf900011f),
            (0x26ffc, 0xd0000fe8),
            (0x27000, 0xf943b908),
            (0x27004, 0x1e22c000),
            (0x27008, 0xfd400101),
            (0x2700c, 0x1e602820),
            (0x27010, 0xfd000100),
            (0x27014, 0xb00009e8),
            (0x27018, 0xfd466d01),
            (0x2701c, 0xb00009e8),
            (0x27020, 0x1e611800),
            (0x27024, 0xfd465101),
            (0x27028, 0x1e612000),
            (0x2702c, 0x540000ac),
            (0x27030, 0x1e790008),
            (0x27034, 0xb0000fe9),
            (0x27038, 0xf9424929),
            (0x2703c, 0xb9000528),
            (0x66590, 0xbd400660),
            (0x66594, 0x7e61d800),
            (0x66598, 0xd00007e8),
            (0x6659c, 0xfd466d01),
            (0x665a0, 0xd0000de9),
            (0x665a4, 0xf943b929),
            (0x665a8, 0x39430668),
            (0x665ac, 0x7100011f),
            (0x665b0, 0x1a9f07e0),
            (0x665b4, 0x1e610800),
            (0x665b8, 0xfd000120),
            (0xfbab0, 0xd0000a28),
            (0xfbab4, 0xbd401d00),
            (0xfbab8, 0xd65f03c0),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native{address:x}"
            );
        }
        for (address, expected) in [(0x164cd8, 60.0_f64), (0x164ca0, u32::MAX as f64)] {
            assert_eq!(
                f64::from_le_bytes(exe[address + 256..address + 264].try_into().unwrap()),
                expected
            );
        }
    }
    #[test]
    fn restored_clock_keeps_fractional_frames_and_truncates_only_saved_seconds() {
        let mut clock = PlayClock::from_saved_seconds(123);
        assert_eq!(clock.ticks(), 7380.0);
        clock.advance(59.5);
        assert_eq!(clock.saved_seconds(), 123);
        clock.advance(0.5);
        assert_eq!(clock.saved_seconds(), 124);
        clock.advance(0.125);
        assert_eq!(clock.ticks(), 7440.125);
        assert_eq!(clock.saved_seconds(), 124);
        assert_eq!(
            super::super::system_menu_view::play_time_text(clock.ticks()),
            "2分"
        );
    }
    #[test]
    fn double_precision_and_overflow_match_native_global_clock() {
        let mut clock = PlayClock::from_saved_seconds(u32::MAX);
        let initial = clock.ticks();
        clock.advance(0.25);
        assert_eq!(clock.ticks(), initial + 0.25);
        assert_eq!(clock.saved_seconds(), u32::MAX);
        for delta in [-1.0, f32::NAN, f32::INFINITY] {
            clock.advance(delta);
        }
        assert_eq!(clock.ticks(), initial + 0.25);
        let mut clock = PlayClock::from_saved_seconds(u32::MAX - 1);
        clock.advance(120.0);
        assert_eq!(
            clock.saved_seconds(),
            u32::MAX - 1,
            "native skips assignment on overflow"
        );
    }
}
