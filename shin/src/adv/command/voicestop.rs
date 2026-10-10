use super::prelude::*;

impl StartableCommand for command::runtime::VOICESTOP {
    type StateInfo = ();
    fn apply_state(&self, _state: &mut VmState) {}

    fn start(
        self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &VmState,
        _state_info: (),
        adv_state: &mut AdvState,
    ) -> CommandStartResult {
        adv_state.message_layer_mut().voice_player_mut().stop();
        self.token.finish().into()
    }
}
