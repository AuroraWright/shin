use shin_core::format::scenario::Scenario;

use super::prelude::*;

impl StartableCommand for command::runtime::SGET {
    type StateInfo = i32;

    fn apply_state(&self, state: &mut VmState) -> i32 {
        let value = state.get_persistent(self.slot_number);
        state.route.record_persistent(value);
        value
    }

    fn start(
        self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &VmState,
        value: i32,
        _adv_state: &mut AdvState,
    ) -> CommandStartResult {
        self.token.finish(value).into()
    }
}
