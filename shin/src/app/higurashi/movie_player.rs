//! Updated Player92ca4/90d34 used by the standalone Movie gallery viewer.
//! The shared Higurashi transport/conversion is also used by story Movie7;
//! this owner keeps the standalone Sprite pass and its caller-supplied gain.
use std::{future::Future, sync::Arc};

use anyhow::{Context, Result, bail, ensure};
use glam::{Mat4, Vec2, Vec3, Vec4};
use parking_lot::Mutex;
use shin_audio::AudioManager;
use shin_core::{
    primitives::{color::UnormColor, update::FrameId},
    vm::command::types::Volume,
};
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    dynamic_buffer::DynamicBuffer,
    pipelines::PipelineStorage,
    render_pass::RenderPass,
    render_texture::RenderTexture,
    resize::{SurfaceResizeSource, ViewportParams},
    shaders::types::{
        buffer::{BytesAddress, VertexSource},
        texture::{TextureSamplerStore, TextureSource},
        vertices::PosColTexVertex,
    },
};
use shin_video::higurashi::HigurashiVideoPlayerHandle;

use crate::{
    asset::{movie::Movie, system::AssetServer},
    layer::{
        render_params::higurashi::HigurashiTransform,
        user::higurashi::{movie::ConversionPacket, selected_asset_name},
    },
};

#[cfg(test)]
mod tests;

struct Published {
    player: Option<HigurashiVideoPlayerHandle>,
    desired_playing: bool,
    gain: f32,
    color: Vec4,
    fault: Option<String>,
}
struct Loading {
    task: Option<shin_tasks::AsyncTask<Result<()>>>,
    retirement: shin_tasks::LoaderRetirement,
}
/// Execute transport shutdown while LoaderRetirement scopes its cleanup, but
/// retain the externally owned frame facade until the later Player release.
struct ShutdownHandle<'a>(Option<&'a HigurashiVideoPlayerHandle>);
impl Drop for ShutdownHandle<'_> {
    fn drop(&mut self) {
        if let Some(player) = self.0 {
            player.terminate();
        }
    }
}

pub(super) struct Player {
    assets: Arc<AssetServer>,
    // Keep the mixer alive until scoped codec/AAC retirement has completed.
    _audio: Arc<AudioManager>,
    label: String,
    published: Arc<Mutex<Published>>,
    loading: Option<Loading>,
    cache: Option<Cache>,
    conversion: Option<ConversionResources>,
    shutdown: bool,
    retired: bool,
}
impl Player {
    /// Native filenamebcc84(type0) and file57f00 happen synchronously before
    /// submitting the Player worker. Source category1 does not imply a mixer
    /// preference or looping; the original new Player gain is exactly1.
    pub fn open(
        assets: Arc<AssetServer>,
        audio: Arc<AudioManager>,
        movie_name: &str,
    ) -> Result<Self> {
        ensure!(
            assets.context().game == crate::cli::Game::Higurashi,
            "Standalone Higurashi Player requires Higurashi assets"
        );
        let name = selected_asset_name(movie_name, 0);
        let movie: Arc<Movie> = assets.load_sync(&format!("/movie/{name}.mp4"))?;
        let label = movie.label().to_owned();
        let device = assets.context().wgpu_device.clone();
        let manager = audio.clone();
        Ok(Self::preparing(
            assets,
            audio,
            label,
            move || movie.prepare_higurashi_with_volume(&device, &manager, Volume(1.), None),
            async {},
        ))
    }

    /// Worker publication is separate from observing its executor completion.
    /// The generic tail is empty in production and cancellation-safe in tests.
    fn preparing<F>(
        assets: Arc<AssetServer>,
        audio: Arc<AudioManager>,
        label: String,
        prepare: impl FnOnce() -> Result<HigurashiVideoPlayerHandle> + Send + 'static,
        after_publish: F,
    ) -> Self
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let published = Arc::new(Mutex::new(Published {
            player: None,
            desired_playing: false, //8e848 clears both a88/a89.
            gain: 1.,               //8e854 writes ab0=1; worker reads it.
            color: Vec4::ONE,
            fault: None,
        }));
        let destination = published.clone();
        let task = shin_tasks::async_io::spawn_loader(async move {
            let result = shin_tasks::compute::spawn(prepare).await;
            match result {
                Ok(player) => {
                    {
                        let mut slot = destination.lock();
                        player.set_volume(Volume(slot.gain));
                        if slot.desired_playing {
                            player.start();
                        }
                        slot.player = Some(player);
                    }
                    after_publish.await;
                    Ok(())
                }
                Err(error) => {
                    destination.lock().fault = Some(format!("{error:#}"));
                    Err(error)
                }
            }
        });
        let retirement = task.retirement_handle().expect("Player loader scope");
        Self {
            assets,
            _audio: audio,
            label,
            published,
            loading: Some(Loading {
                task: Some(task),
                retirement,
            }),
            cache: None,
            conversion: None,
            shutdown: false,
            retired: false,
        }
    }
    pub fn status(&self) -> u32 {
        let slot = self.published.lock();
        if self.shutdown || self.retired || slot.fault.is_some() {
            2 // Explicit desktop retirement/failure, never simulated native EOF.
        } else {
            slot.player.as_ref().map_or(0, |player| player.status())
        }
    }
    pub fn is_playing(&self) -> bool {
        !self.retired && self.published.lock().desired_playing
    }
    pub fn start(&self) {
        let mut slot = self.published.lock();
        if !self.shutdown && !self.retired && slot.fault.is_none() {
            slot.desired_playing = true;
            if let Some(player) = &slot.player {
                player.start();
            }
        }
    }
    pub fn pause(&self) {
        let mut slot = self.published.lock();
        slot.desired_playing = false;
        if let Some(player) = &slot.player {
            player.stop();
        }
    }
    pub fn sync_suspension(&self) {
        if let Some(player) = &self.published.lock().player {
            player.sync_suspension();
        }
    }
    pub fn set_gain(&self, gain: f32) -> Result<()> {
        ensure!(gain.is_finite(), "Standalone Movie gain is not finite");
        let mut slot = self.published.lock();
        slot.gain = gain; // The raw viewer setting255 is allowed to exceed1.
        if let Some(player) = &slot.player {
            player.set_volume(Volume(gain));
        }
        Ok(())
    }
    pub fn set_color(&self, color: Vec4) -> Result<()> {
        ensure!(color.is_finite(), "Standalone Movie color is not finite");
        self.published.lock().color = color;
        Ok(())
    }
    /// Native +58 updates and converts even when locally paused. Neither the
    /// model's tween delta nor a second gain preference reaches the transport.
    pub fn update(&mut self, frame: FrameId) -> Result<()> {
        let result = self.update_inner(frame);
        if let Err(error) = &result {
            let mut slot = self.published.lock();
            slot.fault = Some(format!("{error:#}"));
            slot.desired_playing = false;
            if let Some(player) = &slot.player {
                player.stop();
            }
        }
        result.with_context(|| format!("Standalone Higurashi Movie{}", self.label))
    }
    fn update_inner(&mut self, frame: FrameId) -> Result<()> {
        if self.shutdown || self.retired {
            return Ok(());
        }
        if let Some(loading) = &mut self.loading {
            let result = loading.task.as_mut().and_then(|task| task.poll_naive());
            if let Some(result) = result {
                loading.task = None;
                result?;
            }
        }
        let slot = self.published.lock();
        if let Some(fault) = &slot.fault {
            bail!("{fault}");
        }
        let Some(player) = &slot.player else {
            return Ok(());
        };
        let context = self.assets.context();
        if player.update(frame, &context.wgpu_device, &context.wgpu_queue)? {
            let extent = player
                .with_frame_texture(|frame| {
                    let size = frame.get_size();
                    (size.x as u32, size.y as u32)
                })
                .context("Standalone Movie update lost retained NV12")?;
            if self
                .cache
                .as_ref()
                .is_none_or(|cache| cache.extent != extent)
            {
                self.cache = Some(Cache::new(&context.wgpu_device, extent));
            }
            self.cache.as_mut().unwrap().dirty = true;
        }
        let Some(cache) = self.cache.as_mut().filter(|cache| cache.dirty) else {
            return Ok(());
        };
        let resources = self
            .conversion
            .get_or_insert_with(|| ConversionResources::new(&context.wgpu_device));
        let mut encoder = context
            .wgpu_device
            .create_command_encoder(&Default::default());
        let converted = player
            .with_frame_texture(|frame| {
                let mut pass = RenderPass::new(
                    &mut resources.pipelines,
                    &mut resources.dynamic,
                    &resources.samplers,
                    &context.wgpu_device,
                    &mut encoder,
                    cache.texture.as_texture_target(),
                    None,
                    Some((0., 0., cache.extent.0 as f32, cache.extent.1 as f32)),
                    "Higurashi standalone Movie original RGBA conversion",
                );
                ConversionPacket::new(cache.extent).render(
                    &mut pass,
                    frame.get_y_source(),
                    frame.get_uv_source(),
                );
            })
            .is_some();
        ensure!(converted, "Standalone Movie conversion lost retained frame");
        let mut uploads = context
            .wgpu_device
            .create_command_encoder(&Default::default());
        resources.dynamic.finish(&mut uploads);
        context
            .wgpu_queue
            .submit([uploads.finish(), encoder.finish()]);
        resources.dynamic.recall();
        cache.dirty = false;
        Ok(())
    }
    /// Original90d34 tests only the retained-frame getter. Its Viewer caller
    /// must gate the whole gain -> color -> draw sequence with is_playing.
    /// Playing with alpha0 still submits this Sprite2 packet.
    pub fn draw(&self, pass: &mut RenderPass) -> Result<bool> {
        let slot = self.published.lock();
        if let Some(fault) = &slot.fault {
            bail!("{fault}");
        }
        let Some(cache) = &self.cache else {
            return Ok(false);
        };
        ensure!(!cache.dirty, "Standalone Movie RGBA conversion is pending");
        Packet::new(Vec2::ZERO, Vec2::ONE, slot.color)?
            .render(pass, cache.texture.as_texture_source());
        Ok(true)
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    pub fn reference_frame_extent(&self) -> Option<(u32, u32)> {
        self.cache
            .as_ref()
            .filter(|cache| !cache.dirty)
            .map(|cache| cache.extent)
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    pub fn reference_time_micros(&self) -> Option<u64> {
        self.published
            .lock()
            .player
            .as_ref()
            .map(|player| player.current_time_micros())
    }
    #[cfg(test)]
    pub fn reference_retirement(&self) -> shin_tasks::LoaderRetirement {
        self.loading.as_ref().expect("Live Player loader").retirement.clone()
    }
    /// Native8fc24/+20 joins workers before Viewer destroys its tweens. Its
    /// external Player and retained RGBA survive until the later +8 release.
    /// Do not replace this with +30/pause, which keeps codec work alive.
    /// Desktop codecs cancel by dropping their decoder object earlier than the
    /// SDK; the stopped transport facade and decoded frame remain owned here.
    pub fn shutdown(&mut self) {
        if self.shutdown {
            return;
        }
        self.shutdown = true;
        if let Some(loading) = &mut self.loading {
            #[cfg(not(target_arch = "wasm32"))]
            {
                // Never drain descendants while a live published decoder
                // owns them, nor hold its publication mutex across the join.
                let output = loading.task.take().and_then(|task| task.cancel_and_wait());
                let player = self.published.lock().player.take();
                loading
                    .retirement
                    .clone()
                    .retire((output, ShutdownHandle(player.as_ref())));
                self.published.lock().player = player;
            }
            #[cfg(target_arch = "wasm32")]
            {
                loading.task.take();
                if let Some(player) = &self.published.lock().player {
                    player.terminate();
                }
            }
        }
        //8fc24 does not clear a88. Preserve the desired flag independently of
        // terminal shutdown, just as external suspension preserves it.
    }
    /// External +8 release follows shutdown. Run outside executor polls; drop
    /// the retained stopped facade under its loader scope before the last drain.
    pub fn retire(&mut self) {
        if self.retired {
            return;
        }
        self.shutdown();
        self.retired = true;
        let player = {
            let mut slot = self.published.lock();
            slot.desired_playing = false;
            slot.player.take()
        };
        if let Some(loading) = self.loading.take() {
            #[cfg(not(target_arch = "wasm32"))]
            loading.retirement.retire(player);
            #[cfg(target_arch = "wasm32")]
            drop((loading, player));
        } else {
            drop(player);
        }
        self.cache = None;
        self.conversion = None;
    }
}
impl Drop for Player {
    fn drop(&mut self) {
        self.retire();
    }
}

struct Cache {
    extent: (u32, u32),
    texture: RenderTexture,
    dirty: bool,
}
impl Cache {
    fn new(device: &wgpu::Device, extent: (u32, u32)) -> Self {
        let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
            extent.0, extent.1,
        )));
        Self {
            extent,
            texture: RenderTexture::new(
                device.clone(),
                resize.handle(),
                "Higurashi standalone retained Movie RGBA".to_owned(),
            ),
            dirty: false,
        }
    }
}
struct ConversionResources {
    pipelines: PipelineStorage,
    dynamic: DynamicBuffer,
    samplers: TextureSamplerStore,
}
impl ConversionResources {
    fn new(device: &wgpu::Device) -> Self {
        Self {
            pipelines: PipelineStorage::new(device.clone(), wgpu::TextureFormat::Rgba8Unorm),
            dynamic: DynamicBuffer::new(device.clone(), BytesAddress::new(4096)),
            samplers: TextureSamplerStore::new(device),
        }
    }
}

/// Raw original GL packet; only render() bridges depth and packed BGRA.
pub(super) struct Packet {
    pub matrix: Mat4,
    pub color: u32,
    pub blend: u32,
    pub vertices: [[u32; 6]; 4],
}
impl Packet {
    pub fn new(origin: Vec2, extent: Vec2, color: Vec4) -> Result<Self> {
        ensure!(
            origin.is_finite() && extent.is_finite() && color.is_finite(),
            "Standalone Movie Sprite descriptor is not finite"
        );
        let [r, g, b, a] = color.to_array().map(|v| (v.clamp(0., 1.) * 255.) as u8);
        let packed = u32::from_le_bytes([b, g, r, a]);
        let right = origin.x + extent.x;
        let bottom = origin.y + extent.y;
        Ok(Self {
            matrix: Mat4::from_cols_array(&[
                2., 0., 0., 0., 0., -2., 0., 0., 0., 0., -1., 0., -1., 1., -0., 1.,
            ]),
            color: packed,
            blend: if color.w >= 1. { 1 } else { 2 },
            vertices: [
                [0., 0., origin.x, origin.y],
                [1., 0., right, origin.y],
                [0., 1., origin.x, bottom],
                [1., 1., right, bottom],
            ]
            .map(|[x, y, u, v]| {
                [
                    x.to_bits(),
                    y.to_bits(),
                    0,
                    packed,
                    u.to_bits(),
                    v.to_bits(),
                ]
            }),
        })
    }
    pub fn render(&self, pass: &mut RenderPass, texture: TextureSource) {
        let bytes = self.color.to_le_bytes();
        let color = UnormColor::from_rgba(bytes[2], bytes[1], bytes[0], bytes[3]);
        let vertices = self.vertices.map(|v| PosColTexVertex {
            position: Vec3::from_array([v[0], v[1], v[2]].map(f32::from_bits)),
            color,
            texture_position: Vec2::from_array([v[4], v[5]].map(f32::from_bits)),
        });
        pass.run(
            RenderRequestBuilder::new()
                .color_blend_type(if self.blend == 1 {
                    ColorBlendType::Opaque
                } else {
                    ColorBlendType::Layer1
                })
                .build(
                    RenderProgramWithArguments::Sprite {
                        vertices: VertexSource::VertexData {
                            vertices: &vertices,
                        },
                        sprite: texture,
                        transform: HigurashiTransform::convert_clip_depth(self.matrix),
                    },
                    DrawPrimitive::TrianglesStrip,
                ),
        );
    }
}
