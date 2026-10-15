//! Updated13140 pre-poll input and12c44 post-poll auto/skip ownership.
use super::*;
use crate::adv::higurashi::{
    input::{self, Context, InputState, OverlayInput, Request},
    read_state::ReadServices,
    ui_state::HigurashiUiState,
};
use shin_core::vm::command::types::Volume;
use std::collections::VecDeque;

/// Root contact/optional-overlay owners publish these before logical input.
/// These are explicit native boundaries until their complete UI is attached.
#[derive(Default, Clone, Copy)]
pub(super) struct Boundary {
    pub captured: bool,
    pub disabled: bool,
    pub forced: bool,
    pub overlay: Option<OverlayInput>,
    pub scene_jump_available: bool,
}
pub(super) struct Controls {
    pub state: InputState,
    pub boundary: Boundary,
    pub requests: VecDeque<Request>,
    pub flush_input: bool,
    pub desktop_held_skip: bool,
    temporary_skip_active: bool,
    pub desktop_mouse_release: Option<crate::adv::input_area::MouseRelease>,
    #[cfg(test)]
    pub(super) reference_cues: Vec<Request>,
}
impl Default for Controls {
    fn default() -> Self {
        Self {
            state: InputState {
                modes: 0,
                temporary_indicator: 0,
            },
            boundary: Default::default(),
            requests: VecDeque::new(),
            flush_input: false,
            desktop_held_skip: false,
            temporary_skip_active: false,
            desktop_mouse_release: None,
            #[cfg(test)]
            reference_cues: Vec::new(),
        }
    }
}
impl Controls {
    #[cfg(test)]
    pub fn process(
        &mut self,
        host: &mut HigurashiHost,
        bindings: &mut impl ReadServices,
        permission: bool,
        buttons: Buttons,
        policy: Policy,
        quick_return: bool,
    ) -> Result<Vec<Request>> {
        let available = self.boundary.scene_jump_available;
        self.process_with_scene_jump(
            host,
            bindings,
            permission,
            buttons,
            policy,
            quick_return,
            move |_, _| Ok(available),
        )
    }
    #[cfg(test)]
    pub fn process_with_scene_jump<S: ReadServices>(
        &mut self,
        host: &mut HigurashiHost,
        bindings: &mut S,
        permission: bool,
        buttons: Buttons,
        policy: Policy,
        quick_return: bool,
        mut probe: impl FnMut(&HigurashiRunner, &S) -> Result<bool>,
    ) -> Result<Vec<Request>> {
        self.process_with_pointer(
            host,
            bindings,
            permission,
            buttons,
            None,
            policy,
            quick_return,
            &mut probe,
        )
    }
    pub fn process_with_pointer<S: ReadServices>(
        &mut self,
        host: &mut HigurashiHost,
        bindings: &mut S,
        permission: bool,
        buttons: Buttons,
        pointer: Option<&PointerFrame<'_>>,
        policy: Policy,
        quick_return: bool,
        mut probe: impl FnMut(&HigurashiRunner, &S) -> Result<bool>,
    ) -> Result<Vec<Request>> {
        let context = Context {
            permission: permission && self.requests.is_empty(),
            captured: self.boundary.captured,
            disabled: self.boundary.disabled,
            system_locked: quick_return,
            vm: None,
            paused: policy.paused,
            continuation_blocked: false,
            overlay_transition_busy: false,
            overlay_transition_value: 0.0,
            notification_busy: false,
            animations_enabled: policy.animations_enabled,
            overlay: self.boundary.overlay,
            hint_busy: false,
            scene_jump_available: self.boundary.scene_jump_available,
            wait_allow_interrupt: 0,
        };
        let native =
            host.runner.native_vm_attached() || host.runner.reconstruction_phase().is_some();
        if native {
            host.runner.flush_message_completions(bindings)?;
        }
        let mut jump_probe = |runner: &HigurashiRunner| probe(runner, &*bindings);
        let mouse_release = self.desktop_mouse_release.take();
        let outcome = if let Some(pointer) = pointer {
            host.process_desktop_input_with_pointer_and_scene_jump(
                &mut self.state,
                context,
                buttons,
                pointer,
                &mut jump_probe,
                mouse_release,
            )?
        } else {
            host.process_input_with_scene_jump(&mut self.state, context, buttons, &mut jump_probe)?
        };
        if native {
            host.runner.flush_message_completions(bindings)?;
        }
        let mut cues = Vec::new();
        for request in outcome.requests {
            if !matches!(request, Request::FlushInput) {
                tracing::debug!(?request, "Higurashi input request");
            }
            match request {
                //1315c/13240 calls5eb0c for ADV+30 only. The host already
                //cancelled that root contact owner; a modal's live pointer
                //and held controls belong to its own InputArea.
                Request::FlushInput => {}
                Request::CueEnabled | Request::CueDisabled | Request::CueSceneJump => {
                    #[cfg(test)]
                    self.reference_cues.push(request);
                    cues.push(request)
                }
                other => self.requests.push_back(other),
            }
        }
        Ok(cues)
    }
    pub fn cue(request: Request, sounds: &mut SystemSoundPlayer, server: &AssetServer) {
        let name = match request {
            Request::CueEnabled => "son",
            Request::CueDisabled => "cancel",
            Request::CueSceneJump => "decide",
            _ => unreachable!("only native cues"),
        };
        sounds.play(server, name, Volume(1.0));
    }
    /// Desktop Ctrl borrows the native read/skip policy without changing latched modes.
    pub fn allow_temporary_skip(&mut self, controls_allowed: bool) {
        self.temporary_skip_active = self.desktop_held_skip
            && controls_allowed
            && !self.boundary.captured
            && !self.boundary.disabled
            && self.requests.is_empty();
    }
    pub fn temporary_skip_modes(&self) -> u32 {
        if self.temporary_skip_active { 2 } else { 0 }
    }
    pub fn read_policy(&self, read_only: bool) -> ReadPolicy {
        ReadPolicy {
            modes: self.state.modes | self.temporary_skip_modes(),
            forced: self.boundary.forced,
            read_only,
        }
    }
}
pub(super) fn post_poll(
    runner: &mut HigurashiRunner,
    audio: &mut crate::adv::higurashi::audio::HigurashiAudio,
    ui: &mut HigurashiUiState,
    policy: ReadPolicy,
) -> Result<()> {
    let request = input::message_requests(
        runner.native_read_state(),
        policy.modes,
        policy.forced,
        policy.read_only,
    );
    if request.fast_forward {
        runner.fast_forward_visuals(audio);
        audio.fast_forward_voice_and_effects()?;
        ui.indicator.fast_forward();
        ui.menu.fast_forward();
        runner.clear_wait_timer_for_skip();
    } else if request.autoplay {
        runner.request_message_update(true, false);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
