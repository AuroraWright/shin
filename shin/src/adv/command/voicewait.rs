use shin_core::vm::command::types::AudioWaitStatus;

use super::prelude::*;

#[derive(Debug)]
pub struct VOICEWAIT {
    token: Option<command::token::VOICEWAIT>,
    unwanted_statuses: AudioWaitStatus,
}

impl StartableCommand for command::runtime::VOICEWAIT {
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
            VOICEWAIT {
                token: Some(self.token),
                unwanted_statuses: self.unwanted_statuses,
            }
            .into(),
        )
    }
}

impl UpdatableCommand for VOICEWAIT {
    fn update(
        &mut self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &mut VmState,
        adv_state: &mut AdvState,
        _is_fast_forwarding: bool,
    ) -> Option<CommandResult> {
        let mut message = adv_state.message_layer_mut();
        let player = message.voice_player_mut();
        player.update(shin_core::time::Ticks::ZERO);
        if (player.get_wait_status() & self.unwanted_statuses).is_empty() {
            Some(self.token.take().unwrap().finish())
        } else {
            None
        }
    }
}
