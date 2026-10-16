//! SceneJump InputArea callbacks (aa6b0..aafdc), with captured press targets.
use crate::adv::{backlog::gesture::Gesture, backlog::jump::SceneJump};
use glam::Vec2;
#[derive(Clone, Copy)]
enum Zone {
    Close,
    Scroll,
    Blocked,
}
#[derive(Debug, PartialEq)]
pub(crate) enum Event {
    HoldClose,
    CancelClose,
    CloseClick,
    Select(usize),
    Drag(f32),
    Fling(Vec2),
}
#[derive(Default)]
pub(crate) struct Pointer {
    held: bool,
    zone: Option<Zone>,
    marker: Option<usize>,
    dragging: bool,
    duration: f64,
    long_hold: bool,
    gesture: Gesture,
}
impl Pointer {
    pub fn take_consumed(&mut self) -> bool {
        self.gesture.take_consumed()
    }
    pub fn update(
        &mut self,
        model: &mut SceneJump,
        p: Vec2,
        held: bool,
        eligible: bool,
        seconds: f64,
    ) -> Vec<Event> {
        self.update_with_mouse_release(model, p, held, eligible, seconds, None)
    }
    pub fn update_with_mouse_release(
        &mut self,
        model: &mut SceneJump,
        p: Vec2,
        held: bool,
        eligible: bool,
        seconds: f64,
        mouse_release: Option<crate::adv::input_area::MouseRelease>,
    ) -> Vec<Event> {
        let mut events = Vec::new();
        let motion =
            self.gesture
                .update_with_mouse_release(p, held, eligible, seconds, 1.0, mouse_release);
        if !eligible {
            if matches!(self.zone, Some(Zone::Close | Zone::Blocked)) {
                events.push(Event::CancelClose);
            }
            self.zone = None;
            self.marker = None;
        } else if held && !self.held {
            self.dragging = false;
            self.duration = 0.0;
            self.long_hold = false;
            self.zone = Some(
                if (1764.0..1848.0).contains(&p.x) && (32.0..116.0).contains(&p.y) {
                    Zone::Close
                } else if (150.0..992.0).contains(&p.y) {
                    Zone::Scroll
                } else {
                    Zone::Blocked
                },
            );
            self.marker = if matches!(self.zone, Some(Zone::Scroll)) {
                model.press_row(p.y)
            } else {
                None
            };
        } else if held {
            if motion.cancel_click {
                self.dragging = true;
                if matches!(self.zone, Some(Zone::Close | Zone::Blocked)) {
                    events.push(Event::CancelClose);
                    self.zone = Some(Zone::Blocked);
                }
            }
            if let Some(d) = motion.drag {
                match self.zone {
                    Some(Zone::Scroll) => events.push(Event::Drag(d.y)),
                    Some(Zone::Close | Zone::Blocked) => {
                        events.push(Event::CancelClose);
                        self.zone = Some(Zone::Blocked);
                    }
                    None => {}
                }
            }
            if !self.dragging && self.zone.is_some() {
                if seconds.is_finite() && seconds > 0.0 {
                    self.duration += seconds;
                }
                if !self.long_hold && self.duration >= 0.15 {
                    self.long_hold = true;
                    match self.zone {
                        Some(Zone::Close) => events.push(Event::HoldClose),
                        Some(Zone::Scroll) => {
                            if let Some(i) = self.marker {
                                events.push(Event::Select(i));
                            }
                        }
                        _ => {}
                    }
                }
            }
        } else if self.held {
            if motion.cancel_click {
                self.dragging = true;
                if matches!(self.zone, Some(Zone::Close | Zone::Blocked)) {
                    events.push(Event::CancelClose);
                    self.zone = Some(Zone::Blocked);
                }
            }
            if matches!(self.zone, Some(Zone::Scroll)) {
                if let Some(delta) = motion.drag {
                    events.push(Event::Drag(delta.y));
                }
                if let Some(v) = motion.fling {
                    events.push(Event::Fling(v));
                }
            }
            if !self.dragging {
                match self.zone {
                    Some(Zone::Close) => events.push(Event::CloseClick),
                    Some(Zone::Scroll) => {
                        if let Some(i) = self.marker {
                            events.push(Event::Select(i));
                        }
                    }
                    _ => {}
                }
            }
            self.zone = None;
            self.marker = None;
        }
        self.held = held;
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adv::backlog::{Backlog, BacklogEntry, BacklogItem};
    fn model(count: usize) -> SceneJump {
        let history = Backlog {
            entries: (0..count)
                .map(|i| BacklogEntry {
                    save_position: i as u32 + 1,
                    item: BacklogItem::Checkpoint {
                        title: String::new(),
                        text: "章".into(),
                    },
                })
                .collect(),
        };
        SceneJump::new(&history, 0, 0).unwrap()
    }
    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
    fn reference_native_scene_jump_pointer_and_momentum_callbacks() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (a, expected) in [
            (0xaa6d4, 0xb900e414u32),
            (0xaa718, 0xb9028109),
            (0xaa750, 0xb9028149),
            (0xaa774, 0x540004aa),
            (0xaa790, 0x1e380008),
            (0xaa7f8, 0xb900e674),
            (0xaaa10, 0x940043fc),
            (0xaaa68, 0x17fffa96),
            (0xaab48, 0x97fdf1da),
            (0xaab88, 0x9400439e),
            (0xaab90, 0xb900e268),
            (0xaac98, 0x1e293901),
            (0xaace8, 0x3903a268),
            (0xaae38, 0x1e37d001),
            (0xaae54, 0x1e202140),
            (0xaae74, 0xb900e268),
            (0xaae90, 0xbd00ee68),
            (0xa9208, 0x1e280800),
            (0xa9270, 0xbd45d900),
            (0xa9298, 0x54000065),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap()),
                expected,
                "native jump pointer{a:x}"
            );
        }
        for (a, value) in [
            (0x159a20, 150.0f32),
            (0x159944, 992.0),
            (0x159694, 32.0),
            (0x159658, 116.0),
            (0x159480, 1764.0),
            (0x15971c, 1848.0),
            (0x1595d8, 0.95),
            (0x159570, 0.1),
            (0x159ab8, f32::EPSILON),
        ] {
            assert_eq!(
                f32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap()),
                value
            );
        }
        assert_eq!(
            f64::from_le_bytes(exe[0x164c90 + 256..0x164c90 + 264].try_into().unwrap()),
            0.15
        );
        let relocation = |a: u64| {
            exe[0x11e040 + 256..0x152eb8 + 256]
                .chunks_exact(24)
                .find(|e| u64::from_le_bytes(e[..8].try_into().unwrap()) == a)
                .map(|e| u64::from_le_bytes(e[16..24].try_into().unwrap()))
                .unwrap()
        };
        for (slot, target) in [
            (0x1da270, 0xaa6b0),
            (0x1da278, 0xaa990),
            (0x1da280, 0xaa9d0),
            (0x1da288, 0xaab10),
            (0x1da298, 0xaac30),
            (0x1da2a0, 0xaadd0),
            (0x1da2a8, 0xaaf90),
        ] {
            assert_eq!(relocation(slot), target);
        }
    }
    #[test]
    fn marker_press_is_captured_by_y_and_commits_on_hold_or_release() {
        let mut model = model(20);
        let mut pointer = Pointer::default();
        assert!(
            pointer
                .update(&mut model, Vec2::new(-50.0, 450.0), true, true, 0.01)
                .is_empty()
        );
        assert_eq!(
            pointer.update(&mut model, Vec2::new(-50.0, 450.0), true, true, 0.15),
            vec![Event::Select(3)]
        );
        assert!(pointer.take_consumed());
        assert_eq!(
            pointer.update(&mut model, Vec2::new(-50.0, 455.0), false, true, 0.01),
            vec![Event::Select(3)]
        );
        assert!(pointer.take_consumed());
        assert!(!pointer.take_consumed());
    }
    #[test]
    fn close_drag_cancels_feedback_and_never_scrolls_or_closes() {
        let mut model = model(20);
        let mut pointer = Pointer::default();
        let p = Vec2::new(1800.0, 80.0);
        pointer.update(&mut model, p, true, true, 0.01);
        assert_eq!(
            pointer.update(&mut model, p, true, true, 0.15),
            vec![Event::HoldClose]
        );
        assert_eq!(
            pointer.update(&mut model, p + Vec2::Y * 40.0, true, true, 0.01),
            vec![Event::CancelClose]
        );
        assert!(
            pointer
                .update(&mut model, p + Vec2::Y * 40.0, false, true, 0.01)
                .is_empty()
        );
        pointer.update(&mut model, p, true, true, 0.01);
        assert_eq!(
            pointer.update(&mut model, p, false, true, 0.01),
            vec![Event::CloseClick]
        );
    }
    #[test]
    fn moving_row_press_stops_inertia_and_held_opening_press_is_ignored() {
        let mut model = model(20);
        let mut pointer = Pointer::default();
        let p = Vec2::new(900.0, 450.0);
        assert!(!model.release(Vec2::new(0.0, -40.0)));
        pointer.update(&mut model, p, true, true, 0.01);
        assert!(pointer.update(&mut model, p, false, true, 0.01).is_empty());
        model.update_with_delta(shin_core::time::Ticks::from_f32(1.0));
        assert_eq!(model.scroll(), 0.0);
        pointer.update(&mut model, p, true, false, 0.01);
        assert!(pointer.update(&mut model, p, true, true, 0.2).is_empty());
        assert!(pointer.update(&mut model, p, false, true, 0.01).is_empty());
    }
    #[test]
    fn menu_drag_has_no_angle_filter_and_right_fling_closes() {
        let mut model = model(30);
        let mut pointer = Pointer::default();
        let p = Vec2::new(900.0, 500.0);
        pointer.update(&mut model, p, true, true, 0.01);
        assert_eq!(
            pointer.update(&mut model, p + Vec2::new(100.0, -20.0), true, true, 0.01),
            vec![Event::Drag(-20.0)]
        );
        let events = pointer.update(&mut model, p, false, true, 0.016683333333333335);
        assert_eq!(events, vec![Event::Fling(Vec2::new(100.0, -20.0))]);
        assert!(model.release(Vec2::new(100.0, -20.0)));
        assert!(!model.release(Vec2::new(-100.0, 0.0)));
        assert!(!model.release(Vec2::new(0.0, -40.0)));
        model.update_with_delta(shin_core::time::Ticks::from_f32(2.0));
        assert_eq!(model.scroll(), 80.0);
        model.update_with_delta(shin_core::time::Ticks::from_f32(1.0));
        assert!((model.scroll() - 116.1).abs() < 0.001);
    }
}

#[cfg(test)]
mod desktop_tests;
