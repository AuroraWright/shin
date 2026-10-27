//! Screen1498/14a0 ownership belongs to the live Screen, including restoration.
//! Once graphics are bound, allocation/swaps occur eagerly at enabled tick end.
//! CPU-only/legacy construction journals EVERY earlier sampled update until its
//! first physical extent is supplied; the graphics adapter replays all of them.
use super::temporal::Afterimages;
use crate::render::PreRenderContext;
use anyhow::{Result, ensure};
use parking_lot::Mutex;
use shin_render::{
    render_texture::RenderTexture,
    resize::{SurfaceResizeSource, ViewportParams},
};
use std::sync::{Arc, Weak};

#[derive(Clone)]
pub(crate) struct ScreenHistory {
    state: Arc<Mutex<State>>,
}
pub(crate) type WeakHistory = Weak<Mutex<State>>;
struct LogicalColor {
    id: u64,
}
pub(crate) struct Color {
    pub(crate) texture: RenderTexture,
    id: u64,
    /// Newly allocated previous14a0 is deliberately not explicitly cleared.
    /// WGPU initialization is a desktop service, not proof of native contents.
    pub(crate) known_pixels: bool,
}
pub(crate) struct Graphics {
    device: wgpu::Device,
    // Drop depth before previous and current, as native complete Screen does.
    _depth: wgpu::Texture,
    pub(crate) depth_view: wgpu::TextureView,
    pub(crate) images: Afterimages<Color>,
    // Complete Screen releases incoming1490 before outgoing1488, after history.
    pub(crate) incoming: Option<RenderTexture>,
    pub(crate) outgoing: Option<RenderTexture>,
    extent: [u32; 2],
    next_id: u64,
}
pub(crate) struct State {
    // Graphics contains its own depth-before-colors destruction order.
    graphics: Option<Graphics>,
    logical: Afterimages<LogicalColor>,
    next_id: u64,
    allocated: u64,
    retired: u64,
    pending_updates: Vec<f32>,
}

fn color(device: &wgpu::Device, extent: [u32; 2], id: u64, known_pixels: bool) -> Color {
    let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
        extent[0], extent[1],
    )));
    Color {
        texture: RenderTexture::new(
            device.clone(),
            resize.handle(),
            format!("Higurashi Screen color{id}"),
        ),
        id,
        known_pixels,
    }
}
fn depth(device: &wgpu::Device, extent: [u32; 2]) -> (wgpu::Texture, wgpu::TextureView) {
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Higurashi Screen14a8 depth"),
        size: wgpu::Extent3d {
            width: extent[0],
            height: extent[1],
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
    (depth, view)
}
impl Graphics {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, extent: [u32; 2]) -> Self {
        let current = color(device, extent, 1, true);
        let (depth, depth_view) = depth(device, extent);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Higurashi Screen constructor black clear"),
        });
        {
            let source = current.texture.as_texture_source();
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                multiview_mask: None,
                label: Some("Native initial1498 black"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    depth_slice: None,
                    view: source.view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        }
        queue.submit([encoder.finish()]);
        Self {
            device: device.clone(),
            _depth: depth,
            depth_view,
            images: Afterimages::new(extent, current),
            incoming: None,
            outgoing: None,
            extent,
            next_id: 3,
        }
    }
    /// The same images at `extent`, for a resized desktop window. Their pixels
    /// are not carried over: the next prepare draws current1498 again.
    fn resized(&self, extent: [u32; 2]) -> Self {
        let device = &self.device;
        let (depth, depth_view) = depth(device, extent);
        Self {
            device: device.clone(),
            _depth: depth,
            depth_view,
            images: self
                .images
                .resized(extent, |image| color(device, extent, image.id, false)),
            incoming: None,
            outgoing: None,
            extent,
            next_id: self.next_id,
        }
    }
    fn after_update(&mut self, sampled39: f32) -> Result<()> {
        let next = &mut self.next_id;
        let device = &self.device;
        self.images.after_update(sampled39, |extent| {
            let id = *next;
            *next += 1;
            Ok(color(device, extent, id, false))
        })
    }
    pub(crate) fn extent(&self) -> [u32; 2] {
        self.extent
    }
    pub(crate) fn ensure_outgoing(&mut self) {
        if self.outgoing.is_none() {
            self.outgoing = Some(color(&self.device, self.extent, 0, false).texture);
        }
    }
    pub(crate) fn ensure_incoming(&mut self) {
        if self.incoming.is_none() {
            self.incoming = Some(color(&self.device, self.extent, 0, false).texture);
        }
    }
    pub(crate) fn retire_planes(&mut self) {
        // No-wiper b9848 releases outgoing1488, then incoming1490.
        self.outgoing = None;
        self.incoming = None;
    }
}
impl Default for ScreenHistory {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                graphics: None,
                logical: Afterimages::new([0, 0], LogicalColor { id: 1 }),
                // Logical2 denotes the distinct constructor depth, not a color.
                next_id: 3,
                allocated: 2,
                retired: 0,
                pending_updates: Vec::new(),
            })),
        }
    }
}
impl ScreenHistory {
    pub(crate) fn weak(&self) -> WeakHistory {
        Arc::downgrade(&self.state)
    }
    pub(crate) fn from_weak(weak: &WeakHistory) -> Option<Self> {
        Some(Self {
            state: weak.upgrade()?,
        })
    }
    pub(crate) fn is_bound(&self) -> bool {
        self.state.lock().graphics.is_some()
    }
    pub(crate) fn bind(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        extent: [u32; 2],
    ) -> Result<()> {
        let mut state = self.state.lock();
        if state.graphics.is_some() {
            return Ok(());
        } // A bound Screen follows host resizes only through `fit`.
        ensure!(
            extent[0] > 0 && extent[1] > 0,
            "Higurashi Screen constructor requires a nonempty physical extent"
        );
        let mut graphics = Graphics::new(device, queue, extent);
        for sample in state.pending_updates.drain(..) {
            graphics.after_update(sample)?;
        }
        ensure!(
            graphics.images.current().id == state.logical.current().id
                && graphics.images.previous().map(|image| image.id)
                    == state.logical.previous().map(|image| image.id),
            "Higurashi deferred history journal lost ordered allocation/swap ownership"
        );
        state.graphics = Some(graphics);
        Ok(())
    }
    /// The native Screen keeps its constructor extent; the desktop follows the
    /// window's canvas, as the other games render, re-creating bound graphics.
    pub(crate) fn fit(&self, extent: [u32; 2]) -> Result<()> {
        ensure!(
            extent[0] > 0 && extent[1] > 0,
            "Higurashi Screen requires a nonempty physical extent"
        );
        let mut state = self.state.lock();
        if let Some(graphics) = &mut state.graphics
            && graphics.extent != extent
        {
            *graphics = graphics.resized(extent);
        }
        Ok(())
    }
    pub(crate) fn after_update(&self, sampled39: f32) -> Result<()> {
        let mut state = self.state.lock();
        let had_previous = state.logical.previous().is_some();
        let State {
            logical,
            next_id,
            allocated,
            ..
        } = &mut *state;
        logical.after_update(sampled39, |_| {
            let id = *next_id;
            *next_id += 1;
            *allocated += 1;
            Ok(LogicalColor { id })
        })?;
        if had_previous && !(sampled39 > 0.) {
            state.retired += 1;
        }
        if let Some(graphics) = &mut state.graphics {
            graphics.after_update(sampled39)?;
        } else {
            state.pending_updates.push(sampled39);
        }
        Ok(())
    }
    pub(crate) fn with_graphics<R>(&self, visit: impl FnOnce(&mut Graphics) -> R) -> Result<R> {
        let mut state = self.state.lock();
        let graphics = state.graphics.as_mut().ok_or_else(|| {
            anyhow::anyhow!("Higurashi Screen graphics have no bound constructor extent")
        })?;
        Ok(visit(graphics))
    }
    pub(crate) fn capture_current(&self, context: &mut PreRenderContext) -> Result<RenderTexture> {
        self.with_graphics(|graphics| {
            graphics
                .images
                .current()
                .texture
                .copy_snapshot(&mut context.render_clone_ctx())
        })
    }
    #[cfg(test)]
    pub(crate) fn reference_snapshot(&self) -> ([u32; 2], u64, Option<u64>, u64, u64, usize) {
        let state = self.state.lock();
        let extent = state.graphics.as_ref().map_or([0, 0], Graphics::extent);
        (
            extent,
            state.logical.current().id,
            state.logical.previous().map(|image| image.id),
            state.allocated,
            state.retired,
            state.pending_updates.len(),
        )
    }
}
