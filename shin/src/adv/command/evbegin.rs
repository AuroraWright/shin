use super::prelude::*;

impl StartableCommand for command::runtime::EVBEGIN {
    type StateInfo = ();
    fn apply_state(&self, state: &mut VmState) {
        state.commit_persistent();
        state.record_event_boundary();
        // Constructor 0x28a0 stores arg in VM event word 0x2ddc.
        state.current_event = (self.arg >= 0).then_some(self.arg);
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
