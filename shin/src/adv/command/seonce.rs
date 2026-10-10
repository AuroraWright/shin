use shin_core::{
    format::scenario::{info::SeId, instruction_elements::FromNumber},
    vm::command::types::{AudioWaitStatus, Pan, Volume},
};

use super::prelude::*;

#[derive(Debug)]
pub struct SEONCE {
    token: Option<command::token::SEONCE>,
    slot: i32,
    unwanted_statuses: AudioWaitStatus,
}

impl StartableCommand for command::runtime::SEONCE {
    type StateInfo = ();

    fn apply_state(&self, _state: &mut VmState) {
        // Transient sound effects are not restored when replaying a scene.
    }

    fn start(
        self,
        context: &mut UpdateContext,
        scenario: &Arc<Scenario>,
        _vm_state: &VmState,
        _state_info: (),
        adv_state: &mut AdvState,
    ) -> CommandStartResult {
        let info = scenario.info_tables().se_info(SeId::from_number(self.arg1));
        let slot = adv_state.audio.se_player.request_once(
            context.asset_server.clone(),
            info.path().to_owned(),
            SeId::from_number(self.arg1),
            Volume::from_number(self.arg3),
            Pan::from_number(self.arg4),
        );
        let unwanted_statuses = AudioWaitStatus::from_number(self.arg2);
        if (adv_state.audio.se_player.get_once_wait_status(slot) & unwanted_statuses).is_empty() {
            self.token.finish().into()
        } else {
            Yield(
                SEONCE {
                    token: Some(self.token),
                    slot,
                    unwanted_statuses,
                }
                .into(),
            )
        }
    }
}

impl UpdatableCommand for SEONCE {
    fn update(
        &mut self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &mut VmState,
        adv_state: &mut AdvState,
        is_fast_forwarding: bool,
    ) -> Option<CommandResult> {
        // Native733c finishes on skip even after the one-shot has started;
        // playback remains owned by the SE player rather than being stopped.
        (is_fast_forwarding
            || (adv_state.audio.se_player.get_once_wait_status(self.slot) & self.unwanted_statuses)
                .is_empty())
        .then(|| self.token.take().unwrap().finish())
    }
}
