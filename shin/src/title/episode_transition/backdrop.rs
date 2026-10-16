//! Native ScenarioSelectBridge four-level silhouette blur and cover.
use crate::title::{
    episode_transition::{EpisodeFrame, capture_sizes},
    episode_transition::view::render_doors,
};
use crate::render::{PreRenderContext, VIRTUAL_CANVAS_SIZE_VEC, top_left_projection_matrix};
use glam::vec4;
use shin_core::primitives::color::UnormColor;
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    quad_vertices::{QuadVertices, build_quad_vertices},
    render_pass::RenderPass,
    shaders::types::{
        buffer::VertexSource,
        texture::{TextureSampler, TextureSource, TextureTarget, TextureTargetKind},
        vertices::PosTexVertex,
    },
};

struct Capture {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}
impl Capture {
    fn new(ctx: &PreRenderContext, width: u32, height: u32) -> Self {
        let texture = ctx.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Episode transition blur capture"),
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
                | wgpu::TextureUsages::COPY_SRC,
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
    fn target(&self) -> TextureTarget<'_> {
        TextureTarget {
            view: &self.view,
            kind: TextureTargetKind::RenderTexture,
        }
    }
}

pub(crate) struct EpisodeBackdrop {
    levels: [(Capture, Capture); 4],
}
impl EpisodeBackdrop {
    pub fn new(ctx: &PreRenderContext, extent: (u16, u16)) -> Self {
        Self {
            levels: capture_sizes(extent)
                .map(|(w, h)| (Capture::new(ctx, w, h), Capture::new(ctx, w, h))),
        }
    }
    pub fn pre_render(
        &self,
        prior: TextureSource<'_>,
        frame: EpisodeFrame,
        ctx: &mut PreRenderContext,
    ) {
        let vertices = build_quad_vertices(|uv| PosTexVertex {
            position: uv * VIRTUAL_CANVAS_SIZE_VEC,
            texture_position: uv,
        });
        for (mask, scratch) in &self.levels {
            {
                let mut pass = ctx.begin_pass(mask.target(), None, "Episode door silhouette");
                pass.clear(Some(UnormColor::WHITE), None, None);
                render_doors(prior, frame, UnormColor::BLACK, &mut pass);
            }
            for (source, target, step) in [
                (
                    mask,
                    scratch,
                    vec4(1.5 / mask.texture.width() as f32, 0.0, 0.0, 0.0),
                ),
                (
                    scratch,
                    mask,
                    vec4(0.0, 1.5 / mask.texture.height() as f32, 0.0, 0.0),
                ),
            ] {
                let mut pass = ctx.begin_pass(target.target(), None, "Episode silhouette blur");
                pass.run(
                    RenderRequestBuilder::new()
                        .color_blend_type(ColorBlendType::Opaque)
                        .build(
                            RenderProgramWithArguments::Blur {
                                vertices: VertexSource::VertexData {
                                    vertices: &vertices,
                                },
                                texture: source.source(),
                                transform: top_left_projection_matrix(),
                                step,
                            },
                            DrawPrimitive::TrianglesStrip,
                        ),
                );
            }
        }
    }
    pub fn render(&self, prior: TextureSource<'_>, frame: EpisodeFrame, pass: &mut RenderPass) {
        let quad = QuadVertices::new()
            .with_box(0.0, 0.0, 1920.0, 1080.0)
            .with_tex_box(0.0, 0.0, 1.0, 1.0);
        // e44b0 fills white before drawing textured doors. This also provides
        // the gap outside the projected door halves.
        quad.clone().with_color(UnormColor::WHITE).fill(
            pass,
            RenderRequestBuilder::new().color_blend_type(ColorBlendType::Opaque),
            top_left_projection_matrix(),
        );
        render_doors(prior, frame, UnormColor::WHITE, pass);
        for ((mask, _), weight) in self.levels.iter().zip([17u8, 34, 68, 136]) {
            quad.clone()
                .with_color(UnormColor::from_rgba(weight, weight, weight, 255))
                .render_sprite(
                    pass,
                    RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer2),
                    mask.source(),
                    top_left_projection_matrix(),
                );
        }
        // Native56840 truncates float*255. While alpha<1, cf780(3) is
        // additive, including the final full-screen solid-color draw.
        let gray = (frame.cover_luminance.max(0.0) * 255.0) as u8;
        let alpha = (frame.cover_alpha.max(0.0) * 255.0) as u8;
        quad.with_color(UnormColor::from_rgba(gray, gray, gray, alpha))
            .fill(
                pass,
                RenderRequestBuilder::new().color_blend_type(if frame.cover_alpha < 1.0 {
                    ColorBlendType::Layer2
                } else {
                    ColorBlendType::Opaque
                }),
                top_left_projection_matrix(),
            );
    }
}

#[cfg(test)]
mod reference;
