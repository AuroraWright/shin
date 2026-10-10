use shin_core::{
    format::scenario::info::MaskIdOpt,
    vm::command::types::{MaskFlags, PlaneId},
};
use shin_render::shaders::types::RenderCloneCtx;
use shin_tasks::AsyncTask;

use super::prelude::*;
use crate::asset::mask::MaskTexture;

#[derive(Debug)]
pub struct MASKLOAD {
    token: Option<command::token::MASKLOAD>,
    plane: PlaneId,
    flags: MaskFlags,
    load_task: AsyncTask<Arc<MaskTexture>>,
}

impl StartableCommand for command::runtime::MASKLOAD {
    type StateInfo = ();

    fn apply_state(&self, state: &mut VmState) {
        let layers = &mut state.layers;

        layers.plane_layergroups[layers.current_plane].mask_id = self.mask_id;
        layers.plane_layergroups[layers.current_plane].mask_flags = self.mask_flags;
    }

    fn start(
        self,
        context: &mut UpdateContext,
        scenario: &Arc<Scenario>,
        vm_state: &VmState,
        _state_info: (),
        adv_state: &mut AdvState,
    ) -> CommandStartResult {
        adv_state.create_back_layer_group_if_needed(&mut context.pre_render.render_clone_ctx());

        let mask_info = self
            .mask_id
            .repr()
            .and_then(|id| scenario.info_tables().mask_info_opt(id));
        let Some(mask_info) = mask_info else {
            adv_state
                .plane_layer_group_mut(vm_state.layers.current_plane)
                .clear_mask_texture();
            adv_state.allow_running_animations = true;
            return self.token.finish().into();
        };

        // Native d004/d0a4 tests the third argument before suspending layer
        // animation. Completion d23c unconditionally restores animation.
        if !self.smth_smth_transition {
            adv_state.allow_running_animations = false;
        }
        let load_task = shin_tasks::async_io::spawn({
            let asset_server = context.asset_server.clone();
            let path = mask_info.path();

            async move {
                asset_server
                    .load(&path)
                    .await
                    .expect("Failed to load mask texture")
            }
        });

        Yield(
            MASKLOAD {
                token: Some(self.token),
                plane: vm_state.layers.current_plane,
                flags: self.mask_flags,
                load_task,
            }
            .into(),
        )
    }
}

impl UpdatableCommand for MASKLOAD {
    fn update(
        &mut self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &mut VmState,
        adv_state: &mut AdvState,
        _is_fast_forwarding: bool,
    ) -> Option<CommandResult> {
        let mask_texture = self.load_task.poll_naive()?;

        adv_state
            .plane_layer_group_mut(self.plane)
            .set_mask_texture(mask_texture, self.flags);
        adv_state.allow_running_animations = true;

        Some(self.token.take().unwrap().finish())
    }
}

#[cfg(test)]
pub(super) mod tests;
