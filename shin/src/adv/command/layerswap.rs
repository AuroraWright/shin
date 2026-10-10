use shin_core::vm::command::types::{LAYERS_COUNT, LayerId, PlaneId};
use shin_render::shaders::types::RenderCloneCtx;

use super::prelude::*;
use crate::adv::vm_state::layers::LayerOperationTargetList;

pub struct LayerSwapStateInfo {
    plane: PlaneId,
    targets: LayerOperationTargetList,
}

impl StartableCommand for command::runtime::LAYERSWAP {
    type StateInfo = LayerSwapStateInfo;

    fn apply_state(&self, state: &mut VmState) -> LayerSwapStateInfo {
        let plane = state.layers.current_plane;
        let mut info = LayerSwapStateInfo {
            plane,
            targets: Default::default(),
        };
        // b7e4..b804 unsigned-validates both raw IDs and skips equal IDs.
        if !(0..LAYERS_COUNT as i32).contains(&self.arg1)
            || !(0..LAYERS_COUNT as i32).contains(&self.arg2)
            || self.arg1 == self.arg2
        {
            return info;
        }
        let first = LayerId::new(self.arg1 as u16);
        let second = LayerId::new(self.arg2 as u16);
        state
            .layers
            .layerbank_allocator
            .swap_layerbanks(plane, first, second);
        // The native constructor queries banks AFTER swapping their mappings.
        // Resources and property values remain attached to the same bank.
        for layer in [first, second] {
            if let Some(layerbank) = state
                .layers
                .layerbank_allocator
                .get_layerbank_id(plane, layer)
            {
                state.layers.layerbanks[layerbank].layer_id = layer;
                info.targets
                    .push(crate::adv::vm_state::layers::LayerOperationTarget { layer, layerbank })
                    .unwrap();
            }
        }
        info
    }

    fn start(
        self,
        context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &VmState,
        info: LayerSwapStateInfo,
        adv_state: &mut AdvState,
    ) -> CommandStartResult {
        // b8c0 retains the back root even for an invalid/equal-ID command.
        adv_state.create_back_layer_group_if_needed(&mut context.pre_render.render_clone_ctx());
        let group = adv_state.plane_layer_group_mut(info.plane);
        for target in info.targets {
            group
                .get_layer_mut(target.layerbank)
                .expect("LAYERSWAP: allocated bank has no drawable layer")
                .properties_mut()
                .set_layer_id(target.layer);
        }
        self.token.finish().into()
    }
}

#[cfg(test)]
pub(super) mod tests;
