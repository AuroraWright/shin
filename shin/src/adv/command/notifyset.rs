use super::prelude::*;

impl StartableCommand for command::runtime::NOTIFYSET {
    type StateInfo = ();
    fn apply_state(&self, state: &mut VmState) {
        state.information.notification_style = self.arg;
    }

    fn start(
        self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &VmState,
        _state_info: (),
        _adv_state: &mut AdvState,
    ) -> CommandStartResult {
        self.token.finish().into()
    }
}
