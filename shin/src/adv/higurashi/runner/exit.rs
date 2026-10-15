//! Actual448c services over the retained ADV message,clock,wiper and audio.
use super::*;
use crate::adv::higurashi::{
    exit::{Begin, Destination, ExitCommand, ExitServices},
    input::InputState,
    live_reconstruction::ReconstructionAudio,
    progress::ProgressMemory,
    ui_state::HigurashiUiState,
};

#[derive(Default)]
pub(super) struct Closing {
    pub exiting: bool,
    pub wait: Option<Wait>,
}
struct Services<'a, A, D> {
    closing: &'a mut Closing,
    message: &'a mut HigurashiMessageOwner,
    scene: &'a HigurashiScene,
    audio: &'a mut A,
    ui: &'a mut HigurashiUiState,
    input: &'a mut InputState,
    modes_ready: bool,
    destination: D,
}
impl<A: ReconstructionAudio, D: FnMut(Destination, &mut ProgressMemory, &mut A) -> Result<()>> ExitServices
    for Services<'_, A, D>
{
    fn begin(&mut self, action: Begin) -> Result<()> {
        match action {
            Begin::MarkExiting => self.closing.exiting = true,
            // ADV3fc is the auto/skip word. Counted pause is ADV3f4.
            Begin::ClearAutoSkip => self.input.modes = 0,
            Begin::HideMessage => self.message.close_for_exit()?,
            Begin::StopVoice => self.audio.stop(),
            Begin::FinishStoppingEffects => self.audio.prepare_effect_reconstruction()?,
            Begin::StopEffects => self.audio.stop_all_effects(0)?,
            Begin::StopNotification => self.ui.indicator.stop(),
            Begin::StopMenuIndicator => self.ui.menu.stop(),
            Begin::WaitThirtyTicks => self.closing.wait = Some(Wait::new(30, 0)),
        }
        Ok(())
    }
    fn wait_busy(&mut self) -> Result<bool> {
        Ok(self.closing.wait.is_some_and(|w| w.remaining > 0.0))
    }
    fn message_busy(&mut self) -> Result<bool> {
        Ok(self.message.playback.busy())
    }
    fn view_child_busy(&mut self) -> Result<bool> {
        // b94a8 returns the retained current-view wiper;presence blocks EXIT.
        Ok(self.scene.transition().is_some())
    }
    fn modes_ready(&mut self) -> Result<bool> {
        Ok(self.modes_ready)
    }
    fn achievement(&mut self, _: u32) -> Result<()> {
        // e8eb0 has only mutex lock/unlock on this updated Switch build.
        Ok(())
    }
    fn destination(&mut self, dest: Destination, progress: &mut ProgressMemory) -> Result<()> {
        (self.destination)(dest, progress, self.audio)
    }
}
impl HigurashiRunner {
    /// Concrete Gameplay retains EXIT until its real closing/save/scene owner
    /// retires the ADV. Explicit asset/VM inspections keep terminal reporting.
    pub(crate) fn retain_live_exit(&mut self) {
        self.live_exit_services = true;
    }
    pub fn exit_closing(&self) -> bool {
        self.closing.exiting
    }
    pub fn exit_command(&self) -> Option<&ExitCommand> {
        self.pending.as_ref()?.exit.as_ref()
    }
    pub fn poll_exit_command<A: ReconstructionAudio>(
        &mut self,
        progress: &mut ProgressMemory,
        commits_enabled: bool,
        audio: &mut A,
        ui: &mut HigurashiUiState,
        input: &mut InputState,
        modes_ready: bool,
        destination: impl FnMut(Destination, &mut ProgressMemory, &mut A) -> Result<()>,
    ) -> Result<Option<bool>> {
        ensure!(
            self.reconstruction.is_none(),
            "Higurashi reconstruction mode is active"
        );
        let pending = self
            .pending
            .as_mut()
            .context("No Higurashi command is pending")?;
        if !matches!(pending.command, Cmd::EXIT { arg1: 0, .. }) {
            return Ok(None);
        }
        ensure!(
            matches!(pending.owner, Owner::External),
            "Live EXIT requires its external continuation"
        );
        let command = pending
            .exit
            .as_mut()
            .context("Live EXIT constructor has not completed")?;
        Ok(Some(command.poll_live(
            progress,
            commits_enabled,
            &mut Services {
                closing: &mut self.closing,
                message: &mut self.frame.message,
                scene: &self.frame.scene,
                audio,
                ui,
                input,
                modes_ready,
                destination,
            },
        )?))
    }
}
