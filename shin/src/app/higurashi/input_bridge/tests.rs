use super::*;
use std::{
    io::Write,
    process::{Command, Stdio},
};
use winit::keyboard::KeyCode;

const SOURCE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../test-fixtures/higurashi/input-bridge-capture/input.txt"
));
const ORIGINAL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../test-fixtures/higurashi/input-bridge-capture/output.txt"
));
const FRAME: Duration = Duration::from_nanos(16_666_667);

fn words(buttons: Buttons) -> [u32; 4] {
    [
        buttons.held,
        buttons.pressed,
        buttons.repeat,
        buttons.rapid_repeat,
    ]
}
fn raw_button(button: GamepadButton) -> RawInputState {
    let mut raw = RawInputState::new();
    raw.gamepads.unified.buttons[button] = true;
    raw
}
fn raw_key(key: KeyCode) -> RawInputState {
    let mut raw = RawInputState::new();
    raw.keyboard.insert(key);
    raw
}
fn original(line: &str) -> ([u32; 4], [i32; 4]) {
    let mut fields = line.split_whitespace();
    assert_eq!(fields.next(), Some("I"));
    let buttons = std::array::from_fn(|_| u32::from_str_radix(fields.next().unwrap(), 16).unwrap());
    let axes = std::array::from_fn(|_| fields.next().unwrap().parse().unwrap());
    assert!(fields.next().is_none());
    (buttons, axes)
}

// Standard Npad FullKey transport supplied by the checked native SDK endpoint;
// actual995c0 does all engine-bit translation and actual999f0 owns its repeats.
fn transport(pad: u32, axes: [i32; 4]) -> RawInputState {
    let mut raw = RawInputState::new();
    for (bit, button) in [
        (0, GamepadButton::A),
        (1, GamepadButton::B),
        (2, GamepadButton::X),
        (3, GamepadButton::Y),
        (4, GamepadButton::StickL),
        (5, GamepadButton::StickR),
        (6, GamepadButton::L),
        (7, GamepadButton::R),
        (8, GamepadButton::ZL),
        (9, GamepadButton::ZR),
        (10, GamepadButton::Plus),
        (11, GamepadButton::Minus),
        (12, GamepadButton::Left),
        (13, GamepadButton::Up),
        (14, GamepadButton::Right),
        (15, GamepadButton::Down),
    ] {
        raw.gamepads.unified.buttons[button] = pad & (1 << bit) != 0;
    }
    for (bit, direction) in [
        (16, VirtualGamepadButton::StickLLeft),
        (17, VirtualGamepadButton::StickLUp),
        (18, VirtualGamepadButton::StickLRight),
        (19, VirtualGamepadButton::StickLDown),
        (20, VirtualGamepadButton::StickRLeft),
        (21, VirtualGamepadButton::StickRUp),
        (22, VirtualGamepadButton::StickRRight),
        (23, VirtualGamepadButton::StickRDown),
    ] {
        raw.gamepads.unified.virtual_keys[direction] = pad & (1 << bit) != 0;
    }
    for (axis, value) in [
        GamepadAxis::LeftStickX,
        GamepadAxis::LeftStickY,
        GamepadAxis::RightStickX,
        GamepadAxis::RightStickY,
    ]
    .into_iter()
    .zip(axes)
    {
        raw.gamepads.unified.axes[axis] = (value as f32 / 32767.0).clamp(-1.0, 1.0);
    }
    raw
}

// Required desktop policy expressed over independently captured native words.
// The producer keeps16..19 separate; the desktop promises first-stick menu
// navigation. Consumer navigation identities4/5/6/7 are native, not invented.
fn desktop_navigation(mut native: [u32; 4]) -> [u32; 4] {
    for word in &mut native {
        for (source, destination) in [(16, 4), (17, 6), (18, 7), (19, 5)] {
            if *word & (1 << source) != 0 {
                *word |= 1 << destination;
            }
        }
    }
    let held = native[0];
    for word in &mut native {
        if held & 0x80 != 0 {
            *word &= !0x20;
        }
        if held & 0x10 != 0 {
            *word &= !0x40;
        }
    }
    native
}

#[test]
fn higurashi_input_bridge_matches_original_source_and_100_frame_repeats() {
    let mut bridge = Input::default();
    let mut outputs = ORIGINAL.lines();
    let mut raw = RawInputState::new();
    let mut observations = 0;
    let mut adapted = 0;
    for line in SOURCE.lines() {
        let mut fields = line.split_whitespace();
        match fields.next().unwrap() {
            "N" => bridge = Input::default(),
            "P" => {
                let pad = u32::from_str_radix(fields.next().unwrap(), 16).unwrap();
                let axes = std::array::from_fn(|_| fields.next().unwrap().parse().unwrap());
                raw = transport(pad, axes);
                bridge.raw_input(&raw);
                bridge.update(FRAME);
            }
            "S" => {
                // Extra raw notifications and cached getters are not another
                // input tick; native995c0 likewise reuses its frame snapshot.
                bridge.raw_input(&raw);
                bridge.raw_input(&raw);
            }
            other => panic!("Unexpected original source opcode{other}"),
        }
        if line == "N" {
            continue;
        }
        let (native, native_axes) = original(outputs.next().unwrap());
        assert_eq!(
            bridge.analog().axes.map(i32::from),
            native_axes,
            "native signed axes at observation{observations}"
        );
        let actual = words(bridge.buttons());
        let expected = desktop_navigation(native);
        // Outside4..7, compare unchanged actual original snapshot words.
        for index in 0..4 {
            assert_eq!(
                actual[index] & !0xf0,
                native[index] & !0xf0,
                "original non-navigation word{index} at observation{observations}"
            );
        }
        assert_eq!(
            actual, expected,
            "original source observation{observations}"
        );
        adapted += usize::from(expected != native);
        observations += 1;
    }
    assert!(outputs.next().is_none());
    assert_eq!(observations, 216);
    assert!(
        adapted > 0,
        "Required first-stick aliases must be exercised"
    );
    eprintln!(
        "Higurashi input bridge:{observations} original producer observations/100 held frames; non-navigation exact and{adapted} explicitly adapted first-stick observations"
    );
}

#[test]
fn higurashi_analog_pan_keeps_real_dpad_and_quarantines_four_axes() {
    let mut bridge = Input::default();
    let mut raw = RawInputState::new();
    for (axis, value) in [
        (GamepadAxis::LeftStickX, 0.5),
        (GamepadAxis::LeftStickY, -0.25),
        (GamepadAxis::RightStickX, -0.75),
        (GamepadAxis::RightStickY, 1.),
    ] {
        raw.gamepads.unified.axes[axis] = value;
    }
    raw.gamepads.unified.virtual_keys[VirtualGamepadButton::StickLUp] = true;
    bridge.raw_input(&raw);
    bridge.update(FRAME);
    assert_eq!(bridge.analog().axes, [63, 31, -95, -127]);
    assert_eq!(bridge.buttons().held & 0xf0, 0x10);
    assert_eq!(bridge.analog().buttons(bridge.buttons()).held & 0xf0, 0);
    assert_eq!(
        bridge.analog().buttons(bridge.buttons()).repeat & 0xf0,
        0x10,
        "Questions retain approved first-stick directional navigation"
    );
    raw.gamepads.unified.buttons[GamepadButton::Down] = true;
    bridge.raw_input(&raw);
    bridge.update(FRAME);
    assert_eq!(bridge.analog().buttons(bridge.buttons()).held & 0xf0, 0x40);
    bridge.flush();
    bridge.raw_input(&raw);
    bridge.update(FRAME);
    assert_eq!(bridge.analog().axes, [0; 4]);
    assert_eq!(bridge.analog().dpad_held, 0);
    bridge.focus_lost();
    bridge.raw_input(&RawInputState::new());
    bridge.update(FRAME);
    bridge.set_focused(true);
    bridge.raw_input(&raw);
    bridge.update(FRAME);
    assert_eq!(
        bridge.analog().axes,
        [0; 4],
        "blank background samples are not releases"
    );
    raw.gamepads.unified.axes[GamepadAxis::LeftStickX] = 0.;
    bridge.raw_input(&raw);
    bridge.update(FRAME);
    raw.gamepads.unified.axes[GamepadAxis::LeftStickX] = 0.5;
    bridge.raw_input(&raw);
    bridge.update(FRAME);
    assert_eq!(
        bridge.analog().axes,
        [63, 0, 0, 0],
        "axis releases are independent"
    );
    bridge.raw_input(&RawInputState::new());
    bridge.update(FRAME);
    bridge.raw_input(&raw);
    bridge.update(FRAME);
    assert_eq!(bridge.analog().axes, [63, 31, -95, -127]);
    assert_eq!(bridge.analog().dpad_held, 0x40);
    for sign in [-1., 1.] {
        let mut bridge = Input::default();
        let mut raw = RawInputState::new();
        let axes = [
            GamepadAxis::LeftStickX,
            GamepadAxis::LeftStickY,
            GamepadAxis::RightStickX,
            GamepadAxis::RightStickY,
        ];
        for axis in axes {
            raw.gamepads.unified.axes[axis] = 0.25 * sign;
        }
        bridge.raw_input(&raw);
        bridge.update(FRAME);
        bridge.flush();
        bridge.raw_input(&raw);
        bridge.update(FRAME);
        assert_eq!(
            bridge.analog().axes,
            [0; 4],
            "four subthreshold holds stay quarantined"
        );
        raw.gamepads.unified.axes[axes[2]] = 0.001;
        bridge.raw_input(&raw);
        bridge.update(FRAME);
        raw.gamepads.unified.axes[axes[2]] = 0.25 * sign;
        bridge.raw_input(&raw);
        bridge.update(FRAME);
        assert_eq!(
            bridge.analog().axes,
            [0, 0, (31. * sign) as i8, 0],
            "quantized neutral releases just one component"
        );
    }
}

#[test]
#[ignore = "executes the frozen updated99550/995c0/999f0/99b40 producer with checked Npad transport; no hardware"]
fn higurashi_input_bridge_original_capture_is_reproducible() {
    let exe = std::env::var("SHIN_HIGURASHI_NATIVE_INPUT_BRIDGE").unwrap();
    let mut child = Command::new(exe)
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../higurashi/uncompressed_executable_update"
        ))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || input.write_all(SOURCE.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, ORIGINAL.as_bytes());
}

#[test]
fn higurashi_approved_keys_and_first_stick_match_controller_navigation() {
    for (key, button, native_mask) in [
        (KeyCode::ArrowUp, GamepadButton::Up, 0x10),
        (KeyCode::ArrowRight, GamepadButton::Right, 0x20),
        (KeyCode::ArrowDown, GamepadButton::Down, 0x40),
        (KeyCode::ArrowLeft, GamepadButton::Left, 0x80),
        (KeyCode::Enter, GamepadButton::A, 0x0100_2000),
        (KeyCode::Escape, GamepadButton::B, 0x0200_4000),
        (KeyCode::KeyB, GamepadButton::X, 0x1000),
        (KeyCode::KeyM, GamepadButton::Y, 0x8000),
        (KeyCode::KeyH, GamepadButton::L, 0x400),
        (KeyCode::KeyJ, GamepadButton::R, 0x800),
    ] {
        let mut keyboard = Input::default();
        let mut controller = Input::default();
        keyboard.raw_input(&raw_key(key));
        controller.raw_input(&raw_button(button));
        for _ in 0..75 {
            let a = words(keyboard.update(FRAME));
            let b = words(controller.update(FRAME));
            assert_eq!(a, b, "approved key{key:?}");
            assert_eq!(a[0], native_mask, "primary native control identity");
        }
    }
    for (key, direction, high, navigation) in [
        (KeyCode::ArrowUp, VirtualGamepadButton::StickLUp, 16, 0x10),
        (
            KeyCode::ArrowDown,
            VirtualGamepadButton::StickLDown,
            17,
            0x40,
        ),
        (
            KeyCode::ArrowLeft,
            VirtualGamepadButton::StickLLeft,
            18,
            0x80,
        ),
        (
            KeyCode::ArrowRight,
            VirtualGamepadButton::StickLRight,
            19,
            0x20,
        ),
    ] {
        let mut raw = RawInputState::new();
        raw.gamepads.unified.virtual_keys[direction] = true;
        let mut stick = Input::default();
        let mut keyboard = Input::default();
        stick.raw_input(&raw);
        keyboard.raw_input(&raw_key(key));
        for _ in 0..75 {
            let a = words(stick.update(FRAME));
            let b = words(keyboard.update(FRAME));
            for index in 0..4 {
                assert_eq!(a[index] & !0x000f_0000, b[index]);
            }
            assert_eq!(a[0], (1 << high) | navigation);
            assert_eq!(
                a[0] & 0xf,
                0,
                "Stick directions must not masquerade as Plus/clicks"
            );
        }
    }
    // Semantic priorities stay native rather than being reordered by keys.
    let mut conflicts = RawInputState::new();
    for key in [
        KeyCode::ArrowUp,
        KeyCode::ArrowDown,
        KeyCode::ArrowLeft,
        KeyCode::ArrowRight,
        KeyCode::Enter,
        KeyCode::Escape,
    ] {
        conflicts.keyboard.insert(key);
    }
    let mut bridge = Input::default();
    bridge.raw_input(&conflicts);
    assert_eq!(words(bridge.update(FRAME)), [0x0300_6090; 4]);
}

#[test]
fn higurashi_right_stick_keys_preserve_axes_and_have_no_shoulder_aliases() {
    for (key, virtual_key, y, bit) in [
        (KeyCode::KeyK, VirtualGamepadButton::StickRUp, 1.0, 20),
        (KeyCode::KeyL, VirtualGamepadButton::StickRDown, -1.0, 21),
    ] {
        let mut raw = RawInputState::new();
        raw.gamepads.unified.axes[GamepadAxis::RightStickY] = y;
        raw.gamepads.unified.virtual_keys[virtual_key] = true;
        let mut controller = Input::default();
        let mut keyboard = Input::default();
        controller.raw_input(&raw);
        keyboard.raw_input(&raw_key(key));
        for _ in 0..75 {
            assert_eq!(
                words(controller.update(FRAME)),
                words(keyboard.update(FRAME))
            );
            assert_eq!(keyboard.right_stick(), Vec2::new(0.0, y));
            assert_eq!(keyboard.buttons().held, 1 << bit);
            assert_eq!(keyboard.right_stick_up(), bit == 20);
            assert_eq!(keyboard.right_stick_down(), bit == 21);
        }
    }
    let mut both = raw_key(KeyCode::KeyK);
    both.keyboard.insert(KeyCode::KeyL);
    let mut bridge = Input::default();
    bridge.raw_input(&both);
    assert_eq!(words(bridge.update(FRAME)), [0; 4]);
    assert_eq!(bridge.right_stick(), Vec2::ZERO);
    for key in [
        KeyCode::Space,
        KeyCode::KeyA,
        KeyCode::KeyS,
        KeyCode::KeyD,
        KeyCode::KeyY,
        KeyCode::KeyX,
        KeyCode::Tab,
        KeyCode::PageUp,
        KeyCode::PageDown,
        KeyCode::Home,
        KeyCode::End,
        KeyCode::F11,
        KeyCode::ControlLeft,
        KeyCode::ShiftLeft,
    ] {
        let mut bridge = Input::default();
        bridge.raw_input(&raw_key(key));
        assert_eq!(
            words(bridge.update(Duration::from_secs(2))),
            [0; 4],
            "removed key{key:?}"
        );
        assert_eq!(bridge.right_stick(), Vec2::ZERO);
    }
}

#[test]
fn higurashi_input_reuses_elapsed_cadence_and_combined_direction_has_one_edge() {
    let mut bridge = Input::default();
    let mut raw = raw_button(GamepadButton::Up);
    bridge.raw_input(&raw);
    assert_eq!(words(bridge.update(Duration::ZERO)), [0x10; 4]);
    raw.gamepads.unified.virtual_keys[VirtualGamepadButton::StickLUp] = true;
    bridge.raw_input(&raw);
    let state = bridge.update(Duration::from_millis(200));
    assert_eq!(state.held, 0x10010);
    assert_eq!(
        state.pressed, 0x10000,
        "New physical stick edge, no duplicate combined navigation edge"
    );
    assert_eq!(state.repeat, 0x10000);
    assert_eq!(bridge.update(Duration::from_nanos(199_999_999)).repeat, 0);
    let repeated = bridge.update(Duration::from_nanos(1));
    assert_eq!(
        repeated.repeat, 0x10,
        "Original400ms D-pad repeat, independent from newly held stick"
    );
    // Switching to the still-held stick retains the combined navigation clock.
    raw.gamepads.unified.buttons[GamepadButton::Up] = false;
    bridge.raw_input(&raw);
    let held = bridge.update(Duration::ZERO);
    assert_eq!(held.held, 0x10010);
    assert_eq!(held.pressed, 0);
}

#[test]
fn higurashi_flush_and_focus_quarantine_holds_until_real_release() {
    let mut bridge = Input::default();
    let mut raw = raw_key(KeyCode::Enter);
    raw.keyboard.insert(KeyCode::KeyK);
    raw.mouse.buttons[MouseButton::Left] = true;
    raw.mouse.position = Vec2::new(1001.0, 728.0);
    bridge.raw_input(&raw);
    bridge.update(FRAME);
    assert_eq!(
        bridge.pointer_position(),
        raw.mouse.position,
        "No pointer normalization in button bridge"
    );
    assert!(bridge.mouse_left_held());
    bridge.flush();
    assert_eq!(words(bridge.buttons()), [0; 4]);
    assert_eq!(bridge.right_stick(), Vec2::ZERO);
    assert!(!bridge.mouse_left_held());
    bridge.raw_input(&raw);
    assert_eq!(words(bridge.update(Duration::from_secs(2))), [0; 4]);
    // A new unrelated shoulder remains available while the old A is blocked.
    raw.keyboard.insert(KeyCode::KeyH);
    bridge.raw_input(&raw);
    assert_eq!(words(bridge.update(FRAME)), [0x400; 4]);
    bridge.focus_lost();
    for _ in 0..3 {
        bridge.raw_input(&RawInputState::new());
        assert_eq!(words(bridge.update(Duration::from_secs(1))), [0; 4]);
    }
    bridge.raw_input(&raw);
    bridge.set_focused(true);
    assert_eq!(
        words(bridge.update(FRAME)),
        [0; 4],
        "Unfocused blank samples are not physical release"
    );
    assert_eq!(bridge.right_stick(), Vec2::ZERO);
    assert!(!bridge.mouse_left_held());
    bridge.raw_input(&RawInputState::new());
    bridge.update(FRAME);
    bridge.raw_input(&raw);
    assert_eq!(words(bridge.update(FRAME)), [0x0110_2400; 4]);
    assert_eq!(bridge.right_stick(), Vec2::Y);
    assert!(bridge.mouse_left_held());
    // Mouse contact is exposed separately and never fabricates controller A.
    let mut mouse = RawInputState::new();
    mouse.mouse.buttons[MouseButton::Left] = true;
    bridge.raw_input(&mouse);
    assert_eq!(words(bridge.update(FRAME)), [0; 4]);
    assert!(bridge.mouse_left_held());
}

#[test]
fn temporary_ctrl_skip_is_separate_and_quarantined_until_physical_release() {
    let mut input = Input::default();
    let ctrl = raw_key(KeyCode::ControlLeft);
    input.raw_input(&ctrl);
    assert_eq!(words(input.update(FRAME)), [0; 4]);
    assert!(input.temporary_skip_held());
    input.flush();
    input.raw_input(&ctrl);
    input.update(FRAME);
    assert!(!input.temporary_skip_held());
    input.raw_input(&RawInputState::new());
    input.update(FRAME);
    input.raw_input(&ctrl);
    input.update(FRAME);
    assert!(input.temporary_skip_held());
    input.focus_lost();
    input.raw_input(&RawInputState::new());
    input.update(FRAME); // unfocused blank sample must not release quarantine
    assert!(!input.temporary_skip_held());
    input.set_focused(true);
    input.raw_input(&ctrl);
    input.update(FRAME);
    assert!(!input.temporary_skip_held());
    input.raw_input(&RawInputState::new());
    input.update(FRAME);
    input.raw_input(&ctrl);
    input.update(FRAME);
    assert!(input.temporary_skip_held());
    input.raw_input(&raw_key(KeyCode::ControlRight));
    assert_eq!(words(input.update(FRAME)), [0; 4]);
    assert!(!input.temporary_skip_held());
}

#[test]
fn right_mouse_is_controller_b_and_quarantined_like_it() {
    let mut right = RawInputState::new();
    right.mouse.buttons[MouseButton::Right] = true;
    let mut mouse = Input::default();
    let mut controller = Input::default();
    mouse.raw_input(&right);
    controller.raw_input(&raw_button(GamepadButton::B));
    for _ in 0..75 {
        let a = words(mouse.update(FRAME));
        assert_eq!(a, words(controller.update(FRAME)));
        assert_eq!(a[0], 0x0200_4000);
    }
    assert!(
        !mouse.mouse_left_held(),
        "B never fabricates a touch contact"
    );
    mouse.flush();
    mouse.raw_input(&right);
    assert_eq!(words(mouse.update(FRAME)), [0; 4]);
    mouse.focus_lost();
    mouse.raw_input(&RawInputState::new());
    mouse.update(FRAME);
    mouse.set_focused(true);
    mouse.raw_input(&right);
    assert_eq!(
        words(mouse.update(FRAME)),
        [0; 4],
        "Unfocused blank samples are not physical release"
    );
    mouse.raw_input(&RawInputState::new());
    mouse.update(FRAME);
    mouse.raw_input(&right);
    assert_eq!(words(mouse.update(FRAME))[1], 0x0200_4000);
}

#[test]
fn higurashi_wheel_is_fractional_frame_displacement_and_never_a_button_or_stick() {
    let mut input = Input::default();
    let mut raw = RawInputState::new();
    raw.mouse.scroll_amount = -3.015625;
    input.raw_input(&raw);
    input.raw_input(&raw); // replacement, not a second packet
    assert_eq!(input.analog().wheel_scroll, 0.);
    assert_eq!(words(input.update(FRAME)), [0; 4]);
    assert_eq!(input.analog().wheel_scroll, 3.015625);
    assert_eq!(input.analog().wheel_scroll, 3.015625); // cached frame getter
    assert_eq!(input.analog().axes, [0; 4]);
    assert_eq!(words(input.update(FRAME)), [0; 4]);
    assert_eq!(input.analog().wheel_scroll, 0.); // consumed even without a new raw sample
    raw.keyboard.insert(KeyCode::KeyK);
    raw.mouse.scroll_amount = 0.125;
    input.raw_input(&raw);
    input.update(FRAME);
    assert_eq!(input.analog().wheel_scroll, -0.125);
    assert_eq!(input.analog().axes[3], -127); // keyboard keeps its native byte
    for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        raw.mouse.scroll_amount = invalid;
        input.raw_input(&raw);
        input.update(FRAME);
        assert_eq!(input.analog().wheel_scroll, 0.);
    }
}

#[test]
fn higurashi_wheel_flush_and_focus_discard_pending_and_published_motion() {
    let mut input = Input::default();
    let mut raw = RawInputState::new();
    raw.mouse.scroll_amount = -0.5;
    for published in [false, true] {
        input.raw_input(&raw);
        if published {
            input.update(FRAME);
        }
        input.flush();
        assert_eq!(input.analog().wheel_scroll, 0.);
        input.update(FRAME);
        assert_eq!(input.analog().wheel_scroll, 0.);
    }
    input.raw_input(&raw);
    input.set_focused(false);
    input.raw_input(&raw); // background callbacks may still arrive
    input.update(FRAME);
    input.set_focused(true);
    input.update(FRAME);
    assert_eq!(input.analog().wheel_scroll, 0.);
    input.raw_input(&raw); // a fresh focused displacement is immediately usable
    input.update(FRAME);
    assert_eq!(input.analog().wheel_scroll, 0.5);
}
