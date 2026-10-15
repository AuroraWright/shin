//! Desktop release adaptation; the unchanged native poll remains independently
//! compared by the existing original navigation corpus.
use super::*;
use crate::adv::{
    higurashi::browser_controls::{RowMetadata, Viewport},
    input_area::PointerSampler,
};
use glam::vec2;

#[derive(Default)]
struct Events(Vec<Event>);
impl Services for Events {
    fn event(&mut self, event: Event) {
        self.0.push(event);
    }
    fn voice_is_playing(&self) -> bool {
        false
    }
}
struct Harness {
    nav: Navigation,
    browser: BrowserControls,
    events: Events,
    pointer: PointerSampler,
    origin: Vec2,
}
impl Harness {
    fn new() -> Self {
        let mut browser = BrowserControls::new(
            (0..32)
                .map(|i| RowMetadata {
                    kind: 2,
                    voice_count: 0,
                    top: i as f32 * 100.,
                    height: 100.,
                })
                .collect(),
            Viewport {
                first: 0,
                end: 32,
                minimum: 0.,
                extent: 3200.,
            },
            15,
            0.,
        )
        .unwrap();
        let nav = Navigation::new(
            &(0..32).collect::<Vec<_>>(),
            &(0..32).collect::<Vec<_>>(),
            15,
            &mut browser,
        )
        .unwrap();
        let origin = vec2(
            600.,
            (240. + nav.selected() as f32 * 60. - nav.scroll() + 30.) / 1.5,
        );
        let mut h = Self {
            nav,
            browser,
            events: Events::default(),
            pointer: PointerSampler::default(),
            origin,
        };
        for _ in 0..16 {
            h.sample(h.origin, false, 1. / 60., true, true);
        }
        assert_eq!(h.nav.phase(), 1);
        h.events.0.clear();
        h
    }
    fn sample(&mut self, p: Vec2, held: bool, seconds: f64, permission: bool, desktop: bool) {
        self.pointer.sample(p, held, seconds);
        let frame = self.pointer.frame();
        self.nav.poll_with_mouse_release(
            Ticks::from_f32((seconds * 60.) as f32),
            u32::from(permission),
            0,
            0,
            &frame,
            &mut self.browser,
            &mut self.events,
            if desktop {
                self.pointer.mouse_release()
            } else {
                None
            },
        );
    }
    fn drag(&mut self, hz: usize, displacement: Vec2, desktop: bool) {
        let dt = 1. / hz as f64;
        self.sample(self.origin, true, dt, true, desktop);
        for _ in 0..(hz * 3 / 10) {
            self.sample(self.origin, true, dt, true, desktop);
        }
        for i in 1..=(hz / 5) {
            self.sample(
                self.origin + displacement * (i as f32 / (hz / 5) as f32),
                true,
                dt,
                true,
                desktop,
            );
        }
        // A stationary gap deliberately destroys the native one-frame fling.
        for _ in 0..((hz as f64 * 0.04).ceil() as usize) {
            self.sample(self.origin + displacement, true, dt, true, desktop);
        }
        self.sample(self.origin + displacement, false, dt, true, desktop);
    }
    fn cancels(&self) -> usize {
        self.events
            .0
            .iter()
            .filter(|e| **e == Event::Cue(Cue::Cancel))
            .count()
    }
}

#[test]
fn higurashi_chapter_desktop_slow_right_drag_closes_at_60_and_120_hz() {
    for hz in [60, 120] {
        let mut h = Harness::new();
        h.drag(hz, vec2(128., 0.), true);
        assert_eq!(h.cancels(), 1, "rate {hz}");
        assert!(h.nav.phase() != 1, "closing begins on real mouse Up");
        assert_eq!(
            h.browser.navigation(),
            1,
            "parent clears flag at child retirement"
        );
        assert!(
            !h.events
                .0
                .iter()
                .any(|e| matches!(e, Event::Close | Event::Jump(_) | Event::Voice(_)))
        );
        for _ in 0..16 {
            h.sample(h.origin, false, 1. / hz as f64, true, true);
        }
        assert!(h.nav.complete());
        h.nav.retire(&mut h.browser);
        assert_eq!(h.browser.navigation(), 0);
    }
}

#[test]
fn higurashi_chapter_desktop_preserves_direction_jitter_and_native_release() {
    for hz in [60, 120] {
        for d in [
            vec2(-128., 0.),
            vec2(0., -128.),
            vec2(128., 100.),
            vec2(12., 0.),
        ] {
            let mut h = Harness::new();
            h.drag(hz, d, true);
            assert_eq!(h.cancels(), 0, "rate {hz}, displacement {d:?}");
            assert_eq!(h.nav.phase(), 1);
            if d.x == 0. {
                assert_eq!(
                    h.nav.state.velocity, 0.,
                    "slow stationary vertical release stays below native producer threshold"
                );
            }
        }
        let mut native = Harness::new();
        native.drag(hz, vec2(128., 0.), false);
        assert_eq!(
            native.cancels(),
            0,
            "native no-desktop path retains stationary release behavior"
        );
        assert_eq!(native.nav.phase(), 1);
    }
}

#[test]
fn higurashi_chapter_desktop_denial_and_contact_cancel_do_not_close() {
    for denied in [false, true] {
        let mut h = Harness::new();
        let p = h.origin;
        h.sample(p, true, 0.01, true, true);
        h.sample(p + vec2(128., 0.), true, 0.01, true, true);
        if denied {
            h.sample(p + vec2(128., 0.), true, 0.01, false, true);
        } else {
            h.pointer.cancel_contact(true);
            h.sample(p + vec2(128., 0.), true, 0.01, true, true);
        }
        h.sample(p + vec2(128., 0.), true, 0.01, true, true);
        h.sample(p + vec2(128., 0.), false, 0.04, true, true);
        assert_eq!(h.cancels(), 0);
        assert_eq!(h.nav.phase(), 1);
        // Permission/cancellation recovery requires a genuinely fresh Down.
        h.events.0.clear();
        h.drag(60, vec2(128., 0.), true);
        assert_eq!(h.cancels(), 1);
    }
}

#[test]
fn higurashi_chapter_desktop_close_fallback_preserves_vertical_velocity() {
    let mut vertical = Harness::new();
    let p = vertical.origin;
    vertical.sample(p, true, 1. / 60., true, true);
    for _ in 0..18 {
        vertical.sample(p, true, 1. / 60., true, true);
    }
    for i in 1..=12 {
        vertical.sample(
            p - vec2(0., 128. * i as f32 / 12.),
            true,
            1. / 60.,
            true,
            true,
        );
    }
    // The sampler measures the final80ms; a100ms stationary gap is an exact
    // zero-velocity release, unlike the deliberate40ms close regression.
    for _ in 0..6 {
        vertical.sample(p - vec2(0., 128.), true, 1. / 60., true, true);
    }
    vertical.sample(p - vec2(0., 128.), false, 1. / 60., true, true);
    assert_eq!(vertical.nav.state.velocity, 0.);
    assert_eq!(vertical.nav.phase(), 1);
    assert_eq!(vertical.cancels(), 0);
    for velocity in [
        Vec2::ZERO,
        vec2(0., -5.),
        vec2(0., -20.),
        vec2(0., 20.),
        vec2(0., -25.),
        vec2(0., 25.),
    ] {
        let mut h = Harness::new();
        let mut contact = Contact {
            state: &mut h.nav.state,
            browser: &mut h.browser,
            services: &mut h.events,
        };
        contact.mouse_gesture(vec2(0., -192.), velocity);
        assert_eq!(
            h.nav.state.velocity,
            if velocity.length() > 20. {
                velocity.y
            } else {
                0.
            }
        );
        assert_eq!(h.nav.phase(), 1);
        assert_eq!(h.cancels(), 0);
    }
    for speed in [19.999, 20., 20.001] {
        let mut h = Harness::new();
        let mut contact = Contact {
            state: &mut h.nav.state,
            browser: &mut h.browser,
            services: &mut h.events,
        };
        contact.mouse_gesture(vec2(12., 0.), vec2(speed, 0.));
        assert_eq!(
            h.cancels(),
            usize::from(speed > 20.),
            "short right flick uses strict native producer gate at{speed}"
        );
        assert_eq!(h.nav.phase(), if speed > 20. { 2 } else { 1 });
    }
}
