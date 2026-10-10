use std::fmt::{Debug, Formatter};

use super::prelude::*;
use crate::layer::message_layer::{MessageFlags, MsgsetParams};

pub struct MSGSET {
    #[allow(unused)]
    token: Option<command::token::MSGSET>,
}

impl StartableCommand for command::runtime::MSGSET {
    type StateInfo = ();
    fn apply_state(&self, state: &mut VmState) {
        // CPU replay has no renderer. Keep its snapshot at this boundary;
        // the live dispatcher captures only after install_message returns.
        apply_state_before_install(self, state);
        state.capture_autosave();
    }

    fn start(
        self,
        context: &mut UpdateContext,
        scenario: &Arc<Scenario>,
        vm_state: &VmState,
        _state_info: (),
        adv_state: &mut AdvState,
    ) -> CommandStartResult {
        install_message(&self, context, scenario, vm_state, adv_state, false);
        initial_wait(self, adv_state)
    }
}

pub(super) fn apply_state_before_install(message: &command::runtime::MSGSET, state: &mut VmState) {
    // Event messages share their event's snapshot (constructor 0x5140).
    if state.current_event.is_none() {
        state.commit_persistent();
    }
    state.record_message_boundary();
    // TODO: think about async messages (those where you would use MSGWAIT)
    state.messagebox_state.text = Some(message.text.clone());
    state.messagebox_state.message_id = message.msg_id;
    state.messagebox_state.was_read = (message.msg_id.0 as i32) < 0
        || state
            .save_vectors
            .seen_messages
            .contains(message.msg_id.0 as usize);
    state.messagebox_state.received_syncs = 0;
    state.messagebox_state.messagebox_shown = true;
    state.messagebox_state.close_inhibited = !message.auto_wait;
}

pub(super) fn start_with_skip(
    message: command::runtime::MSGSET,
    context: &mut UpdateContext,
    scenario: &Arc<Scenario>,
    vm_state: &mut VmState,
    adv_state: &mut AdvState,
    is_fast_forwarding: bool,
) -> CommandStartResult {
    install_message(
        &message,
        context,
        scenario,
        vm_state,
        adv_state,
        is_fast_forwarding,
    );
    // Native first poll 0x54cc captures after text/window installation and
    // before its first wait query. Repeated polls do not capture again.
    vm_state.capture_autosave();
    initial_wait(message, adv_state)
}

fn install_message(
    message: &command::runtime::MSGSET,
    context: &mut UpdateContext,
    scenario: &Arc<Scenario>,
    vm_state: &VmState,
    adv_state: &mut AdvState,
    is_fast_forwarding: bool,
) {
    adv_state
        .message_layer_mut()
        .set_settings(&vm_state.settings);
    adv_state.root_layer_group.message_layer_mut().on_msgset(
        context.pre_render,
        scenario,
        &message.text,
        MsgsetParams {
            flags: {
                let mut flags = MessageFlags::empty();

                if vm_state.current_event.is_some() {
                    flags |= MessageFlags::IGNORE_INPUT;
                }
                if vm_state.messagebox_state.was_read {
                    flags |= MessageFlags::READ;
                }

                flags
            },
            messagebox_type: vm_state.messagebox_state.msginit.messagebox_type,
            text_layout: vm_state.messagebox_state.msginit.text_layout,
            message_id: message.msg_id,
        },
        !is_fast_forwarding,
    );
}

fn initial_wait(message: command::runtime::MSGSET, adv_state: &AdvState) -> CommandStartResult {
    if message.auto_wait && adv_state.message_layer().recv_sync_is_waiting(-1) {
        Yield(
            MSGSET {
                token: Some(message.token),
            }
            .into(),
        )
    } else {
        message.token.finish().into()
    }
}

impl UpdatableCommand for MSGSET {
    fn update(
        &mut self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &mut VmState,
        adv_state: &mut AdvState,
        // TODO: I think we __should__ somehow to react to fast-forwarding here
        // rn it's kludged in the ADV update function though
        _is_fast_forwarding: bool,
    ) -> Option<CommandResult> {
        if adv_state
            .root_layer_group
            .message_layer()
            .recv_sync_is_waiting(-1)
        {
            None
        } else {
            Some(self.token.take().unwrap().finish())
        }
    }
}

impl Debug for MSGSET {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("MSGSET").finish()
    }
}
