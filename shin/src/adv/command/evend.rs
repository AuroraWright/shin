use super::prelude::*;

impl StartableCommand for command::runtime::EVEND {
    type StateInfo = ();
    fn apply_state(&self, state: &mut VmState) {
        state.commit_persistent();
        state.record_event_boundary();
        // Constructor 0x2a00 clears the event word to -1.
        state.current_event = None;
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
