use shin_core::{format::scenario::info::MaskIdOpt, vm::command::types::PlaneId};
use shin_render::shaders::types::RenderCloneCtx;

use super::prelude::*;

impl StartableCommand for command::runtime::MASKUNLOAD {
    type StateInfo = PlaneId;

    fn apply_state(&self, state: &mut VmState) -> PlaneId {
        // Native constructors 4290..42a4 capture the selected plane and clear
        // only its saved mask ID. Mask flags survive unloading.
        let plane = state.layers.current_plane;
        state.layers.plane_layergroups[plane].mask_id = MaskIdOpt::none();
        plane
    }

    fn start(
        self,
        context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &VmState,
        plane: PlaneId,
        adv_state: &mut AdvState,
    ) -> CommandStartResult {
        // d7f0 retains the first back root before clearing the captured plane
        // through 7ba70. Unlike MASKLOAD, animation permission is unchanged.
        adv_state.create_back_layer_group_if_needed(&mut context.pre_render.render_clone_ctx());
        adv_state.plane_layer_group_mut(plane).clear_mask_texture();
        self.token.finish().into()
    }
}

#[cfg(test)]
pub(super) mod tests;
