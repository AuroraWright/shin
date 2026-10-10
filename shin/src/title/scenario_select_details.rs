//! Native details target278, snapshot2d8, and composition dde5c..de100.
use super::scenario_select::ScenarioEntry;
use crate::{adv::character_view::CharacterTextures, render::PreRenderContext};
use glam::Mat4;
use shin_core::primitives::color::UnormColor;
use shin_render::{
    ColorBlendType, RenderRequestBuilder,
    render_pass::RenderPass,
    shaders::types::texture::{TextureSampler, TextureSource, TextureTarget, TextureTargetKind},
};

struct Capture {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}
impl Capture {
    fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ScenarioSelect details capture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: shin_render::TEXTURE_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        Self { texture, view }
    }
    fn source(&self) -> TextureSource<'_> {
        TextureSource {
            view: &self.view,
            sampler: TextureSampler::Linear,
        }
    }
}

pub(super) struct ScenarioDetails {
    current: Capture,
    previous: Option<Capture>,
}
impl ScenarioDetails {
    /// Native constructor allocates ceil(canvas scale * 1824/726), once.
    pub fn new(ctx: &PreRenderContext, canvas: winit::dpi::PhysicalSize<u32>) -> Self {
        Self {
            current: Capture::new(
                ctx.device,
                (canvas.width as f32 / 1920.0 * 1824.0).ceil() as u32,
                (canvas.height as f32 / 1080.0 * 726.0).ceil() as u32,
            ),
            previous: None,
        }
    }
    /// Consume CaptureDetails before recomposing this frame. The source is the
    /// last completed composite, including any interrupted previous fade.
    pub fn capture_previous(&mut self, ctx: &mut PreRenderContext) {
        let size = self.current.texture.size();
        let previous = Capture::new(ctx.device, size.width, size.height);
        ctx.encoder.copy_texture_to_texture(
            self.current.texture.as_image_copy(),
            previous.texture.as_image_copy(),
            size,
        );
        self.previous = Some(previous);
    }
    pub fn pre_render(
        &mut self,
        atlas: &CharacterTextures,
        entry: &ScenarioEntry,
        previous_fade: f32,
        ctx: &mut PreRenderContext,
    ) {
        let mut pass = ctx.begin_pass(
            TextureTarget {
                kind: TextureTargetKind::RenderTexture,
                view: &self.current.view,
            },
            None,
            "ScenarioSelect details",
        );
        let projection =
            shin_render::shin_orthographic_projection_matrix(0.0, 1824.0, 726.0, 0.0, -1.0, 1.0);
        shin_render::quad_vertices::QuadVertices::new()
            .with_box(0.0, 0.0, 1824.0, 726.0)
            .with_tex_box(0.025, 0.20185184, 0.97499996, 0.87407404)
            .with_color(UnormColor::WHITE)
            .render_sprite(
                &mut pass,
                RenderRequestBuilder::new().color_blend_type(ColorBlendType::Opaque),
                atlas.get("bg").as_source(),
                projection,
            );
        shin_render::quad_vertices::QuadVertices::new()
            .with_box(528.0, 15.0, 1814.0, 711.0)
            .with_tex_box(0.0, 0.0, 1.0, 1.0)
            .with_color(UnormColor::WHITE)
            .render_sprite(
                &mut pass,
                RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1),
                atlas.get(&entry.details_name()).as_source(),
                projection,
            );
        if previous_fade > 0.0 {
            if let Some(previous) = &self.previous {
                // Native56840 truncates positive RGBA*255 and saturates to u8.
                let alpha = (previous_fade.max(0.0) * 255.0).min(255.0) as u8;
                shin_render::quad_vertices::QuadVertices::new()
                    .with_box(0.0, 0.0, 1824.0, 726.0)
                    .with_tex_box(0.0, 0.0, 1.0, 1.0)
                    .with_color(UnormColor::from_rgba(255, 255, 255, alpha))
                    .render_sprite(
                        &mut pass,
                        RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1),
                        previous.source(),
                        projection,
                    );
            }
        }
    }
    pub fn source(&self) -> TextureSource<'_> {
        self.current.source()
    }
    pub fn render(&self, pass: &mut RenderPass, transform: Mat4) {
        shin_render::quad_vertices::QuadVertices::new()
            .with_box(48.0, 218.0, 1872.0, 944.0)
            .with_tex_box(0.0, 0.0, 1.0, 1.0)
            .with_color(UnormColor::WHITE)
            .render_sprite(
                pass,
                RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1),
                self.current.source(),
                transform,
            );
    }
}

#[cfg(test)]
#[path = "scenario_select_details_reference.rs"]
mod reference;
