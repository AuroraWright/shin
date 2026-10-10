use super::*;
fn frame(m: &mut CgViewerMotion, ready: bool, eligible: bool, delta: f32) {
    m.prepare_frame(ready, eligible);
    m.advance(Ticks::from_f32(delta));
}
fn active() -> CgViewerMotion {
    let mut m = CgViewerMotion::new();
    frame(&mut m, true, true, 30.0);
    frame(&mut m, true, true, 0.0);
    frame(&mut m, true, true, 0.0);
    assert!(m.accepts_input());
    m.take_events();
    m
}
#[test]
fn loading_publication_and_completed_waiter_each_suppress_input() {
    let mut m = CgViewerMotion::new();
    frame(&mut m, false, true, 100.0);
    assert!(!m.accepts_input());
    assert!(m.take_events().is_empty());
    assert_eq!(m.capture_opacity(), 0.0);
    assert!(!m.begin_load());
    assert!(!m.close());
    frame(&mut m, true, true, 15.0);
    assert_eq!(m.take_events(), [ViewerEvent::PublishPicture]);
    assert_eq!(m.capture_opacity(), 0.5);
    assert!(!m.accepts_input());
    frame(&mut m, true, true, 15.0);
    assert!(!m.accepts_input());
    frame(&mut m, true, true, 0.0);
    assert!(!m.accepts_input()); // Completion poll still blocks.
    frame(&mut m, true, true, 0.0);
    assert!(m.accepts_input());
    assert!(!m.is_finished());
}
#[test]
fn next_load_excludes_insertion_poll_and_parent_eligibility_survives_retirement() {
    let mut m = active();
    m.prepare_frame(false, true);
    assert!(m.begin_load());
    m.prepare_frame(true, true);
    assert!(m.take_events().is_empty());
    m.advance(Ticks::ZERO);
    frame(&mut m, true, true, 30.0);
    assert_eq!(m.take_events(), [ViewerEvent::PublishPicture]);
    frame(&mut m, true, true, 0.0);
    assert!(!m.accepts_input());
    frame(&mut m, true, false, 0.0);
    assert!(!m.accepts_input());
    frame(&mut m, true, true, 0.0);
    assert!(m.accepts_input());
}
#[test]
fn signed_capture_can_reappear_after_waiter_completion_and_retirement() {
    let mut m = CgViewerMotion::new();
    frame(&mut m, true, true, -3.0);
    assert_eq!(m.take_events(), [ViewerEvent::PublishPicture]);
    assert!((m.capture_opacity() - 1.1).abs() < 0.00001);
    frame(&mut m, true, true, 33.0);
    assert_eq!(m.capture_opacity(), 0.0);
    frame(&mut m, true, true, -3.0);
    assert!(!m.accepts_input());
    assert!((m.capture_opacity() - 0.1).abs() < 0.00001);
    frame(&mut m, true, true, 0.0);
    assert!(m.accepts_input());
    assert!((m.capture_opacity() - 0.1).abs() < 0.00001);
    frame(&mut m, true, true, -3.0);
    assert!(m.accepts_input());
    assert!((m.capture_opacity() - 0.2).abs() < 0.00001);
}
#[test]
fn close_waiter_completion_requires_following_poll_before_child_finished() {
    let mut m = active();
    m.prepare_frame(false, true);
    assert!(m.close());
    assert!(!m.close());
    assert_eq!(m.take_events(), [ViewerEvent::CaptureClosing]);
    m.advance(Ticks::from_f32(30.0));
    assert!(!m.is_finished());
    assert!(!m.accepts_input());
    frame(&mut m, false, true, 0.0);
    assert!(!m.is_finished());
    frame(&mut m, false, true, 0.0);
    assert!(m.is_finished());
    assert!(!m.accepts_input());
}

#[test]
#[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
fn reference_native_blocking_tasks_retirement_and_signed_capture_clock() {
    let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let exe = std::fs::read(
        std::path::Path::new(&path)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
    for (a, expected) in [
        (0x17ad0, 0x39410408),
        (0x17b20, 0x39010688),
        (0x17b24, 0x39410288),
        (0x17b34, 0x0a130100),
        (0x329bc, 0x39010268),
        (0x329c0, 0x385fc3a8),
        (0x1000f4, 0xd63f0100),
        (0x100310, 0x72001e9f),
        (0x100318, 0xd63f0100),
        (0x10031c, 0x12000014),
        (0x4ae20, 0x52802029),
        (0x4ae28, 0x79008009),
        (0x4dbbc, 0x97ff6ae9),
        (0x4ddac, 0x9400543d),
        (0x4de00, 0x52802029),
        (0x4de10, 0x79008009),
        (0x4e068, 0x140053aa),
        (0x4e138, 0x14005376),
        (0x4b220, 0x94005f48),
        (0x4aad4, 0x3900029f),
        (0x4aae4, 0x1a9f17e0),
        (0x4b180, 0x91020000),
        (0x1000a0, 0xa940a408),
        (0x62f18, 0x2d408400),
        (0x62f1c, 0x1e212000),
        (0x62f20, 0x1a9f57e0),
        (0x62f4c, 0x1e202840),
        (0x62f54, 0xbd000400),
        (0x62f58, 0x5400004d),
        (0x62f5c, 0xbd000401),
    ] {
        assert_eq!(word(a), expected, "native CG task gate at {a:x}");
    }
}
