use super::prelude::*;
use crate::adv::vm_state::layers::LayerTransitionSettings;

impl StartableCommand for command::runtime::TRANSSET {
    type StateInfo = ();

    fn apply_state(&self, state: &mut VmState) {
        state.layers.transition = LayerTransitionSettings {
            ty: self.ty,
            flags: self.flags,
            duration: Ticks::from_f32(self.duration as f32),
            params: self.params,
        };
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
