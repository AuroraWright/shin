use super::prelude::*;
use crate::adv::vm_state::layers::LayerOperationTargetList;
use shin_core::vm::command::types::PlaneId;

#[derive(Debug)]
pub struct TRANSWAIT {
    token: Option<command::token::TRANSWAIT>,
    layer_id: VLayerId,
    targets: Option<TransitionWaitTargets>,
}

#[derive(Debug)]
struct TransitionWaitTargets {
    plane: PlaneId,
    layers: LayerOperationTargetList,
}

impl TransitionWaitTargets {
    // Native c290 captures the plane and allocated banks on the first poll.
    fn capture(state: &mut VmState, layer_id: VLayerId) -> Self {
        let layers = &mut state.layers;
        let plane = layers.current_plane;
        let targets = match layer_id.repr() {
            VLayerIdRepr::Selected => layers.layerbank_allocator.layers_in_range(
                plane,
                layers.layer_selection.from,
                layers.layer_selection.to,
            ),
            VLayerIdRepr::Layer(id) => layers.layerbank_allocator.layers_in_range(plane, id, id),
            _ => LayerOperationTargetList::new(),
        };
        Self {
            plane,
            layers: targets,
        }
    }

    fn is_waiting(
        &self,
        mut has_wiper: impl FnMut(shin_core::vm::command::types::LayerbankId) -> bool,
    ) -> bool {
        self.layers.iter().any(|target| has_wiper(target.layerbank))
    }
}

impl StartableCommand for command::runtime::TRANSWAIT {
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
            TRANSWAIT {
                token: Some(self.token),
                layer_id: self.layer_id,
                targets: None,
            }
            .into(),
        )
    }
}

impl UpdatableCommand for TRANSWAIT {
    fn update(
        &mut self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        vm_state: &mut VmState,
        adv_state: &mut AdvState,
        _is_fast_forwarding: bool,
    ) -> Option<CommandResult> {
        let targets = self
            .targets
            .get_or_insert_with(|| TransitionWaitTargets::capture(vm_state, self.layer_id));
        let group = adv_state.plane_layer_group(targets.plane);
        if targets.is_waiting(|bank| group.has_wiper_for_layerbank(bank)) {
            None
        } else {
            Some(self.token.take().unwrap().finish())
        }
    }
}

#[cfg(test)]
pub(super) mod tests;
