use enum_map::{EnumMap, enum_map};
use glam::{Vec2, vec2};
use indexmap::IndexMap;
use petitset::PetitSet;
use winit::{
    event::{ElementState, WindowEvent},
    keyboard::{KeyCode, PhysicalKey},
};

use crate::{
    gamepad_backend::{GamepadBackend, Gamepads},
    inputs::{GamepadAxis, GamepadButton, MouseButton, VirtualGamepadButton},
};

#[derive(Clone)]
pub struct MouseState {
    /// Mouse buttons state, simple state of each button
    pub buttons: EnumMap<MouseButton, bool>,
    pub position: Vec2,
    /// Accumulated vertical wheel displacement this frame, up-positive.
    /// One line is one unit; pixel scrolling uses 32 logical pixels per unit.
    pub scroll_amount: f32,
}

impl MouseState {
    pub fn new() -> Self {
        Self {
            buttons: enum_map! { _ => false },
            position: vec2(0.0, 0.0),
            scroll_amount: 0.0,
        }
    }
}

#[derive(Clone, Default)]
pub struct UnifiedGamepadState {
    pub buttons: EnumMap<GamepadButton, bool>,
    pub axes: EnumMap<GamepadAxis, f32>,
    pub virtual_keys: EnumMap<VirtualGamepadButton, bool>,
}

impl UnifiedGamepadState {
    pub fn new() -> Self {
        Self {
            buttons: enum_map! { _ => false },
            axes: enum_map! { _ => 0.0 },
            virtual_keys: enum_map! { _ => false },
        }
    }

    fn merge(&mut self, other: &Self) {
        self.buttons = self
            .buttons
            .map(|button, held| held || other.buttons[button]);
        self.virtual_keys = self
            .virtual_keys
            .map(|key, held| held || other.virtual_keys[key]);
        self.axes = self.axes.map(|axis, value| {
            let candidate = other.axes[axis];
            if candidate.abs() > value.abs() {
                candidate
            } else {
                value
            }
        });
    }

    fn update_virtual_keys(&mut self) {
        self.virtual_keys = self.virtual_keys.map(|key, prev_state| {
            let (axis, thresholds) = match key {
                VirtualGamepadButton::StickLUp => (GamepadAxis::LeftStickY, (0.4, 0.7)),
                VirtualGamepadButton::StickLDown => (GamepadAxis::LeftStickY, (-0.4, -0.7)),
                VirtualGamepadButton::StickLLeft => (GamepadAxis::LeftStickX, (-0.4, -0.7)),
                VirtualGamepadButton::StickLRight => (GamepadAxis::LeftStickX, (0.4, 0.7)),
                VirtualGamepadButton::StickRUp => (GamepadAxis::RightStickY, (0.4, 0.7)),
                VirtualGamepadButton::StickRDown => (GamepadAxis::RightStickY, (-0.4, -0.7)),
                VirtualGamepadButton::StickRLeft => (GamepadAxis::RightStickX, (-0.4, -0.7)),
                VirtualGamepadButton::StickRRight => (GamepadAxis::RightStickX, (0.4, 0.7)),
            };

            let threshold = if prev_state {
                thresholds.0
            } else {
                thresholds.1
            };
            let matches_threshold = if threshold > 0.0 {
                self.axes[axis] >= threshold
            } else {
                self.axes[axis] <= threshold
            };

            matches_threshold
        });
    }
}

#[derive(Clone)]
pub struct GamepadsState {
    pub gamepads: Gamepads,
    pub unified: UnifiedGamepadState,
}

impl GamepadsState {
    pub fn new() -> Self {
        Self {
            gamepads: IndexMap::new(),
            unified: UnifiedGamepadState::new(),
        }
    }

    pub fn is_held(&self, button: GamepadButton) -> bool {
        self.unified.buttons[button]
    }

    pub fn is_vheld(&self, axis: VirtualGamepadButton) -> bool {
        self.unified.virtual_keys[axis]
    }
}

pub struct RawInputAccumulator {
    gamepad_backend: Option<GamepadBackend>,
    state: RawInputState,
    mouse_scale_factor: f64,
}

impl RawInputAccumulator {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            gamepad_backend: GamepadBackend::new(),
            state: RawInputState::new(),
            mouse_scale_factor: 1.0,
        }
    }

    pub fn set_mouse_scale_factor(&mut self, scale_factor: f64) {
        if scale_factor.is_finite() && scale_factor > 0.0 {
            self.mouse_scale_factor = scale_factor;
        }
    }

    pub fn on_winit_event(&mut self, event: &WindowEvent) {
        let state = &mut self.state;

        match event {
            WindowEvent::KeyboardInput {
                event,
                // On some platforms, winit sends "synthetic" key press events when the window
                // gains or loses focus. These are not implemented on every platform, so we ignore
                // winit's synthetic key pressed and just reset keyboard state on unfocus.
                is_synthetic: false,
                ..
            } => {
                if let PhysicalKey::Code(keycode) = event.physical_key {
                    match event.state {
                        ElementState::Pressed => {
                            state.keyboard.insert(keycode);
                        }
                        ElementState::Released => {
                            state.keyboard.remove(&keycode);
                        }
                    }
                }
            }
            WindowEvent::Focused(false) => {
                state.keyboard.clear();
                state.mouse.buttons = EnumMap::default();
                state.mouse.scroll_amount = 0.0;
            }
            WindowEvent::CursorMoved { position, .. } => {
                state.mouse.position = vec2(position.x as f32, position.y as f32);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let amount = match delta {
                    &winit::event::MouseScrollDelta::LineDelta(_x, y) => y,
                    winit::event::MouseScrollDelta::PixelDelta(p) => {
                        (p.y / self.mouse_scale_factor / 32.0) as f32
                    }
                };
                if amount.is_finite() {
                    let total = state.mouse.scroll_amount + amount;
                    if total.is_finite() {
                        state.mouse.scroll_amount = total;
                    }
                    state.mouse.buttons[MouseButton::WheelUp] = state.mouse.scroll_amount > 0.0;
                    state.mouse.buttons[MouseButton::WheelDown] = state.mouse.scroll_amount < 0.0;
                }
            }
            &WindowEvent::MouseInput { button, state, .. } => {
                if let Some(button) = convert_winit_mouse_button(button) {
                    self.state.mouse.buttons[button] = match state {
                        ElementState::Pressed => true,
                        ElementState::Released => false,
                    }
                }
            }
            _ => {
                // don't care about other events
            }
        }
    }

    pub fn start_frame(&mut self) -> RawInputState {
        if let Some(backend) = &mut self.gamepad_backend {
            let gamepads = &mut self.state.gamepads;
            backend.read(&mut gamepads.gamepads);

            for (_, gamepad) in &mut gamepads.gamepads {
                gamepad.update_virtual_keys();
            }

            // fuse all gamepads into a single state
            gamepads.unified =
                gamepads
                    .gamepads
                    .values()
                    .fold(UnifiedGamepadState::new(), |mut acc, gamepad| {
                        acc.merge(gamepad);
                        acc
                    });
        }

        self.state.clone()
    }

    pub fn finish_frame(&mut self) {
        // NOTE: this should be done __after__ everything has handled the events
        self.state.mouse.scroll_amount = 0.0;
        self.state.mouse.buttons[MouseButton::WheelUp] = false;
        self.state.mouse.buttons[MouseButton::WheelDown] = false;
    }
}

#[derive(Clone)]
pub struct RawInputState {
    /// Keyboard state, set of pressed keys
    pub keyboard: PetitSet<KeyCode, 16>,
    pub mouse: MouseState,
    pub gamepads: GamepadsState,
    // TODO: touchscreen
    // need to figure out how exactly the game handles it first though
}

impl RawInputState {
    pub fn new() -> Self {
        Self {
            keyboard: PetitSet::new(),
            mouse: MouseState::new(),
            gamepads: GamepadsState::new(),
        }
    }
}

// impl Display for RawInputState {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         writeln!(f, "RawInputState: {{")?;
//         writeln!(
//             f,
//             "  keyboard: [{}]",
//             self.keyboard.iter().map(|v| format!("{:?}", v)).join(", ")
//         )?;
//         writeln!(
//             f,
//             "  mouse.buttons: [{}]",
//             self.mouse
//                 .buttons
//                 .iter()
//                 .filter_map(|(but, state)| state.then(|| format!("{:?}", but)))
//                 .join(", ")
//         )?;
//         writeln!(f, "}}")?;
//         Ok(())
//     }
// }

// impl OverlayVisitable for RawInputState {
//     fn visit_overlay(&self, collector: &mut crate::render::overlay::OverlayCollector) {
//         collector.overlay(
//             "Input State",
//             |_ctx, top_left| {
//                 top_left.label(format!(
//                     "Input State: [{}] [{}]",
//                     self.mouse_buttons
//                         .iter()
//                         .filter_map(|(but, state)| state.then(|| format!("{:?}", but)))
//                         .join(", "),
//                     self.keyboard.iter().map(|v| format!("{:?}", v)).join(", ")
//                 ));
//             },
//             true,
//         );
//     }
// }

#[inline]
fn convert_winit_mouse_button(winit: winit::event::MouseButton) -> Option<MouseButton> {
    match winit {
        winit::event::MouseButton::Left => Some(MouseButton::Left),
        winit::event::MouseButton::Right => Some(MouseButton::Right),
        winit::event::MouseButton::Middle => Some(MouseButton::Middle),
        // TODO: how should we model those?
        winit::event::MouseButton::Back | winit::event::MouseButton::Forward => None,
        winit::event::MouseButton::Other(_) => None,
    }
}

#[cfg(test)]
mod tests {
    fn wheel(
        input: &mut super::RawInputAccumulator,
        delta: winit::event::MouseScrollDelta,
        phase: winit::event::TouchPhase,
    ) {
        input.on_winit_event(&winit::event::WindowEvent::MouseWheel {
            device_id: winit::event::DeviceId::dummy(),
            delta,
            phase,
        });
    }
    #[test]
    fn trackpad_wheel_accumulates_retina_pixels_and_resets_each_frame() {
        use super::*;
        use winit::{
            dpi::PhysicalPosition,
            event::{MouseScrollDelta, TouchPhase},
        };
        let mut input = RawInputAccumulator {
            gamepad_backend: None,
            state: RawInputState::new(),
            mouse_scale_factor: 1.0,
        };
        input.set_mouse_scale_factor(2.0);
        // Many subpixel packets before redraw, then a large momentum packet.
        for _ in 0..16 {
            wheel(
                &mut input,
                MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, -0.5)),
                TouchPhase::Moved,
            );
        }
        wheel(
            &mut input,
            MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, -200.0)),
            TouchPhase::Moved,
        );
        assert_eq!(input.start_frame().mouse.scroll_amount, -3.25);
        input.finish_frame();
        assert_eq!(input.start_frame().mouse.scroll_amount, 0.0);
        // Horizontal-only/ended packets must never invent a downward scroll.
        wheel(
            &mut input,
            MouseScrollDelta::PixelDelta(PhysicalPosition::new(20.0, 0.0)),
            TouchPhase::Ended,
        );
        assert!(input.start_frame().mouse.buttons.values().all(|held| !held));
        wheel(
            &mut input,
            MouseScrollDelta::LineDelta(0.0, 0.25),
            TouchPhase::Moved,
        );
        wheel(
            &mut input,
            MouseScrollDelta::LineDelta(0.0, -0.125),
            TouchPhase::Moved,
        );
        assert_eq!(input.start_frame().mouse.scroll_amount, 0.125);
        assert!(input.start_frame().mouse.buttons[MouseButton::WheelUp]);
        assert!(!input.start_frame().mouse.buttons[MouseButton::WheelDown]);
        wheel(
            &mut input,
            MouseScrollDelta::LineDelta(0.0, f32::NAN),
            TouchPhase::Moved,
        );
        assert_eq!(input.start_frame().mouse.scroll_amount, 0.125);
    }
    #[test]
    fn trackpad_wheel_distance_is_independent_of_retina_scale_and_packet_batching() {
        use super::*;
        use winit::{
            dpi::PhysicalPosition,
            event::{MouseScrollDelta, TouchPhase},
        };
        for scale in [1.0, 2.0, 3.0] {
            for packets_per_frame in [1, 4, 16] {
                let mut input = RawInputAccumulator {
                    gamepad_backend: None,
                    state: RawInputState::new(),
                    mouse_scale_factor: scale,
                };
                let mut distance = 0.0;
                for _ in 0..32 / packets_per_frame {
                    for _ in 0..packets_per_frame {
                        wheel(
                            &mut input,
                            MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, -0.5 * scale)),
                            TouchPhase::Moved,
                        );
                    }
                    distance += input.start_frame().mouse.scroll_amount;
                    input.finish_frame();
                }
                assert_eq!(distance, -0.5);
            }
        }
    }
    #[test]
    fn losing_focus_clears_captured_mouse_buttons_and_scroll() {
        use super::*;
        let mut input = RawInputAccumulator {
            gamepad_backend: None,
            state: RawInputState::new(),
            mouse_scale_factor: 1.0,
        };
        input.state.keyboard.insert(KeyCode::ControlLeft);
        input.state.mouse.buttons[MouseButton::Left] = true;
        input.state.mouse.buttons[MouseButton::WheelUp] = true;
        input.state.mouse.scroll_amount = 3.0;
        input.on_winit_event(&WindowEvent::Focused(false));
        let frame = input.start_frame();
        assert!(frame.keyboard.is_empty());
        assert!(frame.mouse.buttons.values().all(|held| !held));
        assert_eq!(frame.mouse.scroll_amount, 0.0);
    }
    use super::*;
    #[test]
    fn merged_controllers_keep_signed_strongest_axes_and_button_unions() {
        let mut unified = UnifiedGamepadState::new();
        let mut first = UnifiedGamepadState::new();
        first.axes[GamepadAxis::RightStickY] = -0.8;
        first.buttons[GamepadButton::A] = true;
        first.update_virtual_keys();
        unified.merge(&first);
        assert_eq!(unified.axes[GamepadAxis::RightStickY], -0.8);
        let mut second = UnifiedGamepadState::new();
        second.axes[GamepadAxis::RightStickY] = 0.5;
        second.buttons[GamepadButton::B] = true;
        second.update_virtual_keys();
        unified.merge(&second);
        assert_eq!(unified.axes[GamepadAxis::RightStickY], -0.8);
        assert!(unified.buttons[GamepadButton::A] && unified.buttons[GamepadButton::B]);
        assert!(unified.virtual_keys[VirtualGamepadButton::StickRDown]);
        second.axes[GamepadAxis::RightStickY] = 0.9;
        unified.merge(&second);
        assert_eq!(unified.axes[GamepadAxis::RightStickY], 0.9);
    }
    #[test]
    fn signed_stick_thresholds_preserve_native_hysteresis() {
        let mut pad = UnifiedGamepadState::new();
        for (value, held) in [
            (-0.69, false),
            (-0.7, true),
            (-0.4, true),
            (-0.39, false),
            (0.7, false),
        ] {
            pad.axes[GamepadAxis::LeftStickX] = value;
            pad.update_virtual_keys();
            assert_eq!(pad.virtual_keys[VirtualGamepadButton::StickLLeft], held);
        }
        assert!(pad.virtual_keys[VirtualGamepadButton::StickLRight]);
    }
}
