use super::prelude::*;

#[derive(Debug)]
pub struct MSGSYNC {
    token: Option<command::token::MSGSYNC>,
    voice_index: i32,
    sync_time: i32,
}

impl StartableCommand for command::runtime::MSGSYNC {
    type StateInfo = ();
    fn apply_state(&self, _state: &mut VmState) {}

    fn start(
        self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &VmState,
        _state_info: (),
        _adv_state: &mut AdvState,
    ) -> CommandStartResult {
        Yield(
            MSGSYNC {
                token: Some(self.token),
                voice_index: self.voice_index,
                sync_time: self.sync_time,
            }
            .into(),
        )
    }
}

impl UpdatableCommand for MSGSYNC {
    fn update(
        &mut self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &mut VmState,
        adv_state: &mut AdvState,
        _is_fast_forwarding: bool,
    ) -> Option<CommandResult> {
        // Native57c0 delegates to MessageLayer b19b0 even when skipping.
        if adv_state
            .message_layer()
            .voice_sync_is_waiting(self.voice_index, self.sync_time)
        {
            None
        } else {
            Some(self.token.take().unwrap().finish())
        }
    }
}
