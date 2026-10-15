use super::*;
use crate::layer::{
    higurashi_properties::HigurashiTweener, user::higurashi::temporary::wrapper_fixture,
};
use std::{
    fmt::Write as _,
    sync::atomic::{AtomicU32, Ordering},
};

const INPUT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../reverse-engineering/higurashi/live-temporary-wrapper-child-capture/input.txt"
));
const OUTPUT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../reverse-engineering/higurashi/live-temporary-wrapper-child-capture/output.txt"
));
fn fresh(counter: &AtomicU32) -> [u32; 4] {
    std::array::from_fn(|_| 0xabc0_0000 | (counter.fetch_add(1, Ordering::SeqCst) + 1))
}
fn tween(out: &mut String, value: &HigurashiTweener) {
    write!(
        out,
        " {} {}",
        value.value().to_bits(),
        u32::from(value.is_waiting())
    )
    .unwrap();
}
fn base(out: &mut String, properties: &HigurashiProperties) {
    out.push_str(" B");
    for value in properties.values() {
        write!(out, " {}", value.to_bits()).unwrap();
    }
    for oscillator in properties.oscillators {
        write!(out, " {} {}", oscillator.seed, oscillator.phase.to_bits()).unwrap();
    }
    for i in 0..65 {
        write!(
            out,
            " {}",
            u32::from(properties.tweener(i).unwrap().is_waiting())
        )
        .unwrap();
    }
}
fn result(value: Option<Completion>) -> i32 {
    match value {
        None => -1,
        Some(Completion::Notification) => -2,
        Some(Completion::Tips(value)) => value,
    }
}
fn compare(actual: &str, expected: &str, operation: usize) {
    let actual = actual.split_whitespace().collect::<Vec<_>>();
    let expected = expected.split_whitespace().collect::<Vec<_>>();
    assert_eq!(
        actual.len(),
        expected.len(),
        "Native wrapper token count at operation {operation}"
    );
    if let Some((index, (actual, expected))) = actual
        .iter()
        .zip(&expected)
        .enumerate()
        .find(|(_, (a, b))| a != b)
    {
        panic!(
            "Native wrapper operation {operation}, word {index}: Rust {actual}, original {expected}"
        );
    }
}

/// Captured actual d8708/98df0/60a34 and c0dbc/c1f20; the original
/// SimpleText endpoint is bounded to checked 48px metrics, not a GPU claim.
#[test]
fn actual_wrappers_and_direct_choice_match_14_original_flows() {
    let mut expected = OUTPUT.lines();
    let mut widget: Option<Owner> = None;
    let mut wrapper: Option<Shared> = None;
    let mut job: Option<Shared> = None;
    let mut weak = None;
    let mut counter = Arc::new(AtomicU32::new(0));
    let mut kind = 0;
    let mut profiles = 0;
    let mut callbacks = 0;
    let mut observations = 0;
    let mut child_observations = 0;
    for (index, line) in INPUT.lines().enumerate() {
        let operation = index + 1;
        let mut args = line.split_whitespace();
        let op = args.next().unwrap().chars().next().unwrap();
        let mut probe = false;
        let before = wrapper
            .as_ref()
            .and_then(|wrapper| wrapper.completion().unwrap());
        match op {
            'N' => {
                assert!(wrapper.is_none() && widget.is_none());
                kind = args.next().unwrap().parse().unwrap();
                let alternate = args.next().unwrap() == "1";
                let append = args.next().unwrap() == "1";
                counter = Arc::new(AtomicU32::new(4)); // Actual widget base precedes the wrapper.
                widget = Some(wrapper_fixture::notification(kind, alternate, append));
                let child_counter = counter.clone();
                let created = Shared::reference(
                    widget.as_ref().unwrap(),
                    fresh(&counter),
                    Box::new(move || fresh(&child_counter)),
                    false,
                );
                assert_eq!(
                    created.widget_identity(),
                    widget.as_ref().unwrap().widget_identity()
                );
                weak = Some(Arc::downgrade(&created.0));
                job = Some(created.clone());
                assert_eq!(created.identity(), job.as_ref().unwrap().identity());
                wrapper = Some(created);
                callbacks = 0;
                profiles += 1;
            }
            'U' => {
                let delta = Ticks::from_f32(args.next().unwrap().parse().unwrap());
                wrapper.as_ref().unwrap().advance_checked(delta).unwrap();
                // Actual wrapper is the earlier child0: scene alone advances the private widget.
                widget.as_mut().unwrap().advance(delta);
            }
            'I' => {
                let permission = args.next().unwrap() == "1";
                let held = args.next().unwrap().parse().unwrap();
                let pressed = args.next().unwrap().parse().unwrap();
                let repeat = args.next().unwrap().parse().unwrap();
                wrapper.as_mut().unwrap().input(
                    permission,
                    Buttons {
                        held,
                        pressed,
                        repeat,
                        rapid_repeat: 0,
                    },
                );
            }
            'P' => {
                let callback: u32 = args.next().unwrap().parse().unwrap();
                let position = Vec2::new(
                    args.next().unwrap().parse().unwrap(),
                    args.next().unwrap().parse().unwrap(),
                );
                let wrapper = wrapper.as_mut().unwrap();
                match callback {
                    0 => probe = wrapper.probe(position),
                    1 => wrapper.press(position),
                    2 => wrapper.release(position),
                    3 => wrapper.cancel(),
                    _ => unreachable!(),
                }
            }
            'H' => wrapper
                .as_mut()
                .unwrap()
                .set_hidden(args.next().unwrap() == "1"),
            'F' => wrapper.as_mut().unwrap().fast_forward(),
            'C' => assert!(wrapper.as_ref().unwrap().native_copy().is_none()),
            'X' => {
                assert_eq!(callbacks, 1, "Exactly one original callback per owner");
                let actual = format!(
                    "E {profiles} {} 0 {callbacks} 0",
                    counter.load(Ordering::SeqCst)
                );
                compare(&actual, expected.next().unwrap(), operation);
                drop(wrapper.take());
                assert!(
                    weak.as_ref().unwrap().upgrade().is_some(),
                    "Job retains actual wrapper callback owner"
                );
                drop(job.take());
                assert!(
                    weak.take().unwrap().upgrade().is_none(),
                    "Last wrapper retain retires callback owner"
                );
                assert_eq!(
                    widget.as_ref().unwrap().result(),
                    None,
                    "Scene widget has no wrapper callback result"
                );
                drop(widget.take());
                continue;
            }
            _ => unreachable!(),
        }
        assert!(args.next().is_none());
        let wrapper = wrapper.as_mut().unwrap();
        let completion = wrapper.completion().unwrap();
        let delta = u32::from(before.is_none() && completion.is_some());
        callbacks += delta;
        assert_eq!(
            job.as_ref().unwrap().completion().unwrap(),
            completion,
            "Job and overlay retain the same callback result"
        );
        assert_eq!(
            widget.as_ref().unwrap().result(),
            None,
            "Private drawable result is not the wrapper's local result"
        );
        let mut actual = format!(
            "Z {operation} {op} {kind} {} 0 {delta} {} {}",
            counter.load(Ordering::SeqCst),
            result(completion),
            u32::from(probe)
        );
        {
            let state = wrapper.0.lock();
            base(&mut actual, &state.properties);
            let words = state.controller.reference_native_words();
            write!(actual, " D {}", words.len()).unwrap();
            for word in words {
                write!(actual, " {word}").unwrap();
            }
            write!(actual, " C {}", u32::from(state.child.is_some())).unwrap();
            if let Some(child) = &state.child {
                child_observations += 1;
                assert_eq!(
                    child.properties().values()[5],
                    1000.,
                    "Direct UI constructor keeps native priority"
                );
                base(&mut actual, child.properties());
                let model = child.model();
                write!(
                    actual,
                    " M {TIPS_FLAGS} {} {} {} {} {}",
                    model.phase,
                    model.selected as u32,
                    model.opacity.to_bits(),
                    u32::from(model.hidden),
                    model.rows.len()
                )
                .unwrap();
                tween(&mut actual, &model.badge_slide);
                tween(&mut actual, &model.badge_alpha);
                for row in &model.rows {
                    assert_eq!(row.spec.label, TIPS_LABELS[row.spec.original]);
                    assert!(!row.spec.seen);
                    write!(
                        actual,
                        " R {} {} {} {} {}",
                        row.id,
                        row.spec.original,
                        row.spec.badges,
                        row.y.to_bits(),
                        row.spec.height.to_bits()
                    )
                    .unwrap();
                    tween(&mut actual, &row.x);
                    tween(&mut actual, &row.alpha);
                    tween(&mut actual, &row.focus);
                }
            }
        }
        let permissions = wrapper.input_permissions();
        write!(
            actual,
            " V {} {}",
            u32::from(permissions.menus),
            u32::from(permissions.hide_window)
        )
        .unwrap();
        let cues = wrapper.take_cues();
        write!(actual, " S {}", cues.len()).unwrap();
        for cue in cues {
            let id = ["select", "move", "decide", "suzu", "seldecide", "error"]
                .iter()
                .position(|candidate| *candidate == cue)
                .unwrap();
            write!(actual, " {id}").unwrap();
        }
        compare(&actual, expected.next().unwrap(), operation);
        observations += 1;
    }
    assert!(expected.next().is_none());
    assert_eq!((profiles, observations), (14, 344));
    assert!(child_observations > 100);
}

#[test]
fn wrapper_retention_is_distinct_from_widget_and_failed_child_is_latched() {
    let mut widget = wrapper_fixture::notification(12, true, true);
    let counter = Arc::new(AtomicU32::new(4));
    let child_counter = counter.clone();
    let wrapper = Shared::reference(
        &widget,
        fresh(&counter),
        Box::new(move || fresh(&child_counter)),
        true,
    );
    let replacement = Shared::reference(
        &widget,
        [21, 22, 23, 24],
        Box::new(|| [25, 26, 27, 28]),
        false,
    );
    assert_ne!(wrapper.identity(), replacement.identity());
    assert_eq!(wrapper.widget_identity(), replacement.widget_identity());
    wrapper.advance_checked(Ticks::from_f32(120.)).unwrap();
    widget.advance(Ticks::from_f32(120.));
    assert!(wrapper.advance_checked(Ticks::ZERO).is_err());
    assert_eq!(counter.load(Ordering::SeqCst), 12);
    assert!(wrapper.fault().is_some() && wrapper.completion().is_err());
    assert!(wrapper.advance_checked(Ticks::ZERO).is_err());
    assert_eq!(
        counter.load(Ordering::SeqCst),
        12,
        "Failed direct child is never reconstructed or resampled"
    );
    assert_eq!(widget.result(), None);
    let mut overlay = wrapper.clone();
    overlay.input(
        true,
        Buttons {
            pressed: 1 << 24,
            ..Default::default()
        },
    );
    assert!(!overlay.probe(Vec2::ZERO));
    let permissions = overlay.input_permissions();
    assert!(!permissions.menus && !permissions.hide_window);
    assert!(wrapper.native_copy().is_none());
}
