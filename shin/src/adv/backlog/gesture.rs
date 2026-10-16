//! InputArea dispatch (6b718/6b824/6b1d4), specialized for MessageBrowser.
use glam::Vec2;

#[derive(Debug, Default)]
pub(crate) struct Gesture {
    held: bool,
    consumed: bool,
    active: bool,
    dragging: bool,
    scroll_allowed: bool,
    origin: Vec2,
    previous: Vec2,
    displacement: Vec2,
}

#[derive(Debug, Default)]
pub(crate) struct Motion {
    pub cancel_click: bool,
    pub drag: Option<Vec2>,
    pub fling: Option<Vec2>,
}

#[derive(Clone, Copy)]
enum Direction {
    Left,
    Right,
}

impl Gesture {
    pub fn take_consumed(&mut self) -> bool {
        let consumed = std::mem::take(&mut self.consumed);
        self.held || consumed
    }

    pub fn update(&mut self, position: Vec2, held: bool, eligible: bool, seconds: f64) -> Motion {
        self.update_with_threshold(position, held, eligible, seconds, 1.5)
    }
    pub fn update_with_threshold(
        &mut self,
        position: Vec2,
        held: bool,
        eligible: bool,
        seconds: f64,
        sample_threshold: f32,
    ) -> Motion {
        self.update_with_mouse_release(position, held, eligible, seconds, sample_threshold, None)
    }
    pub fn update_with_mouse_release(
        &mut self,
        position: Vec2,
        held: bool,
        eligible: bool,
        seconds: f64,
        sample_threshold: f32,
        mouse_release: Option<crate::adv::input_area::MouseRelease>,
    ) -> Motion {
        self.update_desktop(
            position,
            held,
            eligible,
            seconds,
            sample_threshold,
            mouse_release,
            Direction::Right,
        )
    }
    pub fn update_browser_with_mouse_release(
        &mut self,
        position: Vec2,
        held: bool,
        eligible: bool,
        seconds: f64,
        mouse_release: Option<crate::adv::input_area::MouseRelease>,
    ) -> Motion {
        self.update_desktop(
            position,
            held,
            eligible,
            seconds,
            1.5,
            mouse_release,
            Direction::Left,
        )
    }
    fn update_desktop(
        &mut self,
        position: Vec2,
        held: bool,
        eligible: bool,
        seconds: f64,
        sample_threshold: f32,
        mouse_release: Option<crate::adv::input_area::MouseRelease>,
        direction: Direction,
    ) -> Motion {
        let mut result = Motion::default();
        self.consumed = held || self.held;
        if !eligible {
            self.active = false;
        } else if held && !self.held {
            self.active = true;
            self.dragging = false;
            self.scroll_allowed = (150.0..992.0).contains(&position.y);
            self.origin = position;
            self.previous = position;
            self.displacement = Vec2::ZERO;
        } else if held && self.active {
            let delta = position - self.previous;
            let first_drag = !self.dragging && (position - self.origin).length() > 16.0;
            if first_drag {
                self.dragging = true;
                result.cancel_click = true;
            }
            if self.dragging && (first_drag || delta.length() > sample_threshold) {
                if self.scroll_allowed {
                    result.drag = Some(delta);
                }
                self.previous = position;
            }
            // InputArea retains the last displacement even below the sampling
            // threshold, and stationary samples clear release momentum.
            self.displacement = delta;
        } else if !held && self.held && self.active {
            if let Some(release) = mouse_release {
                let displacement = release.position() - self.origin;
                if displacement.length() > 16.0 {
                    if !self.dragging {
                        self.dragging = true;
                        result.cancel_click = true;
                    }
                    if self.scroll_allowed && release.position() != self.previous {
                        result.drag = Some(release.position() - self.previous);
                    }
                }
            }
            if self.dragging && self.scroll_allowed {
                let velocity = if let Some(release) = mouse_release {
                    let candidate = crate::adv::input_area::desktop_gesture_vector(
                        release.position() - self.origin,
                        release.velocity(),
                    );
                    let degrees = angle(candidate);
                    // Only the pane's horizontal action uses displacement.
                    // Ordinary vertical scrolling keeps measured velocity.
                    let horizontal = match direction {
                        Direction::Left => degrees > 150.0 || degrees < -150.0,
                        Direction::Right => degrees > -30.0 && degrees < 30.0,
                    };
                    if horizontal {
                        candidate
                    } else {
                        release.velocity()
                    }
                } else if seconds > 0.0 && seconds.is_finite() {
                    self.displacement * (0.016683333333333335 / seconds) as f32
                } else {
                    Vec2::ZERO
                };
                if velocity.length() > 20.0 {
                    result.fling = Some(velocity);
                }
            }
            self.active = false;
        }
        self.held = held;
        result
    }
}

fn angle(delta: Vec2) -> f32 {
    delta.y.atan2(delta.x) / f32::from_bits(0x40490fd8) * 180.0
}

pub(crate) fn scroll_drag(delta: Vec2) -> bool {
    let angle = angle(delta);
    (-150.0..=150.0).contains(&angle) && !(-30.0..30.0).contains(&angle)
}

/// a7dd0 accepts the forward half-plane for scrolling. A leftward fling
/// outside this interval opens the chapter list.
pub(crate) fn scroll_fling(velocity: Vec2) -> bool {
    (-150.0..=150.0).contains(&angle(velocity))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_ownership_blocks_keys_through_release_then_clears() {
        let mut gesture = Gesture::default();
        gesture.update(Vec2::new(400.0, 400.0), true, true, 1.0 / 60.0);
        assert!(gesture.take_consumed());
        assert!(gesture.take_consumed());
        gesture.update(Vec2::new(400.0, 400.0), false, true, 1.0 / 60.0);
        assert!(gesture.take_consumed());
        assert!(!gesture.take_consumed());
    }

    #[test]
    fn threshold_samples_full_initial_drag_and_excludes_click_release() {
        let mut gesture = Gesture::default();
        let origin = Vec2::new(400.0, 400.0);
        gesture.update(origin, true, true, 1.0 / 60.0);
        assert!(
            !gesture
                .update(origin + Vec2::Y * 16.0, true, true, 1.0 / 60.0)
                .cancel_click
        );
        let motion = gesture.update(origin + Vec2::Y * 17.0, true, true, 1.0 / 60.0);
        assert!(motion.cancel_click);
        assert_eq!(motion.drag, Some(Vec2::Y * 17.0));
        assert_eq!(
            gesture
                .update(origin + Vec2::Y * 18.0, true, true, 1.0 / 60.0)
                .drag,
            None
        );
        assert_eq!(
            gesture
                .update(origin + Vec2::Y * 19.0, true, true, 1.0 / 60.0)
                .drag,
            Some(Vec2::Y * 2.0)
        );
    }

    #[test]
    fn fling_uses_last_sample_time_and_stationary_hold_stops_it() {
        for stationary in [false, true] {
            let mut gesture = Gesture::default();
            let origin = Vec2::new(400.0, 400.0);
            gesture.update(origin, true, true, 1.0 / 60.0);
            gesture.update(origin - Vec2::Y * 40.0, true, true, 1.0 / 60.0);
            if stationary {
                gesture.update(origin - Vec2::Y * 40.0, true, true, 1.0 / 60.0);
            }
            let motion = gesture.update(origin - Vec2::Y * 40.0, false, true, 0.016683333333333335);
            assert_eq!(motion.fling, (!stationary).then_some(-Vec2::Y * 40.0));
        }
    }

    #[test]
    fn native_direction_filters_preserve_leftward_rollback_branch() {
        assert!(scroll_drag(Vec2::Y));
        assert!(scroll_drag(-Vec2::Y));
        assert!(!scroll_drag(Vec2::X));
        assert!(!scroll_drag(-Vec2::X));
        assert!(scroll_fling(Vec2::Y));
        assert!(scroll_fling(Vec2::X));
        assert!(!scroll_fling(-Vec2::X));
    }

    #[test]
    fn opening_and_close_region_cannot_acquire_scroll() {
        let mut gesture = Gesture::default();
        gesture.update(Vec2::new(400.0, 400.0), true, false, 1.0 / 60.0);
        assert_eq!(
            gesture
                .update(Vec2::new(400.0, 500.0), true, true, 1.0 / 60.0)
                .drag,
            None
        );
        gesture.update(Vec2::ZERO, false, true, 1.0 / 60.0);
        gesture.update(Vec2::new(1800.0, 70.0), true, true, 1.0 / 60.0);
        let motion = gesture.update(Vec2::new(1800.0, 200.0), true, true, 1.0 / 60.0);
        assert!(motion.cancel_click);
        assert_eq!(motion.drag, None);
        assert_eq!(
            gesture
                .update(Vec2::new(1800.0, 200.0), false, true, 1.0 / 60.0)
                .fling,
            None
        );
    }

    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
    fn reference_native_backlog_gesture_dispatch_and_inertia() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
        for (address, expected) in [
            (0xa6a04, 0x97ff13bf),
            (0xa6a08, 0x370004e0),
            (0xa21a8, 0x32190fe2),
            (0xa21ac, 0x97ff2385),
            (0x6b734, 0x531d7108),
            (0x6b744, 0x1e211800),
            (0x6b74c, 0x540001ad),
            (0x6b770, 0x1e2e1001),
            (0x6b778, 0x5400056c),
            (0x6b82c, 0xf9401d08),
            (0x6b84c, 0xd63f0100),
            (0x6b1ec, 0x1e601940),
            (0x6b244, 0x1e202100),
            (0x6b254, 0xf9402108),
            (0x6b25c, 0x1e2c8800),
            (0x6b268, 0xd63f0100),
            (0xa7dc4, 0x17ffff7f),
            (0xa7f24, 0x17ffffab),
            (0xa8004, 0x17ffffcb),
            (0xa7ca4, 0x9401d3bb),
            (0xa7ccc, 0x54fff9cc),
            (0xa7cdc, 0x54fff944),
            (0xa7cf0, 0x54fff8ac),
            (0xa7d0c, 0x1e293801),
            (0xa7e24, 0x1e212000),
            (0xa7e68, 0x5400016c),
            (0xa7e78, 0x540000e4),
            (0xa7e7c, 0xbd028a68),
            (0xa3e90, 0x1e292020),
            (0xa3e94, 0x540005a4),
            (0xa3f48, 0xb9028a7f),
        ] {
            assert_eq!(
                word(address),
                expected,
                "native backlog gesture {address:x}"
            );
        }
        for (address, expected) in [
            (0x1594bc, f32::from_bits(0x40490fd8)),
            (0x159434, -150.0),
            (0x159a20, 150.0),
            (0x159814, 180.0),
            (0x159624, 960.0),
            (0x159944, 992.0),
            (0x159ab8, f32::EPSILON),
        ] {
            assert_eq!(f32::from_bits(word(address)), expected);
        }
        let tick = f64::from_le_bytes(exe[0x164d08 + 256..0x164d10 + 256].try_into().unwrap());
        assert_eq!(tick, 0.016683333333333335);
    }
}
