use super::prelude::*;

#[derive(Debug)]
pub struct WIPEWAIT {
    token: Option<command::token::WIPEWAIT>,
}

impl StartableCommand for command::runtime::WIPEWAIT {
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
            WIPEWAIT {
                token: Some(self.token),
            }
            .into(),
        )
    }
}

impl UpdatableCommand for WIPEWAIT {
    fn update(
        &mut self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &mut VmState,
        adv_state: &mut AdvState,
        _is_fast_forwarding: bool,
    ) -> Option<CommandResult> {
        if adv_state.screen_layer().is_transition_active() {
            None
        } else {
            Some(self.token.take().unwrap().finish())
        }
    }
}
