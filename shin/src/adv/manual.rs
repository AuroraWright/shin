//! Native ManualViewer state86370/86670, hosted by the ADV system menu.
pub mod screen;
pub mod view;
use crate::app::AppAction;
use glam::{Vec2, Vec4, vec4};
use shin_core::time::{Easing, Ticks, Tween, Tweener};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManualPhase {
    Opening,
    Active,
    CloseFlash,
    Closing,
    Closed,
}
/// Native100: negative is page capture,0 blank caption/footer,1 close button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManualPointerTarget {
    Page,
    Outside,
    Close,
}
#[derive(Debug, Clone)]
pub struct ManualModel {
    count: usize,
    selected: usize,
    progress: f32,
    phase: ManualPhase,
    scroll: Tweener,
    close_flash: Tweener,
    sounds: Vec<&'static str>,
    pointer_target: ManualPointerTarget,
    swipe_accepted: bool,
}
impl ManualModel {
    pub fn new(count: usize) -> Option<Self> {
        (count > 0).then(|| Self {
            count,
            selected: 0,
            progress: 0.0,
            phase: ManualPhase::Opening,
            scroll: Tweener::new(0.0),
            close_flash: Tweener::new(0.0),
            sounds: Vec::new(),
            pointer_target: ManualPointerTarget::Page,
            swipe_accepted: false,
        })
    }
    pub fn phase(&self) -> ManualPhase {
        self.phase
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn count(&self) -> usize {
        self.count
    }
    pub fn progress(&self) -> f32 {
        self.progress
    }
    pub fn scroll(&self) -> f32 {
        self.scroll.value()
    }
    pub fn close_flash(&self) -> f32 {
        self.close_flash.value()
    }
    pub fn take_sounds(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.sounds)
    }
    pub fn update(&mut self, ticks: f32) {
        if !ticks.is_finite() || ticks < 0.0 {
            return;
        }
        match self.phase {
            ManualPhase::Opening => {
                self.progress = (self.progress + ticks * 0.1).min(1.0);
                if self.progress == 1.0 {
                    self.phase = ManualPhase::Active;
                }
            }
            ManualPhase::CloseFlash => {
                // Native87594 waits before this frame's close-flash clock advance.
                if self.close_flash.is_idle() {
                    self.phase = ManualPhase::Closing;
                }
            }
            ManualPhase::Closing => {
                self.progress = (self.progress - ticks * 0.1).max(0.0);
                if self.progress == 0.0 {
                    self.phase = ManualPhase::Closed;
                }
            }
            _ => {}
        }
        self.scroll.update(Ticks::from_f32(ticks));
        self.close_flash.update(Ticks::from_f32(ticks));
    }
    pub fn input(&mut self, state: &enum_map::EnumMap<AppAction, shin_input::ActionState>) {
        if self.phase != ManualPhase::Active || !self.scroll.is_idle() {
            return;
        }
        // Native866c0 prioritizes Left,Right,L,R. B is read only if page stays put.
        let previous = state[AppAction::Left].is_clicked_or_repeated;
        let next = state[AppAction::Right].is_clicked_or_repeated;
        let page = if previous {
            self.selected.saturating_sub(1)
        } else if next {
            (self.selected + 1).min(self.count - 1)
        } else if state[AppAction::CharacterPageUp].is_clicked_or_repeated {
            self.selected.saturating_sub(1)
        } else if state[AppAction::CharacterPageDown].is_clicked_or_repeated {
            (self.selected + 1).min(self.count - 1)
        } else {
            self.selected
        };
        if page != self.selected {
            self.select(page);
        } else if state[AppAction::Cancel].is_clicked {
            self.request_close("cancel");
        }
    }
    fn select(&mut self, page: usize) {
        self.selected = page;
        self.retarget_page_scroll();
        self.sounds.push("move");
    }
    /// Native867b0 captures the full area; page eligibility depends only onY.
    /// Coordinates are already in the native1920x1080 canvas. Platform ownership
    /// and callback sequencing remain the host's responsibility.
    pub fn pointer_probe(&mut self, position: Vec2) -> bool {
        if self.phase != ManualPhase::Active {
            return false;
        }
        self.swipe_accepted = false;
        self.pointer_target =
            if (1764.0..1848.0).contains(&position.x) && (32.0..116.0).contains(&position.y) {
                ManualPointerTarget::Close
            } else if (162.0..980.0).contains(&position.y) {
                ManualPointerTarget::Page
            } else {
                ManualPointerTarget::Outside
            };
        true
    }
    pub fn pointer_target(&self) -> ManualPointerTarget {
        self.pointer_target
    }
    pub fn pointer_press(&mut self) {
        if self.phase == ManualPhase::Active && self.pointer_target == ManualPointerTarget::Close {
            self.retarget_close_flash(0.7);
        }
    }
    pub fn pointer_release(&mut self) {
        if self.phase == ManualPhase::Active && self.pointer_target == ManualPointerTarget::Close {
            self.request_close("decide");
        }
    }
    /// Native868d0 rounds dragged scroll to nearest page unless a swipe already
    /// supplied its own target. This callback resets capture to the negative state.
    pub fn pointer_reset(&mut self) {
        if self.pointer_target == ManualPointerTarget::Page && !self.swipe_accepted {
            self.selected = ((self.scroll.value() / 1500.0 + 0.5) as usize).min(self.count - 1);
            self.retarget_page_scroll();
        }
        self.pointer_target = ManualPointerTarget::Page;
    }
    pub fn pointer_cancel(&mut self) {
        if self.pointer_target != ManualPointerTarget::Page {
            self.retarget_close_flash(0.0);
            self.pointer_target = ManualPointerTarget::Outside;
        }
    }
    /// Native86b30 subtracts current-minus-priorX and clamps without choosing a
    /// page yet. Dragging caption/close cancels feedback instead of moving pages.
    pub fn pointer_drag(&mut self, delta_x: f32) {
        if self.phase != ManualPhase::Active || !delta_x.is_finite() {
            return;
        }
        if self.pointer_target != ManualPointerTarget::Page {
            self.pointer_cancel();
            return;
        }
        let max = (self.count - 1) as f32 * 1500.0;
        self.scroll
            .fast_forward_to((self.scroll.value() - delta_x).clamp(0.0, max));
    }
    /// Native86c90 accepts horizontal swipes only at norm>20. Gesture changes
    /// the existing selected page, even after drag, and emits no move cue.
    pub fn pointer_gesture(&mut self, velocity: Vec2) {
        if self.phase != ManualPhase::Active || self.pointer_target != ManualPointerTarget::Page {
            return;
        }
        let norm = (velocity.x * velocity.x + velocity.y * velocity.y).sqrt();
        let angle = velocity.y.atan2(velocity.x) / 3.141592025756836_f32 * 180.0;
        if norm <= 20.0 || norm.is_nan() {
            return;
        }
        if angle > 150.0 || angle < -150.0 {
            self.selected = (self.selected + 1).min(self.count - 1);
        } else if angle < 30.0 && angle > -30.0 {
            self.selected = self.selected.saturating_sub(1);
        } else {
            return;
        }
        self.retarget_page_scroll();
        self.swipe_accepted = true;
    }
    fn retarget_page_scroll(&mut self) {
        let target = self.selected as f32 * 1500.0;
        self.scroll.enqueue_now(
            target,
            Tween {
                duration: Ticks::from_f32((target - self.scroll.value()).abs() / 100.0),
                easing: Easing::SineOut,
            },
        );
    }
    fn retarget_close_flash(&mut self, target: f32) {
        self.close_flash.enqueue_now(
            target,
            Tween::linear(Ticks::from_f32(
                (target - self.close_flash.value()).abs() / 0.2,
            )),
        );
    }
    pub fn request_close(&mut self, cue: &'static str) {
        if self.phase != ManualPhase::Active {
            return;
        }
        let current = self.close_flash.value();
        self.close_flash.enqueue_now(
            0.7,
            Tween::linear(Ticks::from_f32((0.7 - current).abs() / 0.2)),
        );
        self.close_flash
            .enqueue(0.0, Tween::linear(Ticks::from_f32(3.5)));
        self.phase = ManualPhase::CloseFlash;
        self.sounds.push(cue);
    }
    /// Native85de4: authored pages are1440x810, centered at x240 and spaced1500.
    pub fn page_artwork(&self, page: usize) -> Option<(Vec4, f32)> {
        if page >= self.count {
            return None;
        }
        let offset = page as f32 * 1500.0 - self.scroll.value();
        Some((
            vec4(240.0 + offset, 166.0, 1440.0, 810.0),
            (1.0 - (offset / 1500.0 * 0.5).abs()).max(0.5),
        ))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn active(count: usize) -> ManualModel {
        let mut m = ManualModel::new(count).unwrap();
        m.update(10.0);
        m
    }
    #[test]
    fn pointer_capture_uses_native_static_close_and_full_width_page_strip() {
        let mut m = active(24);
        for (position, target) in [
            (glam::vec2(1764.0, 32.0), ManualPointerTarget::Close),
            (glam::vec2(1847.999, 115.999), ManualPointerTarget::Close),
            (glam::vec2(1848.0, 32.0), ManualPointerTarget::Outside),
            (glam::vec2(1764.0, 116.0), ManualPointerTarget::Outside),
            (glam::vec2(-500.0, 162.0), ManualPointerTarget::Page),
            (glam::vec2(2500.0, 979.999), ManualPointerTarget::Page),
            (glam::vec2(500.0, 980.0), ManualPointerTarget::Outside),
        ] {
            assert!(m.pointer_probe(position));
            assert_eq!(m.pointer_target(), target);
        }
        let mut m = ManualModel::new(24).unwrap();
        assert!(!m.pointer_probe(glam::vec2(1800.0, 80.0)));
    }
    #[test]
    fn drag_rounding_is_silent_bounded_and_uses_native_half_page_threshold() {
        for (drag, page) in [
            (-749.999, 0),
            (-750.0, 1),
            (-1500.0, 1),
            (-100000.0, 23),
            (1000.0, 0),
        ] {
            let mut m = active(24);
            m.pointer_probe(glam::vec2(0.0, 500.0));
            m.pointer_drag(drag);
            assert_eq!(m.selected(), 0);
            assert_eq!(m.scroll(), (-drag).clamp(0.0, 34500.0));
            m.pointer_release();
            assert_eq!(m.phase(), ManualPhase::Active);
            m.pointer_reset();
            assert_eq!(m.selected(), page);
            m.update(400.0);
            assert_eq!(m.scroll(), page as f32 * 1500.0);
            assert!(m.take_sounds().is_empty());
        }
        let mut m = active(24);
        m.select(1);
        m.take_sounds();
        m.update(7.5);
        let position = m.scroll();
        m.pointer_probe(glam::vec2(0.0, 500.0));
        m.pointer_press();
        assert_eq!(m.scroll(), position);
        assert!(!m.scroll.is_idle());
        m.pointer_drag(100.0);
        assert!(m.scroll.is_idle());
        assert_eq!(m.scroll(), position - 100.0);
    }
    #[test]
    fn swipe_targets_current_selection_and_prevents_rounding_override() {
        for (velocity, target, accepted) in [
            (glam::vec2(-20.0, 0.0), 2, false),
            (glam::vec2(-20.001, 0.0), 3, true),
            (glam::vec2(20.001, 0.0), 1, true),
            (glam::vec2(0.0, 50.0), 2, false),
            (glam::vec2(-40.0, 22.0), 3, true),
            (glam::vec2(-40.0, 24.0), 2, false),
            (glam::vec2(40.0, -22.0), 1, true),
            (glam::vec2(40.0, -24.0), 2, false),
            (glam::vec2(f32::NAN, 0.0), 2, false),
        ] {
            let mut m = active(24);
            m.selected = 2;
            m.scroll.fast_forward_to(3000.0);
            m.pointer_probe(glam::vec2(0.0, 500.0));
            m.pointer_drag(-800.0);
            m.pointer_gesture(velocity);
            assert_eq!(m.selected(), target);
            assert_eq!(m.swipe_accepted, accepted);
            m.pointer_reset();
            assert_eq!(m.selected(), if accepted { target } else { 3 });
            m.update(400.0);
            assert!(m.take_sounds().is_empty());
        }
        for (start, velocity, expected) in [
            (0, glam::vec2(40.0, 0.0), 0),
            (23, glam::vec2(-40.0, 0.0), 23),
        ] {
            let mut m = active(24);
            m.selected = start;
            m.scroll.fast_forward_to(start as f32 * 1500.0);
            m.pointer_probe(glam::vec2(0.0, 500.0));
            m.pointer_gesture(velocity);
            assert_eq!(m.selected(), expected);
            assert!(m.swipe_accepted);
        }
    }
    #[test]
    fn close_press_drag_cancel_and_release_keep_native_feedback_and_cues() {
        let mut m = active(24);
        m.pointer_probe(glam::vec2(1800.0, 80.0));
        m.pointer_press();
        m.update(1.0);
        assert!((m.close_flash() - 0.2).abs() < 0.00001);
        m.pointer_drag(20.0);
        assert_eq!(m.pointer_target(), ManualPointerTarget::Outside);
        m.update(1.0);
        assert_eq!(m.close_flash(), 0.0);
        m.pointer_release();
        assert_eq!(m.phase(), ManualPhase::Active);
        assert!(m.take_sounds().is_empty());
        m.pointer_probe(glam::vec2(1800.0, 80.0));
        m.pointer_press();
        m.update(1.0);
        m.pointer_release();
        assert_eq!(m.phase(), ManualPhase::CloseFlash);
        assert_eq!(m.take_sounds(), ["decide"]);
        m.update(2.5);
        assert_eq!(m.close_flash(), 0.7);
        m.update(3.5);
        assert_eq!(m.close_flash(), 0.0);
        assert_eq!(m.phase(), ManualPhase::CloseFlash);
        m.update(0.0);
        assert_eq!(m.phase(), ManualPhase::Closing);
    }
    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_manual_pointer_capture_drag_reset_and_swipe() {
        let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&path)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0x867b0, 0xf0000688),
            (0x867b4, 0xbd469502),
            (0x867b8, 0x1e222020),
            (0x867bc, 0x3904101f),
            (0x867c0, 0x5400022b),
            (0x867c4, 0xf0000688),
            (0x867c8, 0xbd465902),
            (0x867cc, 0x1e222020),
            (0x867d0, 0x540001a5),
            (0x867d4, 0xf0000688),
            (0x867d8, 0xbd448102),
            (0x867dc, 0x1e222000),
            (0x867e0, 0x5400012b),
            (0x867e4, 0xf0000688),
            (0x867e8, 0xbd471d02),
            (0x867ec, 0x1e222000),
            (0x867f0, 0x540000a5),
            (0x867f4, 0x320003e8),
            (0x867f8, 0xb9010008),
            (0x867fc, 0x320003e0),
            (0x86800, 0xd65f03c0),
            (0x86804, 0xf0000688),
            (0x86808, 0xbd466500),
            (0x8680c, 0xf0000689),
            (0x86810, 0x1e202020),
            (0x86814, 0xbd440d20),
            (0x86818, 0x1a9f47e8),
            (0x8681c, 0x1e202020),
            (0x86820, 0x1a9fa7e9),
            (0x86824, 0x0a080128),
            (0x86828, 0x13000108),
            (0x8682c, 0xb9010008),
            (0x86830, 0x320003e0),
            (0x868e0, 0xb9410008),
            (0x868e4, 0xaa0003f3),
            (0x868e8, 0x36f802e8),
            (0x868ec, 0x39441268),
            (0x868f0, 0x350002a8),
            (0x868f4, 0x91044274),
            (0x868f8, 0xaa1403e0),
            (0x868fc, 0x9400d2e9),
            (0x86900, 0xf0000688),
            (0x86904, 0xbd46fd01),
            (0x86908, 0x1e211800),
            (0x8690c, 0x1e2c1001),
            (0x86910, 0x1e212800),
            (0x86914, 0xaa1403e0),
            (0x86918, 0x1e380015),
            (0x8691c, 0xb9010e75),
            (0x86920, 0x9400d2c0),
            (0x86924, 0x5280bb88),
            (0x86928, 0x1b087ea8),
            (0x8692c, 0x52800841),
            (0x86930, 0x1e220100),
            (0x86934, 0xf0000688),
            (0x86938, 0xbd4a6101),
            (0x8693c, 0x2a1f03e2),
            (0x86940, 0x9400d430),
            (0x86944, 0x12800008),
            (0x86948, 0xb9010268),
            (0x869f0, 0xb9410008),
            (0x869f4, 0x7100051f),
            (0x869f8, 0x540001a1),
            (0x869fc, 0xa9bf7bfd),
            (0x86a00, 0x910003fd),
            (0x86a04, 0x9105c000),
            (0x86a08, 0x9400d286),
            (0x86a0c, 0xf0000688),
            (0x86a10, 0xbd45a500),
            (0x86a14, 0xf0000688),
            (0x86a18, 0xbd497501),
            (0x86a1c, 0x321a03e1),
            (0x86a20, 0x2a1f03e2),
            (0x86a24, 0x9400d3f7),
            (0x86a28, 0xa8c17bfd),
            (0x86a70, 0xb9410008),
            (0x86a74, 0x7100051f),
            (0x86a78, 0x540001c1),
            (0x86a7c, 0xf81e0ff3),
            (0x86a80, 0xa9017bfd),
            (0x86a84, 0x910043fd),
            (0x86a88, 0xd0000ce8),
            (0x86a8c, 0xf9437108),
            (0x86a90, 0xaa0003f3),
            (0x86a94, 0xf9400100),
            (0x86a98, 0x1e2e1000),
            (0x86a9c, 0x97fe8205),
            (0x86aa0, 0xa9417bfd),
            (0x86aa4, 0xaa1303e0),
            (0x86aa8, 0xf84207f3),
            (0x86aac, 0x17fffe99),
            (0x86ab0, 0xd65f03c0),
            (0x86b40, 0xb9410008),
            (0x86b44, 0xaa0003f3),
            (0x86b48, 0x37f801c8),
            (0x86b4c, 0x9105c260),
            (0x86b50, 0x9400d234),
            (0x86b54, 0xf0000688),
            (0x86b58, 0xbd497501),
            (0x86b5c, 0x1e2703e0),
            (0x86b60, 0x321a03e1),
            (0x86b64, 0x2a1f03e2),
            (0x86b68, 0x9400d3a6),
            (0x86b6c, 0xb901027f),
            (0x86b70, 0xa9427bfd),
            (0x86b74, 0xa9414ff4),
            (0x86b78, 0x6cc323e9),
            (0x86b7c, 0xd65f03c0),
            (0x86b80, 0x91044274),
            (0x86b84, 0xaa1403e0),
            (0x86b88, 0x4ea21c48),
            (0x86b8c, 0x4ea01c09),
            (0x86b90, 0x9400d244),
            (0x86b94, 0x1e293901),
            (0x86b98, 0x1e213800),
            (0x86b9c, 0x1e202008),
            (0x86ba0, 0x54000065),
            (0x86ba4, 0x1e2703e0),
            (0x86ba8, 0x14000005),
            (0x86bac, 0xbd416a61),
            (0x86bb0, 0x1e212000),
            (0x86bb4, 0x5400004d),
            (0x86bb8, 0x4ea11c20),
            (0x86bbc, 0xaa1403e0),
            (0x86bc0, 0x9400d1c8),
            (0x86ca4, 0xb9410008),
            (0x86ca8, 0x36f80868),
            (0x86cac, 0x4ea01c0a),
            (0x86cb0, 0x4ea11c29),
            (0x86cb4, 0x4ea11c20),
            (0x86cb8, 0x4eaa1d41),
            (0x86cbc, 0xaa0003f3),
            (0x86cc0, 0x940257b4),
            (0x86cc4, 0x4ea01c08),
            (0x86cc8, 0x1e2a0940),
            (0x86ccc, 0x1e290921),
            (0x86cd0, 0x1e212801),
            (0x86cd4, 0x1e21c020),
            (0x86cd8, 0x1e202000),
            (0x86cdc, 0x540000a6),
            (0x86ce0, 0x1e269001),
            (0x86ce4, 0x1e212000),
            (0x86ce8, 0x540000ec),
            (0x86cec, 0x14000032),
            (0x86cf0, 0x4ea11c20),
            (0x86cf4, 0x940257ab),
            (0x86cf8, 0x1e269001),
            (0x86cfc, 0x1e212000),
            (0x86d00, 0x540005ad),
            (0x86d04, 0xf0000688),
            (0x86d08, 0xbd44bd00),
            (0x86d0c, 0xf0000688),
            (0x86d10, 0xbd481501),
            (0x86d14, 0xf0000688),
            (0x86d18, 0x1e201900),
            (0x86d1c, 0x1e210800),
            (0x86d20, 0xbd4a2101),
            (0x86d24, 0x1e212000),
            (0x86d28, 0x540001ec),
            (0x86d2c, 0xf0000688),
            (0x86d30, 0xbd443501),
            (0x86d34, 0x1e212000),
            (0x86d38, 0x54000164),
            (0x86d3c, 0x1e27d001),
            (0x86d40, 0x1e212000),
            (0x86d44, 0x54000385),
            (0x86d48, 0x1e37d001),
            (0x86d4c, 0x1e212000),
            (0x86d50, 0x5400032d),
            (0x86d54, 0xb9410e68),
            (0x86d58, 0x71000508),
            (0x86d5c, 0x1a88b3f4),
            (0x86d60, 0x14000008),
            (0x86d64, 0xb9410e68),
            (0x86d68, 0x11000514),
            (0x86d6c, 0xb9410a68),
            (0x86d70, 0xb9010e74),
            (0x86d74, 0x6b08029f),
            (0x86d78, 0x5400006b),
            (0x86d7c, 0x51000514),
            (0x86d80, 0xb9010e74),
            (0x86d84, 0x91044260),
            (0x86d88, 0x9400d1a6),
            (0x86d8c, 0x5280bb88),
            (0x86d90, 0x1b087e88),
            (0x86d94, 0x52800841),
            (0x86d98, 0x1e220100),
            (0x86d9c, 0xf0000688),
            (0x86da0, 0xbd4a6101),
            (0x86da4, 0x2a1f03e2),
            (0x86da8, 0x9400d316),
            (0x86dac, 0x320003e8),
            (0x86db0, 0x39041268),
            (0x86de0, 0xb9410008),
            (0x86de4, 0x37f80208),
            (0x86de8, 0xf81e0ff3),
            (0x86dec, 0xa9017bfd),
            (0x86df0, 0x910043fd),
            (0x86df4, 0xaa0003f3),
            (0x86df8, 0x9105c000),
            (0x86dfc, 0x9400d189),
            (0x86e00, 0xf0000688),
            (0x86e04, 0xbd497501),
            (0x86e08, 0x1e2703e0),
            (0x86e0c, 0x321a03e1),
            (0x86e10, 0x2a1f03e2),
            (0x86e14, 0x9400d2fb),
            (0x86e18, 0xb901027f),
            (0x86e1c, 0xa9417bfd),
            (0x86e20, 0xf84207f3),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native{address:x}"
            );
        }
        for (address, value) in [
            (0x159694, 32.0_f32),
            (0x159658, 116.0),
            (0x159480, 1764.0),
            (0x15971c, 1848.0),
            (0x159664, 162.0),
            (0x15940c, 980.0),
            (0x1594bc, 3.141592025756836),
            (0x159814, 180.0),
            (0x159a20, 150.0),
            (0x159434, -150.0),
        ] {
            assert_eq!(
                f32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                value
            );
        }
    }
    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_manual_page_count_controller_clocks_and_artwork() {
        let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&path)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0x86380, 0xb9410808),
            (0x86384, 0xaa0003f3),
            (0x86388, 0x350003e8),
            (0x8638c, 0x900006e1),
            (0x86390, 0x9104c421),
            (0x86394, 0x910003e0),
            (0x86398, 0x320003e2),
            (0x8639c, 0x94025a19),
            (0x863a0, 0x9102e268),
            (0x863a4, 0xc8dffd00),
            (0x863a8, 0x910003e1),
            (0x863ac, 0x940247a5),
            (0x863b0, 0x360001e0),
            (0x863b4, 0x900006f4),
            (0x863b8, 0x9104c694),
            (0x863bc, 0xb9410a68),
            (0x863c0, 0x11000509),
            (0x863c4, 0x11000902),
            (0x863c8, 0x910003e0),
            (0x863cc, 0xaa1403e1),
            (0x863d0, 0xb9010a69),
            (0x863d4, 0x94025a0b),
            (0x863d8, 0x9102e268),
            (0x863dc, 0xc8dffd00),
            (0x863e0, 0x910003e1),
            (0x863e4, 0x94024797),
            (0x863e8, 0x3707fea0),
            (0x863ec, 0xb9410a68),
            (0x863f0, 0x5280bb89),
            (0x863f4, 0x1b097d08),
            (0x863f8, 0x51177108),
            (0x863fc, 0x1e220100),
            (0x86400, 0xbd016a60),
            (0x866b8, 0xb9410e68),
            (0x866bc, 0xb94013e9),
            (0x866c0, 0x37d00229),
            (0x866c4, 0x37d80449),
            (0x866c8, 0x375001e9),
            (0x866cc, 0x530b2d29),
            (0x866d0, 0x0b080129),
            (0x866d4, 0x36f801c9),
            (0x866d8, 0x1400001f),
            (0x866dc, 0x97ff9271),
            (0x866e0, 0x3900029f),
            (0x866e4, 0x39441668),
            (0x866e8, 0x7100011f),
            (0x866ec, 0xa9457bfd),
            (0x866f0, 0x1a9f17e0),
            (0x866f4, 0xa9444ff4),
            (0x866f8, 0xa94357f6),
            (0x866fc, 0x910183ff),
            (0x86700, 0xd65f03c0),
            (0x86704, 0x51000509),
            (0x86708, 0x37f80269),
            (0x8670c, 0xb9410a6a),
            (0x86710, 0x5100054b),
            (0x86714, 0x6b0a013f),
            (0x86718, 0x1a8bb136),
            (0x8671c, 0x6b16011f),
            (0x86720, 0x54000201),
            (0x86724, 0x39403fe8),
            (0x86728, 0x362ffdc8),
            (0x8672c, 0xd0000ce8),
            (0x86730, 0xf943bd08),
            (0x86734, 0xf9400100),
            (0x86738, 0x1e2e1000),
            (0x8673c, 0x97fe82dd),
            (0x86740, 0xaa1303e0),
            (0x86744, 0x97ffff73),
            (0x86748, 0x17ffffe6),
            (0x8674c, 0x11000509),
            (0x86750, 0x36fffde9),
            (0x86754, 0x2a1f03f6),
            (0x86758, 0x6b16011f),
            (0x8675c, 0x54fffe40),
            (0x86760, 0xd0000ce8),
            (0x86764, 0xf943c108),
            (0x86768, 0xf9400100),
            (0x8676c, 0x1e2e1000),
            (0x86770, 0x97fe82d0),
            (0x86774, 0xaa1503e0),
            (0x86778, 0xb9010e76),
            (0x8677c, 0x9400d329),
            (0x86780, 0x5280bb88),
            (0x86784, 0x1b087ec8),
            (0x86788, 0x52800841),
            (0x8678c, 0x1e220100),
            (0x86790, 0xf0000688),
            (0x86794, 0xbd4a6101),
            (0x86798, 0x2a1f03e2),
            (0x8679c, 0x9400d499),
            (0x867a0, 0x17ffffd0),
            (0x87250, 0xd0000688),
            (0x87254, 0xbd400020),
            (0x87258, 0xf9400414),
            (0x8725c, 0xbd457101),
            (0x87260, 0x1e210800),
            (0x87264, 0xbd40c281),
            (0x87268, 0x1e212800),
            (0x8726c, 0x1e2e1001),
            (0x87270, 0x1e212000),
            (0x87274, 0x320003f3),
            (0x87278, 0xbd00c280),
            (0x8727c, 0x54000624),
            (0x87280, 0x32091be8),
            (0x87284, 0x52800a00),
            (0x87288, 0xb900c288),
            (0x87590, 0xf9400413),
            (0x87594, 0x9105c260),
            (0x87598, 0x9400d116),
            (0x8759c, 0x360000c0),
            (0x875a0, 0x320003e0),
            (0x875a4, 0xa9457bfd),
            (0x875a8, 0xa9444ff4),
            (0x875ac, 0x910183ff),
            (0x875b0, 0xd65f03c0),
            (0x875b4, 0xb90003ff),
            (0x875b8, 0xf9405260),
            (0x877ac, 0xd0000688),
            (0x877b0, 0xbd400020),
            (0x877b4, 0xf9400413),
            (0x877b8, 0xbd499d01),
            (0x877bc, 0x1e210800),
            (0x877c0, 0xbd40c261),
            (0x877c4, 0x1e202820),
            (0x877c8, 0x1e202008),
            (0x877cc, 0x320003e0),
            (0x877d0, 0xbd00c260),
            (0x877d4, 0x5400018c),
            (0x877d8, 0xb900c27f),
            (0x877dc, 0xb81fc3a0),
            (0x877e0, 0xf9405260),
            (0x877e4, 0xb4000160),
            (0x877e8, 0xf9400008),
            (0x877ec, 0xf9401908),
            (0x877f0, 0xd10013a1),
            (0x877f4, 0xd63f0100),
            (0x877f8, 0x2a1f03e0),
            (0x877fc, 0x320003e8),
            (0x87800, 0x3902c268),
            (0x85de4, 0xaa1503e0),
            (0x85de8, 0x9400d5ae),
            (0x85dec, 0x1b187e88),
            (0x85df0, 0x1e220101),
            (0x85df4, 0x1e203820),
            (0x85df8, 0x1e28280e),
            (0x85dfc, 0x1e2a21c0),
            (0x85e00, 0x5400070a),
            (0x85e04, 0x1e2d29cf),
            (0x85e08, 0x1e2021e8),
            (0x85e0c, 0x540006a9),
            (0x85e10, 0xbc5cc3a1),
            (0x85e14, 0x1e211800),
            (0x85e18, 0x1e2c1001),
            (0x85e1c, 0x1e210800),
            (0x85e20, 0x1e20c000),
            (0x85e24, 0x1e203920),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native{address:x}"
            );
        }
        for (address, value) in [
            (0x159570, 0.1_f32),
            (0x15999c, -0.1),
            (0x159a60, 100.0),
            (0x159660, 240.0),
            (0x1596fc, 1500.0),
            (0x1599d4, 1440.0),
            (0x1597fc, 166.0),
            (0x159674, 976.0),
        ] {
            assert_eq!(
                f32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                value
            );
        }
        assert_eq!(&exe[0x16000d + 256..0x16000d + 268], b"/manual.txa\0");
    }
    #[test]
    #[ignore = "requires supplied ROM (SHIN_REFERENCE_ROM)"]
    fn reference_native_manual_supplied_numbered_pages() {
        use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
        use std::io::Read;
        let mut rom = RomReader::new(
            StatelessFile::new(
                std::fs::File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        let file = rom.find_file("/manual.txa").unwrap();
        let mut bytes = Vec::new();
        rom.open_file(file)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let atlas = shin_core::format::texture_archive::read_texture_archive(&bytes).unwrap();
        let count = (1..)
            .take_while(|i| atlas.name_to_index.contains_key(&format!("{i:03}")))
            .count();
        assert_eq!(count, 24);
        let numbered = atlas
            .name_to_index
            .keys()
            .filter(|name| name.len() == 3 && name.bytes().all(|c| c.is_ascii_digit()))
            .count();
        assert_eq!(
            count, numbered,
            "native stops at first missing numbered page"
        );
        let model = ManualModel::new(count).unwrap();
        assert_eq!(model.count(), count);
        for i in 1..=count {
            let im = &atlas.textures[atlas.name_to_index[&format!("{i:03}")]];
            assert_eq!(im.dimensions(), (1440, 810));
            eprintln!("Manual page{i:03}:{}x{}", im.width(), im.height());
        }
    }
    #[test]
    fn help_physical_h_j_change_pages_and_k_l_do_not() {
        use shin_input::{Action, RawInputState};
        use winit::keyboard::KeyCode;
        let mut m = active(3);
        for (key, expected) in [
            (KeyCode::KeyL, 0),
            (KeyCode::KeyJ, 1),
            (KeyCode::KeyK, 1),
            (KeyCode::KeyH, 0),
        ] {
            let mut raw = RawInputState::new();
            raw.keyboard.insert(key);
            let input = AppAction::lower(&raw).map(|_, held| shin_input::ActionState {
                is_clicked: held,
                is_clicked_or_repeated: held,
                ..Default::default()
            });
            m.input(&input);
            assert_eq!(m.selected(), expected, "{key:?}");
            m.update(100.0);
        }
    }

    #[test]
    fn native_page_movement_has_priority_over_cancel_and_clamps_endpoints() {
        use AppAction as A;
        for count in [1, 2, 24] {
            for start in 0..count {
                for bits in 0..32 {
                    let mut m = active(count);
                    m.selected = start;
                    m.scroll.fast_forward_to(start as f32 * 1500.0);
                    let mut input = enum_map::EnumMap::<A, shin_input::ActionState>::default();
                    for (bit, a) in [
                        A::Left,
                        A::Right,
                        A::CharacterPageUp,
                        A::CharacterPageDown,
                        A::Cancel,
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        input[a].is_clicked = bits & (1 << bit) != 0;
                        input[a].is_clicked_or_repeated = input[a].is_clicked;
                    }
                    let expected = if bits & 1 != 0 {
                        start.saturating_sub(1)
                    } else if bits & 2 != 0 {
                        (start + 1).min(count - 1)
                    } else if bits & 4 != 0 {
                        start.saturating_sub(1)
                    } else if bits & 8 != 0 {
                        (start + 1).min(count - 1)
                    } else {
                        start
                    };
                    m.input(&input);
                    assert_eq!(m.selected(), expected);
                    if expected != start {
                        assert_eq!(m.phase(), ManualPhase::Active);
                        assert_eq!(m.take_sounds(), ["move"]);
                    } else if bits & 16 != 0 {
                        assert_eq!(m.phase(), ManualPhase::CloseFlash);
                        assert_eq!(m.take_sounds(), ["cancel"]);
                    } else {
                        assert!(m.take_sounds().is_empty());
                    }
                }
            }
        }
    }
    #[test]
    fn opening_scroll_speed_and_close_flash_clock_order_match_native() {
        assert!(ManualModel::new(0).is_none());
        let mut m = ManualModel::new(3).unwrap();
        m.update(2.5);
        assert_eq!(m.progress(), 0.25);
        m.update(7.5);
        m.select(1);
        m.update(7.5);
        assert!((m.scroll() - 1500.0 * std::f32::consts::FRAC_PI_4.sin()).abs() < 0.001);
        let mut input = enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
        input[AppAction::Cancel].is_clicked = true;
        m.input(&input);
        assert_eq!(m.phase(), ManualPhase::Active);
        m.update(7.5);
        assert_eq!(m.scroll(), 1500.0);
        m.input(&input);
        assert_eq!(m.phase(), ManualPhase::CloseFlash);
        m.update(3.5);
        assert_eq!(m.close_flash(), 0.7);
        m.update(3.5);
        assert_eq!(m.close_flash(), 0.0);
        assert_eq!(m.phase(), ManualPhase::CloseFlash);
        m.update(0.0);
        assert_eq!(m.phase(), ManualPhase::Closing);
        m.update(5.0);
        assert_eq!(m.progress(), 0.5);
        m.update(5.0);
        assert_eq!(m.phase(), ManualPhase::Closed);
    }
    #[test]
    fn artwork_metadata_retains_native_page_pitch_and_opacity() {
        let m = active(3);
        assert_eq!(
            m.page_artwork(0),
            Some((vec4(240.0, 166.0, 1440.0, 810.0), 1.0))
        );
        assert_eq!(
            m.page_artwork(1),
            Some((vec4(1740.0, 166.0, 1440.0, 810.0), 0.5))
        );
        assert_eq!(m.page_artwork(3), None);
    }
}
