//! Scene-local GPU caches for original saved effect lower-prefix preparation.
//! Constructors use fixed native extents, independently of thumbnail size.
use super::effect as effects;
use crate::{
    adv::higurashi::group,
    layer::{
        higurashi_properties::HigurashiProperties, render_params::higurashi::HigurashiTransform,
    },
    render::PreRenderContext,
};
use anyhow::Result;
use parking_lot::Mutex;
use shin_core::primitives::color::UnormColor;
use shin_render::shaders::types::texture::{TextureSampler, TextureSource};
use shin_render::{
    render_texture::RenderTexture,
    resize::{SurfaceResizeSource, ViewportParams},
    shaders::types::texture::DepthStencilTarget,
};
use std::sync::Arc;

pub(crate) struct Depth {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
}
impl Depth {
    #[cfg(test)]
    pub(crate) fn actual_extent(&self) -> (u32, u32) {
        (self._texture.width(), self._texture.height())
    }

    fn new(context: &PreRenderContext, extent: (u32, u32)) -> Self {
        let texture = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Higurashi native effect depth"),
            size: wgpu::Extent3d {
                width: extent.0,
                height: extent.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: shin_render::DEPTH_STENCIL_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        Self {
            _texture: texture,
            view,
        }
    }
}
pub(crate) type SharedDepth = Arc<Mutex<Option<Depth>>>;

pub(crate) struct Cache {
    colors: Vec<RenderTexture>,
    depths: Vec<Depth>,
}
impl Cache {
    /// Observe allocated GPU textures for comparison to the native N events.
    #[cfg(test)]
    pub(crate) fn actual_resources(&self) -> Vec<(u32, u32, u32)> {
        self.colors
            .iter()
            .map(|texture| {
                let (width, height) = texture.dimensions();
                (0, width, height)
            })
            .chain(self.depths.iter().map(|depth| {
                let (width, height) = depth.actual_extent();
                (1, width, height)
            }))
            .collect()
    }

    pub fn new(context: &PreRenderContext, view: &effects::View) -> Self {
        let plan = view.resource_plan();
        let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
            plan.extent.0,
            plan.extent.1,
        )));
        Self {
            colors: (0..plan.color_count)
                .map(|index| {
                    RenderTexture::new(
                        context.device.clone(),
                        resize.handle(),
                        format!("Higurashi saved effect color{index}"),
                    )
                })
                .collect(),
            depths: (0..plan.depth_count)
                .map(|_| Depth::new(context, plan.extent))
                .collect(),
        }
    }
    pub fn source(&self, sampler: TextureSampler) -> TextureSource<'_> {
        let mut source = self.colors.last().unwrap().as_texture_source();
        source.sampler = sampler;
        source
    }
    pub fn prepare(
        &mut self,
        context: &mut PreRenderContext,
        view: &effects::View,
        properties: &HigurashiProperties,
        shared_depth: &SharedDepth,
        lower: &[group::DrawableChild<'_>],
        parent: &HigurashiTransform,
    ) -> Result<()> {
        let plan = view.capture_plan(properties);
        let depth = if plan.resources.shared_depth {
            shared_depth
                .lock()
                .get_or_insert_with(|| Depth::new(context, (1920, 1080)))
                .view
                .clone()
        } else {
            self.depths[0].view.clone()
        };
        {
            let mut pass = context.begin_pass(
                self.colors[0].as_texture_target(),
                Some(DepthStencilTarget { view: &depth }),
                "Higurashi effect lower-prefix capture",
            );
            pass.set_viewport(plan.viewport.0 as f32, plan.viewport.1 as f32);
            pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.));
            group::render_ordered_drawables(&mut pass, lower, parent)?;
        }
        if view.kind() == 5 {
            let (source, destination) = self.colors.split_at_mut(1);
            let mut pass = context.begin_pass(
                destination[0].as_texture_target(),
                Some(DepthStencilTarget {
                    view: &self.depths[1].view,
                }),
                "Higurashi Blur horizontal pass",
            );
            view.render_blur_stage0(&mut pass, source[0].as_texture_source(), properties)?;
        }
        Ok(())
    }
}
