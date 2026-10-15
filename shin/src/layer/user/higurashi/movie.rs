//! Updated live Movie7 (7079c/708a8/92dd4/931cc/93284), independent of Umineko.
//! The caller constructs/inherits the native base before starting this owner.
use std::{fmt, sync::Arc};

use anyhow::{Context, Result, bail, ensure};
use glam::{Mat4, Vec2, Vec4, vec2, vec4};
use shin_audio::AudioManager;
use shin_core::{
    format::{save::higurashi::SaveData, scenario::higurashi::HigurashiScenario},
    primitives::{color::FloatColor4, update::FrameId},
    vm::command::types::Volume,
};
use shin_render::{
    ColorBlendType, DrawPrimitive, LayerBlendType, LayerFragmentShader, PassKind,
    RenderProgramWithArguments, RenderRequestBuilder,
    render_pass::RenderPass,
    render_texture::RenderTexture,
    resize::{SurfaceResizeSource, ViewportParams},
    shaders::types::{buffer::VertexSource, texture::TextureSource, vertices::PosTexVertex},
};
use shin_video::higurashi::HigurashiVideoPlayerHandle;

use crate::{
    asset::{movie::Movie, system::AssetServer},
    layer::{
        higurashi_properties::HigurashiProperties, render_params::higurashi::HigurashiTransform,
    },
    render::PreRenderContext,
};

mod runtime;
#[cfg(test)]
mod tests;
pub(crate) use runtime::{LiveOwner, Services};

pub(crate) type ActiveClock = Arc<dyn Fn() -> u64 + Send + Sync>;

/// Raw MovieInfo words are acquisition IDs, never volume/transparency options.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Acquisitions {
    pub movie: usize,
    pub bgm: Option<usize>,
    pub cg: Option<usize>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Gain {
    pub category: u32,
    pub local: f32,
}
impl Gain {
    pub fn unpack(packed: i32) -> Self {
        let raw = packed as u32;
        Self {
            category: (raw >> 12) & 15,
            local: ((raw & 4095) as f32 * f32::from_bits(0x3a83126f)).clamp(0., 1.),
        }
    }
    #[cfg(test)]
    pub fn category_one_source(self) -> bool {
        self.category == 1
    }
    pub fn sample(self, percentages: [u8; 3]) -> Volume {
        let preference = match self.category {
            1..=3 => {
                f32::from(percentages[(self.category - 1) as usize]) * f32::from_bits(0x3c23d70a)
            }
            _ => 1.,
        };
        Volume(preference * self.local)
    }
}
pub(crate) fn percentages(data: &SaveData) -> [u8; 3] {
    let settings = &data.settings.common;
    [settings.v0_bgmvol, settings.v1_sfxvol, settings.v2_voicevol]
}

/// Asset descriptors prepare asynchronously; no decoder/audio/OS/MT is started
/// here. Missing metadata is native Null fallback, expressed as None to caller.
pub(crate) struct Prepared {
    path: String,
    gain: Gain,
    #[allow(
        dead_code,
        reason = "Retain native Movie/BGM/CG acquisition IDs until the live acquired-bit service consumes them"
    )]
    acquisitions: Acquisitions,
}
impl Prepared {
    pub async fn load(
        assets: &AssetServer,
        scenario: &HigurashiScenario,
        params: &[i32; 8],
        setting: u8,
    ) -> Result<Option<Self>> {
        ensure!(
            assets.context().game == crate::cli::Game::Higurashi,
            "Higurashi Movie requires Higurashi assets"
        );
        let Some(info) = usize::try_from(params[0])
            .ok()
            .and_then(|id| scenario.movie_info.get(id))
        else {
            return Ok(None);
        };
        let name = super::selected_asset_name(info.name.as_str(), setting);
        Ok(Some(Self {
            path: format!("/movie/{name}.mp4"),
            gain: Gain::unpack(params[1]),
            acquisitions: Acquisitions {
                movie: params[0] as usize,
                bgm: usize::try_from(info.raw_fields[0]).ok(),
                cg: usize::try_from(info.raw_fields[1]).ok(),
            },
        }))
    }
    #[allow(
        dead_code,
        reason = "Native acquisition descriptor API retained for live acquired-bit integration"
    )]
    pub fn acquisitions(&self) -> Acquisitions {
        self.acquisitions
    }
    /// Call only AFTER the inherited/fresh65-property/four-seed base is built.
    /// New native Player is unconditional; the previous media is never inherited.
    pub fn construct(
        self,
        device: wgpu::Device,
        audio: Arc<AudioManager>,
        preferences: [u8; 3],
        active_clock: Option<ActiveClock>,
        assets: &AssetServer,
    ) -> Result<Owner> {
        // Original92dd4 calls57f00 only after6717c constructs the base.
        // Descriptor readiness does not read or parse the MP4 file.
        let movie: Arc<Movie> = assets.load_sync(&self.path)?;
        let label = movie.label().to_owned();
        let gain = self.gain;
        let retained_audio = audio.clone();
        let task = shin_tasks::compute::spawn_loader(move || {
            movie.prepare_higurashi_with_volume(
                &device,
                &audio,
                gain.sample(preferences),
                active_clock,
            )
        });
        let retirement = task.retirement_handle();
        Ok(Owner {
            label,
            gain,
            _audio: retained_audio,
            desired_playing: true,
            loading: Some(Loading {
                task: Some(task),
                retirement,
            }),
            player: None,
            cache: None,
            fault: None,
            retired: false,
        })
    }
}

struct Loading {
    task: Option<shin_tasks::LoaderComputeTask<Result<HigurashiVideoPlayerHandle>>>,
    retirement: shin_tasks::LoaderRetirement,
}

/// Live Movie has no virtual38 clone. Parent may retain one owner via Arc when
/// moving the outgoing bank; native_copy must return NULL instead of Arc clone.
pub(crate) struct Owner {
    label: String,
    gain: Gain,
    // Keep mixer callbacks available while retirement drains AAC descendants.
    _audio: Arc<AudioManager>,
    desired_playing: bool,
    loading: Option<Loading>,
    player: Option<HigurashiVideoPlayerHandle>,
    cache: Option<Cache>,
    fault: Option<String>,
    retired: bool,
}
impl fmt::Debug for Owner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HigurashiMovie")
            .field("label", &self.label)
            .field("gain", &self.gain)
            .field("status", &self.status())
            .finish_non_exhaustive()
    }
}
impl Owner {
    pub fn status(&self) -> u32 {
        if self.retired || self.fault.is_some() {
            2 // Explicit desktop failure/retirement releases the owning wait.
        } else {
            self.player
                .as_ref()
                .map_or(0, HigurashiVideoPlayerHandle::status)
        }
    }
    #[cfg(test)]
    pub fn media_time_micros(&self) -> u64 {
        self.player
            .as_ref()
            .map_or(0, HigurashiVideoPlayerHandle::current_time_micros)
    }
    /// Caller advances the native base first, inside its permitted visual owner
    /// tick. No game delta reaches the independent media transport clock.
    fn update_media(
        &mut self,
        frame_id: FrameId,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Result<()> {
        if self.retired {
            return Ok(());
        }
        if let Some(fault) = &self.fault {
            bail!("Higurashi Movie{}: {fault}", self.label);
        }
        let ready = self
            .loading
            .as_mut()
            .and_then(|loading| loading.task.as_mut().and_then(|task| task.poll_naive()));
        if let Some(ready) = ready {
            self.loading.as_mut().unwrap().task = None;
            match ready {
                Ok(player) => {
                    if self.desired_playing {
                        player.start();
                    }
                    self.player = Some(player);
                }
                Err(error) => {
                    self.fault = Some(format!("{error:#}"));
                    return Err(error.context(format!("Preparing Higurashi Movie{}", self.label)));
                }
            }
        }
        let Some(player) = &self.player else {
            return Ok(());
        };
        match player.update(frame_id, device, queue) {
            Ok(changed) => {
                if changed {
                    let extent = player
                        .with_frame_texture(|frame| {
                            let size = frame.get_size();
                            (size.x as u32, size.y as u32)
                        })
                        .context("Updated Higurashi Movie has no retained NV12")?;
                    if self
                        .cache
                        .as_ref()
                        .is_none_or(|cache| cache.extent != extent)
                    {
                        self.cache = Some(Cache::new(device, extent));
                    }
                    self.cache.as_mut().unwrap().dirty = true;
                }
                Ok(())
            }
            Err(error) => {
                self.fault = Some(format!("{error:#}"));
                Err(error.context(format!("Updating Higurashi Movie{}", self.label)))
            }
        }
    }
    fn apply_preferences(&self, preferences: [u8; 3]) {
        if !self.retired {
            if let Some(player) = &self.player {
                //931cc samples current settings after8ff80, including its RGBA
                // conversion. The registered runtime calls this last.
                player.set_volume(self.gain.sample(preferences));
            }
        }
    }
    /// Convert before any following lower-prefix capture/final render consumes
    /// this owner. Dirty NV12 produces full-frame original program20 RGBA once.
    pub fn prepare(&mut self, context: &mut PreRenderContext) -> Result<bool> {
        if let Some(fault) = &self.fault {
            bail!("Higurashi Movie{}: {fault}", self.label);
        }
        let Some(player) = &self.player else {
            return Ok(false);
        };
        let Some(cache) = &mut self.cache else {
            return Ok(false);
        };
        if !cache.dirty {
            return Ok(false);
        }
        let converted = player
            .with_frame_texture(|frame| {
                let packet = ConversionPacket::new(cache.extent);
                let mut pass = context.begin_pass(
                    cache.texture.as_texture_target(),
                    None,
                    "Higurashi Movie original NV12-to-RGBA",
                );
                pass.set_viewport(cache.extent.0 as f32, cache.extent.1 as f32);
                packet.render(&mut pass, frame.get_y_source(), frame.get_uv_source());
            })
            .is_some();
        ensure!(
            converted,
            "Higurashi Movie conversion lost its retained frame"
        );
        cache.dirty = false;
        Ok(true)
    }
    pub fn render(
        &self,
        pass: &mut RenderPass,
        properties: &HigurashiProperties,
        parent: &HigurashiTransform,
        stencil: u8,
        kind: PassKind,
    ) -> Result<()> {
        if !properties.is_visible() {
            return Ok(());
        }
        let Some(cache) = &self.cache else {
            return Ok(());
        };
        ensure!(
            !cache.dirty,
            "Higurashi Movie needs original RGBA preparation before rendering"
        );
        if let Some(packet) =
            Packet::new(FrameInfo::decoded(cache.extent), properties, parent, kind)?
        {
            let clipped = super::leaf_clip::Clip::from_properties(properties).prepare(pass);
            packet.render_clipped(pass, cache.texture.as_texture_source(), stencil, clipped);
        }
        Ok(())
    }
    /// Must run off executor polls/workers: producer outputs and all codec/AAC
    /// descendants are drained before releasing the scene's retained owner.
    pub fn retire(&mut self) {
        if self.retired {
            return;
        }
        self.retired = true;
        let player = self.player.take();
        if let Some(mut loading) = self.loading.take() {
            #[cfg(not(target_arch = "wasm32"))]
            {
                if let Some(task) = loading.task.take() {
                    task.cancel_and_retire();
                }
                loading.retirement.retire(player);
            }
            #[cfg(target_arch = "wasm32")]
            {
                drop(loading);
                drop(player);
            }
        } else {
            drop(player);
        }
        self.cache = None;
    }
    #[cfg(test)]
    pub(crate) fn reference_retirement(&self) -> Option<shin_tasks::LoaderRetirement> {
        self.loading
            .as_ref()
            .map(|loading| loading.retirement.clone())
    }
}
impl Drop for Owner {
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
                "Higurashi Movie retained RGBA".to_owned(),
            ),
            dirty: false,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct FrameInfo {
    pub size: Vec2,
    pub origin: Vec2,
    pub extent: Vec2,
}
impl FrameInfo {
    fn decoded((width, height): (u32, u32)) -> Self {
        // Shared decoder exposes visible NV12; SDK coded padding/crop remains
        // the explicit imported-service boundary, never guessed from params.
        Self {
            size: vec2(width as f32, height as f32),
            origin: Vec2::ZERO,
            extent: Vec2::ONE,
        }
    }
}
fn quad(size: Vec2, low: Vec2, high: Vec2) -> [PosTexVertex; 4] {
    [
        PosTexVertex {
            position: Vec2::ZERO,
            texture_position: low,
        },
        PosTexVertex {
            position: vec2(size.x, 0.),
            texture_position: vec2(high.x, low.y),
        },
        PosTexVertex {
            position: vec2(0., size.y),
            texture_position: vec2(low.x, high.y),
        },
        PosTexVertex {
            position: size,
            texture_position: high,
        },
    ]
}

/// Original b924 polls the captured Movie target; absent/non-Movie completes.
/// Selector/bank capture belongs to the command constructor, not this helper.
pub(crate) fn wait_pending(mode: i32, movie_status: Option<u32>) -> bool {
    movie_status.is_some_and(|status| match mode {
        1 => status == 0,
        2 => status != 2,
        _ => false,
    })
}
pub(crate) struct Packet {
    pub matrix: Mat4, // Raw original OpenGL projection, bridge exactly once.
    pub vertices: [PosTexVertex; 4],
    pub color: FloatColor4,
    pub operation: LayerFragmentShader,
    pub param: Vec4,
    pub blend: ColorBlendType,
}
impl Packet {
    pub fn new(
        frame: FrameInfo,
        properties: &HigurashiProperties,
        parent: &HigurashiTransform,
        kind: PassKind,
    ) -> Result<Option<Self>> {
        if !properties.is_visible() {
            return Ok(None);
        }
        let drawable = properties.drawable_params();
        let native_kind =
            if drawable.blend_type == LayerBlendType::Type1 && drawable.color_multiplier.a >= 1. {
                PassKind::Opaque
            } else {
                PassKind::Transparent
            };
        if native_kind != kind {
            return Ok(None);
        }
        let low = vec2(
            if frame.extent.x < 0. {
                frame.origin.x + frame.extent.x
            } else {
                frame.origin.x
            },
            if frame.extent.y < 0. {
                frame.origin.y + frame.extent.y
            } else {
                frame.origin.y
            },
        );
        let high = vec2(
            if frame.extent.x < 0. {
                frame.origin.x
            } else {
                frame.origin.x + frame.extent.x
            },
            if frame.extent.y < 0. {
                frame.origin.y
            } else {
                frame.origin.y + frame.extent.y
            },
        );
        Ok(Some(Self {
            matrix: properties
                .composed_transform(parent)
                .compute_native_message_transform(Vec2::ZERO),
            vertices: quad(frame.size, low, high),
            color: drawable.color_multiplier,
            operation: drawable.fragment_shader.simplify(drawable.shader_param),
            param: drawable.shader_param,
            // Even original opaque Movie934f8 uses ordinary blend2, never1.
            blend: ColorBlendType::from_higurashi_layer(drawable.blend_type),
        }))
    }
    #[cfg(test)]
    pub fn render(&self, pass: &mut RenderPass, source: TextureSource, stencil: u8) {
        self.render_clipped(pass, source, stencil, false);
    }
    pub fn render_clipped(
        &self,
        pass: &mut RenderPass,
        source: TextureSource,
        stencil: u8,
        clipped: bool,
    ) {
        pass.run(
            RenderRequestBuilder::new()
                .color_blend_type(self.blend)
                .depth_stencil_shorthand(stencil, false, clipped)
                .build(
                    RenderProgramWithArguments::HigurashiPostprocess {
                        vertices: VertexSource::VertexData {
                            vertices: &self.vertices,
                        },
                        texture: source,
                        transform: HigurashiTransform::convert_clip_depth(self.matrix),
                        color: self.color,
                        operation: self.operation,
                        param: self.param,
                    },
                    DrawPrimitive::TrianglesStrip,
                ),
        );
    }
}

/// Actual90584 program20 packet uses no layer tint, source alpha or transparency.
pub(crate) struct ConversionPacket {
    pub matrix: Mat4,
    pub vertices: [PosTexVertex; 4],
    pub bias: Vec4,
    pub rows: [Vec4; 3],
}
impl ConversionPacket {
    pub fn render(&self, pass: &mut RenderPass, luma: TextureSource, chroma: TextureSource) {
        pass.run(
            RenderRequestBuilder::new()
                .color_blend_type(ColorBlendType::Opaque)
                .build(
                    RenderProgramWithArguments::Movie {
                        vertices: VertexSource::VertexData {
                            vertices: &self.vertices,
                        },
                        texture_luma: luma,
                        texture_chroma: chroma,
                        transform: HigurashiTransform::convert_clip_depth(self.matrix),
                        color_bias: self.bias,
                        color_transform: self.rows,
                        color_multiplier: Vec4::ONE,
                        alpha_mode: false,
                        texture_size: vec4(0., 0., 0., 0.), // Dead branch when alpha_mode=false.
                    },
                    DrawPrimitive::TrianglesStrip,
                ),
        );
    }
    pub fn new((width, height): (u32, u32)) -> Self {
        Self {
            matrix: Mat4::from_cols_array(&[
                2. / width as f32,
                0.,
                0.,
                0.,
                0.,
                -2. / height as f32,
                0.,
                0.,
                0.,
                0.,
                -1.,
                0.,
                -1.,
                1.,
                -0.,
                1.,
            ]),
            vertices: quad(vec2(width as f32, height as f32), Vec2::ZERO, Vec2::ONE),
            bias: Vec4::from_array([0x3d800000, 0x3f000000, 0x3f000000, 0].map(f32::from_bits)),
            rows: [
                [0x3f950a85, 0, 0x3fe5788a, 0],
                [0x3f950a85, 0xbe5a5699, 0xbf086aef, 0],
                [0x3f950a85, 0x40073197, 0, 0],
            ]
            .map(|row| Vec4::from_array(row.map(f32::from_bits))),
        }
    }
}
