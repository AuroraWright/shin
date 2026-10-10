use super::prelude::*;

impl StartableCommand for command::runtime::TROPHY {
    type StateInfo = ();
    // Switch116930 only locks/unlocks a mutex; it does not acquire a trophy
    // or mutate the ADV save state. The command completes immediately.
    fn apply_state(&self, _state: &mut VmState) {}

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
