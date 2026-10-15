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
    load_task: Option<AsyncTask<anyhow::Result<Arc<MaskTexture>>>>,
    path: String,
    block_animations: bool,
    error: Option<anyhow::Error>,
}

fn spawn_loader(
    server: Arc<crate::asset::system::AssetServer>,
    path: String,
) -> AsyncTask<anyhow::Result<Arc<MaskTexture>>> {
    shin_tasks::async_io::spawn(async move {
        super::catch_preparation_panic(async { server.load(&path).await }).await?
    })
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
        let path = mask_info.path();
        let load_task = spawn_loader(context.asset_server.clone(), path.clone());

        Yield(
            MASKLOAD {
                token: Some(self.token),
                plane: vm_state.layers.current_plane,
                flags: self.mask_flags,
                load_task: Some(load_task),
                path,
                block_animations: !self.smth_smth_transition,
                error: None,
            }
            .into(),
        )
    }
}

impl UpdatableCommand for MASKLOAD {
    fn take_error(&mut self) -> Option<anyhow::Error> {
        self.error.take()
    }
    fn update(
        &mut self,
        context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &mut VmState,
        adv_state: &mut AdvState,
        _is_fast_forwarding: bool,
    ) -> Option<CommandResult> {
        if self.error.is_some() {
            return None;
        }
        if self.load_task.is_none() {
            if self.block_animations {
                adv_state.allow_running_animations = false;
            }
            self.load_task = Some(spawn_loader(
                context.asset_server.clone(),
                self.path.clone(),
            ));
        }
        let result = self.load_task.as_mut().unwrap().poll_naive()?;
        self.load_task = None;
        let mask_texture = match result {
            Ok(mask) => mask,
            Err(error) => {
                self.error = Some(error);
                adv_state.allow_running_animations = true;
                return None;
            }
        };

        adv_state
            .plane_layer_group_mut(self.plane)
            .set_mask_texture(mask_texture, self.flags);
        adv_state.allow_running_animations = true;

        Some(self.token.take().unwrap().finish())
    }
}

#[cfg(test)]
pub(super) mod tests;
