//! Reads game controllers into per-controller states. gilrs reads them,
//! except on macOS: there, once another app uses a PlayStation or Xbox pad
//! through the GameController framework, raw HID clients such as gilrs stop
//! getting it until it reconnects, so the framework is polled directly (as
//! SDL does).

use indexmap::IndexMap;

use crate::raw_input_state::UnifiedGamepadState;

/// Controller states, by backend controller id.
pub type Gamepads = IndexMap<usize, UnifiedGamepadState>;

#[cfg(not(target_os = "macos"))]
pub use gilrs_backend::GamepadBackend;
#[cfg(target_os = "macos")]
pub use macos_backend::GamepadBackend;

#[cfg(not(target_os = "macos"))]
mod gilrs_backend {
    use gilrs::{EventType, Gilrs};
    use tracing::{error, warn};

    use super::Gamepads;
    use crate::{
        inputs::{GamepadAxis, GamepadButton},
        raw_input_state::UnifiedGamepadState,
    };

    pub struct GamepadBackend(Gilrs);

    impl GamepadBackend {
        pub fn new() -> Option<Self> {
            match Gilrs::new() {
                Ok(gilrs) => Some(Self(gilrs)),
                Err(gilrs::Error::NotImplemented(gilrs)) => {
                    warn!("Gamepad input is not implemented on this platform");
                    Some(Self(gilrs))
                }
                Err(err) => {
                    error!("Failed to initialize gamepad input: {:?}", err);
                    None
                }
            }
        }

        pub fn read(&mut self, gamepads: &mut Gamepads) {
            while let Some(event) = self.0.next_event() {
                let gamepad = gamepads.entry(event.id.into()).or_default();
                match event.event {
                    EventType::ButtonPressed(button, _) => {
                        if let Some(button) = GamepadButton::from_gilrs(button) {
                            gamepad.buttons[button] = true;
                        }
                    }
                    EventType::ButtonRepeated(_button, _) => {
                        // we do not handle repeated events, our input system has its own way of synthesizing these
                    }
                    EventType::ButtonReleased(button, _) => {
                        if let Some(button) = GamepadButton::from_gilrs(button) {
                            gamepad.buttons[button] = false;
                        }
                    }
                    EventType::ButtonChanged(_button, _value, _) => {
                        // we do not support analog buttons
                    }
                    EventType::AxisChanged(axis, value, _) => {
                        if let Some(axis) = GamepadAxis::from_gilrs(axis) {
                            gamepad.axes[axis] = value;
                        }
                    }
                    EventType::Connected | EventType::Disconnected => {
                        // reset the state just in case
                        *gamepad = UnifiedGamepadState::new();
                    }
                    EventType::Dropped | EventType::ForceFeedbackEffectCompleted => {
                        // nothing to do here
                    }
                    _ => {
                        // ignore events added in the future
                    }
                }
            }
        }
    }
}

#[cfg(target_os = "macos")]
mod macos_backend {
    use objc2_game_controller::{
        GCController, GCControllerButtonInput, GCControllerDirectionPad, GCDevice,
    };
    use tracing::info;

    use super::Gamepads;
    use crate::{
        inputs::{GamepadAxis, GamepadButton},
        raw_input_state::UnifiedGamepadState,
    };

    /// Polls `GCController.controllers`, which the main run loop (driven by
    /// winit) keeps up to date.
    pub struct GamepadBackend {
        /// Controller count when the controllers were last logged.
        logged: usize,
    }

    impl GamepadBackend {
        pub fn new() -> Option<Self> {
            Some(Self { logged: 0 })
        }

        pub fn read(&mut self, gamepads: &mut Gamepads) {
            gamepads.clear();
            // SAFETY: plain property reads of the framework's controller
            // objects, on the main thread.
            unsafe {
                let controllers = GCController::controllers();
                if controllers.count() != self.logged {
                    self.logged = controllers.count();
                    for controller in controllers.iter() {
                        let name = controller
                            .vendorName()
                            .map(|name| name.to_string())
                            .unwrap_or_default();
                        info!("Controller: {name}");
                    }
                }
                for (id, controller) in controllers.iter().enumerate() {
                    let Some(pad) = controller.extendedGamepad() else {
                        continue;
                    };
                    let mut state = UnifiedGamepadState::new();
                    let mut set = |button: GamepadButton, input: &GCControllerButtonInput| {
                        state.buttons[button] = input.isPressed()
                    };
                    // The framework's names follow the Xbox layout: buttonA
                    // is the bottom face button, like gilrs's South.
                    set(GamepadButton::A, &pad.buttonA());
                    set(GamepadButton::B, &pad.buttonB());
                    set(GamepadButton::X, &pad.buttonX());
                    set(GamepadButton::Y, &pad.buttonY());
                    set(GamepadButton::Plus, &pad.buttonMenu());
                    if let Some(button) = pad.buttonOptions() {
                        set(GamepadButton::Minus, &button);
                    }
                    set(GamepadButton::L, &pad.leftShoulder());
                    set(GamepadButton::R, &pad.rightShoulder());
                    set(GamepadButton::ZL, &pad.leftTrigger());
                    set(GamepadButton::ZR, &pad.rightTrigger());
                    if let Some(button) = pad.leftThumbstickButton() {
                        set(GamepadButton::StickL, &button);
                    }
                    if let Some(button) = pad.rightThumbstickButton() {
                        set(GamepadButton::StickR, &button);
                    }
                    let dpad = pad.dpad();
                    set(GamepadButton::Up, &dpad.up());
                    set(GamepadButton::Down, &dpad.down());
                    set(GamepadButton::Left, &dpad.left());
                    set(GamepadButton::Right, &dpad.right());
                    // Up is positive on both sticks, as with gilrs.
                    let stick = |stick: &GCControllerDirectionPad| {
                        (stick.xAxis().value(), stick.yAxis().value())
                    };
                    let (x, y) = stick(&pad.leftThumbstick());
                    state.axes[GamepadAxis::LeftStickX] = x;
                    state.axes[GamepadAxis::LeftStickY] = y;
                    let (x, y) = stick(&pad.rightThumbstick());
                    state.axes[GamepadAxis::RightStickX] = x;
                    state.axes[GamepadAxis::RightStickY] = y;
                    state.axes[GamepadAxis::LeftTrigger] = pad.leftTrigger().value();
                    state.axes[GamepadAxis::RightTrigger] = pad.rightTrigger().value();
                    gamepads.insert(id, state);
                }
            }
        }
    }
}
