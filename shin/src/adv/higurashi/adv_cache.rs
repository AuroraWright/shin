//! ADV98 flag1 Group owns a retained composed color1488 and depth1490.
//! Both start NULL. Getter6c928 creates black color only; prepare6d200 creates
//! depth after child preparation. Captures and outer draws never prepare.
use crate::{
    layer::render_params::{higurashi::HigurashiTransform, multiply_transforms},
    render::PreRenderContext,
};
use anyhow::Result;
#[cfg(test)]
use anyhow::ensure;
use glam::{Vec2, Vec3, Vec4};
use parking_lot::Mutex;
use shin_core::primitives::color::{FloatColor4, UnormColor};
use shin_render::{
    DrawPrimitive, LayerFragmentShader, RenderProgramWithArguments, RenderRequestBuilder,
    render_pass::RenderPass,
    render_texture::RenderTexture,
    resize::{SurfaceResizeSource, ViewportParams},
    shaders::types::{buffer::VertexSource, texture::DepthStencilTarget, vertices::PosTexVertex},
};
use std::sync::Arc;

/// Updated cc9a0/cc9b0 return literals, independent of handheld/docked output.
pub(crate) const NATIVE_EXTENT: [u32; 2] = [1920, 1080];
struct Depth {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
}
pub(crate) struct State {
    // Complete Group destroys depth before cached color, after its children.
    depth: Option<Depth>,
    color: Option<RenderTexture>,
    extent: [u32; 2],
    prepares: u64,
}
pub(crate) struct AdvCache {
    state: Arc<Mutex<State>>,
}
impl Default for AdvCache {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                depth: None,
                color: None,
                extent: NATIVE_EXTENT,
                prepares: 0,
            })),
        }
    }
}
impl State {
    fn color(&mut self, context: &mut PreRenderContext) -> &mut RenderTexture {
        self.color.get_or_insert_with(|| {
            let resize = SurfaceResizeSource::new(ViewportParams::both(
                winit::dpi::PhysicalSize::new(self.extent[0], self.extent[1]),
            ));
            let mut color = RenderTexture::new(
                context.device.clone(),
                resize.handle(),
                "Higurashi ADV98 color1488".into(),
            );
            {
                let mut pass = context.begin_pass(
                    color.as_texture_target(),
                    None,
                    "ADV98 initial black getter",
                );
                pass.clear(Some(UnormColor::BLACK), None, None);
            }
            color
        })
    }
    fn ensure_depth(&mut self, device: &wgpu::Device) {
        self.depth.get_or_insert_with(|| {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Higurashi ADV98 depth1490"),
                size: wgpu::Extent3d {
                    width: self.extent[0],
                    height: self.extent[1],
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
            Depth {
                _texture: texture,
                view,
            }
        });
    }
}
impl AdvCache {
    pub(crate) fn extent(&self) -> [u32; 2] {
        self.state.lock().extent
    }
    pub(crate) fn capture(&self, context: &mut PreRenderContext) -> RenderTexture {
        self.state
            .lock()
            .color(context)
            .copy_snapshot(&mut context.render_clone_ctx())
    }
    /// Caller has prepared every child before this cache attachment/draw.
    pub(crate) fn prepare(
        &self,
        context: &mut PreRenderContext,
        draw: impl FnOnce(&mut RenderPass) -> Result<()>,
    ) -> Result<()> {
        let mut state = self.state.lock();
        state.color(context);
        state.ensure_depth(context.device);
        {
            let State { color, depth, .. } = &mut *state;
            let mut pass = context.begin_pass(
                color.as_mut().unwrap().as_texture_target(),
                Some(DepthStencilTarget {
                    view: &depth.as_ref().unwrap().view,
                }),
                "Higurashi composed ADV98",
            );
            pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.));
            draw(&mut pass)?;
        }
        // ADV98's own Group properties are identity; its children carry the
        // Screen/Page/overlay material. No second root transformation applies.
        state.prepares += 1;
        Ok(())
    }
    /// Native6dfc4 returns for NULL color; drawing must never allocate it.
    pub(crate) fn render(&self, pass: &mut RenderPass, stencil: u8) {
        let state = self.state.lock();
        let Some(color) = &state.color else {
            return;
        };
        let projection = multiply_transforms(
            HigurashiTransform::default().compute_native_final_transform(),
            glam::Mat4::from_translation(Vec3::new(-640., -720., 0.)),
        );
        let vertices =
            [(0., 0.), (1280., 0.), (0., 720.), (1280., 720.)].map(|(x, y)| PosTexVertex {
                position: Vec2::new(x, y),
                texture_position: Vec2::new(x / 1280., y / 720.),
            });
        pass.run(
            RenderRequestBuilder::new()
                .depth_stencil_shorthand(stencil, false, false)
                .build(
                    RenderProgramWithArguments::HigurashiPostprocess {
                        vertices: VertexSource::VertexData {
                            vertices: &vertices,
                        },
                        texture: color.as_texture_source(),
                        transform: HigurashiTransform::convert_clip_depth(projection),
                        color: FloatColor4::WHITE,
                        operation: LayerFragmentShader::Default,
                        param: Vec4::ZERO,
                    },
                    DrawPrimitive::TrianglesStrip,
                ),
        );
    }
    #[cfg(test)]
    pub(crate) fn reference_set_extent(&self, extent: [u32; 2]) -> Result<()> {
        let mut state = self.state.lock();
        ensure!(
            state.color.is_none() && state.depth.is_none() && extent.iter().all(|&n| n > 0),
            "Dimension service must be supplied before native cache allocation"
        );
        state.extent = extent;
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn reference_snapshot(&self) -> Snapshot {
        let state = self.state.lock();
        Snapshot {
            extent: state.extent,
            color_id: state.color.as_ref().map(|_| 1),
            depth_id: state.depth.as_ref().map(|_| 2),
            prepares: state.prepares,
        }
    }
    #[cfg(test)]
    pub(crate) fn reference_weak(&self) -> std::sync::Weak<Mutex<State>> {
        Arc::downgrade(&self.state)
    }
}
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub(crate) extent: [u32; 2],
    /// Cache-local ownership symbols; not native global texture allocator IDs.
    pub(crate) color_id: Option<u64>,
    pub(crate) depth_id: Option<u64>,
    pub(crate) prepares: u64,
}
