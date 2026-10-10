use shin_core::vm::command::types::AudioWaitStatus;

use super::prelude::*;

#[derive(Debug)]
pub struct BGMWAIT {
    token: Option<command::token::BGMWAIT>,
    unwanted_statuses: AudioWaitStatus,
}

impl StartableCommand for command::runtime::BGMWAIT {
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
            BGMWAIT {
                token: Some(self.token),
                unwanted_statuses: self.unwanted_statuses,
            }
            .into(),
        )
    }
}

impl UpdatableCommand for BGMWAIT {
    fn update(
        &mut self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &mut VmState,
        adv_state: &mut AdvState,
        _is_fast_forwarding: bool,
    ) -> Option<CommandResult> {
        if (adv_state.audio.bgm_player.get_wait_status() & self.unwanted_statuses).is_empty() {
            Some(self.token.take().unwrap().finish())
        } else {
            None
        }
    }
}
