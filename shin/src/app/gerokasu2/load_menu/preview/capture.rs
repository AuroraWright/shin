//! D61E0/D7870: private960x540 scene, then480x270 and336x189 opaque copies.
use crate::{
    adv::gerokasu2::{root::Gerokasu2Root, save_menu::preview::capture_extents},
    asset::system::AssetServer,
    layer::{Layer, render_params::TransformParams},
    render::PreRenderContext,
    update::{AdvUpdatable, AdvUpdateContext},
};
use anyhow::Result;
use glam::{vec2, vec3};
use shin_core::{
    primitives::{color::UnormColor, update::FrameId},
    time::Ticks,
};
use shin_render::{
    ColorBlendType, DrawPrimitive, PassKind, RenderProgramWithArguments, RenderRequestBuilder,
    render_texture::RenderTexture,
    resize::{SurfaceResizeSource, ViewportParams},
    shaders::types::{
        buffer::VertexSource, texture::DepthStencilTarget, vertices::PosColTexVertex,
    },
};
use std::sync::Arc;

pub(super) struct Capture {
    resize: SurfaceResizeSource,
    color: RenderTexture,
    half: RenderTexture,
    depth: wgpu::TextureView,
}

fn resize(size: [u32; 2]) -> SurfaceResizeSource {
    SurfaceResizeSource::new(ViewportParams::both((size[0], size[1]).into()))
}
fn texture(device: &wgpu::Device, size: [u32; 2], label: &str) -> RenderTexture {
    RenderTexture::new(device.clone(), resize(size).handle(), label.into())
}
pub(super) fn depth(device: &wgpu::Device, size: [u32; 2]) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("Ger private preview depth"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: shin_render::DEPTH_STENCIL_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&Default::default())
}
impl Capture {
    pub(super) fn new(device: &wgpu::Device) -> Self {
        let extents = capture_extents([336, 189]);
        let resize = resize(extents[0]);
        Self {
            color: RenderTexture::new(device.clone(), resize.handle(), "Ger preview scene".into()),
            half: texture(device, extents[1], "Ger preview half"),
            depth: depth(device, extents[0]),
            resize,
        }
    }

    pub(super) fn capture(
        &mut self,
        scene: Option<&mut Gerokasu2Root>,
        assets: &Arc<AssetServer>,
        context: &mut PreRenderContext,
    ) -> Result<RenderTexture> {
        // A published row retains its pixels while another row is constructed.
        // Allocate a distinct final owner; intermediate targets are reusable.
        let mut output = texture(context.device, [336, 189], "Ger saved thumbnail");
        let Some(scene) = scene else {
            let mut pass = context.begin_pass(
                output.as_texture_target(),
                None,
                "Ger preview failed worker",
            );
            pass.clear(Some(UnormColor::BLACK), None, None);
            drop(pass);
            return Ok(output);
        };
        let mut frame = FrameId::default();
        frame.advance();
        scene.update(&AdvUpdateContext {
            frame_id: frame,
            delta_ticks: Ticks::ZERO,
            asset_server: assets,
            device: context.device,
            queue: context.queue,
            are_animations_allowed: true,
            voice_callback: None,
            movie_volume_settings: Default::default(),
        });
        let mut private = PreRenderContext {
            device: context.device,
            breakup_texture: context.breakup_texture,
            queue: context.queue,
            resize_source: &self.resize,
            sampler_store: context.sampler_store,
            depth_stencil: DepthStencilTarget { view: &self.depth },
            pipeline_storage: context.pipeline_storage,
            dynamic_buffer: context.dynamic_buffer,
            encoder: context.encoder,
        };
        let transform = TransformParams::default();
        scene.pre_render(&mut private, &transform);
        {
            // D7870 passes flags4 to AD720: bit2 enables stencil Always/Replace
            // with reference0; bit1 (depth Always/write) remains disabled.
            // Old stencil would suppress this scene's opaque layers.
            let mut pass = private.begin_pass_preserving_depth_stencil(
                self.color.as_texture_target(),
                private.depth_stencil,
                "Ger private preview root",
            );
            pass.clear(None, Some(0), None);
            for kind in [PassKind::Opaque, PassKind::Transparent] {
                scene.render(&mut pass, &transform, 0, kind);
            }
        }
        scene.check_render()?;
        copy(&mut private, &self.color, &mut self.half);
        copy(&mut private, &self.half, &mut output);
        Ok(output)
    }
}

fn copy(context: &mut PreRenderContext, source: &RenderTexture, target: &mut RenderTexture) {
    let vertices = [(0., 0.), (2., 0.), (0., 2.)].map(|(x, y)| PosColTexVertex {
        position: vec3(x, y, 0.),
        color: UnormColor::WHITE,
        texture_position: vec2(x, y),
    });
    let mut pass = context.begin_pass(
        target.as_texture_target(),
        None,
        "Ger preview opaque downsample",
    );
    pass.run(
        RenderRequestBuilder::new()
            .color_blend_type(ColorBlendType::Opaque)
            .build(
                RenderProgramWithArguments::Sprite {
                    vertices: VertexSource::VertexData {
                        vertices: &vertices,
                    },
                    sprite: source.as_texture_source(),
                    transform: crate::render::normalized_projection_matrix(),
                },
                DrawPrimitive::Triangles,
            ),
    );
}
