//! Generic dialog keyboard and captured button callbacks (239c0/248a0/24a90).
//! Captured window dragging, release inertia and elastic screen bounds are
//! updated separately from the cached dialog texture; the live backlog owns this input task.
use super::{backlog_rollback::Confirmation, dialog_view::DialogGeometry};
use crate::app::AppAction;
use enum_map::EnumMap;
use glam::Vec2;
use shin_input::ActionState;

//247b0/247a8 use30 and46 for button hit coordinates, whereas24090
// renders its content at30,30. Preserve this native sixteen-pixel offset.
const BUTTON_HIT_OFFSET: Vec2 = Vec2::new(30.0, 46.0);

#[derive(Default)]
pub(super) struct DialogInput {
    held: bool,
    consumed: bool,
    captured: Option<usize>,
    press_position: Vec2,
    previous: Vec2,
    displacement: Vec2,
    active: bool,
    dragging: bool,
    window_drag: bool,
    velocity: Vec2,
    pointer_scale: Option<Vec2>,
    sounds: Vec<&'static str>,
}

impl DialogInput {
    /// InputArea measures small samples in physical pixels before scaling
    /// callbacks to its1920x1080 canvas. Default matches Switch1280x720.
    pub fn set_pointer_scale(&mut self, scale: Vec2) {
        assert!(scale.is_finite() && scale.cmpgt(Vec2::ZERO).all());
        self.pointer_scale = Some(scale);
    }
    pub fn ignore_pointer(&mut self, held: bool) {
        self.consumed |= held || self.held;
        self.held = held;
        self.captured = None;
        self.active = false;
        self.window_drag = false;
    }
    pub fn take_sounds(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.sounds)
    }

    pub fn button_at(geometry: &DialogGeometry, point: Vec2) -> Option<usize> {
        let point = point - geometry.origin - BUTTON_HIT_OFFSET;
        geometry.buttons[..geometry.button_count]
            .iter()
            .position(|rect| {
                point.x >= rect.x
                    && point.x < rect.x + rect.z
                    && point.y >= rect.y
                    && point.y < rect.y + rect.w
            })
    }

    /// Native InputArea captures its press-time button and abandons it once
    /// movement passes sixteen pixels. Passive hover does not select buttons.
    /// A canceled press cannot retarget another button before release.
    pub fn pointer(
        &mut self,
        dialog: &mut Confirmation,
        geometry: &mut DialogGeometry,
        position: Vec2,
        held: bool,
    ) {
        self.pointer_timed(dialog, geometry, position, held, 1.0 / 60.0);
    }

    pub fn pointer_timed(
        &mut self,
        dialog: &mut Confirmation,
        geometry: &mut DialogGeometry,
        position: Vec2,
        held: bool,
        elapsed_seconds: f64,
    ) {
        let pressed = held && !self.held;
        let released = !held && self.held;
        self.consumed |= held || self.held;
        self.held = held;
        if !dialog.is_open() {
            self.captured = None;
            self.active = false;
            self.window_drag = false;
            return;
        }
        if pressed {
            self.active = true;
            self.dragging = false;
            self.press_position = position;
            self.previous = position;
            self.displacement = Vec2::ZERO;
            self.captured = Self::button_at(geometry, position);
            let local = position - geometry.origin;
            self.window_drag = self.captured.is_none()
                && local.x >= 0.0
                && local.y >= 0.0
                && local.x < geometry.outer.x
                && local.y < geometry.outer.y;
            if self.window_drag {
                self.velocity = Vec2::ZERO;
            }
            if let Some(index) = self.captured {
                dialog.hold(index);
            }
        } else if held && self.active {
            let delta = position - self.previous;
            let scale = self.pointer_scale.unwrap_or(Vec2::splat(1.5));
            let first_drag = !self.dragging
                && ((position - self.press_position) / scale).length() * scale.x > 16.0;
            if first_drag {
                self.dragging = true;
                if let Some(index) = self.captured.take() {
                    dialog.cancel_hold(index);
                }
            }
            if self.dragging && (first_drag || (delta / scale).length() > 1.0) {
                if self.window_drag {
                    geometry.origin += delta;
                }
                self.previous = position;
            }
            self.displacement = delta;
        } else if released && self.active {
            if self.dragging {
                if self.window_drag && elapsed_seconds > 0.0 && elapsed_seconds.is_finite() {
                    let velocity =
                        self.displacement * (0.016683333333333335 / elapsed_seconds) as f32;
                    let scale = self.pointer_scale.unwrap_or(Vec2::splat(1.5));
                    if (velocity / scale).length() * scale.x > 20.0 {
                        self.velocity = velocity;
                    }
                }
            } else if let Some(index) = self.captured.take() {
                //24a90 confirms d4, not the button under the release point.
                dialog.select(index);
                if dialog.confirm() {
                    self.sounds.push("decide");
                }
            }
            // InputArea calls fling/release first, then its reset callback.
            self.active = false;
            self.window_drag = false;
        }
    }

    ///23d50: velocity is added once per update, not multiplied by ticks.
    /// Decay uses .9^ticks; offscreen edges recover by .8 once per update.
    /// A captured window press disables edge recovery until reset on release.
    pub fn update_motion(&mut self, geometry: &mut DialogGeometry, delta: shin_core::time::Ticks) {
        geometry.origin += self.velocity;
        self.velocity *= 0.9f32.powf(delta.as_f32());
        if !self.window_drag {
            for axis in 0..2 {
                let limit = [1920.0, 1080.0][axis] - geometry.outer[axis];
                if geometry.origin[axis] < 0.0 {
                    geometry.origin[axis] *= 0.8;
                } else if geometry.origin[axis] > limit {
                    geometry.origin[axis] = limit + (geometry.origin[axis] - limit) * 0.8;
                }
            }
        }
    }

    ///239c0: pointer ownership first, then decide, cancel, previous/next and
    /// orthogonal first/last. Main-axis navigation wraps; orthogonal navigation
    /// requires that neither main-axis direction is held.
    pub fn keyboard(
        &mut self,
        dialog: &mut Confirmation,
        vertical: bool,
        state: &EnumMap<AppAction, ActionState>,
    ) {
        let pointer_consumed = std::mem::take(&mut self.consumed) || self.held;
        if !dialog.is_open() || pointer_consumed {
            return;
        }
        if state[AppAction::Enter].is_clicked {
            if dialog.confirm() {
                self.sounds.push("decide");
            } else {
                self.sounds.push("error");
            }
            return;
        }
        if state[AppAction::Cancel].is_clicked {
            if dialog.cancel() {
                self.sounds.push("cancel");
            }
            return;
        }
        let (previous, next, first, last) = if vertical {
            (
                AppAction::Up,
                AppAction::AnyDown,
                AppAction::Left,
                AppAction::Right,
            )
        } else {
            (
                AppAction::Left,
                AppAction::Right,
                AppAction::Up,
                AppAction::AnyDown,
            )
        };
        let selected = dialog.selected();
        let count = dialog.button_count();
        let index = if state[previous].is_clicked_or_repeated {
            Some(selected.map_or(count - 1, |index| (index + count - 1) % count))
        } else if state[next].is_clicked_or_repeated {
            Some(selected.map_or(0, |index| (index + 1) % count))
        } else if state[previous].is_held || state[next].is_held {
            None
        } else if state[first].is_clicked_or_repeated {
            Some(0)
        } else if state[last].is_clicked_or_repeated {
            Some(count - 1)
        } else {
            None
        };
        if index.is_some_and(|index| dialog.select(index)) {
            self.sounds.push("move");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shin_core::time::Ticks;
    fn open() -> Confirmation {
        let mut dialog = Confirmation::default();
        dialog.update(Ticks::from_f32(12.0));
        dialog.update(Ticks::ZERO);
        dialog
    }
    fn keys(action: AppAction) -> EnumMap<AppAction, ActionState> {
        let mut state = EnumMap::default();
        state[action] = ActionState {
            is_clicked: true,
            is_clicked_or_repeated: true,
            is_held: true,
            ..Default::default()
        };
        state
    }
    fn geometry() -> DialogGeometry {
        DialogGeometry::new(
            Vec2::new(762.0, 130.0),
            [Vec2::new(94.0, 58.0), Vec2::new(138.0, 58.0)],
        )
    }
    fn point(g: &DialogGeometry, index: usize) -> Vec2 {
        let r = g.buttons[index];
        g.origin + BUTTON_HIT_OFFSET + Vec2::new(r.x + r.z * 0.5, r.y + r.w * 0.5)
    }
    #[test]
    fn press_during_dialog_loading_cannot_reacquire_on_open_or_release() {
        let mut input = DialogInput::default();
        input.ignore_pointer(true);
        let mut dialog = open();
        let mut g = geometry();
        let p = point(&g, 0);
        input.pointer(&mut dialog, &mut g, p, true);
        input.pointer(&mut dialog, &mut g, p, false);
        assert!(dialog.is_open());
        assert_eq!(dialog.selected(), Some(1));
        assert!(input.take_sounds().is_empty());
        input.pointer(&mut dialog, &mut g, p, true);
        input.pointer(&mut dialog, &mut g, p, false);
        assert!(!dialog.is_open());
        assert_eq!(input.take_sounds(), vec!["decide"]);
    }
    #[test]
    fn layout_axes_wrap_and_orthogonal_keys_choose_ends() {
        for vertical in [false, true] {
            let mut dialog = open();
            let mut input = DialogInput::default();
            let (prev, next, first, last) = if vertical {
                (
                    AppAction::Up,
                    AppAction::AnyDown,
                    AppAction::Left,
                    AppAction::Right,
                )
            } else {
                (
                    AppAction::Left,
                    AppAction::Right,
                    AppAction::Up,
                    AppAction::AnyDown,
                )
            };
            input.keyboard(&mut dialog, vertical, &keys(next));
            assert_eq!(dialog.selected(), Some(0));
            input.keyboard(&mut dialog, vertical, &keys(prev));
            assert_eq!(dialog.selected(), Some(1));
            input.keyboard(&mut dialog, vertical, &keys(first));
            assert_eq!(dialog.selected(), Some(0));
            input.keyboard(&mut dialog, vertical, &keys(first));
            assert_eq!(input.take_sounds(), vec!["move", "move", "move"]);
            let mut state = keys(last);
            state[prev].is_held = true;
            input.keyboard(&mut dialog, vertical, &state);
            assert_eq!(dialog.selected(), Some(0));
            input.keyboard(&mut dialog, vertical, &keys(last));
            assert_eq!(dialog.selected(), Some(1));
        }
    }
    #[test]
    fn decide_and_cancel_precede_navigation_and_opening_ignores_keys() {
        let mut dialog = Confirmation::default();
        let mut input = DialogInput::default();
        input.keyboard(&mut dialog, false, &keys(AppAction::Enter));
        assert!(input.take_sounds().is_empty());
        dialog = open();
        let mut state = keys(AppAction::Enter);
        state[AppAction::Cancel] = state[AppAction::Enter];
        state[AppAction::Left] = state[AppAction::Enter];
        input.keyboard(&mut dialog, false, &state);
        assert_eq!(dialog.selected(), Some(1));
        assert!(!dialog.is_open());
        assert_eq!(input.take_sounds(), vec!["decide"]);
        dialog = open();
        input.keyboard(&mut dialog, false, &keys(AppAction::Cancel));
        assert_eq!(dialog.selected(), None);
        assert_eq!(input.take_sounds(), vec!["cancel"]);
    }
    #[test]
    fn press_holds_flash_and_release_confirms_exact_captured_button() {
        let mut g = geometry();
        let p = point(&g, 0);
        let mut dialog = open();
        let mut input = DialogInput::default();
        input.pointer(&mut dialog, &mut g, p, false);
        assert_eq!(dialog.selected(), Some(1));
        input.pointer(&mut dialog, &mut g, p, true);
        assert_eq!(dialog.selected(), Some(0));
        assert!(dialog.is_open());
        dialog.update(Ticks::from_f32(3.5));
        assert_eq!(dialog.flashes[0].value(), 0.7);
        input.keyboard(&mut dialog, false, &keys(AppAction::Cancel));
        assert!(dialog.is_open());
        input.pointer(&mut dialog, &mut g, p, false);
        assert!(!dialog.is_open());
        assert_eq!(input.take_sounds(), vec!["decide"]);
        dialog.update(Ticks::from_f32(3.5));
        assert_eq!(dialog.flashes[0].value(), 0.0);
    }
    #[test]
    fn movement_cancels_without_retarget_and_release_still_consumes_keys() {
        let mut g = geometry();
        let p = point(&g, 0);
        let mut dialog = open();
        let mut input = DialogInput::default();
        input.pointer(&mut dialog, &mut g, p, true);
        dialog.update(Ticks::from_f32(3.5));
        input.pointer(&mut dialog, &mut g, p + Vec2::X * 16.0, true);
        assert_eq!(input.captured, Some(0));
        let other = point(&g, 1);
        input.pointer(&mut dialog, &mut g, other, true);
        assert_eq!(input.captured, None);
        dialog.update(Ticks::from_f32(3.5));
        assert_eq!(dialog.flashes[0].value(), 0.0);
        input.pointer(&mut dialog, &mut g, p, true);
        input.pointer(&mut dialog, &mut g, p, false);
        input.keyboard(&mut dialog, false, &keys(AppAction::Enter));
        assert!(dialog.is_open());
        assert!(input.take_sounds().is_empty());
        input.keyboard(&mut dialog, false, &keys(AppAction::Enter));
        assert!(!dialog.is_open());
    }
    #[test]
    fn opening_held_button_cannot_reacquire_and_press_outside_does_not_retarget() {
        let mut g = geometry();
        let p = point(&g, 0);
        let mut dialog = Confirmation::default();
        let mut input = DialogInput::default();
        input.pointer(&mut dialog, &mut g, p, true);
        dialog.update(Ticks::from_f32(12.0));
        dialog.update(Ticks::ZERO);
        input.pointer(&mut dialog, &mut g, p, true);
        input.pointer(&mut dialog, &mut g, p, false);
        assert_eq!(dialog.selected(), Some(1));
        assert!(dialog.is_open());
        input.pointer(&mut dialog, &mut g, Vec2::ZERO, true);
        input.pointer(&mut dialog, &mut g, p, false);
        assert!(dialog.is_open());
    }
    #[test]
    fn native_hit_rect_excludes_far_edges() {
        let mut g = geometry();
        let r = g.buttons[0];
        let far = g.origin + BUTTON_HIT_OFFSET + Vec2::new(r.x + r.z, r.y + r.w);
        assert_eq!(DialogInput::button_at(&g, far), None);
        assert_eq!(DialogInput::button_at(&g, far - Vec2::splat(0.01)), Some(0));
        assert_eq!(DialogInput::button_at(&g, far + Vec2::X), None);
    }
    #[test]
    fn small_captured_drift_outside_button_still_releases_original_choice() {
        let mut g = geometry();
        let r = g.buttons[0];
        let p = g.origin + BUTTON_HIT_OFFSET + Vec2::new(r.x + 2.0, r.y + 20.0);
        let mut dialog = open();
        let mut input = DialogInput::default();
        input.pointer(&mut dialog, &mut g, p, true);
        input.pointer(&mut dialog, &mut g, p - Vec2::X * 12.0, true);
        assert_eq!(DialogInput::button_at(&g, p - Vec2::X * 12.0), None);
        input.pointer(&mut dialog, &mut g, p - Vec2::X * 12.0, false);
        assert_eq!(dialog.selected(), Some(0));
        assert!(!dialog.is_open());
        assert_eq!(input.take_sounds(), vec!["decide"]);
    }

    #[test]
    fn window_drag_samples_threshold_and_releases_last_sample_velocity() {
        let mut g = geometry();
        let mut dialog = open();
        let mut input = DialogInput::default();
        let origin = g.origin;
        let p = origin + Vec2::splat(10.0);
        input.pointer_timed(&mut dialog, &mut g, p, true, 0.02);
        input.pointer_timed(&mut dialog, &mut g, p + Vec2::X * 16.0, true, 0.02);
        assert_eq!(g.origin, origin);
        input.pointer_timed(&mut dialog, &mut g, p + Vec2::X * 40.0, true, 0.02);
        assert_eq!(g.origin, origin + Vec2::X * 40.0);
        input.pointer_timed(&mut dialog, &mut g, p + Vec2::X * 41.5, true, 0.02);
        assert_eq!(g.origin, origin + Vec2::X * 40.0);
        input.pointer_timed(&mut dialog, &mut g, p + Vec2::X * 80.0, true, 0.02);
        assert_eq!(g.origin, origin + Vec2::X * 80.0);
        input.pointer_timed(
            &mut dialog,
            &mut g,
            p + Vec2::X * 80.0,
            false,
            0.016683333333333335,
        );
        assert_eq!(input.velocity, Vec2::X * 40.0);
        input.update_motion(&mut g, Ticks::from_f32(2.0));
        assert_eq!(g.origin, origin + Vec2::X * 120.0);
        assert!((input.velocity.x - 32.4).abs() < 0.0001);
        assert!(dialog.is_open());
        assert!(input.take_sounds().is_empty());
    }
    #[test]
    fn small_drag_samples_use_physical_pixels_at_each_viewport_scale() {
        for scale in [1.0, 1.5, 2.0] {
            let mut g = geometry();
            let mut dialog = open();
            let mut input = DialogInput::default();
            input.set_pointer_scale(Vec2::splat(scale));
            let origin = g.origin;
            let p = origin + Vec2::splat(10.0);
            input.pointer(&mut dialog, &mut g, p, true);
            input.pointer(&mut dialog, &mut g, p + Vec2::X * 18.0, true);
            input.pointer(&mut dialog, &mut g, p + Vec2::X * (18.0 + scale), true);
            assert_eq!(g.origin, origin + Vec2::X * 18.0);
            input.pointer(
                &mut dialog,
                &mut g,
                p + Vec2::X * (18.0 + scale + 0.25),
                true,
            );
            assert_eq!(g.origin, origin + Vec2::X * (18.0 + scale + 0.25));
        }
    }
    #[test]
    fn stationary_hold_or_slow_release_produces_no_momentum() {
        for stationary in [false, true] {
            let mut g = geometry();
            let mut dialog = open();
            let mut input = DialogInput::default();
            let p = g.origin + Vec2::splat(10.0);
            input.pointer_timed(&mut dialog, &mut g, p, true, 0.02);
            input.pointer_timed(&mut dialog, &mut g, p + Vec2::X * 40.0, true, 0.02);
            if stationary {
                input.pointer_timed(&mut dialog, &mut g, p + Vec2::X * 40.0, true, 0.02);
            }
            input.pointer_timed(
                &mut dialog,
                &mut g,
                p + Vec2::X * 40.0,
                false,
                if stationary { 0.02 } else { 1.0 },
            );
            assert_eq!(input.velocity, Vec2::ZERO);
        }
    }
    #[test]
    fn window_press_stops_inertia_but_button_press_retains_it() {
        for button in [false, true] {
            let mut g = geometry();
            let mut dialog = open();
            let mut input = DialogInput::default();
            input.velocity = Vec2::new(20.0, 10.0);
            let p = if button {
                point(&g, 0)
            } else {
                g.origin + Vec2::splat(10.0)
            };
            input.pointer(&mut dialog, &mut g, p, true);
            assert_eq!(
                input.velocity,
                if button {
                    Vec2::new(20.0, 10.0)
                } else {
                    Vec2::ZERO
                }
            );
        }
    }
    #[test]
    fn button_or_outside_drag_never_moves_window_or_reacquires_it() {
        for button in [false, true] {
            let mut g = geometry();
            let mut dialog = open();
            let mut input = DialogInput::default();
            let origin = g.origin;
            let p = if button { point(&g, 0) } else { Vec2::ZERO };
            input.pointer(&mut dialog, &mut g, p, true);
            input.pointer(&mut dialog, &mut g, p + Vec2::X * 40.0, true);
            let inside = origin + Vec2::splat(10.0);
            input.pointer(&mut dialog, &mut g, inside, true);
            input.pointer(&mut dialog, &mut g, inside, false);
            assert_eq!(g.origin, origin);
            assert_eq!(input.velocity, Vec2::ZERO);
            assert!(dialog.is_open());
        }
    }
    #[test]
    fn captured_window_can_leave_screen_then_recovers_elastically_after_release() {
        let mut g = geometry();
        let mut dialog = open();
        let mut input = DialogInput::default();
        let p = g.origin + Vec2::splat(10.0);
        input.pointer(&mut dialog, &mut g, p, true);
        input.pointer(&mut dialog, &mut g, Vec2::splat(-110.0), true);
        assert_eq!(g.origin, Vec2::splat(-120.0));
        input.update_motion(&mut g, Ticks::from_f32(3.0));
        assert_eq!(g.origin, Vec2::splat(-120.0));
        // A stationary sample suppresses fling before release.
        input.pointer(&mut dialog, &mut g, Vec2::splat(-110.0), true);
        input.pointer(&mut dialog, &mut g, Vec2::splat(-110.0), false);
        input.update_motion(&mut g, Ticks::from_f32(3.0));
        assert_eq!(g.origin, Vec2::splat(-96.0));
        g.origin = Vec2::new(1920.0, 1080.0) - g.outer + Vec2::splat(120.0);
        input.update_motion(&mut g, Ticks::from_f32(0.5));
        assert_eq!(
            g.origin,
            Vec2::new(1920.0, 1080.0) - g.outer + Vec2::splat(96.0)
        );
    }
    #[test]
    fn motion_uses_once_per_update_displacement_with_fractional_decay() {
        let mut g = geometry();
        let origin = g.origin;
        let mut input = DialogInput::default();
        input.velocity = Vec2::new(10.0, 20.0);
        input.update_motion(&mut g, Ticks::from_f32(0.5));
        assert_eq!(g.origin, origin + Vec2::new(10.0, 20.0));
        let factor = 0.9f32.sqrt();
        assert!((input.velocity - Vec2::new(10.0, 20.0) * factor).length() < 0.00001);
    }

    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
    fn reference_native_dialog_window_drag_inertia_and_elastic_bounds() {
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
            (0x248c8, 0x3903401f),
            (0x248cc, 0xb900d408),
            (0x248d4, 0xb900d660),
            (0x248dc, 0x91052260),
            (0x24944, 0x1e212120),
            (0x24948, 0x1e2945a2),
            (0x2494c, 0x1e289582),
            (0x24950, 0x1e209500),
            (0x24954, 0x54000085),
            (0x24958, 0x320003e8),
            (0x2495c, 0x39034268),
            (0x24960, 0xf9006e7f),
            (0x249a4, 0xb900d408),
            (0x249a8, 0x3903401f),
            (0x24c08, 0x91020100),
            (0x24c18, 0x1e2703e0),
            (0x24c24, 0x94025b77),
            (0x24c2c, 0xb900d668),
            (0x24c30, 0x39434268),
            (0x24c38, 0x1e2b3940),
            (0x24c3c, 0x1e293901),
            (0x24c48, 0x1e202840),
            (0x24c50, 0x1e222821),
            (0x24c58, 0xbd014a60),
            (0x24c5c, 0xbd014e61),
            (0x24d30, 0x39434008),
            (0x24d34, 0x34000068),
            (0x24d38, 0xbd00d800),
            (0x24d3c, 0xbd00dc01),
            (0x23d58, 0x1e202820),
            (0x23d70, 0x1e202840),
            (0x23d7c, 0x9403e3b1),
            (0x23d84, 0x1e210800),
            (0x23d94, 0x9403e3ab),
            (0x23d9c, 0x1e210803),
            (0x23da8, 0x350005a8),
            (0x23dcc, 0x1400000f),
            (0x23dfc, 0x1e250884),
            (0x23e04, 0x1e223862),
            (0x23e1c, 0x1e200820),
            (0x23e4c, 0x1e230842),
            (0x23e54, 0x1e203820),
            (0x6b1e0, 0x1e603920),
            (0x6b1ec, 0x1e601940),
            (0x6b234, 0x0b202908),
            (0x6b238, 0x531f7908),
            (0x6b240, 0x1e2b1800),
            (0x6b244, 0x1e202100),
            (0x6b248, 0x54001b2d),
            (0x6b254, 0xf9402108),
            (0x6b25c, 0x1e2c8800),
            (0x6b260, 0x1e2d8821),
            (0x6b268, 0xd63f0100),
            (0x6b5b4, 0xf9400d08),
            (0x6b5bc, 0xd63f0100),
            (0x6b774, 0x1e212000),
            (0x6b778, 0x5400056c),
            (0x6b82c, 0xf9401d08),
            (0x6b84c, 0xd63f0100),
            (0x6b854, 0x2d0b326d),
            (0x6b85c, 0x2d0c2a6b),
        ] {
            assert_eq!(word(address), expected, "native dialog motion {address:x}");
        }
        assert_eq!(f32::from_bits(word(0x15957c)), 0.9);
        assert_eq!(f32::from_bits(word(0x1594fc)), 0.8);
        assert_eq!(f32::from_bits(word(0x1596d0)), -1920.0);
        assert_eq!(f32::from_bits(word(0x15943c)), -1080.0);
        assert_eq!(f32::from_bits(word(0x159624)), 960.0);
        assert_eq!(
            f64::from_le_bytes(exe[0x164d08 + 256..0x164d08 + 264].try_into().unwrap()),
            0.016683333333333335
        );
        let relocation = |address: u64| {
            exe[0x11e040 + 256..0x152eb8 + 256]
                .chunks_exact(24)
                .find(|entry| u64::from_le_bytes(entry[..8].try_into().unwrap()) == address)
                .map(|entry| u64::from_le_bytes(entry[16..24].try_into().unwrap()))
                .unwrap()
        };
        // The listener at object+8 uses adjustor callbacks; fling is invoked
        // before its reset clears the window-capture flag.
        let listener = relocation(0x224798) + 0x88;
        assert_eq!(listener, 0x1d2d40);
        for (offset, callback) in [
            (0x10, 0x24980),
            (0x18, 0x249b0),
            (0x20, 0x24a80),
            (0x28, 0x24ba0),
            (0x38, 0x24c80),
            (0x40, 0x24d50),
            (0x48, 0x24dd0),
        ] {
            assert_eq!(relocation(listener + offset), callback);
        }
    }

    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
    fn reference_native_dialog_input_priority_capture_navigation_and_cues() {
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
            (0x23ae8, 0x91086260),
            (0x23aec, 0x94011f85),
            (0x23af0, 0x37000b80),
            (0x23b00, 0x37e01e28),
            (0x23b04, 0x36e82168),
            (0x23b10, 0xb0001008),
            (0x23b14, 0xf943bd08),
            (0x23b20, 0x94000de4),
            (0x23ec4, 0xb940ca68),
            (0x23ed0, 0xf9437108),
            (0x23edc, 0x94000cf5),
            (0x23efc, 0xbd45a500),
            (0x23f04, 0xbd497509),
            (0x23f14, 0x94025ebb),
            (0x23f28, 0x94025eb6),
            (0x23f34, 0x320603e8),
            (0x23f38, 0x320803e9),
            (0x23f40, 0x1a88012e),
            (0x23f44, 0x1a890111),
            (0x23f54, 0x320503ed),
            (0x23f58, 0x320703ef),
            (0x23f5c, 0x1a8d01ec),
            (0x23f70, 0x7100053f),
            (0x23f74, 0x1a89b16a),
            (0x23f78, 0x51000554),
            (0x23fa4, 0x1100052a),
            (0x23fac, 0x1a89a7f4),
            (0x23fd8, 0x6a0a017f),
            (0x23fdc, 0x1a8903f4),
            (0x24008, 0x5100056b),
            (0x24010, 0x1a890174),
            (0x24074, 0xf943c108),
            (0x24080, 0x94000c8c),
            (0x247a8, 0xbd447501),
            (0x247b0, 0x1e27d000),
            (0x247cc, 0x1e203900),
            (0x247d0, 0x1e213921),
            (0x24844, 0x1e209500),
            (0x24848, 0x1e294562),
            (0x2484c, 0x1e219520),
            (0x24850, 0x540000a4),
            (0x248cc, 0xb900d408),
            (0x248d4, 0xb900d660),
            (0x249e0, 0x91020100),
            (0x249ec, 0xbd45a500),
            (0x249f4, 0xbd497501),
            (0x24a00, 0x94025c00),
            (0x24a2c, 0xbd457101),
            (0x24a58, 0x1e2e1000),
            (0x24a90, 0xb940d408),
            (0x24ab0, 0xf9437108),
            (0x24ac0, 0x940009fc),
            (0x24b48, 0xbd45a500),
            (0x24b50, 0xbd497508),
            (0x24b60, 0x94025ba8),
            (0x24b74, 0x94025ba3),
            (0x24b7c, 0xb900c268),
            (0x24d9c, 0xbd497501),
            (0x24da0, 0x1e2703e0),
            (0x24dac, 0x94025b15),
            (0x24db4, 0xb900d668),
            (0x6b718, 0x1e21c008),
            (0x6b748, 0x1e202100),
            (0x6b74c, 0x540001ad),
        ] {
            assert_eq!(word(address), expected, "native dialog input {address:x}");
        }
        assert_eq!(f32::from_bits(word(0x159474)), BUTTON_HIT_OFFSET.y);
        let relocation = |address: u64| {
            exe[0x11e040 + 256..0x152eb8 + 256]
                .chunks_exact(24)
                .find(|entry| u64::from_le_bytes(entry[..8].try_into().unwrap()) == address)
                .map(|entry| u64::from_le_bytes(entry[16..24].try_into().unwrap()))
                .unwrap()
        };
        for (got, expected) in [
            (0x2246e0, "decide"),
            (0x2246e8, "error"),
            (0x224778, "cancel"),
            (0x224780, "move"),
        ] {
            let start = relocation(relocation(got)) as usize + 256;
            let end = exe[start..].iter().position(|&byte| byte == 0).unwrap() + start;
            assert_eq!(std::str::from_utf8(&exe[start..end]).unwrap(), expected);
        }
    }
}
