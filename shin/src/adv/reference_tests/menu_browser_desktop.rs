//! Real sampled desktop releases through the shared modal contact dispatcher.
use crate::adv::{
    backlog::gesture::Gesture,
    input_area::{InputArea, PointerSampler},
    system_menu::{SystemMenuModel, SystemMenuPhase},
};
use glam::{Vec2, vec2};

struct Menu {
    model: SystemMenuModel,
    area: InputArea,
    sampler: PointerSampler,
    scale: Vec2,
    desktop: bool,
}
impl Menu {
    fn new(size: (u16, u16), desktop: bool) -> Self {
        let mut model = SystemMenuModel::new(-1, false);
        for _ in 0..60 {
            model.update(1.0);
        }
        assert_eq!(model.phase(), SystemMenuPhase::Active);
        assert!(model.take_sounds().is_empty());
        Self {
            model,
            area: InputArea::new(size.0, size.1),
            sampler: Default::default(),
            scale: vec2(size.0 as f32 / 1920., size.1 as f32 / 1080.),
            desktop,
        }
    }
    fn sample(&mut self, position: Vec2, held: bool, dt: f64) {
        self.sampler.sample(position * self.scale, held, dt);
        self.model.prepare_frame();
        self.area.input_frame_with_mouse_release(
            &mut self.model,
            &self.sampler.frame(),
            &Default::default(),
            self.desktop.then(|| self.sampler.mouse_release()).flatten(),
        );
        self.model.advance_clocks((dt * 60.) as f32);
    }
    fn drag(&mut self, hz: u32, delta: Vec2) {
        let origin = vec2(1000., 200.);
        let dt = 1. / hz as f64;
        self.sample(origin, false, dt);
        self.sample(origin, true, dt);
        for _ in 0..hz * 3 / 10 {
            self.sample(origin, true, dt);
        }
        for i in 1..=hz / 5 {
            self.sample(origin + delta * (i as f32 / (hz / 5) as f32), true, dt);
            assert_eq!(
                self.model.phase(),
                SystemMenuPhase::Active,
                "hold never closes"
            );
        }
        for _ in 0..(0.04 * hz as f64).ceil() as u32 {
            self.sample(origin + delta, true, dt);
        }
        self.sample(origin + delta, false, dt);
    }
}

#[test]
fn umineko_menu_desktop_left_swipe_reuses_overlay_policy_at_both_rates_and_sizes() {
    for size in [(1280, 720), (1920, 1080), (3840, 2160)] {
        for hz in [60, 120] {
            for desktop in [false, true] {
                let mut m = Menu::new(size, desktop);
                m.drag(hz, vec2(-128., 0.));
                assert_eq!(
                    m.model.phase(),
                    if desktop {
                        SystemMenuPhase::Closing
                    } else {
                        SystemMenuPhase::Active
                    }
                );
                assert_eq!(
                    m.model.take_sounds(),
                    if desktop { vec!["cancel"] } else { vec![] }
                );
                assert_eq!(m.model.take_item(), None, "swipe must not confirm a row");
                for _ in 0..60 {
                    m.sample(vec2(872., 200.), false, 1. / hz as f64);
                }
                assert_eq!(
                    m.model.phase(),
                    if desktop {
                        SystemMenuPhase::Closed
                    } else {
                        SystemMenuPhase::Active
                    }
                );
            }
        }
    }
}

#[test]
fn umineko_menu_desktop_swipe_preserves_direction_and_cancelled_contacts() {
    for delta in [
        vec2(128., 0.),
        vec2(0., 128.),
        vec2(0., -128.),
        vec2(-8., 0.),
    ] {
        let mut m = Menu::new((1920, 1080), true);
        m.drag(60, delta);
        if delta == vec2(-8., 0.) {
            // Below the drag threshold this remains the native blank-background
            // tap, which closes the menu rather than being swallowed.
            assert_eq!(m.model.phase(), SystemMenuPhase::Closing);
            assert_eq!(m.model.take_sounds(), ["cancel"]);
        } else {
            assert_eq!(m.model.phase(), SystemMenuPhase::Active);
            assert!(m.model.take_sounds().is_empty());
        }
        assert_eq!(m.model.take_item(), None);
    }
    let mut m = Menu::new((1280, 720), true);
    m.sample(vec2(1000., 200.), true, 1. / 60.);
    m.sampler.cancel_contact(true);
    m.sample(vec2(872., 200.), true, 1. / 60.);
    m.sample(vec2(872., 200.), false, 1. / 60.);
    assert_eq!(
        m.model.phase(),
        SystemMenuPhase::Active,
        "focus cancellation cannot close"
    );
    assert!(m.model.take_sounds().is_empty());
}

#[test]
fn umineko_browser_desktop_left_swipe_preserves_vertical_velocity_and_native_release() {
    for size in [(1280, 720), (1920, 1080), (3840, 2160)] {
        let scale = vec2(size.0 as f32 / 1920., size.1 as f32 / 1080.);
        for hz in [60, 120] {
            for desktop in [false, true] {
                for delta in [vec2(-128., 0.), vec2(128., 0.), vec2(0., 128.)] {
                    let mut sampler = PointerSampler::default();
                    let mut gesture = Gesture::default();
                    let origin = vec2(1000., 200.);
                    let dt = 1. / hz as f64;
                    let mut sample = |p: Vec2, held| {
                        sampler.sample(p * scale, held, dt);
                        let release = desktop
                            .then(|| sampler.mouse_release())
                            .flatten()
                            .map(|r| r.scaled(Vec2::ONE / scale));
                        gesture.update_browser_with_mouse_release(p, held, true, dt, release)
                    };
                    sample(origin, false);
                    sample(origin, true);
                    for _ in 0..hz * 3 / 10 {
                        sample(origin, true);
                    }
                    for i in 1..=hz / 5 {
                        assert!(
                            sample(origin + delta * (i as f32 / (hz / 5) as f32), true)
                                .fling
                                .is_none()
                        );
                    }
                    for _ in 0..(0.04 * hz as f64).ceil() as u32 {
                        sample(origin + delta, true);
                    }
                    let motion = sample(origin + delta, false);
                    assert_eq!(
                        motion.fling.is_some(),
                        desktop && delta.x < 0.,
                        "only the deliberate left action gets displacement fallback"
                    );
                    if let Some(v) = motion.fling {
                        assert!(!crate::adv::backlog::gesture::scroll_fling(v));
                        assert!(
                            (v - delta).length() < 2.,
                            "physical integer sampling remains scaled once"
                        );
                    }
                }
            }
        }
    }
}
