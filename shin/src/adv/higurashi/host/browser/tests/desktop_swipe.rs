//! Exercise desktop release forwarding through the real retained Host browser,
//! measured history worker, navigation child and async navigation assets.
use super::*;
use crate::adv::input_area::PointerSampler;
use glam::{Vec2, vec2};

fn navigation(host: &mut HigurashiHost) -> Option<(i32, usize, f32)> {
    let mut state = None;
    for mode in host.modes.iter_mut() {
        if let HostMode::Browser(owner) = mode {
            if let Some(life) = owner.browser.lifecycle_mut() {
                life.draw_navigation(|nav| {
                    state = Some((nav.phase(), nav.selected(), nav.scroll()));
                });
            }
        }
    }
    state
}
fn private_source(host: &HigurashiHost) -> String {
    host.modes
        .iter()
        .find_map(|mode| match mode {
            HostMode::Browser(owner) => owner
                .browser
                .lifecycle()
                .map(|life| format!("{:?}", life.private_source())),
            _ => None,
        })
        .expect("live browser source")
}
fn sample(
    host: &mut HigurashiHost,
    sampler: &mut PointerSampler,
    position: Vec2,
    held: bool,
    dt: f64,
) {
    sampler.sample(position, held, dt);
    assert!(
        !host
            .poll_modes_with_mouse_release(
                Ticks::from_f32((dt * 60.) as f32),
                1,
                Buttons::default(),
                &sampler.frame(),
                sampler.mouse_release(),
            )
            .unwrap(),
        "browser keeps lower ADV input denied"
    );
}

pub(super) fn close_and_reopen(host: &mut HigurashiHost) {
    let (phase, selected, scroll) = navigation(host).expect("actual navigation snapshot");
    assert_eq!(phase, 1);
    let source = private_source(host);
    let commands = host.runner.completed_commands();
    let pause = host.modal.pause_depth;
    assert_eq!(pause, 1);
    host.take_browser_notifications();
    let mut sampler = PointerSampler::default();
    // Hold the actual selected marker: the native press must not navigate to
    // a different row and start a selection tween before the drag begins.
    let p = vec2(500., (240. + selected as f32 * 60. - scroll + 30.) / 1.5);
    assert!((162. / 1.5..980. / 1.5).contains(&p.y));
    sample(host, &mut sampler, p, true, 1. / 60.);
    for _ in 0..18 {
        sample(host, &mut sampler, p, true, 1. / 60.);
    }
    for i in 1..=12 {
        sample(
            host,
            &mut sampler,
            p + vec2(128. * i as f32 / 12., 0.),
            true,
            1. / 60.,
        );
    }
    for _ in 0..3 {
        sample(host, &mut sampler, p + vec2(128., 0.), true, 1. / 60.);
    }
    sample(host, &mut sampler, p + vec2(128., 0.), false, 1. / 60.);
    assert!(navigation(host).is_some_and(|(phase, _, _)| phase != 1));
    for _ in 0..20 {
        sample(host, &mut sampler, p, false, 1. / 60.);
    }
    assert!(
        navigation(host).is_none(),
        "navigation is pruned after its closing snapshot"
    );
    assert!(
        host.reference_browser_ready(),
        "B-equivalent swipe leaves browser input ready"
    );
    assert_eq!(host.modal.pause_depth, pause);
    assert!(host.has_modes());
    assert_eq!(host.runner.completed_commands(), commands);
    assert_eq!(private_source(host), source);
    let notices = host.take_browser_notifications();
    assert_eq!(
        notices
            .iter()
            .filter(|n| matches!(
                n.notice,
                Notice::ControlCue(crate::adv::higurashi::system_menu::Cue::Cancel)
            ))
            .count(),
        1
    );
    assert!(!notices.iter().any(|n| matches!(
        n.notice,
        Notice::Reconstruct { .. }
            | Notice::Voice { .. }
            | Notice::ControlCue(crate::adv::higurashi::system_menu::Cue::Confirm)
    )));

    // Reopen using the ordinary controller action. The caller continues its
    // existing Question No/cancel and full-browser close lifecycle assertions.
    poll(host, 1 << 12);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        poll(host, 0);
        if navigation(host).is_some_and(|(phase, _, _)| phase == 1)
            && host.modes.iter().any(|mode| match mode {
                HostMode::Browser(owner) => owner
                    .jobs
                    .iter()
                    .any(|job| matches!(job.id, ChildId::Navigation(_)) && job.task.is_none()),
                _ => false,
            })
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "real navigation child did not reopen"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    for _ in 0..12 {
        poll(host, 0);
    }
    assert_eq!(host.modal.pause_depth, pause);
    assert_eq!(host.runner.completed_commands(), commands);
    assert_eq!(private_source(host), source);
}
