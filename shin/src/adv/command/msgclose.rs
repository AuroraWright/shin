use super::prelude::*;

#[derive(Debug)]
pub struct MSGCLOSE {
    token: Option<command::token::MSGCLOSE>,
    wait_for_close: bool,
    started: bool,
}

impl StartableCommand for command::runtime::MSGCLOSE {
    type StateInfo = bool;
    fn apply_state(&self, state: &mut VmState) -> bool {
        let inhibited = state.messagebox_state.close_inhibited;
        if !inhibited {
            state.messagebox_state.messagebox_shown = false;
            state.messagebox_state.text = None;
        }
        inhibited
    }

    fn start(
        self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &VmState,
        inhibited: bool,
        _adv_state: &mut AdvState,
    ) -> CommandStartResult {
        if inhibited {
            return self.token.finish().into();
        }
        Yield(
            MSGCLOSE {
                token: Some(self.token),
                wait_for_close: self.wait_for_close,
                started: false,
            }
            .into(),
        )
    }
}

impl UpdatableCommand for MSGCLOSE {
    fn update(
        &mut self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &mut VmState,
        adv_state: &mut AdvState,
        is_fast_forwarding: bool,
    ) -> Option<CommandResult> {
        // Native5880 closes once on the first poll, snapping only when that
        // poll permits skip. Later polls wait on the actual message windows.
        if !self.started {
            adv_state.message_layer_mut().close(!is_fast_forwarding);
            self.started = true;
        }
        if self.wait_for_close && adv_state.message_layer().close_is_waiting() {
            None
        } else {
            Some(self.token.take().unwrap().finish())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shin_core::{
        format::scenario::{
            instruction_elements::{MessageId, NumberSpec, U8Bool},
            types::U16FixupString,
        },
        vm::{
            IntoRuntimeForm, VmCtx,
            command::{CompiletimeCommand as C, compiletime as c},
        },
    };
    #[test]
    fn asynchronous_message_inhibits_close_until_negative_wait() {
        for auto_wait in [false, true] {
            let mut state = VmState::for_scenario(0, 42);
            let apply = |command: C, state: &mut VmState| {
                crate::adv::command::apply_command_state(
                    command.into_runtime_form(&VmCtx::new(0, 42)),
                    state,
                );
            };
            apply(
                C::MSGSET(c::MSGSET {
                    msg_id: MessageId(1),
                    auto_wait: U8Bool(auto_wait),
                    text: U16FixupString::new("Keep"),
                }),
                &mut state,
            );
            apply(
                C::MSGWAIT(c::MSGWAIT {
                    signal_num: NumberSpec::constant(0),
                }),
                &mut state,
            );
            apply(
                C::MSGCLOSE(c::MSGCLOSE {
                    wait_for_close: U8Bool(true),
                }),
                &mut state,
            );
            assert_eq!(
                state.messagebox_state.text.as_deref(),
                if auto_wait { None } else { Some("Keep") }
            );
            assert_eq!(state.messagebox_state.messagebox_shown, !auto_wait);
            for signal in [-1, -2] {
                apply(
                    C::MSGWAIT(c::MSGWAIT {
                        signal_num: NumberSpec::constant(signal),
                    }),
                    &mut state,
                );
                assert!(!state.messagebox_state.close_inhibited);
                apply(
                    C::MSGCLOSE(c::MSGCLOSE {
                        wait_for_close: U8Bool(false),
                    }),
                    &mut state,
                );
                assert!(state.messagebox_state.text.is_none());
                assert!(!state.messagebox_state.messagebox_shown);
            }
        }
    }
}
