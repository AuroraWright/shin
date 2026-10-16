#[test]
fn tap_input_matches_ripple_contact_rules_and_cancels_inherited_holds() {
    for width in [960u16, 1920, 3840] {
        let extent = (width, width * 9 / 16);
        let scale = width as f32 / 1920.0;
        let start = Vec2::new(800.0, 500.0) * scale;
        let mut sampler = PointerSampler::default();
        let mut tap = TapInput::default();
        let sample = |tap: &mut TapInput, sampler: &mut PointerSampler, p, held, seconds| {
            sampler.sample(p, held, seconds);
            tap.update(sampler, extent)
        };
        assert!(!sample(&mut tap, &mut sampler, start, true, 0.016));
        assert!(
            !sample(&mut tap, &mut sampler, start, true, 1.5),
            "Holding never taps, including long hold"
        );
        assert!(sample(&mut tap, &mut sampler, start, false, 0.016));
        assert!(
            !tap.update(&sampler, extent),
            "Same release frame cannot tap twice"
        );
        assert!(!sample(&mut tap, &mut sampler, start, true, 0.016));
        assert!(!sample(
            &mut tap,
            &mut sampler,
            start + Vec2::new(20.0, 0.0) * scale,
            true,
            0.016
        ));
        assert!(!sample(&mut tap, &mut sampler, start, true, 0.016));
        assert!(
            !sample(&mut tap, &mut sampler, start, false, 0.016),
            "Returning a dragged pointer cannot tap"
        );
        assert!(!sample(&mut tap, &mut sampler, start, true, 0.016));
        sampler.cancel_contact(true);
        assert!(
            !sample(&mut tap, &mut sampler, start, false, 0.016),
            "Focus cancel never taps"
        );
        assert!(!sample(&mut tap, &mut sampler, start, true, 0.016));
        tap.cancel();
        assert!(
            !sample(&mut tap, &mut sampler, start, false, 0.016),
            "Inherited held release never taps"
        );
        assert!(!sample(
            &mut tap,
            &mut sampler,
            Vec2::new(-32.0, 100.0),
            true,
            0.016
        ));
        assert!(
            !sample(&mut tap, &mut sampler, start, false, 0.016),
            "Out-of-canvas press never taps"
        );
    }
}

#[test]
fn desktop_notice_cancels_capture_without_releasing_or_restarting_held_contact() {
    use super::*;
    let mut sampler = PointerSampler::default();
    sampler.sample(Vec2::new(100.0, 100.0), true, 0.0);
    assert!(matches!(sampler.frame().events[0].kind, PointerKind::Down));
    sampler.cancel_contact(true);
    sampler.sample(Vec2::new(200.0, 100.0), true, 0.1);
    assert_eq!(sampler.frame().events.len(), 1);
    assert!(matches!(
        sampler.frame().events[0].kind,
        PointerKind::Cancel
    ));
    sampler.sample(Vec2::ZERO, true, 0.1);
    assert!(sampler.frame().events.is_empty());
    sampler.sample(Vec2::ZERO, false, 0.1);
    assert!(sampler.frame().events.is_empty());
    sampler.sample(Vec2::ZERO, true, 0.1);
    assert!(matches!(sampler.frame().events[0].kind, PointerKind::Down));
}

use super::*;
use glam::ivec2;
#[derive(Debug, PartialEq)]
enum Call {
    Probe(Vec2),
    Press(Vec2),
    Release(Vec2),
    Long(Vec2),
    Drag(Vec2, Vec2),
    Gesture(Vec2),
    Cancel,
    Reset,
}
#[derive(Default)]
struct Recorder {
    reject: bool,
    calls: Vec<Call>,
}
impl InputAreaTarget for Recorder {
    fn probe(&mut self, p: Vec2) -> bool {
        self.calls.push(Call::Probe(p));
        !self.reject
    }
    fn reset(&mut self) {
        self.calls.push(Call::Reset);
    }
    fn press(&mut self, p: Vec2) {
        self.calls.push(Call::Press(p));
    }
    fn release(&mut self, p: Vec2) {
        self.calls.push(Call::Release(p));
    }
    fn long_press(&mut self, p: Vec2) {
        self.calls.push(Call::Long(p));
    }
    fn drag(&mut self, p: Vec2, q: Vec2) {
        self.calls.push(Call::Drag(p, q));
    }
    fn gesture(&mut self, p: Vec2) {
        self.calls.push(Call::Gesture(p));
    }
    fn cancel(&mut self) {
        self.calls.push(Call::Cancel);
    }
}
fn event(kind: PointerKind, id: i32, x: i32, y: i32) -> PointerEvent {
    PointerEvent {
        kind,
        id,
        position: ivec2(x, y),
    }
}
fn step(
    area: &mut InputArea,
    target: &mut impl InputAreaTarget,
    number: u32,
    time_seconds: f64,
    events: &[PointerEvent],
) -> bool {
    area.update(
        target,
        &PointerFrame {
            number,
            time_seconds,
            events,
        },
    )
}
#[test]
fn delayed_press_frame_cache_and_release_keep_first_capture() {
    let mut a = InputArea::new(960, 540);
    let mut r = Recorder::default();
    assert!(step(
        &mut a,
        &mut r,
        0,
        0.0,
        &[event(PointerKind::Down, 4, 100, 50)]
    ));
    assert_eq!(r.calls, [Call::Probe(Vec2::new(200.0, 100.0))]);
    // Same frame ignores new events/time and never invokes callbacks twice.
    assert!(step(
        &mut a,
        &mut r,
        0,
        0.5,
        &[event(PointerKind::Cancel, 4, 0, 0)]
    ));
    assert!(step(&mut a, &mut r, 1, 0.149, &[]));
    assert_eq!(r.calls.len(), 1);
    assert!(step(
        &mut a,
        &mut r,
        2,
        0.15,
        &[event(PointerKind::Move, 4, 108, 50)]
    ));
    assert_eq!(r.calls[1], Call::Press(Vec2::new(216.0, 100.0)));
    assert!(step(
        &mut a,
        &mut r,
        3,
        0.2,
        &[event(PointerKind::Up, 4, 900, 400)]
    ));
    // Up coordinates and sub-threshold movement do not change release baseline.
    assert_eq!(
        r.calls[2..],
        [Call::Release(Vec2::new(200.0, 100.0)), Call::Reset]
    );
    assert!(!step(&mut a, &mut r, 4, 0.3, &[]));
}
#[test]
fn quick_tap_releases_without_press_and_rejected_or_unknown_contacts_pass_through() {
    let mut a = InputArea::canvas();
    let mut r = Recorder::default();
    assert!(step(
        &mut a,
        &mut r,
        1,
        0.0,
        &[
            event(PointerKind::Down, 1, 20, 30),
            event(PointerKind::Up, 1, 90, 90)
        ]
    ));
    assert_eq!(
        r.calls,
        [
            Call::Probe(Vec2::new(20.0, 30.0)),
            Call::Release(Vec2::new(20.0, 30.0)),
            Call::Reset
        ]
    );
    r.reject = true;
    assert!(!step(
        &mut a,
        &mut r,
        2,
        0.1,
        &[event(PointerKind::Down, 1, 20, 30)]
    ));
    assert!(!step(
        &mut a,
        &mut r,
        3,
        0.2,
        &[
            event(PointerKind::Move, 1, 80, 80),
            event(PointerKind::Up, 1, 80, 80),
            event(PointerKind::Cancel, 99, 0, 0)
        ]
    ));
    assert_eq!(r.calls.len(), 4);
}
#[test]
fn drag_threshold_endpoints_velocity_and_stationary_release() {
    for stationary in [false, true] {
        let mut a = InputArea::new(960, 540);
        let mut r = Recorder::default();
        step(
            &mut a,
            &mut r,
            0,
            0.0,
            &[event(PointerKind::Down, 1, 500, 200)],
        );
        step(
            &mut a,
            &mut r,
            1,
            0.01,
            &[event(PointerKind::Move, 1, 492, 200)],
        );
        assert_eq!(r.calls.len(), 1); // Exactly8px still a tap.
        step(
            &mut a,
            &mut r,
            2,
            0.02,
            &[event(PointerKind::Move, 1, 491, 200)],
        );
        assert_eq!(
            r.calls[1],
            Call::Drag(Vec2::new(1000.0, 400.0), Vec2::new(982.0, 400.0))
        );
        step(
            &mut a,
            &mut r,
            3,
            0.03,
            &[event(PointerKind::Move, 1, 441, 200)],
        );
        assert_eq!(
            r.calls[2],
            Call::Drag(Vec2::new(982.0, 400.0), Vec2::new(882.0, 400.0))
        );
        if stationary {
            step(&mut a, &mut r, 4, 0.04, &[]);
        }
        step(&mut a, &mut r, 5, 0.05, &[event(PointerKind::Up, 1, 0, 0)]);
        if stationary {
            assert_eq!(r.calls[3..], [Call::Reset]);
        } else {
            let Call::Gesture(v) = r.calls[3] else {
                panic!("missing gesture: {:?}", r.calls);
            };
            assert!((v.x + 83.416666).abs() < 0.00002 && v.y == 0.0);
            assert_eq!(r.calls[4], Call::Reset);
        }
    }
}
#[test]
fn extra_contacts_disable_tap_and_reset_centroid_before_motion() {
    let mut a = InputArea::canvas();
    let mut r = Recorder::default();
    step(
        &mut a,
        &mut r,
        0,
        0.0,
        &[
            event(PointerKind::Down, 1, 100, 200),
            event(PointerKind::Down, 2, 300, 200),
        ],
    );
    assert_eq!(r.calls, [Call::Probe(Vec2::new(100.0, 200.0))]);
    step(
        &mut a,
        &mut r,
        1,
        0.2,
        &[event(PointerKind::Move, 1, 102, 200)],
    );
    assert_eq!(r.calls.len(), 1); // Centroid moves exactly1px; no drag.
    step(
        &mut a,
        &mut r,
        2,
        0.3,
        &[event(PointerKind::Move, 2, 304, 200)],
    );
    assert_eq!(
        r.calls[1],
        Call::Drag(Vec2::new(200.0, 200.0), Vec2::new(203.0, 200.0))
    );
    step(
        &mut a,
        &mut r,
        3,
        0.4,
        &[event(PointerKind::Cancel, 1, 0, 0)],
    );
    assert_eq!(r.calls.len(), 2); // Removing one contact is not owner cancellation.
    step(&mut a, &mut r, 4, 0.5, &[event(PointerKind::Up, 2, 0, 0)]);
    assert_eq!(r.calls[2..], [Call::Reset]); // Structural change cleared swipe delta.
}
#[test]
fn cancel_and_disable_order_and_optional_long_press() {
    let mut a = InputArea::canvas();
    let mut r = Recorder::default();
    a.set_long_press_enabled(true);
    step(
        &mut a,
        &mut r,
        0,
        0.0,
        &[event(PointerKind::Down, 1, 100, 200)],
    );
    step(&mut a, &mut r, 1, 1.149, &[]);
    assert_eq!(r.calls.len(), 2);
    step(&mut a, &mut r, 2, 1.15, &[]);
    assert_eq!(r.calls[2], Call::Long(Vec2::new(100.0, 200.0)));
    step(&mut a, &mut r, 3, 1.2, &[event(PointerKind::Up, 1, 0, 0)]);
    assert_eq!(r.calls[3..], [Call::Reset]); // Long press suppresses tap release.
    step(
        &mut a,
        &mut r,
        4,
        1.3,
        &[event(PointerKind::Down, 2, 100, 200)],
    );
    step(
        &mut a,
        &mut r,
        5,
        1.4,
        &[event(PointerKind::Cancel, 2, 0, 0)],
    );
    assert_eq!(r.calls[5..], [Call::Cancel, Call::Reset]);
    step(
        &mut a,
        &mut r,
        6,
        1.5,
        &[event(PointerKind::Down, 3, 100, 200)],
    );
    assert!(a.disable(&mut r));
    assert!(!a.disable(&mut r));
    assert_eq!(r.calls[8..], [Call::Cancel, Call::Reset]);
}
fn controls() -> EnumMap<AppAction, ActionState> {
    EnumMap::default()
}
#[test]
fn manual_dispatch_blocks_cancel_during_capture_and_swipes_silently() {
    let mut m = ManualModel::new(24).unwrap();
    m.update(10.0);
    let mut a = InputArea::canvas();
    let mut input = controls();
    input[AppAction::Cancel].is_clicked = true;
    a.input_frame(
        &mut m,
        &PointerFrame {
            number: 0,
            time_seconds: 0.0,
            events: &[event(PointerKind::Down, 1, 1000, 500)],
        },
        &input,
    );
    assert_eq!(m.phase(), ManualPhase::Active);
    a.input_frame(
        &mut m,
        &PointerFrame {
            number: 1,
            time_seconds: 0.1,
            events: &[event(PointerKind::Move, 1, 200, 500)],
        },
        &input,
    );
    assert_eq!(m.scroll(), 800.0);
    a.input_frame(
        &mut m,
        &PointerFrame {
            number: 2,
            time_seconds: 0.11668333333333334,
            events: &[event(PointerKind::Up, 1, 1800, 80)],
        },
        &input,
    );
    assert_eq!(m.selected(), 1);
    assert_eq!(m.phase(), ManualPhase::Active);
    assert!(m.take_sounds().is_empty());
    a.input_frame(
        &mut m,
        &PointerFrame {
            number: 3,
            time_seconds: 0.2,
            events: &[],
        },
        &input,
    );
    assert_eq!(m.phase(), ManualPhase::Active); // Controller still gated by page tween.
    m.update(20.0);
    a.input_frame(
        &mut m,
        &PointerFrame {
            number: 4,
            time_seconds: 0.3,
            events: &[],
        },
        &input,
    );
    assert_eq!(m.phase(), ManualPhase::CloseFlash);
    assert_eq!(m.take_sounds(), ["cancel"]);
}
#[test]
fn menu_dispatch_selects_original_capture_and_drag_cancels_confirmation() {
    for drag in [false, true] {
        let mut m = SystemMenuModel::new(0, true);
        m.update(22.0);
        m.update(0.0);
        let mut a = InputArea::canvas();
        let mut input = controls();
        input[AppAction::Cancel].is_clicked = true;
        a.input_frame(
            &mut m,
            &PointerFrame {
                number: 0,
                time_seconds: 0.0,
                events: &[event(PointerKind::Down, 1, 30, 72 + 5 * 128)],
            },
            &input,
        );
        a.input_frame(
            &mut m,
            &PointerFrame {
                number: 1,
                time_seconds: 0.15,
                events: &[],
            },
            &input,
        );
        assert_eq!(m.selected(), 5);
        if drag {
            a.input_frame(
                &mut m,
                &PointerFrame {
                    number: 2,
                    time_seconds: 0.2,
                    events: &[event(PointerKind::Move, 1, 200, 72 + 5 * 128)],
                },
                &input,
            );
        }
        a.input_frame(
            &mut m,
            &PointerFrame {
                number: 3,
                time_seconds: 0.4,
                events: &[event(PointerKind::Up, 1, 30, 80)],
            },
            &input,
        );
        m.update(7.0);
        m.update(0.0);
        assert_eq!(m.take_item(), if drag { None } else { Some(5) });
        assert_eq!(m.take_sounds(), if drag { vec![] } else { vec!["decide"] });
    }
}

#[test]
fn desktop_sampler_keeps_physical_contacts_and_monotonic_time_before_modal_open() {
    let mut sampler = PointerSampler::default();
    sampler.sample(Vec2::new(20.9, 30.7), true, 0.1);
    assert_eq!(sampler.frame().events[0].kind, PointerKind::Down);
    assert_eq!(sampler.frame().events[0].position, IVec2::new(20, 30));
    // Opening a fresh area under an already held pointer cannot capture it.
    sampler.sample(Vec2::new(21.0, 30.0), true, 0.2);
    let mut area = InputArea::new(960, 540);
    let mut r = Recorder::default();
    assert!(!area.update(&mut r, &sampler.frame()));
    assert_eq!(sampler.frame().events[0].kind, PointerKind::Move);
    sampler.sample(Vec2::new(21.0, 30.0), false, 0.1);
    assert!(!area.update(&mut r, &sampler.frame()));
    sampler.sample(Vec2::new(21.0, 30.0), true, 0.1);
    assert!(area.update(&mut r, &sampler.frame()));
    assert_eq!(r.calls, [Call::Probe(Vec2::new(42.0, 60.0))]);
    sampler.sample(Vec2::new(21.0, 30.0), true, 0.1);
    assert!(sampler.frame().events.is_empty());
    assert!((sampler.frame().time_seconds - 0.6).abs() < 1e-12);
}
#[test]
#[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
fn reference_native_input_area_contacts_timing_centroid_swipe_and_priority() {
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let exe = std::fs::read(
        std::path::Path::new(&rom)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    for (address, expected) in [
        (0x6b27c, 0x51000508),
        (0x6b280, 0x71000d1f),
        (0x6b298, 0x10000089),
        (0x6b29c, 0x3868696a),
        (0x6b2a0, 0x8b0a0929),
        (0x6b2b4, 0x3900e27f),
        (0x6b3b4, 0x2900d11c),
        (0x6b3f8, 0x6d042669),
        (0x6b3fc, 0x79007268),
        (0x6b404, 0x2d0b0a61),
        (0x6b408, 0x2d0a0a61),
        (0x6b418, 0xf9400908),
        (0x6b41c, 0x1e210800),
        (0x6b420, 0x1e220861),
        (0x6b43c, 0x36000ee0),
        (0x6b580, 0x3940e268),
        (0x6b590, 0xf9401508),
        (0x6b594, 0x2d4b0660),
        (0x6b59c, 0x1e220800),
        (0x6b5a0, 0x1e230821),
        (0x6b1a0, 0xf9402508),
        (0x6b1b4, 0xf9400d08),
        (0x6b1d4, 0x3940ea68),
        (0x6b1e0, 0x1e603920),
        (0x6b1e8, 0x54001e2d),
        (0x6b1ec, 0x1e601940),
        (0x6b1f4, 0x1e22c021),
        (0x6b1fc, 0x1e62402c),
        (0x6b230, 0x12003c08),
        (0x6b234, 0x0b202908),
        (0x6b238, 0x531f7908),
        (0x6b248, 0x54001b2d),
        (0x6b254, 0xf9402108),
        (0x6b25c, 0x1e2c8800),
        (0x6b260, 0x1e2d8821),
        (0x6b6c4, 0x1e22182d),
        (0x6b6c8, 0x1e22180c),
        (0x6b6d0, 0x2d0b326d),
        (0x6b6d4, 0xf900327f),
        (0x6b6dc, 0xfd002669),
        (0x6b6f0, 0x1e2c382a),
        (0x6b6f4, 0x1e2d380b),
        (0x6b734, 0x531d7108),
        (0x6b744, 0x1e211800),
        (0x6b74c, 0x540001ad),
        (0x6b750, 0x3900e27f),
        (0x6b778, 0x5400056c),
        (0x6b790, 0xfd464901),
        (0x6b79c, 0x5400016b),
        (0x6b7a8, 0xf9401108),
        (0x6b7c4, 0x3900e668),
        (0x6b7cc, 0x34000488),
        (0x6b7d8, 0xfd46a501),
        (0x6b7e4, 0x540003cb),
        (0x6b7f0, 0xf9401908),
        (0x6b808, 0x3900e27f),
        (0x6b82c, 0xf9401d08),
        (0x6b830, 0x2d4b0660),
        (0x6b840, 0x1e2209a2),
        (0x6b844, 0x1e230983),
        (0x6b854, 0x2d0b326d),
        (0x6b858, 0x3900ea68),
        (0x6b85c, 0x2d0c2a6b),
        (0x6b864, 0xfd002669),
        (0x6b924, 0x350000c8),
        (0x6b92c, 0x6b14011f),
        (0x6b930, 0x54000061),
        (0x6b934, 0x39404668),
        (0x6b948, 0x39004668),
        (0x6b950, 0xb9001674),
        (0x1d2d8, 0x9401398a),
        (0x1d2dc, 0x3707ede0),
        (0x86698, 0x97ff949a),
        (0x8669c, 0x37000220),
        (0x866a8, 0x9400d4d2),
        (0x866ac, 0x370001a0),
        (0x866dc, 0x97ff9271),
        (0x86b94, 0x1e293901),
        (0x86b98, 0x1e213800),
    ] {
        assert_eq!(
            u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
            expected,
            "instruction at{address:x}"
        );
    }
    assert_eq!(&exe[0x16fb04 + 256..0x16fb08 + 256], &[0, 55, 17, 36]);
    for (address, expected) in [
        (0x164c90, 0.15_f64),
        (0x164d48, 1.15),
        (0x164d08, 0.016683333333333335),
    ] {
        assert_eq!(
            f64::from_le_bytes(exe[address + 256..address + 264].try_into().unwrap()),
            expected
        );
    }
    // Manual and menu vtable+10..48: probe/reset/press/release/no-op/drag/gesture/cancel.
    for (slot, expected) in [
        (0x1d8700, 0x86840_i64),
        (0x1d8708, 0x86960),
        (0x1d8710, 0x86a30),
        (0x1d8718, 0x86ac0),
        (0x1d8720, 0x86b20),
        (0x1d8728, 0x86be0),
        (0x1d8730, 0x86dd0),
        (0x1d8738, 0x86e30),
        (0x1d2628, 0x1e7b0),
        (0x1d2630, 0x1e7f0),
        (0x1d2638, 0x1e870),
        (0x1d2640, 0x1e950),
        (0x1d2648, 0x1e9d0),
        (0x1d2650, 0x1ea40),
        (0x1d2658, 0x1eb80),
        (0x1d2660, 0x1ecd0),
    ] {
        let value = (0x11e040..0x152eb8).step_by(24).find_map(|offset| {
            let address =
                u64::from_le_bytes(exe[offset + 256..offset + 264].try_into().unwrap());
            (address == slot).then(|| {
                i64::from_le_bytes(exe[offset + 272..offset + 280].try_into().unwrap())
            })
        });
        assert_eq!(value, Some(expected));
    }
}
