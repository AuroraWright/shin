//! Updated58b44/59480/594fc startup. The supplied logo worker sets phase-1;
//! the dormant timed logo table must not be used by normal application startup.
use crate::{
    adv::{character_view::CharacterTextures, higurashi::message_ui::HigurashiMessageTextures},
    asset::{font::GpuFontLazy, system::AssetServer},
    audio::{SystemSoundPlayer, VoicePlayer},
};
use anyhow::{Result, bail, ensure};
use parking_lot::Mutex;
use shin_core::{
    format::{scenario::higurashi::HigurashiScenario, sysse::SysSe},
    time::Ticks,
};
use std::sync::{
    Arc,
    atomic::{AtomicI32, Ordering},
    mpsc,
};

pub(super) struct CommonAssets {
    pub scenario: Arc<HigurashiScenario>,
    pub font: Arc<GpuFontLazy>,
    pub system: Arc<CharacterTextures>,
    pub message: Arc<HigurashiMessageTextures>,
    // Higurashi's effects archive has no Umineko breakup texture.
    #[allow(dead_code, reason = "Keep the startup effects archive alive for the shared native resource lifetime.")]
    pub effects: Arc<CharacterTextures>,
    pub sysse: Arc<SysSe>,
}
impl CommonAssets {
    async fn load(server: &AssetServer, bank: mpsc::Sender<Arc<SysSe>>) -> Result<Self> {
        let sysse: Arc<SysSe> = server.load("/sysse.bin").await?;
        let _ = bank.send(sysse.clone());
        let font = server.load("/newrodin.fnt").await?;
        let system = server.load("/systex.txa").await?;
        let message = server.load("/msgtex.txa").await?;
        let effects = server.load("/effects.txa").await?;
        let scenario = server.load("/main.snr").await?;
        Ok(Self {
            scenario,
            font,
            system,
            message,
            effects,
            sysse,
        })
    }
}
#[derive(Default)]
struct Model {
    phase: u32,
}
impl Model {
    fn poll(&mut self, logo_phase: i32, common_status: u32, tasks_empty: bool) -> bool {
        if self.phase == 0 && tasks_empty && logo_phase < 0 && common_status == 2 {
            self.phase = 1;
            true
        } else {
            false
        }
    }
}
struct Job<T> {
    task: Option<shin_tasks::AsyncTask<Result<T>>>,
    retirement: Option<shin_tasks::LoaderRetirement>,
}
impl<T: Send + 'static> Job<T> {
    fn start(future: impl std::future::Future<Output = Result<T>> + Send + 'static) -> Self {
        let task = shin_tasks::async_io::spawn_loader(future);
        let retirement = task.retirement_handle().unwrap();
        Self {
            task: Some(task),
            retirement: Some(retirement),
        }
    }
    fn poll(&mut self) -> Option<Result<T>> {
        let result = self.task.as_mut()?.poll_naive()?;
        self.task = None;
        Some(result)
    }
}
impl<T> Drop for Job<T> {
    fn drop(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(task) = self.task.take() {
                task.cancel_and_retire();
            }
            if let Some(retirement) = self.retirement.take() {
                retirement.retire(());
            }
        }
    }
}
/// The save-loading child is supplied by the game-specific progress adapter.
/// Publication merely requests that child; it does not invent loaded progress.
pub(super) struct FirstBoot {
    model: Model,
    logo_phase: Arc<AtomicI32>,
    logo: Job<()>,
    common: Job<CommonAssets>,
    assets: Option<Arc<CommonAssets>>,
    bank: mpsc::Receiver<Arc<SysSe>>,
    sounds: Arc<Mutex<SystemSoundPlayer>>,
    voice: VoicePlayer,
    fault: Option<String>,
}
impl FirstBoot {
    pub fn new(
        server: Arc<AssetServer>,
        manager: Arc<shin_audio::AudioManager>,
        sounds: Arc<Mutex<SystemSoundPlayer>>,
    ) -> Self {
        let logo_phase = Arc::new(AtomicI32::new(0));
        let phase = logo_phase.clone();
        let logo = Job::start(async move {
            phase.store(-1, Ordering::Release);
            Ok(())
        });
        let (send, bank) = mpsc::channel();
        let worker_server = server.clone();
        let common = Job::start(async move { CommonAssets::load(&worker_server, send).await });
        Self {
            model: Model::default(),
            logo_phase,
            logo,
            common,
            assets: None,
            bank,
            sounds,
            voice: VoicePlayer::new(manager, server),
            fault: None,
        }
    }
    /// Call after the retained child mode stack's poll. The native condition
    /// reads the logo side effect independently of its future's status word.
    pub fn poll(&mut self, delta: Ticks, tasks_empty: bool) -> Result<Option<Arc<CommonAssets>>> {
        ensure!(
            delta.as_f32().is_finite() && delta.as_f32() >= 0.0,
            "Invalid startup delta"
        );
        if let Some(fault) = &self.fault {
            bail!("Higurashi startup failed: {fault}");
        }
        let result = (|| {
            if let Some(result) = self.logo.poll() {
                result?;
            }
            if let Ok(bank) = self.bank.try_recv() {
                self.sounds.lock().bank = Some(bank);
            }
            if let Some(result) = self.common.poll() {
                let assets = Arc::new(result?);
                self.sounds.lock().bank = Some(assets.sysse.clone());
                self.assets = Some(assets);
            }
            let request = self.model.poll(
                self.logo_phase.load(Ordering::Acquire),
                if self.assets.is_some() { 2 } else { 0 },
                tasks_empty,
            );
            self.voice.update(delta);
            Ok(if request { self.assets.clone() } else { None })
        })();
        if let Err(error) = &result {
            self.fault = Some(format!("{error:#}"));
        }
        result
    }
    /// Updated592a4's active path clears black,then draws retained children.
    pub fn render(
        &self,
        pass: &mut shin_render::render_pass::RenderPass,
        children: impl FnOnce(&mut shin_render::render_pass::RenderPass),
    ) {
        pass.clear(
            Some(shin_core::primitives::color::UnormColor::BLACK),
            Some(0),
            Some(1.0),
        );
        children(pass);
    }
    #[cfg(test)]
    pub(super) fn reference_retirements(&self) -> Vec<shin_tasks::LoaderRetirement> {
        [self.logo.retirement.clone(), self.common.retirement.clone()]
            .into_iter()
            .flatten()
            .collect()
    }
}

#[cfg(test)]
mod tests;
