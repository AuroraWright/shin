use super::prelude::*;

impl StartableCommand for command::runtime::RESUMESET {
    type StateInfo = ();

    fn apply_state(&self, state: &mut VmState) {
        // Constructor 0x2aa0 snapshots, increments 0x2db0, then sets 0x2dd4.
        state.commit_persistent();
        state.advance_save_position();
        state.resume_position = Some(state.save_position);
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
