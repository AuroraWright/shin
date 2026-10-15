//! Main-thread media resources shared by current and outgoing older scene banks.
//! A permitted root tick advances one frame ID; media time stays independent.
use super::*;
use parking_lot::Mutex;
use shin_render::{
    dynamic_buffer::DynamicBuffer,
    pipelines::PipelineStorage,
    shaders::types::{buffer::BytesAddress, texture::TextureSamplerStore},
};
use std::sync::atomic::{AtomicU32, Ordering};

struct Inner {
    assets: Arc<AssetServer>,
    audio: Arc<AudioManager>,
    clock: Option<ActiveClock>,
    preferences: AtomicU32,
    frame: Mutex<FrameId>,
    conversion: Mutex<Option<ConversionResources>>,
}

#[derive(Clone)]
pub(crate) struct Services(Arc<Inner>);
impl fmt::Debug for Services {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HigurashiMovieServices")
            .field("frame", &*self.0.frame.lock())
            .finish_non_exhaustive()
    }
}
impl Services {
    pub fn new(
        assets: Arc<AssetServer>,
        audio: Arc<AudioManager>,
        preferences: [u8; 3],
        clock: Option<ActiveClock>,
    ) -> Self {
        let result = Self(Arc::new(Inner {
            assets,
            audio,
            clock,
            preferences: AtomicU32::new(0),
            frame: Mutex::new(FrameId::default()),
            conversion: Mutex::new(None),
        }));
        result.set_preferences(preferences);
        result
    }
    pub fn set_preferences(&self, preferences: [u8; 3]) {
        self.0.preferences.store(
            u32::from_le_bytes([preferences[0], preferences[1], preferences[2], 0]),
            Ordering::Release,
        );
    }
    pub fn begin_frame(&self) {
        self.0.frame.lock().advance();
    }
    pub fn assets(&self) -> &Arc<AssetServer> {
        &self.0.assets
    }
    pub fn audio(&self) -> &Arc<AudioManager> {
        &self.0.audio
    }
    pub fn active_clock(&self) -> Option<ActiveClock> {
        self.0.clock.clone()
    }
    pub fn retain_owner(&self, owner: Owner) -> LiveOwner {
        LiveOwner {
            owner: Arc::new(Mutex::new(owner)),
            services: self.clone(),
        }
    }
    fn advance(&self, owner: &mut Owner) -> Result<()> {
        let context = self.0.assets.context();
        let bytes = self.0.preferences.load(Ordering::Acquire).to_le_bytes();
        owner.update_media(
            *self.0.frame.lock(),
            &context.wgpu_device,
            &context.wgpu_queue,
        )?;
        // Original8ff80/90584 records conversion in the player's update. Do it
        // now so MOVIEWAIT and later lower-prefix captures see the same frame.
        self.convert(owner)?;
        owner.apply_preferences([bytes[0], bytes[1], bytes[2]]);
        Ok(())
    }
    fn convert(&self, owner: &mut Owner) -> Result<()> {
        let context = self.0.assets.context();
        let Some(cache) = owner.cache.as_mut().filter(|cache| cache.dirty) else {
            return Ok(());
        };
        let player = owner.player.as_ref().context("Dirty Movie has no player")?;
        let mut storage = self.0.conversion.lock();
        let storage = storage.get_or_insert_with(|| ConversionResources::new(&context.wgpu_device));
        let mut encoder = context
            .wgpu_device
            .create_command_encoder(&Default::default());
        let converted = player
            .with_frame_texture(|frame| {
                let mut pass = RenderPass::new(
                    &mut storage.pipelines,
                    &mut storage.dynamic,
                    &storage.samplers,
                    &context.wgpu_device,
                    &mut encoder,
                    cache.texture.as_texture_target(),
                    None,
                    Some((0., 0., cache.extent.0 as f32, cache.extent.1 as f32)),
                    "Higurashi Movie update conversion",
                );
                ConversionPacket::new(cache.extent).render(
                    &mut pass,
                    frame.get_y_source(),
                    frame.get_uv_source(),
                );
            })
            .is_some();
        ensure!(converted, "Higurashi Movie update lost retained NV12");
        let mut uploads = context
            .wgpu_device
            .create_command_encoder(&Default::default());
        storage.dynamic.finish(&mut uploads);
        context
            .wgpu_queue
            .submit([uploads.finish(), encoder.finish()]);
        storage.dynamic.recall();
        cache.dirty = false;
        Ok(())
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

/// Retention is distinct from virtual38, which always returns NULL for Movie.
#[derive(Clone, Debug)]
pub(crate) struct LiveOwner {
    pub(super) owner: Arc<Mutex<Owner>>,
    services: Services,
}
impl LiveOwner {
    #[cfg(test)]
    pub(crate) fn reference_lifetime(&self) -> std::sync::Weak<Mutex<Owner>> {
        Arc::downgrade(&self.owner)
    }
    #[cfg(test)]
    pub(crate) fn reference_with_rgba<R>(
        &self,
        draw: impl FnOnce(TextureSource<'_>, (u32, u32)) -> R,
    ) -> R {
        let owner = self.owner.lock();
        let cache = owner.cache.as_ref().expect("Prepared retained Movie RGBA");
        assert!(!cache.dirty);
        draw(cache.texture.as_texture_source(), cache.extent)
    }
    #[cfg(test)]
    pub(crate) fn reference_state(
        &self,
    ) -> (
        u32,
        bool,
        u64,
        Option<shin_video::FrameTiming>,
        Option<(u32, u32)>,
        bool,
        bool,
        usize,
    ) {
        let owner = self.owner.lock();
        (
            owner.status(),
            owner.desired_playing,
            owner.media_time_micros(),
            owner
                .player
                .as_ref()
                .and_then(HigurashiVideoPlayerHandle::current_frame_timing),
            owner.cache.as_ref().map(|cache| cache.extent),
            owner.cache.as_ref().is_some_and(|cache| cache.dirty),
            owner
                .loading
                .as_ref()
                .is_some_and(|loading| loading.task.is_some()),
            Arc::as_ptr(&self.owner) as usize,
        )
    }
    #[cfg(test)]
    pub(crate) fn reference_retirement(&self) -> Option<shin_tasks::LoaderRetirement> {
        self.owner.lock().reference_retirement()
    }
    pub fn advance(&self) {
        let mut owner = self.owner.lock();
        if let Err(error) = self.services.advance(&mut owner) {
            // Existing layer clocks have a void update interface. Keep the
            // first explicit fault for the next runner/prepare boundary.
            if owner.fault.is_none() {
                owner.fault = Some(format!("{error:#}"));
            }
        }
    }
    pub fn status(&self) -> u32 {
        self.owner.lock().status()
    }
    pub fn fault(&self) -> Option<String> {
        self.owner.lock().fault.clone()
    }
    pub fn prepare(&self, context: &mut PreRenderContext) -> Result<bool> {
        self.owner.lock().prepare(context)
    }
    pub fn render(
        &self,
        pass: &mut RenderPass,
        properties: &HigurashiProperties,
        parent: &HigurashiTransform,
        stencil: u8,
        kind: PassKind,
    ) -> Result<()> {
        self.owner
            .lock()
            .render(pass, properties, parent, stencil, kind)
    }
}
