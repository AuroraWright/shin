//! Updated ADV13140 logical input decisions. Physical bindings, mode-stack
//! ownership, preferences and modal services are supplied by the host.

#[derive(Debug, Clone, Copy)]
pub struct ReadState {
    pub flags: u32,
    pub fast_forward_blocked: u8,
    pub is_read: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageRequests {
    pub autoplay: bool,
    pub fast_forward: bool,
}

/// Native137d0, evaluated from the VM state at the command callback.
pub fn suppress_transient_audio(
    vm: Option<ReadState>,
    modes: u32,
    forced: bool,
    read_only: bool,
) -> bool {
    vm.is_some_and(|vm| {
        vm.flags & 0x8000_0000 != 0
            && vm.fast_forward_blocked == 0
            && (forced || (modes & 2 != 0 && (!read_only || vm.is_read != 0)))
    })
}

/// Native12c44/12fc8/13018 after command polling. A blocked unread skip
/// may still request autoplay; eligible skip takes precedence over autoplay.
pub fn message_requests(
    vm: Option<ReadState>,
    modes: u32,
    forced: bool,
    read_only: bool,
) -> MessageRequests {
    let eligible = vm.is_some_and(|v| v.flags & 0x8000_0000 != 0 && v.fast_forward_blocked == 0);
    let fast_forward = suppress_transient_audio(vm, modes, forced, read_only);
    MessageRequests {
        autoplay: eligible && !fast_forward && modes & 1 != 0,
        fast_forward,
    }
}

#[derive(Debug, Clone, Copy)]
pub struct InputState {
    /// ADV3fc: low bits are auto/skip; preserve other bits on toggles.
    pub modes: u32,
    /// ADV464, cleared when entering HiddenWindow.
    pub temporary_indicator: u8,
}

/// Logical snapshot99b40. ADV13140 reads pressed and regular repeat only.
#[derive(Debug, Default, Clone, Copy)]
pub struct Buttons {
    pub held: u32,
    pub pressed: u32,
    pub repeat: u32,
    pub rapid_repeat: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct OverlayInput {
    /// Native virtual58 allows SystemMenu/MessageBrowser requests.
    pub menus: bool,
    /// Native virtual60 allows HiddenWindow requests.
    pub hide_window: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct Context {
    pub permission: bool,
    pub captured: bool,
    pub disabled: bool,
    pub system_locked: bool,
    pub vm: Option<ReadState>,
    pub paused: bool,
    pub continuation_blocked: bool,
    pub overlay_transition_busy: bool,
    pub overlay_transition_value: f32,
    pub notification_busy: bool,
    pub animations_enabled: bool,
    pub overlay: Option<OverlayInput>,
    pub hint_busy: bool,
    pub scene_jump_available: bool,
    pub wait_allow_interrupt: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    FlushInput,
    CueEnabled,
    CueDisabled,
    CueSceneJump,
    NextChapter,
    PreviousChapter,
    HintPulse,
    SceneJump,
    SystemMenu,
    MessageBrowser,
    PreviousMessage,
    HideWindow,
}

pub trait Services {
    fn request(&mut self, request: Request);
    fn click_message(&mut self) -> bool;
    fn message_interested(&self) -> bool;
    fn interrupt_wait(&mut self);
    /// Actual11664 runs only after the event/B input gates. Pure decision
    /// fixtures may supply their existing resolved availability snapshot.
    fn scene_jump_available(&mut self, resolved: bool) -> bool { resolved }
}

/// Return value is passed to Overlay virtual70. It does not gate VM polling.
pub fn process(
    state: &mut InputState,
    c: Context,
    b: Buttons,
    services: &mut impl Services,
) -> bool {
    use Request::*;
    let pressed = |bit: u32| b.pressed & (1u32 << bit) != 0;
    if !c.permission {
        services.request(FlushInput);
        return false;
    }
    if c.captured {
        return false;
    }
    if !c.disabled {
        for (bit, mask) in [(10, 1), (11, 2)] {
            if pressed(bit) {
                if c.system_locked {
                    return false;
                }
                state.modes ^= mask;
                services.request(if state.modes & mask != 0 {
                    CueEnabled
                } else {
                    CueDisabled
                });
            }
        }
        if state.modes != 0 && b.pressed & 0x0200_9000 != 0 {
            if c.system_locked {
                return false;
            }
            state.modes = 0;
            services.request(CueDisabled);
            return false;
        }
    }
    let Some(vm) = c.vm else {
        return false;
    };
    if c.disabled
        || c.paused
        || c.continuation_blocked
        || c.overlay_transition_busy
        || c.overlay_transition_value > 0.0
        || c.notification_busy
    {
        return false;
    }
    if pressed(9) || b.repeat & (1 << 8) != 0 {
        if !c.system_locked {
            services.request(if pressed(9) {
                NextChapter
            } else {
                PreviousChapter
            });
        }
        return false;
    }
    if vm.flags & 0x8000_0000 == 0 {
        if pressed(24) {
            if !c.system_locked && !c.hint_busy {
                services.request(HintPulse);
            }
            return false;
        }
        if pressed(25) {
            if !c.system_locked && services.scene_jump_available(c.scene_jump_available) {
                services.request(CueSceneJump);
                services.request(SceneJump);
            }
            return false;
        }
        return true;
    }
    fn hide(state: &mut InputState, services: &mut impl Services) {
        state.temporary_indicator = 0;
        services.request(Request::HideWindow);
    }
    if let Some(overlay) = c.overlay {
        if overlay.menus {
            if pressed(15) && c.animations_enabled {
                services.request(SystemMenu);
                return false;
            }
            if pressed(12) {
                services.request(MessageBrowser);
                return false;
            }
        }
        if overlay.hide_window && pressed(25) {
            hide(state, services);
            return false;
        }
        return true;
    }
    let confirm = b.pressed & 0x0100_0040 != 0;
    if confirm && services.click_message() {
        return false;
    }
    if services.message_interested() {
        if b.repeat & (1 << 4) != 0 {
            services.request(PreviousMessage);
            return false;
        }
        if pressed(25) {
            hide(state, services);
            return false;
        }
        if pressed(15) && c.animations_enabled {
            services.request(SystemMenu);
            return false;
        }
        if pressed(12) {
            services.request(MessageBrowser);
            return false;
        }
        return true;
    }
    if c.wait_allow_interrupt == 1 && confirm {
        services.interrupt_wait();
        return false;
    }
    true
}

#[cfg(test)]
mod tests;
