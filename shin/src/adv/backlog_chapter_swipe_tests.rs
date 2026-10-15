//! Desktop chapter-list regression using the real sampled mouse release.
use super::*;
use crate::adv::{
    backlog::{Backlog, BacklogEntry, BacklogItem},
    input_area::PointerSampler,
};
fn model() -> SceneJump {
    let history = Backlog {
        entries: (0..80)
            .map(|i| BacklogEntry {
                save_position: i + 1,
                item: BacklogItem::Checkpoint {
                    title: String::new(),
                    text: "章".into(),
                },
            })
            .collect(),
    };
    SceneJump::new(&history, 0, 0).unwrap()
}
struct Mouse {
    pointer: Pointer,
    sampler: PointerSampler,
    model: SceneJump,
    extent: Vec2,
    desktop: bool,
    events: Vec<Event>,
    closed: bool,
}
impl Mouse {
    fn new(size: (u16, u16), desktop: bool) -> Self {
        Self {
            pointer: Default::default(),
            sampler: Default::default(),
            model: model(),
            extent: Vec2::new(size.0 as f32, size.1 as f32),
            desktop,
            events: Vec::new(),
            closed: false,
        }
    }
    fn step(&mut self, position: Vec2, held: bool, eligible: bool, dt: f64) {
        self.sampler
            .sample(position * self.extent / Vec2::new(1920., 1080.), held, dt);
        let release = self
            .desktop
            .then(|| self.sampler.mouse_release())
            .flatten()
            .map(|r| r.scaled(Vec2::new(1920., 1080.) / self.extent));
        let events = self.pointer.update_with_mouse_release(
            &mut self.model,
            position,
            held,
            eligible,
            dt,
            release,
        );
        for e in &events {
            if let Event::Fling(v) = e {
                self.closed |= self.model.release(*v);
            }
        }
        self.events.extend(events);
    }
    fn drag(&mut self, hz: u32, displacement: Vec2) {
        let origin = Vec2::new(600., 500.);
        let dt = 1. / hz as f64;
        self.step(origin, false, true, dt);
        self.step(origin, true, true, dt);
        for _ in 0..hz * 3 / 10 {
            self.step(origin, true, true, dt);
        }
        for i in 1..=hz / 5 {
            self.step(
                origin + displacement * (i as f32 / (hz / 5) as f32),
                true,
                true,
                dt,
            );
            assert!(!self.closed, "a swipe closes on release, not held motion");
        }
        for _ in 0..(0.04 * hz as f64).ceil() as u32 {
            self.step(origin + displacement, true, true, dt);
        }
        self.step(origin + displacement, false, true, dt);
    }
}
#[test]
fn umineko_chapter_desktop_right_swipe_reuses_story_policy_at_both_rates_and_sizes() {
    for size in [(1280, 720), (1920, 1080), (3840, 2160)] {
        for hz in [60, 120] {
            for desktop in [false, true] {
                let mut m = Mouse::new(size, desktop);
                m.drag(hz, Vec2::X * 128.);
                assert_eq!(
                    m.closed, desktop,
                    "{size:?}/{hz}Hz desktop={desktop}; legacy native dispatch is retained"
                );
                assert!(
                    !m.events.iter().any(|e| matches!(e, Event::CloseClick)),
                    "drag must not also click close"
                );
            }
        }
    }
}
#[test]
fn umineko_chapter_desktop_swipe_preserves_scroll_taps_and_contact_gates() {
    for delta in [
        -Vec2::X * 128.,
        Vec2::Y * 128.,
        -Vec2::Y * 128.,
        Vec2::X * 8.,
    ] {
        let mut m = Mouse::new((1280, 720), true);
        m.drag(120, delta);
        assert!(!m.closed, "{delta:?} must not close chapter list");
        if delta.y != 0. {
            assert!(
                !m.events.iter().any(|e| matches!(e, Event::Fling(_))),
                "slow vertical release must not convert displacement into momentum"
            );
        }
    }
    let p = Vec2::new(600., 500.);
    let mut cancelled = Mouse::new((1920, 1080), true);
    cancelled.step(p, true, true, 0.05);
    cancelled.step(p + Vec2::X * 128., true, false, 0.2);
    cancelled.step(p + Vec2::X * 128., false, true, 0.04);
    assert!(
        !cancelled.closed,
        "denied contact cannot close on later release"
    );
    let mut opening = Mouse::new((1920, 1080), true);
    opening.step(p, true, false, 0.3);
    opening.step(p + Vec2::X * 128., true, true, 0.2);
    opening.step(p + Vec2::X * 128., false, true, 0.04);
    assert!(
        !opening.closed,
        "inherited hold cannot start a newly opened list gesture"
    );
}
#[test]
fn umineko_chapter_desktop_coalesced_release_cancels_click_and_closes_only_content() {
    for position in [Vec2::new(600., 500.), Vec2::new(1800., 70.)] {
        let mut m = Mouse::new((1280, 720), true);
        m.step(position, true, true, 0.05);
        for _ in 0..6 {
            m.step(position, true, true, 0.05);
        }
        m.step(position + Vec2::X * 128., false, true, 0.04);
        assert_eq!(m.closed, position.y == 500.);
        assert!(
            !m.events.iter().any(|e| matches!(e, Event::CloseClick)),
            "release-only drag suppresses captured close click"
        );
        if position.y == 70. {
            assert!(m.events.iter().any(|e| matches!(e, Event::CancelClose)));
        }
    }
}
