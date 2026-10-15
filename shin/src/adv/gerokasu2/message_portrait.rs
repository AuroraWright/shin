//! MSGFACE GPU acquisition, generation-checked publication and frame ownership.
use std::sync::Arc;

use anyhow::Result;
use futures::FutureExt;
use glam::{Mat4, Vec4, vec3};
use parking_lot::Mutex;
use shin_core::{
    primitives::{color::FloatColor4, update::FrameId},
    time::Ticks,
    vm::gerokasu2::message_face::{MessagePortrait, PortraitChange, PortraitJob, PortraitRequest},
};
use shin_render::{
    LayerBlendType, LayerFragmentShader, PassKind, RenderRequestBuilder, render_pass::RenderPass,
};

use super::{message_clock::MessageClock, voice::Gerokasu2VoicePlayer};
use crate::{
    asset::{
        bustup::{BustupArgs, CharacterId, gerokasu2::Gerokasu2Bustup},
        system::AssetServer,
    },
    layer::{
        render_params::{DrawableParams, multiply_transforms},
        user::{BustupLayerImpl, PictureBlockParams},
    },
};

pub struct PortraitLayer {
    request: PortraitRequest,
    layer: BustupLayerImpl,
    _asset: Arc<Gerokasu2Bustup>,
}
impl PortraitLayer {
    pub fn request(&self) -> &PortraitRequest {
        &self.request
    }

    /// The six installed properties, with all other transform properties at
    /// their native neutral values. The default depth1000 composes to depth0.
    fn local_transform(&self) -> Mat4 {
        placement_transform(&self.request)
    }

    fn update(&mut self, ticks: Ticks, voice: &Gerokasu2VoicePlayer) {
        // Native3af60 projects this layer alone when notifying the voice owner.
        let x = self.local_transform().w_axis.x;
        let pan = (x * (1.0 / 960.0)).clamp(-1.0, 1.0);
        let level = voice.character_lipsync(self.layer.character_id(), pan);
        self.layer.update_portrait(ticks.as_f32(), level);
    }

    fn render(&self, pass: &mut RenderPass, parent: Mat4, stencil: u8, kind: PassKind) {
        let drawable = DrawableParams {
            color_multiplier: FloatColor4::WHITE,
            blend_type: LayerBlendType::Type1,
            fragment_shader: LayerFragmentShader::Default,
            shader_param: Vec4::ZERO,
        };
        let params = PictureBlockParams::setup(kind, &drawable).unwrap();
        self.layer.render_gerokasu2(
            pass,
            RenderRequestBuilder::new().depth_stencil_shorthand(stencil, false, false),
            params,
            multiply_transforms(parent, self.local_transform()),
        );
    }
}

pub(crate) fn placement_transform(request: &PortraitRequest) -> Mat4 {
    let scale = request.scale as f32 * 0.001;
    let position = vec3(
        -960.0 - request.negated_translation_x as f32,
        540.0 - request.negated_translation_y as f32,
        0.0,
    );
    multiply_transforms(
        Mat4::from_translation(position),
        Mat4::from_scale(vec3(scale, scale, 1.0)),
    )
}

type SharedLayer = Arc<Mutex<PortraitLayer>>;

/// Acquisition checks the generation before IO, after GPU/cache acquisition
/// and again under the publication lock. A failed load retains the old owner.
async fn acquire(job: PortraitJob<SharedLayer>, assets: Arc<AssetServer>) -> Result<bool> {
    if !job.is_current() {
        return Ok(false);
    }
    let request = job.request().clone();
    let asset = assets
        .load_with_args::<Gerokasu2Bustup, _>(
            &request.path,
            BustupArgs {
                expression: request.expression.clone(),
                character_id: CharacterId::new(request.lipsync_character_id),
                disable_animations: false,
            },
        )
        .await?;
    if !job.is_current() {
        return Ok(false);
    }
    let layer = BustupLayerImpl::new_gerokasu2(
        asset.0.clone(),
        format!("{}[{}]", request.path, request.expression),
        crate::engine_random::next_seed(),
    );
    Ok(job.publish(Arc::new(Mutex::new(PortraitLayer {
        request,
        layer,
        _asset: asset,
    }))))
}

struct Load {
    generation: u32,
    task: shin_tasks::AsyncTask<Result<bool>>,
}

pub struct Gerokasu2MessagePortrait {
    holder: MessagePortrait<SharedLayer>,
    /// Native8ee9c copies the published resource into the render owner on update.
    active: Option<SharedLayer>,
    loads: Vec<Load>,
    updated_frame: Option<FrameId>,
    last_error: Option<String>,
}
impl Default for Gerokasu2MessagePortrait {
    fn default() -> Self {
        Self {
            holder: MessagePortrait::new(),
            active: None,
            loads: Vec::new(),
            updated_frame: None,
            last_error: None,
        }
    }
}
impl Gerokasu2MessagePortrait {
    pub fn apply(&mut self, assets: Arc<AssetServer>, change: PortraitChange) {
        if change != PortraitChange::Unchanged {
            self.last_error = None;
        }
        if let Some(job) = self.holder.apply(change) {
            self.loads.push(Load {
                generation: job.generation(),
                task: shin_tasks::async_io::spawn(acquire(job, assets)),
            });
        }
    }

    pub fn generation(&self) -> u32 {
        self.holder.generation()
    }
    pub fn published_request(&self) -> Option<PortraitRequest> {
        self.holder
            .visible()
            .map(|layer| layer.lock().request.clone())
    }
    pub fn active_request(&self) -> Option<PortraitRequest> {
        self.active
            .as_ref()
            .map(|layer| layer.lock().request.clone())
    }
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    pub fn poll_loads(&mut self) {
        self.loads.retain_mut(|load| {
            let Some(result) = (&mut load.task).now_or_never() else {
                return true;
            };
            if let Err(error) = result {
                if load.generation == self.holder.generation() {
                    let error = format!("{error:#}");
                    tracing::error!(%error, "Gerokasu2 message portrait load failed");
                    self.last_error = Some(error);
                }
            }
            false
        });
    }

    pub fn update(&mut self, frame: FrameId, ticks: Ticks, voice: &Gerokasu2VoicePlayer) {
        self.poll_loads();
        if self.updated_frame == Some(frame) {
            return;
        }
        self.updated_frame = Some(frame);
        self.active = self.holder.visible();
        if let Some(layer) = &self.active {
            layer.lock().update(ticks, voice);
        }
    }

    /// Native8effc and8f460 forward the parent directly, excluding MessageLayer
    /// local text/window geometry. Transparent mode2 suppresses the portrait.
    pub fn render(
        &self,
        pass: &mut RenderPass,
        clock: &MessageClock,
        parent: Mat4,
        stencil: u8,
        kind: PassKind,
    ) {
        if clock.slide * clock.modal_slide < 1.0
            || (kind == PassKind::Transparent && clock.params.mode == 2)
        {
            return;
        }
        if let Some(layer) = &self.active {
            layer
                .lock()
                .render(pass, parent, stencil.wrapping_add(1), kind);
        }
    }
}

#[cfg(test)]
mod tests;
