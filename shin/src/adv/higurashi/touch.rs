//! Updated root callbacks16bb4..17480, on the shared native contact owner.
use super::input::{Context, InputState, OverlayInput, Request, Services};
use crate::adv::input_area::{InputArea, InputAreaTarget, MouseRelease, PointerFrame};
use glam::Vec2;
use shin_core::time::Ticks;

/// The retained interactive owner supplies the original virtual58/60/78..90.
/// Look it up again for each callback: a probe does not retain the old owner.
pub trait Overlay: Send {
    /// Native Group ties equal priorities by the child's base owner ID.
    /// Ordinary ADV child0 constructors retain ID0.
    fn owner_id(&self) -> i32 {
        0
    }
    fn identity(&self) -> usize {
        0
    }
    fn input_permissions(&self) -> OverlayInput;
    fn probe(&mut self, position: Vec2) -> bool;
    fn press(&mut self, position: Vec2);
    fn release(&mut self, position: Vec2);
    fn cancel(&mut self);
    fn set_hidden(&mut self, _hidden: bool) {}
    fn input(&mut self, _permission: bool, _buttons: super::input::Buttons) {}
    fn advance(&mut self, _delta: Ticks) {}
    /// FakeSelect's native child retains ADV+a0 and queries/plays that same
    /// voice owner during its visual update, before the ordinary audio tick.
    fn advance_with_voice(
        &mut self,
        delta: Ticks,
        _voice: &mut dyn super::voice_commands::VoiceCommands,
    ) {
        self.advance(delta);
    }
    fn fast_forward(&mut self) {}
    fn take_cues(&mut self) -> Vec<&'static str> {
        Vec::new()
    }
    fn priority(&self) -> f32 {
        1000.0
    }
    fn render(
        &self,
        _pass: &mut shin_render::render_pass::RenderPass,
        _stencil: u8,
        _kind: shin_render::PassKind,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}
pub trait TouchServices: Services {
    fn current_context(&self, context: Context) -> Context {
        context
    }
    fn overlay_probe(&mut self, position: Vec2) -> bool;
    fn overlay_press(&mut self, position: Vec2);
    fn overlay_release(&mut self, position: Vec2);
    fn overlay_cancel(&mut self);
}

#[derive(Clone, Copy, Debug)]
pub struct State {
    pub selected: i32,
    pub pressed: i32,
    pub overlay: bool,
    pub opacity: f32,
}
impl Default for State {
    fn default() -> Self {
        Self {
            selected: -1,
            pressed: -1,
            overlay: false,
            opacity: 0.0,
        }
    }
}
pub struct RootTouch {
    area: InputArea,
    pub state: State,
    elapsed_seconds: f32,
}
impl Default for RootTouch {
    fn default() -> Self {
        Self {
            area: InputArea::new(1280, 720),
            state: State::default(),
            elapsed_seconds: 0.0,
        }
    }
}
impl RootTouch {
    pub(crate) fn cancel_input(&mut self, overlay: &mut Option<Box<dyn Overlay>>) {
        struct Cancellation<'a> {
            touch: &'a mut State,
            overlay: &'a mut Option<Box<dyn Overlay>>,
        }
        impl InputAreaTarget for Cancellation<'_> {
            fn probe(&mut self, _: Vec2) -> bool {
                unreachable!()
            }
            fn press(&mut self, _: Vec2) {
                unreachable!()
            }
            fn release(&mut self, _: Vec2) {
                unreachable!()
            }
            fn long_press(&mut self, _: Vec2) {
                unreachable!()
            }
            fn drag(&mut self, _: Vec2, _: Vec2) {
                unreachable!()
            }
            fn gesture(&mut self, _: Vec2) {
                unreachable!()
            }
            fn cancel(&mut self) {
                if self.touch.selected >= 0 {
                    self.touch.selected = -1;
                    self.touch.pressed = -1;
                } else if self.touch.overlay {
                    if let Some(overlay) = self.overlay {
                        overlay.cancel();
                    }
                }
            }
            fn reset(&mut self) {
                self.touch.selected = -1;
                self.touch.pressed = -1;
                self.touch.overlay = false;
            }
        }
        self.area.disable(&mut Cancellation {
            touch: &mut self.state,
            overlay,
        });
    }
    pub fn elapsed_seconds(&self) -> f32 {
        self.elapsed_seconds
    }
    pub fn process(
        &mut self,
        input: &mut InputState,
        context: Context,
        frame: &PointerFrame<'_>,
        services: &mut impl TouchServices,
    ) -> bool {
        self.process_with_mouse_release(input, context, frame, services, None)
    }
    pub(crate) fn process_with_mouse_release(
        &mut self,
        input: &mut InputState,
        context: Context,
        frame: &PointerFrame<'_>,
        services: &mut impl TouchServices,
        mouse_release: Option<MouseRelease>,
    ) -> bool {
        let mut target = Contact {
            touch: &mut self.state,
            input,
            context,
            services,
        };
        if !context.permission {
            self.area.disable(&mut target);
            false
        } else {
            self.area
                .update_with_mouse_release(&mut target, frame, mouse_release)
        }
    }
    /// Native12f44 follows the UI clocks even while the VM is paused.
    pub fn advance(&mut self, indicator: u8, delta: Ticks) {
        self.elapsed_seconds += delta.as_f32() * f32::from_bits(0x3c888889);
        let change = delta.as_f32() * 0.1;
        if indicator != 0 {
            self.state.opacity += change;
            if self.state.opacity > 1.0 {
                self.state.opacity = 1.0;
            }
        } else {
            self.state.opacity -= change;
            if self.state.opacity < 0.0 {
                self.state.opacity = 0.0;
            }
        }
    }
}
struct Contact<'a, S> {
    touch: &'a mut State,
    input: &'a mut InputState,
    context: Context,
    services: &'a mut S,
}
impl<S: TouchServices> Contact<'_, S> {
    fn available(&self) -> bool {
        let c = self.services.current_context(self.context);
        c.vm.is_some()
            && !c.disabled
            && !c.paused
            && !c.continuation_blocked
            && !c.overlay_transition_busy
            && !(c.overlay_transition_value > 0.0)
            && !c.notification_busy
    }
    fn message(&self) -> bool {
        self.services
            .current_context(self.context)
            .vm
            .is_some_and(|v| v.flags & 0x8000_0000 != 0)
    }
    fn toggle(&mut self, mask: u32) {
        self.input.modes ^= mask;
        self.services.request(if self.input.modes & mask != 0 {
            Request::CueEnabled
        } else {
            Request::CueDisabled
        });
    }
}
impl<S: TouchServices> InputAreaTarget for Contact<'_, S> {
    fn probe(&mut self, position: Vec2) -> bool {
        self.reset();
        if self.touch.opacity >= 1.0 && self.input.temporary_indicator != 0 {
            for (id, left, right, top, bottom) in [
                (0, 1332.0, 1524.0, 120.0, 240.0),
                (1, 1524.0, 1716.0, 120.0, 240.0),
                (2, 1716.0, 1908.0, 120.0, 240.0),
                (3, 1428.0, 1620.0, 252.0, 372.0),
                (4, 1620.0, 1812.0, 252.0, 372.0),
            ] {
                if position.x > left
                    && position.x < right
                    && position.y >= top
                    && position.y < bottom
                {
                    self.touch.selected = id;
                    return true;
                }
            }
        }
        if self.available() {
            self.touch.overlay = self.services.overlay_probe(position);
        }
        true
    }
    fn reset(&mut self) {
        self.touch.selected = -1;
        self.touch.pressed = -1;
        self.touch.overlay = false;
    }
    fn press(&mut self, position: Vec2) {
        if !self.available() {
            return;
        }
        if self.touch.selected >= 0 {
            self.touch.pressed = self.touch.selected;
        } else if self.touch.overlay {
            self.services.overlay_press(position);
        }
    }
    fn release(&mut self, position: Vec2) {
        if !self.available() {
            return;
        }
        if self.touch.selected >= 0 {
            self.touch.pressed = self.touch.selected;
            match self.touch.selected {
                0 => self.toggle(1),
                1 => self.toggle(2),
                2 if self.message() => self.services.request(Request::MessageBrowser),
                3 => self.services.request(Request::PreviousChapter),
                4 => self.services.request(Request::NextChapter),
                _ => {}
            }
        } else if self.touch.overlay {
            self.services.overlay_release(position);
        } else if !self.message() {
            if !self.services.current_context(self.context).hint_busy {
                self.services.request(Request::HintPulse);
            }
        } else if !self.services.click_message() && self.context.wait_allow_interrupt == 1 {
            self.services.interrupt_wait();
        }
    }
    fn long_press(&mut self, _: Vec2) {} // Actual1703c is a bare return.
    fn drag(&mut self, _: Vec2, _: Vec2) {
        if self.touch.selected >= 0 {
            self.touch.selected = -1;
            self.touch.pressed = -1;
        } else if self.touch.overlay {
            self.services.overlay_cancel();
            self.touch.overlay = false;
        }
    }
    fn gesture(&mut self, velocity: Vec2) {
        if velocity.length() <= 20.0 {
            return;
        }
        self.gesture_direction(velocity);
    }
    fn mouse_gesture(&mut self, displacement: Vec2, velocity: Vec2) {
        tracing::debug!(?displacement, ?velocity, "Higurashi desktop drag released");
        // Match Umineko's desktop fallback: a deliberate 64px authored drag
        // works independently of mouse Up timing and callback frequency.
        self.gesture(crate::adv::input_area::desktop_gesture_vector(
            displacement,
            velocity,
        ));
    }
    fn cancel(&mut self) {
        if self.touch.selected >= 0 {
            self.touch.selected = -1;
            self.touch.pressed = -1;
        } else if self.touch.overlay {
            self.services.overlay_cancel();
        }
    }
}
impl<S: TouchServices> Contact<'_, S> {
    fn gesture_direction(&mut self, velocity: Vec2) {
        let context = self.services.current_context(self.context);
        if self.context.system_locked || self.touch.selected >= 0 || !self.available() {
            return;
        }
        let angle = velocity.y.atan2(velocity.x) / f32::from_bits(0x40490fd8) * 180.0;
        if angle > -30.0 && angle < 30.0 {
            if !self.message() {
                return;
            }
            if self.input.temporary_indicator != 0 {
                self.input.temporary_indicator = 0;
                tracing::debug!("Higurashi touch toolbar closed");
                return;
            }
            if (context.overlay.is_some_and(|o| o.menus) || self.services.message_interested())
                && context.animations_enabled
            {
                self.input.modes = 0;
                self.services.request(Request::SystemMenu);
            }
        } else if angle > 150.0 || angle < -150.0 {
            if self.message() {
                self.input.temporary_indicator = 1;
                tracing::debug!("Higurashi touch toolbar opened");
            }
        } else if angle > 60.0 && angle < 120.0 {
            if self.message() {
                if context.overlay.is_some_and(|o| o.hide_window)
                    || self.services.message_interested()
                {
                    self.input.temporary_indicator = 0;
                    self.input.modes = 0;
                    self.services.request(Request::HideWindow);
                }
            } else if self
                .services
                .scene_jump_available(self.context.scene_jump_available)
            {
                self.services.request(Request::CueSceneJump);
                self.services.request(Request::SceneJump);
            }
        }
    }
}

#[cfg(test)]
mod tests;
