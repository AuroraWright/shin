//! Main-thread desktop source for the older99b40 snapshot.
//!
//! Native995c0/999f0 assigns D-pad4..7, stick directions16..23 and A/B
//! aliases24/25. The user's desktop contract also maps first-stick navigation
//! into4..7; this addition is separate from the native Npad producer. Repeats
//! use shin-window's existing elapsed-duration ActionState cadence once per
//! admitted application update, not rendering or additional raw samples.
use crate::adv::higurashi::input::Buttons;
use enum_map::{Enum, EnumMap};
use glam::Vec2;
use shin_input::{
    ActionState, ActionsState, RawInputState,
    inputs::{GamepadAxis, GamepadButton, MouseButton, VirtualGamepadButton},
};
use std::time::Duration;

#[derive(Clone, Copy, Enum)]
#[repr(u8)]
enum NativeAction {
    Minus,
    StickL,
    StickR,
    Plus,
    Up,
    Right,
    Down,
    Left,
    ZL,
    ZR,
    L,
    R,
    X,
    A,
    B,
    Y,
    StickLUp,
    StickLDown,
    StickLLeft,
    StickLRight,
    StickRUp,
    StickRDown,
    StickRLeft,
    StickRRight,
    Enter,
    Cancel,
}

fn lower(mapped: &RawInputState) -> EnumMap<NativeAction, bool> {
    use NativeAction::*;
    let pad = &mapped.gamepads;
    let button = |key| pad.is_held(key);
    let stick = |key| pad.is_vheld(key);
    EnumMap::from_fn(|action| match action {
        Minus => button(GamepadButton::Minus),
        StickL => button(GamepadButton::StickL),
        StickR => button(GamepadButton::StickR),
        Plus => button(GamepadButton::Plus),
        // Required desktop navigation equivalence; preserve the separate
        // high stick bits below as well as the original D-pad consumers.
        Up => button(GamepadButton::Up) || stick(VirtualGamepadButton::StickLUp),
        Right => button(GamepadButton::Right) || stick(VirtualGamepadButton::StickLRight),
        Down => button(GamepadButton::Down) || stick(VirtualGamepadButton::StickLDown),
        Left => button(GamepadButton::Left) || stick(VirtualGamepadButton::StickLLeft),
        ZL => button(GamepadButton::ZL),
        ZR => button(GamepadButton::ZR),
        L => button(GamepadButton::L),
        R => button(GamepadButton::R),
        X => button(GamepadButton::X),
        A | Enter => button(GamepadButton::A),
        B | Cancel => button(GamepadButton::B),
        Y => button(GamepadButton::Y),
        StickLUp => stick(VirtualGamepadButton::StickLUp),
        StickLDown => stick(VirtualGamepadButton::StickLDown),
        StickLLeft => stick(VirtualGamepadButton::StickLLeft),
        StickLRight => stick(VirtualGamepadButton::StickLRight),
        StickRUp => stick(VirtualGamepadButton::StickRUp),
        StickRDown => stick(VirtualGamepadButton::StickRDown),
        StickRLeft => stick(VirtualGamepadButton::StickRLeft),
        StickRRight => stick(VirtualGamepadButton::StickRRight),
    })
}

fn snapshot(states: &EnumMap<NativeAction, ActionState>) -> Buttons {
    let mut buttons = Buttons::default();
    for (action, state) in states {
        let bit = 1 << action as u8;
        if state.is_held {
            buttons.held |= bit;
        }
        if state.is_clicked {
            buttons.pressed |= bit;
        }
        if state.is_clicked_or_repeated {
            buttons.repeat |= bit;
        }
        if state.is_clicked_or_rapid_repeated {
            buttons.rapid_repeat |= bit;
        }
    }
    buttons
}

/// Magnitude-sensitive consumers use real D-pad holds and all four native
/// signed bytes. Other menus retain the approved first-stick aliases.
#[derive(Debug, Default, Clone, Copy)]
pub(super) struct Analog {
    pub axes: [i8; 4],
    pub dpad_held: u32,
}
impl Analog {
    pub fn buttons(self, mut buttons: Buttons) -> Buttons {
        // Questions still receive the approved directional pressed/repeat
        // aliases. Chart/Viewer inspect held bits for proportional panning.
        buttons.held = (buttons.held & !0xf0) | self.dpad_held;
        buttons
    }
}

pub(super) struct Input {
    raw: EnumMap<NativeAction, bool>,
    blocked: EnumMap<NativeAction, bool>,
    actions: ActionsState<NativeAction>,
    buttons: Buttons,
    focused: bool,
    pointer: Vec2,
    mouse_left: bool,
    mouse_left_blocked: bool,
    right_stick: Vec2,
    right_stick_blocked: [bool; 2],
    left_stick: Vec2,
    analog_blocked: [bool; 4],
    raw_dpad: u32,
    blocked_dpad: u32,
    dpad_held: u32,
}
impl Default for Input {
    fn default() -> Self {
        Self {
            raw: EnumMap::default(),
            blocked: EnumMap::default(),
            actions: ActionsState::new(),
            buttons: Buttons::default(),
            focused: true,
            pointer: Vec2::ZERO,
            mouse_left: false,
            mouse_left_blocked: false,
            right_stick: Vec2::ZERO,
            right_stick_blocked: [false; 2],
            left_stick: Vec2::ZERO,
            analog_blocked: [false; 4],
            raw_dpad: 0,
            blocked_dpad: 0,
            dpad_held: 0,
        }
    }
}
impl Input {
    /// Supply the original shin-window raw snapshot, before any AppAction
    /// lowering. Repeated calls merely replace the pending snapshot.
    pub fn raw_input(&mut self, raw: &RawInputState) {
        let previous_y = self.raw[NativeAction::Y];
        let mapped = crate::app::keyboard_as_controller(raw);
        self.raw = lower(&mapped);
        if previous_y != self.raw[NativeAction::Y] {
            tracing::debug!(
                held = self.raw[NativeAction::Y],
                blocked = self.blocked[NativeAction::Y],
                focused = self.focused,
                "HIGURASHI_Y_RAW"
            );
        }
        self.pointer = mapped.mouse.position;
        self.mouse_left = mapped.mouse.buttons[MouseButton::Left];
        self.left_stick = Vec2::new(
            mapped.gamepads.unified.axes[GamepadAxis::LeftStickX],
            mapped.gamepads.unified.axes[GamepadAxis::LeftStickY],
        );
        self.raw_dpad = 0;
        for (button, bit) in [
            (GamepadButton::Up, 4),
            (GamepadButton::Right, 5),
            (GamepadButton::Down, 6),
            (GamepadButton::Left, 7),
        ] {
            if mapped.gamepads.is_held(button) {
                self.raw_dpad |= 1 << bit;
            }
        }
        self.right_stick = Vec2::new(
            mapped.gamepads.unified.axes[GamepadAxis::RightStickX],
            mapped.gamepads.unified.axes[GamepadAxis::RightStickY],
        );
    }

    pub fn update(&mut self, elapsed: Duration) -> Buttons {
        if !self.focused {
            // shin-window substitutes blank raw samples while unfocused.
            // Those samples are not physical releases of quarantined holds.
            self.buttons = Buttons::default();
            self.dpad_held = 0;
            return self.buttons;
        }
        let mut held = EnumMap::from_fn(|action| {
            self.blocked[action] &= self.raw[action];
            self.raw[action] && !self.blocked[action]
        });
        // Actual999f0 gives Left precedence over Right and Up over Down.
        // Normalize before repeat ownership, retaining raw high stick bits.
        if held[NativeAction::Left] {
            held[NativeAction::Right] = false;
        }
        if held[NativeAction::Up] {
            held[NativeAction::Down] = false;
        }
        self.mouse_left_blocked &= self.mouse_left;
        self.blocked_dpad &= self.raw_dpad;
        self.dpad_held = self.raw_dpad & !self.blocked_dpad;
        if self.dpad_held & 0x80 != 0 {
            self.dpad_held &= !0x20;
        }
        if self.dpad_held & 0x10 != 0 {
            self.dpad_held &= !0x40;
        }
        for (index, value) in [
            self.left_stick.x,
            self.left_stick.y,
            self.right_stick.x,
            self.right_stick.y,
        ]
        .into_iter()
        .enumerate()
        {
            // Below-hysteresis holds can still move native byte consumers.
            if (value.clamp(-1., 1.) * 127.) as i8 == 0 {
                self.analog_blocked[index] = false;
            }
        }
        for (index, value) in [self.right_stick.x, self.right_stick.y]
            .into_iter()
            .enumerate()
        {
            // Existing shin-input stick hysteresis releases at0.4. Keep raw
            // analog values otherwise; this is a desktop flush boundary.
            if value.abs() < 0.4 {
                self.right_stick_blocked[index] = false;
            }
        }
        self.buttons = snapshot(&self.actions.update(held, elapsed));
        self.buttons
    }

    pub fn buttons(&self) -> Buttons {
        self.buttons
    }
    /// Original physical window coordinates. The application removes the
    /// letterbox offset and its owner-specific InputArea does the scaling.
    pub fn pointer_position(&self) -> Vec2 {
        self.pointer
    }
    pub fn mouse_left_held(&self) -> bool {
        self.focused && self.mouse_left && !self.mouse_left_blocked
    }
    /// Y-positive is up, matching RawInputState and the approved K/L mapping.
    pub fn right_stick(&self) -> Vec2 {
        if !self.focused {
            return Vec2::ZERO;
        }
        Vec2::new(
            if self.right_stick_blocked[0] {
                0.0
            } else {
                self.right_stick.x
            },
            if self.right_stick_blocked[1] {
                0.0
            } else {
                self.right_stick.y
            },
        )
    }
    pub fn right_stick_up(&self) -> bool {
        self.buttons.held & (1 << 20) != 0
    }
    pub fn analog(&self) -> Analog {
        if !self.focused {
            return Analog::default();
        }
        //995c0: raw Npad axes*127/32767, integer truncation; both Y bytes
        // are down-positive. RawInputState uses normalized, up-positive Y.
        Analog {
            axes: std::array::from_fn(|index| {
                if self.analog_blocked[index] {
                    0
                } else {
                    ([
                        self.left_stick.x,
                        -self.left_stick.y,
                        self.right_stick.x,
                        -self.right_stick.y,
                    ][index]
                        .clamp(-1., 1.)
                        * 127.) as i8
                }
            }),
            dpad_held: self.dpad_held,
        }
    }
    pub fn right_stick_down(&self) -> bool {
        self.buttons.held & (1 << 21) != 0
    }
    /// Desktop replacement/modal flush: a previously held control cannot
    /// create a new edge/repeat in its replacement until it is released.
    pub fn flush(&mut self) {
        for (action, blocked) in &mut self.blocked {
            *blocked |= self.raw[action];
        }
        self.mouse_left_blocked |= self.mouse_left;
        self.right_stick_blocked[0] |= self.right_stick.x != 0.0;
        self.right_stick_blocked[1] |= self.right_stick.y != 0.0;
        for (index, value) in [
            self.left_stick.x,
            self.left_stick.y,
            self.right_stick.x,
            self.right_stick.y,
        ]
        .into_iter()
        .enumerate()
        {
            self.analog_blocked[index] |= value != 0.;
        }
        self.blocked_dpad |= self.raw_dpad;
        self.dpad_held = 0;
        self.actions = ActionsState::new();
        self.buttons = Buttons::default();
    }
    pub fn focus_lost(&mut self) {
        self.set_focused(false);
    }
    /// The App must supply actual window focus, since unfocused background
    /// raw snapshots are intentionally blank. Lifecycle suspension may also
    /// call flush without changing this focus flag.
    pub fn set_focused(&mut self, focused: bool) {
        if self.focused && !focused {
            self.flush();
        }
        self.focused = focused;
    }
}

#[cfg(test)]
mod tests;
