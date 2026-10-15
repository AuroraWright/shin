//! Retained reconstruction worker. Source VM command pumping stays frozen.
use super::*;
use crate::adv::gerokasu2::{
    reconstruction_mode::{OverlayClock, ReconstructionMode},
    replay::{ReplaySession, ReplayStatus},
};
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) struct Prepared {
    pub replay: ReplaySession,
    pub backend: Box<BasicLoadBackend>,
    pub mask: Option<Arc<crate::asset::mask::MaskTexture>>,
}
pub(super) struct Navigation {
    pub mode: ReconstructionMode,
    pub clock: OverlayClock,
    pub overlay: Option<super::super::reconstruction_mode::overlay::Overlay>,
    replay: Option<ReplaySession>,
    worker: Option<shin_tasks::AsyncTask<Result<Prepared>>>,
    retirement: Option<shin_tasks::LoaderRetirement>,
    pub ready: Option<Prepared>,
    cancelled: Arc<AtomicBool>,
    pub fault: Option<String>,
}
impl Navigation {
    pub fn new(replay: ReplaySession, mode: u32) -> Self {
        Self {
            mode: ReconstructionMode::new(mode),
            clock: OverlayClock::captured(mode != 3),
            overlay: None,
            replay: Some(replay),
            worker: None,
            retirement: None,
            ready: None,
            cancelled: Arc::new(AtomicBool::new(false)),
            fault: None,
        }
    }
    pub fn start(
        &mut self,
        source: Box<BasicLoadBackend>,
        seen: shin_core::format::save::index_set::IndexSet,
    ) {
        let Some(mut replay) = self.replay.take() else {
            return;
        };
        let mode = self.mode.mode;
        let cancelled = self.cancelled.clone();
        let worker = shin_tasks::async_io::spawn_loader(async move {
            let cancellation = cancelled.clone();
            replay = shin_tasks::compute::spawn(move || -> Result<_> {
                loop {
                    let status =
                        replay.pump(&seen, 256, 100000, || cancellation.load(Ordering::Acquire))?;
                    if status == ReplayStatus::Ready {
                        return Ok(replay);
                    }
                    anyhow::ensure!(
                        status == ReplayStatus::CommandBudget,
                        "Ger reconstruction cancelled"
                    );
                }
            })
            .await?;
            let backend = source
                .prepare_reconstruction(&replay.layers, &replay.save.descriptors, mode != 0, || {
                    cancelled.load(Ordering::Acquire)
                })
                .await?;
            let mask = if mode == 0 {
                Some(
                    source
                        .reconstruction_assets()
                        .load::<crate::asset::mask::MaskTexture, _>("/default.msk")
                        .await?,
                )
            } else {
                None
            };
            anyhow::ensure!(
                !cancelled.load(Ordering::Acquire),
                "Ger reconstruction cancelled"
            );
            Ok(Prepared {
                replay,
                backend,
                mask,
            })
        });
        self.retirement = worker.retirement_handle();
        self.worker = Some(worker);
    }
    pub fn needs_start(&self) -> bool {
        self.replay.is_some()
    }
    pub fn release_published_retirement(&mut self) {
        self.retirement = None;
    }
    pub fn poll_worker(&mut self) -> Result<()> {
        if let Some(fault) = &self.fault {
            anyhow::bail!("Ger reconstruction failed: {fault}");
        }
        if let Some(worker) = &mut self.worker {
            if let Some(result) = worker.poll_naive() {
                self.worker = None;
                match result {
                    Ok(prepared) => self.ready = Some(prepared),
                    Err(error) => {
                        self.fault = Some(format!("{error:#}"));
                        return Err(error);
                    }
                }
            }
        }
        Ok(())
    }
}
impl Drop for Navigation {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            #[cfg(not(target_arch = "wasm32"))]
            worker.cancel_and_retire();
            #[cfg(target_arch = "wasm32")]
            drop(worker);
        }
        if let Some(retirement) = self.retirement.take() {
            #[cfg(not(target_arch = "wasm32"))]
            retirement.retire(self.ready.take());
            #[cfg(target_arch = "wasm32")]
            drop(retirement);
        }
    }
}

pub(super) struct Publication<'a> {
    pub scene: &'a mut Gerokasu2PlayScene,
    pub ready: &'a mut Option<Prepared>,
    pub overlay: &'a mut Option<super::super::reconstruction_mode::overlay::Overlay>,
}
impl super::super::reconstruction_mode::Services for Publication<'_> {
    fn publish_resources(&mut self) -> Result<()> {
        let prepared = self.ready.as_mut().expect("ready reconstruction");
        prepared
            .replay
            .check_publication(&self.scene.services.scenario)?;
        self.scene.runner.voice_mut().stop();
        if let Err(error) = super::super::live_reconstruction::restore_audio(
            &prepared.replay.save.descriptors,
            &mut self.scene.services,
        ) {
            self.scene.runner.require_publication_retry(&error);
            return Err(error);
        }
        if let Some(overlay) = self.overlay.as_mut() {
            overlay.mask = prepared.mask.take();
        }
        Ok(())
    }
    fn reset_blackout_and_indicators(&mut self) {
        // Concrete desktop indicator owners are not installed yet.
    }
    fn clear_message(&mut self) {
        self.scene
            .runner
            .message_owner()
            .lock()
            .clear_for_reconstruction();
    }
    fn install_and_retire_source_vm(&mut self) {
        let mut prepared = self.ready.take().expect("published reconstruction");
        self.scene.services.backend.retire_reconstruction_jobs();
        self.scene
            .install_reconstruction(&mut prepared.replay, prepared.backend, false);
        // Loader ownership moved into the live scene; its retirement scope must
        // not be joined while the installed movie decoder is still running.
    }
}
