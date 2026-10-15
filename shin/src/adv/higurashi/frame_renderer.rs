//! Bound runtime renderer: every admitted ADV callback prepares immediately.
//! This desktop context owns pipeline/buffer services, not native cache colors.
use super::{frame::HigurashiFrame, scene_textures::HigurashiSceneTextures, touch::Overlay};
use crate::render::PreRenderContext;
use anyhow::{Result, ensure};
use shin_render::{
    dynamic_buffer::DynamicBuffer,
    pipelines::PipelineStorage,
    resize::{SurfaceResizeSource, ViewportParams},
    shaders::types::{
        buffer::BytesAddress,
        texture::{DepthStencilTarget, TextureSamplerStore},
    },
};

pub(crate) struct FrameRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    resize: SurfaceResizeSource,
    samplers: TextureSamplerStore,
    pipelines: PipelineStorage,
    dynamic: DynamicBuffer,
    // PreRenderContext requires a platform depth service even when all actual
    // Screen/cache passes supply their own native depth. This1x1 placeholder
    // is never attached; it does not represent a native allocation.
    _context_depth: wgpu::Texture,
    context_depth: wgpu::TextureView,
    screens: HigurashiSceneTextures,
}
impl FrameRenderer {
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &HigurashiFrame,
    ) -> Result<Self> {
        let extent = frame.cache.extent();
        let screens = HigurashiSceneTextures::for_scene(device, queue, extent, &frame.scene)?;
        ensure!(
            frame
                .scene
                .history()
                .with_graphics(|graphics| graphics.extent())?
                == extent,
            "Native ADV Screen/cache constructor extents must match before runtime binding"
        );
        let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
            extent[0], extent[1],
        )));
        let depth = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Higurashi desktop context unused depth"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: shin_render::DEPTH_STENCIL_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = depth.create_view(&Default::default());
        Ok(Self {
            device: device.clone(),
            queue: queue.clone(),
            resize,
            samplers: TextureSamplerStore::new(device),
            pipelines: PipelineStorage::new(device.clone(), shin_render::TEXTURE_FORMAT),
            dynamic: DynamicBuffer::new(device.clone(), BytesAddress::new(1024 * 1024)),
            _context_depth: depth,
            context_depth: view,
            screens,
        })
    }
    pub(crate) fn prepare(
        &mut self,
        frame: &HigurashiFrame,
        overlay: Option<&dyn Overlay>,
    ) -> Result<()> {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Complete Higurashi ADV callback prepare"),
            });
        let result = {
            let mut context = PreRenderContext {
                device: &self.device,
                queue: &self.queue,
                breakup_texture: None,
                resize_source: &self.resize,
                sampler_store: &self.samplers,
                depth_stencil: DepthStencilTarget {
                    view: &self.context_depth,
                },
                pipeline_storage: &mut self.pipelines,
                dynamic_buffer: &mut self.dynamic,
                encoder: &mut encoder,
            };
            self.screens
                .prepare_frame_with_overlay(&mut context, frame, overlay)
        };
        let mut uploads = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Higurashi ADV callback buffer uploads"),
            });
        // The shared staging belt's copies must precede every draw using
        // these uniforms/vertices, as in the main engine render submission.
        self.dynamic.finish(&mut uploads);
        self.queue.submit([uploads.finish(), encoder.finish()]);
        self.dynamic.recall();
        let _ = self.device.poll(wgpu::PollType::Poll);
        result
    }
}
