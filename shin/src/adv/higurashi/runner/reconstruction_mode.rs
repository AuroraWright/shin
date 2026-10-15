use super::super::{
    read_history::MessageReadHistory,
    reconstruction_mode::{OverlayClock, Phase, ReconstructionMode, Services},
    replay::{ReplaySession, Status as ReplayStatus},
};
use super::*;
use crate::asset::mask::MaskTexture;
use shin_render::render_texture::RenderTexture;
use std::sync::atomic::{AtomicBool, Ordering};

/// Native overlay source is a captured composed frame, retained even after its
/// fade endpoint. The drawing owner consumes the snapshot and mask weights.
pub struct Overlay {
    pub clock: OverlayClock,
    pub snapshot: RenderTexture,
    pub mask: Option<Arc<MaskTexture>>,
}
struct Prepared {
    replay: ReplaySession,
    layers: Option<super::super::layers::ReadyReconstruction>,
    constructed: bool,
    construction_fault: Option<String>,
    mask: Option<Arc<MaskTexture>>,
}
pub(in crate::adv::higurashi) struct Transition {
    pub mode: ReconstructionMode,
    worker: Option<shin_tasks::AsyncTask<Result<Prepared>>>,
    retirement: Option<shin_tasks::LoaderRetirement>,
    ready: Option<Prepared>,
    cancelled: Arc<AtomicBool>,
    fault: Option<String>,
    publication_started: bool,
    seeds: Box<dyn FnMut() -> [u32; 4] + Send>,
}

impl Drop for Transition {
    fn drop(&mut self) {
        // AsyncTask drop cancels its future. A compute replay already running
        // also needs this flag to leave its next bounded poll and retire.
        self.cancelled.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker.cancel_and_retire();
        }
        if let Some(retirement) = self.retirement.take() {
            retirement.retire(());
        }
    }
}

/// Native55c7c/96eb4 have menu/indicator owners separate from the runner.
/// The platform also resets its ADV410 blackout value here.
pub trait ReconstructionUi {
    fn reset_blackout_and_indicators(&mut self);
}

impl HigurashiRunner {
    /// Original11cd8 creates an ADV without a source VM. Browser jumps use a
    /// fresh empty archive and a snapshot of the previous composed platform frame.
    pub(in crate::adv::higurashi) fn begin_fresh_reconstruction_mode(
        &mut self,
        replay: ReplaySession,
        mode: u32,
        snapshot: RenderTexture,
        history: MessageReadHistory,
        seeds: impl FnMut() -> [u32; 4] + Send + 'static,
    ) -> Result<()> {
        ensure!(
            self.completed_commands == 0 && self.pending.is_none() && self.read_state.is_none(),
            "Fresh ADV reconstruction must precede command execution"
        );
        self.begin_reconstruction_mode(replay, mode, snapshot, history, seeds)?;
        self.vm = None;
        self.retired_vm_position = None;
        self.read_state = Some(super::super::read_state::CommandReadState::new());
        Ok(())
    }
    /// Capture the composed scene/message owner before freezing the
    /// source runner. No manual build or platform save operation is involved.
    pub fn capture_and_begin_reconstruction_mode(
        &mut self,
        context: &mut crate::render::PreRenderContext,
        screens: &mut super::super::scene_textures::HigurashiSceneTextures,
        replay: ReplaySession,
        mode: u32,
        history: MessageReadHistory,
        seeds: impl FnMut() -> [u32; 4] + Send + 'static,
    ) -> Result<()> {
        let snapshot = screens.capture_frame(context, self)?;
        self.begin_reconstruction_mode(replay, mode, snapshot, history, seeds)
    }

    pub(crate) fn check_reconstruction_start(&self, replay: &ReplaySession) -> Result<()> {
        ensure!(
            self.reconstruction.is_none(),
            "Higurashi reconstruction already active"
        );
        ensure!(
            self.vm_fault.is_none(),
            "Cannot reconstruct a faulted Higurashi runner"
        );
        ensure!(
            self.reconstruction_result.is_none(),
            "Install the prior reconstructed progress before starting another mode"
        );
        replay.check_reconstruction(&self.scenario)
    }
    /// The host captures the composed frame (14d30) before starting this mode.
    /// Preparation freezes command/scene clocks, while existing audio and the
    /// overlay keep their host update. No user saves or choice results are made.
    pub fn begin_reconstruction_mode(
        &mut self,
        mut replay: ReplaySession,
        mode: u32,
        snapshot: RenderTexture,
        history: MessageReadHistory,
        seeds: impl FnMut() -> [u32; 4] + Send + 'static,
    ) -> Result<()> {
        self.check_reconstruction_start(&replay)?;
        replay.set_quiz_score(self.quiz_score);
        let assets = self.assets.clone();
        let scenario = self.scenario.clone();
        let setting = self.layer_setting;
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancellation = cancelled.clone();
        let worker = shin_tasks::async_io::spawn_loader(async move {
            let replay = shin_tasks::compute::spawn(move || -> Result<_> {
                while replay.pump(&history, 256, 10000, || {
                    cancellation.load(Ordering::Acquire)
                })? != ReplayStatus::Ready
                {
                    ensure!(
                        replay.status() != ReplayStatus::Cancelled,
                        "Higurashi reconstruction cancelled"
                    );
                }
                Ok(replay)
            })
            .await?;
            let layers = HigurashiLayers::prepare_reconstruction(
                &replay.layers,
                &assets,
                &scenario,
                setting,
            )
            .await?;
            let mask = if mode == 0 {
                Some(assets.load("/default.msk").await?)
            } else {
                None
            };
            Ok(Prepared {
                replay,
                layers: Some(layers),
                constructed: false,
                construction_fault: None,
                mask,
            })
        });
        let retirement = worker.retirement_handle();
        //55380 replaces the capture without clearing an earlier mask binding.
        // The later553e4/55494 fade operation replaces/releases that binding.
        let previous = self.reconstruction_overlay.take();
        let mut clock = OverlayClock::captured(true);
        let mask = previous.and_then(|old| {
            clock.mask_present = old.clock.mask_present;
            clock.softness = old.clock.softness;
            clock.flags = old.clock.flags;
            old.mask
        });
        self.reconstruction_overlay = Some(Overlay {
            clock,
            snapshot,
            mask,
        });
        self.reconstruction = Some(Box::new(Transition {
            mode: ReconstructionMode::new(mode),
            worker: Some(worker),
            retirement,
            ready: None,
            cancelled,
            fault: None,
            publication_started: false,
            seeds: Box::new(seeds),
        }));
        Ok(())
    }
    /// Published session retains the new checkpoint archive and current progress.
    /// The host installs these before its next native command pump.
    pub fn take_reconstruction_result(&mut self) -> Option<ReplaySession> {
        self.reconstruction_result.take()
    }
    pub fn reconstruction_phase(&self) -> Option<Phase> {
        self.reconstruction.as_ref().map(|o| o.mode.phase())
    }
    pub fn reconstruction_overlay(&self) -> Option<&Overlay> {
        self.reconstruction_overlay.as_ref()
    }
    /// Poll outside loader executor workers: old asset commands are cancelled
    /// and their registered decoder work is drained at source VM retirement.
    pub fn poll_reconstruction_mode(
        &mut self,
        audio: &mut impl super::super::live_reconstruction::ReconstructionAudio,
        ui: &mut impl ReconstructionUi,
    ) -> Result<Phase> {
        let phase = self.poll_retained_reconstruction_mode(audio, ui)?;
        if phase == Phase::Complete {
            drop(self.take_completed_reconstruction_mode());
        }
        Ok(phase)
    }
    /// The ADV mode stack transfers the completed owner into its current
    /// snapshot and destroys it at the next poll's completion-pruning pass.
    pub(in crate::adv::higurashi) fn take_completed_reconstruction_mode(
        &mut self,
    ) -> Option<Box<Transition>> {
        if self.reconstruction_phase() == Some(Phase::Complete) {
            self.reconstruction.take()
        } else {
            None
        }
    }
    pub(in crate::adv::higurashi) fn poll_retained_reconstruction_mode(
        &mut self,
        audio: &mut impl super::super::live_reconstruction::ReconstructionAudio,
        ui: &mut impl ReconstructionUi,
    ) -> Result<Phase> {
        self.poll_retained_reconstruction_mode_with_settings(audio, ui, None)
    }
    pub(in crate::adv::higurashi) fn poll_retained_reconstruction_mode_with_settings(
        &mut self,
        audio: &mut impl super::super::live_reconstruction::ReconstructionAudio,
        ui: &mut impl ReconstructionUi,
        settings: Option<&mut shin_core::format::save::higurashi::SaveData>,
    ) -> Result<Phase> {
        let mut owner = self
            .reconstruction
            .take()
            .context("No Higurashi reconstruction mode")?;
        let result = (|| {
            if let Some(fault) = &owner.fault {
                anyhow::bail!("Higurashi reconstruction worker failed: {fault}");
            }
            if let Some(worker) = &mut owner.worker {
                if let Some(result) = worker.poll_naive() {
                    owner.worker = None;
                    match result {
                        Ok(prepared) => owner.ready = Some(prepared),
                        Err(error) => {
                            owner.fault = Some(format!("{error:#}"));
                            return Err(error);
                        }
                    }
                }
            }
            let ready = u32::from(owner.ready.is_some());
            let mask_present = owner.ready.as_ref().is_some_and(|p| p.mask.is_some());
            // Clock is temporarily separated to permit callbacks to install mask
            // resources into the retained snapshot owner without borrow aliasing.
            if ready == 1 && owner.mode.phase() == Phase::Preparing {
                owner.publication_started = true;
            }
            let mut clock = self.reconstruction_overlay.as_ref().unwrap().clock.clone();
            let phase = owner.mode.poll(
                ready,
                mask_present,
                &mut clock,
                &mut Publication {
                    runner: self,
                    ready: &mut owner.ready,
                    audio,
                    ui,
                    seeds: &mut *owner.seeds,
                    settings,
                },
            );
            self.reconstruction_overlay.as_mut().unwrap().clock = clock;
            phase
        })();
        if result.as_ref().is_ok_and(|phase| *phase == Phase::Complete) {
            self.reconstruction_result = owner.ready.take().map(|p| p.replay);
        }
        self.reconstruction = Some(owner);
        result
    }
    /// Desktop cancellation before publication retains the source VM and
    /// releases/drains prepared resources. Once retired, the fade must finish.
    pub fn cancel_reconstruction_mode(&mut self) -> Result<()> {
        ensure!(
            self.vm.is_some(),
            "Fresh ADV reconstruction has no source VM to restore"
        );
        let owner = self
            .reconstruction
            .as_ref()
            .context("No Higurashi reconstruction mode")?;
        ensure!(
            !owner.publication_started,
            "Higurashi publication has begun; retry and finish the mode"
        );
        ensure!(
            owner.mode.phase() == Phase::Preparing,
            "Published Higurashi reconstruction must finish its fade"
        );
        let mut owner = self.reconstruction.take().unwrap();
        owner.cancelled.store(true, Ordering::Release);
        if let Some(worker) = owner.worker.take() {
            worker.cancel_and_retire();
        }
        self.reconstruction_overlay = None;
        Ok(())
    }
}
struct Publication<'a, A, U> {
    runner: &'a mut HigurashiRunner,
    ready: &'a mut Option<Prepared>,
    audio: &'a mut A,
    ui: &'a mut U,
    seeds: &'a mut dyn FnMut() -> [u32; 4],
    settings: Option<&'a mut shin_core::format::save::higurashi::SaveData>,
}
impl<A: super::super::live_reconstruction::ReconstructionAudio, U: ReconstructionUi> Services
    for Publication<'_, A, U>
{
    fn publish_resources(&mut self) -> Result<()> {
        let prepared = self.ready.as_mut().unwrap();
        if !prepared.constructed {
            if let Some(fault) = &prepared.construction_fault {
                anyhow::bail!("Higurashi reconstruction constructor failed: {fault}");
            }
            ensure!(
                self.settings.is_some() || !prepared.layers.as_ref().unwrap().requires_settings(),
                "Live Higurashi TipsGet requires the actual settings acquired-bit bank"
            );
            ensure!(
                self.runner.frame.scene.layers().has_movie_services()
                    || !prepared.layers.as_ref().unwrap().requires_media(),
                "Reconstructed Higurashi Movie requires configured media services"
            );
            let result = self.runner.frame.scene.publish_reconstruction_in_place(
                prepared.layers.take().unwrap(),
                &mut self.seeds,
                self.settings.as_deref_mut(),
            );
            match result {
                Ok(()) => prepared.constructed = true,
                Err(error) => {
                    // Constructors may already have consumed entropy/acquired
                    // bits. Retain the explicit desktop fault rather than
                    // repeating them or unwrapping a consumed descriptor.
                    prepared.construction_fault = Some(format!("{error:#}"));
                    return Err(error);
                }
            }
        }
        if let Some(Pending {
            owner: Owner::Clock(wait),
            ..
        }) = &mut self.runner.pending
        {
            wait.remaining = 0.0;
            wait.allow_interrupt = 0;
        }
        self.audio.stop();
        self.runner
            .frame
            .message
            .prepare_reconstruction(&prepared.replay.read.descriptors().message, self.audio);
        if let Err(error) = super::super::live_reconstruction::restore_audio(
            prepared.replay.read.descriptors(),
            self.audio,
        ) {
            self.runner.vm_fault = Some(format!(
                "Higurashi reconstruction publication requires retry: {error:#}"
            ));
            return Err(error);
        }
        // Resource publication is retained in-place across an audio retry;
        // constructors/retirements are never repeated after a consumed prefix.
        self.runner.reconstruction_overlay.as_mut().unwrap().mask = prepared.mask.take();
        Ok(())
    }
    fn reset_blackout_and_indicators(&mut self) {
        self.ui.reset_blackout_and_indicators();
    }
    fn clear_message(&mut self) {
        self.runner.frame.message.clear_reconstruction_message();
    }
    fn retire_source_vm(&mut self) {
        if self.runner.vm.is_some() {
            self.runner.retired_vm_position = Some((
                self.runner.position(),
                self.runner.next_position(),
                self.runner.random_state(),
            ));
        }
        if let Some(pending) = self.runner.pending.take() {
            if let Owner::Asset { task, .. } = pending.owner {
                task.cancel_and_retire();
            }
        }
        self.runner.frame.scene.retire_source_command();
        self.runner.vm = None;
        self.runner.response = CommandResult::None;
    }
    fn enable_animations(&mut self) {}
    fn install_replayed_vm(&mut self) {
        let mut prepared = self.ready.take().unwrap();
        let replay = &mut prepared.replay;
        self.runner.vm = replay.vm.take();
        self.runner.read_state = Some(replay.read.clone());
        self.runner.pending = replay.pending.take().map(|p| Pending {
            command: p.command,
            owner: Owner::New,
            read_prepared: true,
            message_copied: false,
            autosave: p.autosave,
            choice: p.choice,
            character: p.character,
            tips_get: p.tips_get,
            notification: p.notification,
            kakera: p.kakera,
            scenario_select: p.scenario_select,
            quiz: p.quiz,
            exit: None,
            fake_select: None,
        });
        self.runner.response = std::mem::replace(&mut replay.response, CommandResult::None);
        self.runner.exited = false;
        self.runner.closing = Default::default();
        self.runner.vm_fault = None;
        self.runner.completed_commands = 0;
        self.runner.retired_vm_position = None;
        replay.mark_published();
        // Checkpoint/progress output remains retrievable by the host.
        *self.ready = Some(prepared);
    }
}
