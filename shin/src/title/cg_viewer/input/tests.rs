use crate::title::cg_viewer::ViewerRequest;
use super::*;
#[test]
fn axes_take_left_stick_and_convert_vertical_sign_without_right_stick_alias() {
    let mut raw = RawInputState::new();
    raw.gamepads.unified.axes[GamepadAxis::LeftStickX] = 0.3;
    raw.gamepads.unified.axes[GamepadAxis::LeftStickY] = 0.4;
    raw.gamepads.unified.axes[GamepadAxis::RightStickX] = 0.8;
    raw.gamepads.unified.axes[GamepadAxis::RightStickY] = -0.9;
    assert_eq!(CgViewerInput::axes(&raw), Vec2::new(0.3, -0.4));
}
use crate::adv::input_area::{PointerEvent, PointerKind};
use shin_core::{
    format::scenario::{info::PictureId, instruction_elements::FromNumber},
    time::Ticks,
};
fn row() -> CgRow {
    CgRow {
        entry: 0,
        thumbnail: "test".into(),
        pictures: [1, 2, 3].map(PictureId::from_number).into(),
        unlocked: vec![true, true, true],
        plate: [0.0, 0.0, 412.0, 242.0],
    }
}
fn click() -> ActionState {
    ActionState {
        is_clicked: true,
        is_clicked_or_repeated: true,
        is_held: true,
        ..Default::default()
    }
}
fn event(kind: PointerKind, x: i32, y: i32) -> PointerEvent {
    PointerEvent {
        kind,
        id: 0,
        position: glam::ivec2(x, y),
    }
}
fn frame(
    input: &mut CgViewerInput,
    m: &mut CgViewerNavigation,
    r: &CgRow,
    n: u32,
    t: f64,
    enabled: bool,
    events: &[PointerEvent],
    state: &EnumMap<AppAction, ActionState>,
    axes: Vec2,
) {
    input.input_frame(
        m,
        r,
        enabled,
        &PointerFrame {
            number: n,
            time_seconds: t,
            events,
        },
        state,
        axes,
    );
}
#[test]
fn held_dpad_overrides_axes_with_native_signs_and_combined_navigation_is_ignored() {
    let r = row();
    let mut m = CgViewerNavigation::new(&r);
    let mut input = CgViewerInput::new();
    m.publish_picture(Vec2::new(3840.0, 2160.0));
    let mut state: EnumMap<AppAction, ActionState> = EnumMap::default();
    state[AppAction::ViewerPanUp].is_held = true;
    state[AppAction::ViewerPanDown].is_held = true;
    state[AppAction::ViewerPanLeft].is_held = true;
    state[AppAction::ViewerPanRight].is_held = true;
    frame(
        &mut input,
        &mut m,
        &r,
        0,
        0.0,
        true,
        &[],
        &state,
        Vec2::new(0.7, 0.6),
    );
    assert_eq!(m.position(), Vec2::new(16.0, -16.0));
    frame(
        &mut input,
        &mut m,
        &r,
        1,
        0.02,
        true,
        &[],
        &state,
        Vec2::ZERO,
    );
    assert_eq!(m.position(), Vec2::new(32.0, -32.0)); // Held, without repeat edge.
    state = EnumMap::default();
    state[AppAction::Left] = click();
    state[AppAction::Up] = click();
    frame(
        &mut input,
        &mut m,
        &r,
        2,
        0.04,
        true,
        &[],
        &state,
        Vec2::new(0.25, 0.5),
    );
    assert_eq!(m.position(), Vec2::new(24.0, -16.0));
    state[AppAction::ViewerPanRight].is_held = true;
    state[AppAction::ViewerPanDown].is_held = true;
    frame(
        &mut input,
        &mut m,
        &r,
        3,
        0.06,
        true,
        &[],
        &state,
        Vec2::new(-0.7, -0.8),
    );
    assert_eq!(m.position(), Vec2::new(8.0, 0.0));
}
#[test]
fn half_size_loading_tap_consumes_cancel_and_release_reads_current_parent_row() {
    let r = row();
    let mut m = CgViewerNavigation::new(&r);
    let mut input = CgViewerInput::new();
    m.take_requests();
    input.resize(&mut m, &r, (960, 540));
    let mut state: EnumMap<AppAction, ActionState> = EnumMap::default();
    state[AppAction::Cancel] = click();
    frame(
        &mut input,
        &mut m,
        &r,
        0,
        0.0,
        true,
        &[event(PointerKind::Down, 400, 250)],
        &state,
        Vec2::ZERO,
    );
    assert!(!m.closing()); // No published picture is required for contact capture.
    let changed = CgRow {
        pictures: [4, 5, 6].map(PictureId::from_number).into(),
        unlocked: vec![true, false, true],
        ..row()
    };
    frame(
        &mut input,
        &mut m,
        &changed,
        1,
        0.02,
        true,
        &[event(PointerKind::Up, 2000, -100)],
        &state,
        Vec2::ZERO,
    );
    assert!(!m.closing());
    assert_eq!(m.ordinal(), 2);
    assert_eq!(
        m.take_requests(),
        [ViewerRequest::Load {
            ordinal: 2,
            picture: PictureId::from_number(6)
        }]
    );
    assert_eq!(m.take_sounds(), ["decide"]);
}
#[test]
fn half_size_drag_uses_vertical_factor_and_release_fling_then_tap_stops_and_advances() {
    let r = row();
    let mut m = CgViewerNavigation::new(&r);
    let mut input = CgViewerInput::new();
    m.take_requests();
    m.publish_picture(Vec2::new(3840.0, 2160.0));
    input.resize(&mut m, &r, (960, 540));
    let state = EnumMap::default();
    frame(
        &mut input,
        &mut m,
        &r,
        0,
        0.0,
        true,
        &[event(PointerKind::Down, 400, 300)],
        &state,
        Vec2::ZERO,
    );
    frame(
        &mut input,
        &mut m,
        &r,
        1,
        0.02,
        true,
        &[event(PointerKind::Move, 420, 280)],
        &state,
        Vec2::ZERO,
    );
    assert_eq!(m.position(), Vec2::new(40.0, 22.5));
    frame(
        &mut input,
        &mut m,
        &r,
        2,
        0.04,
        true,
        &[event(PointerKind::Up, 420, 280)],
        &state,
        Vec2::ZERO,
    );
    assert!(m.velocity().x > 0.0);
    assert!(m.velocity().y < 0.0);
    m.update(Ticks::from_f32(2.0));
    assert!(m.position().x > 40.0);
    assert!(m.take_requests().is_empty());
    assert!(m.take_sounds().is_empty());
    frame(
        &mut input,
        &mut m,
        &r,
        3,
        0.06,
        true,
        &[event(PointerKind::Down, 400, 250)],
        &state,
        Vec2::ZERO,
    );
    assert_eq!(m.velocity(), Vec2::ZERO);
    frame(
        &mut input,
        &mut m,
        &r,
        4,
        0.08,
        true,
        &[event(PointerKind::Up, 0, 0)],
        &state,
        Vec2::ZERO,
    );
    assert_eq!(m.ordinal(), 1);
    assert_eq!(m.take_sounds(), ["decide"]);
}
#[test]
fn fit_indicator_edges_and_disabled_or_resized_contacts_do_not_manufacture_release() {
    let r = row();
    let mut m = CgViewerNavigation::new(&r);
    let mut input = CgViewerInput::new();
    m.take_requests();
    m.publish_picture(Vec2::new(3840.0, 2160.0));
    let mut state: EnumMap<AppAction, ActionState> = EnumMap::default();
    state[AppAction::CharacterVariant] = click();
    state[AppAction::CharacterState] = click();
    frame(
        &mut input,
        &mut m,
        &r,
        0,
        0.0,
        true,
        &[],
        &state,
        Vec2::ZERO,
    );
    assert!(m.fit());
    assert!(m.indicators());
    state[AppAction::CharacterVariant].is_clicked = false;
    frame(
        &mut input,
        &mut m,
        &r,
        1,
        0.02,
        true,
        &[],
        &state,
        Vec2::ZERO,
    );
    assert!(!m.indicators());
    state = EnumMap::default();
    state[AppAction::Cancel] = click();
    frame(
        &mut input,
        &mut m,
        &r,
        2,
        0.04,
        true,
        &[event(PointerKind::Down, 300, 300)],
        &state,
        Vec2::ZERO,
    );
    frame(
        &mut input,
        &mut m,
        &r,
        3,
        0.06,
        false,
        &[],
        &state,
        Vec2::ZERO,
    );
    frame(
        &mut input,
        &mut m,
        &r,
        4,
        0.08,
        true,
        &[event(PointerKind::Up, 300, 300)],
        &EnumMap::default(),
        Vec2::ZERO,
    );
    frame(
        &mut input,
        &mut m,
        &r,
        5,
        0.10,
        true,
        &[event(PointerKind::Down, 300, 300)],
        &EnumMap::default(),
        Vec2::ZERO,
    );
    input.resize(&mut m, &r, (960, 540));
    frame(
        &mut input,
        &mut m,
        &r,
        6,
        0.12,
        true,
        &[event(PointerKind::Up, 150, 150)],
        &EnumMap::default(),
        Vec2::ZERO,
    );
    assert_eq!(m.ordinal(), 0);
    assert!(!m.closing());
    assert!(m.take_requests().is_empty());
    assert!(m.take_sounds().is_empty());
}

#[test]
#[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
fn reference_native_cg_input_snapshot_held_dpad_left_axes_and_pointer_dispatch() {
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
        (0xc49a8, 0x29402d2a),
        (0xc49ac, 0x29002d0a),
        (0xc49b8, 0x29442d2a),
        (0xc49bc, 0x29022d0a),
        (0xc49c0, 0x29452d2a),
        (0xc49c4, 0x29032d0a),
        (0xc4684, 0xbd002280),
        (0xc468c, 0xbd002683),
        (0xc4690, 0xbd002a81),
        (0xc4694, 0xbd002e82),
        (0xc4874, 0x1a880128),
        (0xc4884, 0x1a880128),
        (0x4aa04, 0x940083bf),
        (0x4aa0c, 0x910023e8),
        (0x4aa14, 0xb9400fe8),
        (0x4aa5c, 0xbd401be1),
        (0x4aa60, 0xb9400be8),
        (0x4aa64, 0x370005e8),
        (0x4aa6c, 0xbd401fe0),
        (0x4aa78, 0x1e220800),
        (0x4aa7c, 0x1e261002),
        (0x4aa80, 0x1e220c00),
        (0x4aa84, 0x36100528),
        (0x4ab20, 0x1e361000),
        (0x4ab30, 0x1e220821),
        (0x4ab34, 0x1e361002),
        (0x4ab3c, 0x1e220c21),
        (0x4ab54, 0x1e261001),
        (0x4ab58, 0x2d5a0e62),
        (0x4ab60, 0x1e222821),
        (0x4bb90, 0xf900641f),
        (0x4bbb0, 0xd65f03c0),
        (0x4bbd0, 0xd65f03c0),
        (0x4bc90, 0xd65f03c0),
        (0x4bdb0, 0xd65f03c0),
    ] {
        assert_eq!(word(a), expected, "native CG input at {a:x}");
    }
    for (a, expected) in [(0x159694, 32.0f32), (0x1595e8, -32.0)] {
        assert_eq!(f32::from_bits(word(a)), expected);
    }
}

#[test]
fn native_motion_gate_suppresses_loading_fade_and_completion_poll_input() {
    use crate::title::cg_viewer::motion::CgViewerMotion;
    let r = row();
    let mut m = CgViewerNavigation::new(&r);
    m.take_requests(); // Initial owner load.
    let mut input = CgViewerInput::new();
    let mut motion = CgViewerMotion::new();
    let mut state: EnumMap<AppAction, ActionState> = EnumMap::default();
    state[AppAction::Enter] = click();
    state[AppAction::Cancel] = click();
    for (n, ready, delta) in [(0, false, 100.0), (1, true, 30.0), (2, true, 0.0)] {
        motion.prepare_frame(ready, true);
        assert!(!motion.accepts_input());
        frame(
            &mut input,
            &mut m,
            &r,
            n,
            n as f64,
            motion.accepts_input(),
            &[event(PointerKind::Down, 300, 300)],
            &state,
            Vec2::ZERO,
        );
        assert!(m.take_requests().is_empty());
        assert!(!m.closing());
        motion.advance(Ticks::from_f32(delta));
    }
    motion.prepare_frame(true, true);
    assert!(motion.accepts_input());
    // A release without an enabled press must not replay the suppressed gesture.
    frame(
        &mut input,
        &mut m,
        &r,
        3,
        3.0,
        motion.accepts_input(),
        &[event(PointerKind::Up, 300, 300)],
        &EnumMap::default(),
        Vec2::ZERO,
    );
    assert!(m.take_requests().is_empty());
    frame(
        &mut input,
        &mut m,
        &r,
        4,
        4.0,
        motion.accepts_input(),
        &[],
        &state,
        Vec2::ZERO,
    );
    assert!(m.closing()); // Native cancel wins over confirm once eligible.
    assert_eq!(m.take_requests(), [ViewerRequest::Close]);
}
