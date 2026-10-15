//! Desktop release policy; original touch comparisons continue to use input().
use super::*;
use crate::adv::input_area::PointerSampler;
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
    browser: BrowserControls,
    events: Events,
    pointer: PointerSampler,
    origin: Vec2,
}
impl Harness {
    fn new() -> Self {
        let browser = BrowserControls::new(
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
            0,
            0.,
        )
        .unwrap();
        Self {
            browser,
            events: Events::default(),
            pointer: PointerSampler::default(),
            origin: vec2(700., 200. / 1.5),
        }
    }
    fn sample(&mut self, position: Vec2, held: bool, dt: f64, permission: u8, desktop: bool) {
        self.pointer.sample(position, held, dt);
        self.browser.input_with_mouse_release(
            permission,
            0,
            0,
            &self.pointer.frame(),
            &mut self.events,
            if desktop {
                self.pointer.mouse_release()
            } else {
                None
            },
        );
        self.browser
            .tick(Ticks::from_f32((dt * 60.) as f32), false, &mut self.events);
    }
    fn drag(&mut self, hz: usize, displacement: Vec2, permission: u8, desktop: bool) {
        let dt = 1. / hz as f64;
        self.sample(self.origin, true, dt, permission, desktop);
        for _ in 0..hz * 3 / 10 {
            self.sample(self.origin, true, dt, permission, desktop);
        }
        for i in 1..=hz / 5 {
            self.sample(
                self.origin + displacement * (i as f32 / (hz / 5) as f32),
                true,
                dt,
                permission,
                desktop,
            );
        }
        for _ in 0..(hz as f64 * 0.04).ceil() as usize {
            self.sample(self.origin + displacement, true, dt, permission, desktop);
        }
        self.sample(self.origin + displacement, false, dt, permission, desktop);
    }
    fn navigations(&self) -> usize {
        self.events
            .0
            .iter()
            .filter(|e| **e == Event::Navigation)
            .count()
    }
}

#[test]
fn higurashi_browser_desktop_slow_left_opens_chapters_at_60_and_120_hz() {
    for hz in [60, 120] {
        let mut h = Harness::new();
        h.drag(hz, vec2(-128., 0.), 1, true);
        assert_eq!(h.navigations(), 1, "rate {hz}");
        assert_eq!(h.browser.navigation(), 1);
        assert_eq!(
            h.events
                .0
                .iter()
                .filter(|e| **e == Event::Cue(Cue::Confirm))
                .count(),
            1
        );
        assert!(!h.events.0.iter().any(|e| matches!(
            e,
            Event::Close | Event::Jump(_) | Event::Voice(_) | Event::StopVoice
        )));
        let mut native = Harness::new();
        native.drag(hz, vec2(-128., 0.), 1, false);
        assert_eq!(
            native.navigations(),
            0,
            "None keeps stationary native release unchanged"
        );
    }
}

#[test]
fn higurashi_browser_desktop_preserves_other_directions_and_permission() {
    for hz in [60, 120] {
        for displacement in [
            vec2(128., 0.),
            vec2(0., -128.),
            vec2(-128., 100.),
            vec2(-12., 0.),
        ] {
            let mut h = Harness::new();
            h.drag(hz, displacement, 1, true);
            assert_eq!(h.navigations(), 0, "rate {hz}, {displacement:?}");
            if displacement.x == 0. {
                assert_eq!(h.browser.state.velocity, 0.);
            }
        }
        let mut denied = Harness::new();
        denied.drag(hz, vec2(-128., 0.), 0, true);
        assert_eq!(denied.navigations(), 0);
        assert_eq!(denied.browser.navigation(), 0);
    }
}

#[test]
fn higurashi_browser_desktop_cancelled_contact_cannot_open_chapters() {
    let mut h = Harness::new();
    h.sample(h.origin, true, 0.3, 1, true);
    h.sample(h.origin + vec2(-128., 0.), true, 0.2, 1, true);
    h.browser.cancel_input(&mut h.events);
    h.pointer.cancel_contact(true);
    h.sample(h.origin + vec2(-128., 0.), true, 0.04, 1, true);
    h.sample(h.origin + vec2(-128., 0.), false, 0.04, 1, true);
    assert_eq!(h.navigations(), 0);
}

#[test]
fn higurashi_browser_desktop_left_fallback_preserves_native_vertical_threshold() {
    for velocity in [-25., -20., -5., 0., 5., 20., 25.] {
        let mut h = Harness::new();
        Contact {
            state: &mut h.browser.state,
            services: &mut h.events,
        }
        .mouse_gesture(vec2(0., 300.), vec2(0., velocity));
        assert_eq!(
            h.browser.state.velocity,
            if velocity.abs() > 20. { velocity } else { 0. }
        );
        assert_eq!(h.navigations(), 0);
    }
    for speed in [19.999, 20., 20.001] {
        let mut h = Harness::new();
        Contact {
            state: &mut h.browser.state,
            services: &mut h.events,
        }
        .mouse_gesture(vec2(-10., 0.), vec2(-speed, 0.));
        assert_eq!(h.navigations(), usize::from(speed > 20.));
    }
}
