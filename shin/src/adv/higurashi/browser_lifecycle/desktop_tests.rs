//! Exercise parent-to-navigation desktop forwarding through the real task stack.
use super::super::{
    autosave::{ProgressEntry, SaveProgress},
    browser_controls::{RowMetadata, Viewport},
    system_menu::Cue,
};
use super::*;
use crate::adv::input_area::PointerSampler;
use glam::{Vec2, vec2};
use shin_core::format::save::PersistData;

#[derive(Default)]
struct Events {
    events: Vec<Event>,
    returns: Vec<bool>,
    questions: usize,
    reconstructed: usize,
}
impl browser_controls::Services for Events {
    fn event(&mut self, event: Event) {
        self.events.push(event);
    }
    fn voice_is_playing(&self) -> bool {
        false
    }
}
impl Services for Events {
    fn returned(&mut self, complete: bool) {
        self.returns.push(complete);
    }
    fn question(&mut self, _: Question) {
        self.questions += 1;
    }
    fn question_cue(&mut self, _: question::Cue) {}
    fn reconstruct(&mut self, _: &ProgressMemory, _: u32) -> Result<()> {
        self.reconstructed += 1;
        Ok(())
    }
}
fn sample(
    browser: &mut BrowserLifecycle,
    events: &mut Events,
    pointer: &mut PointerSampler,
    p: Vec2,
    held: bool,
    dt: f64,
) {
    pointer.sample(p, held, dt);
    assert_eq!(
        browser
            .poll_with_mouse_release(
                Ticks::from_f32((dt * 60.) as f32),
                1,
                0,
                0,
                &pointer.frame(),
                false,
                events,
                pointer.mouse_release()
            )
            .unwrap(),
        0
    );
}

#[test]
fn higurashi_browser_desktop_left_opens_retained_navigation_and_right_returns_to_browser() {
    let controls = BrowserControls::new(
        (0..16)
            .map(|i| RowMetadata {
                kind: 2,
                voice_count: 0,
                top: i as f32 * 100.,
                height: 100.,
            })
            .collect(),
        Viewport {
            first: 0,
            end: 16,
            minimum: 0.,
            extent: 1600.,
        },
        0,
        0.,
    )
    .unwrap();
    let progress = SaveProgress::new(
        1,
        0x5a17,
        std::array::from_fn(|i| ProgressEntry {
            scenario_id: 2000 + i as i32,
            random_seed: 0x1234 + i as u32,
            save_position: 17 + i as u32,
            replay_target: 29 + i as u32,
            response_data: vec![1, 255, 17 + i as u8],
        }),
    )
    .unwrap();
    let source = ProgressMemory::new(PersistData::default(), progress, vec![3, 5, 0, 99])
        .with_trailing_values(vec![0, 256, 65535]);
    let source_before = format!("{source:?}");
    let mut browser =
        BrowserLifecycle::new(controls, (0..16).map(|i| 100 + i * 7).collect(), source).unwrap();
    browser
        .configure_navigation(
            NavigationHistory::new((0..16).collect(), (0..16).collect(), 0).unwrap(),
        )
        .unwrap();
    browser.publish_readiness(2, false);
    let mut pointer = PointerSampler::default();
    let mut events = Events::default();
    let p = vec2(700., 200. / 1.5);
    for _ in 0..20 {
        sample(&mut browser, &mut events, &mut pointer, p, false, 1. / 60.);
    }
    assert!(browser.input_ready());
    events.events.clear();
    sample(&mut browser, &mut events, &mut pointer, p, true, 1. / 60.);
    for _ in 0..18 {
        sample(&mut browser, &mut events, &mut pointer, p, true, 1. / 60.);
    }
    for i in 1..=12 {
        sample(
            &mut browser,
            &mut events,
            &mut pointer,
            p + vec2(-128. * i as f32 / 12., 0.),
            true,
            1. / 60.,
        );
    }
    for _ in 0..3 {
        sample(
            &mut browser,
            &mut events,
            &mut pointer,
            p + vec2(-128., 0.),
            true,
            1. / 60.,
        );
    }
    sample(
        &mut browser,
        &mut events,
        &mut pointer,
        p + vec2(-128., 0.),
        false,
        1. / 60.,
    );
    assert!(!browser.input_ready());
    assert_eq!(browser.controls.navigation(), 1);
    assert_eq!(
        events
            .events
            .iter()
            .filter(|e| **e == Event::Cue(Cue::Confirm))
            .count(),
        1
    );
    for _ in 0..16 {
        sample(&mut browser, &mut events, &mut pointer, p, false, 1. / 60.);
    }
    let mut phase = None;
    browser.draw_navigation(|nav| phase = Some(nav.phase()));
    assert_eq!(phase, Some(1));
    // Top content band is inside the list but outside its rows, so holding it
    // cannot select another marker and start a scroll tween before release.
    sample(&mut browser, &mut events, &mut pointer, p, true, 1. / 60.);
    for _ in 0..18 {
        sample(&mut browser, &mut events, &mut pointer, p, true, 1. / 60.);
    }
    for i in 1..=12 {
        sample(
            &mut browser,
            &mut events,
            &mut pointer,
            p + vec2(128. * i as f32 / 12., 0.),
            true,
            1. / 60.,
        );
    }
    for _ in 0..3 {
        sample(
            &mut browser,
            &mut events,
            &mut pointer,
            p + vec2(128., 0.),
            true,
            1. / 60.,
        );
    }
    sample(
        &mut browser,
        &mut events,
        &mut pointer,
        p + vec2(128., 0.),
        false,
        1. / 60.,
    );
    for _ in 0..20 {
        sample(&mut browser, &mut events, &mut pointer, p, false, 1. / 60.);
    }
    assert!(browser.input_ready());
    assert_eq!(browser.controls.navigation(), 0);
    assert_eq!(browser.completion_word(), 0);
    assert_eq!(browser.slide(), 1.);
    assert!(!browser.jumped());
    assert_eq!(format!("{:?}", browser.private_source()), source_before);
    assert_eq!(
        events
            .events
            .iter()
            .filter(|e| **e == Event::Cue(Cue::Cancel))
            .count(),
        1
    );
    assert!(events.returns.is_empty());
    assert_eq!(events.questions, 0);
    assert_eq!(events.reconstructed, 0);
    assert!(!events.events.iter().any(|e| matches!(
        e,
        Event::Close | Event::Jump(_) | Event::Voice(_) | Event::StopVoice
    )));
}
