//! Older root transition lifecycle and WIPE/WIPEWAIT command scheduling.
//! Asset loads are awaited by the layer dispatcher before command preparation.
//! The ADV frame coordinator supplies the sibling message owner.
use anyhow::{Result, ensure};
use shin_core::{time::Ticks, vm::higurashi::command::HigurashiRuntimeCommand};

use super::{
    layers::{HigurashiLayers, PreparedBack},
    wipe::HigurashiDefaultTransition,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandStatus {
    Pending,
    Complete,
}

enum PendingCommand {
    Wipe {
        prepared: Option<PreparedBack>,
        duration: i32,
        flags: u32,
        effect: super::wipe::preparation::Preparation,
    },
    Wait,
}

pub struct HigurashiScene {
    layers: HigurashiLayers,
    transition: Option<HigurashiDefaultTransition>,
    pending: Option<PendingCommand>,
    history: super::scene_history::ScreenHistory,
    history_fault: Option<String>,
    #[cfg(test)]
    reference_wipe_gate: Option<super::wipe::preparation::ReferenceGate>,
}
impl Drop for HigurashiScene {
    fn drop(&mut self) {
        // Nativeb8b98 clears outgoing14b8 and releases the wiper before it
        // clears front/back. Rust's ordinary field order would clear layers first.
        drop(self.transition.take());
    }
}

impl HigurashiScene {
    pub fn new(layers: HigurashiLayers) -> Self {
        Self {
            layers,
            transition: None,
            pending: None,
            history: Default::default(),
            history_fault: None,
            #[cfg(test)]
            reference_wipe_gate: None,
        }
    }
    pub fn layers(&self) -> &HigurashiLayers {
        &self.layers
    }
    pub(crate) fn history(&self) -> &super::scene_history::ScreenHistory {
        &self.history
    }
    pub(crate) fn bind_history(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        extent: [u32; 2],
    ) -> Result<()> {
        self.history.bind(device, queue, extent)
    }
    /// b905c after keyed restoration clears outgoing/wiper, retaining this
    /// Screen's front/back retirees and current/previous image ownership.
    /// The source VM's WIPE/WIPEWAIT can still be alive here: native19578
    /// retires that command only after audio/UI publication and fade startup.
    pub(crate) fn publish_reconstruction_in_place(
        &mut self,
        prepared: super::layers::ReadyReconstruction,
        seeds: impl FnMut() -> [u32; 4],
        settings: Option<&mut shin_core::format::save::higurashi::SaveData>,
    ) -> Result<()> {
        let transition = &mut self.transition;
        self.layers
            .publish_reconstruction_with_tail(prepared, seeds, settings, || {
                drop(transition.take());
            })?;
        Ok(())
    }
    /// Source VM175b4 -> command6208 drops only command-owned preparation.
    /// Never poll/publish that old WIPE against the newly restored banks, and
    /// never clear a Screen-owned wiper here (b905c owns that earlier step).
    pub(crate) fn retire_source_command(&mut self) {
        drop(self.pending.take());
        self.layers.retire_source_command();
    }
    pub(crate) fn configure_movies(
        &mut self,
        services: crate::layer::user::higurashi::movie::Services,
    ) {
        self.layers.set_movie_services(services);
    }
    pub(crate) fn set_movie_preferences(&self, preferences: [u8; 3]) {
        self.layers.set_movie_preferences(preferences);
    }
    pub(crate) fn movie_fault(&self) -> Option<String> {
        self.layers.movie_fault().or_else(|| {
            self.transition
                .as_ref()
                .and_then(|t| t.previous.movie_fault())
        })
    }
    pub(crate) fn visual_fault(&self) -> Option<String> {
        self.wipe_fault().map(str::to_owned).or_else(|| {
            self.history_fault
                .clone()
                .or_else(|| self.layers.visual_fault())
                .or_else(|| {
                    self.transition
                        .as_ref()
                        .and_then(|t| t.previous.visual_fault())
                })
        })
    }
    fn wipe_fault(&self) -> Option<&str> {
        match &self.pending {
            Some(PendingCommand::Wipe { effect, .. }) => effect.fault(),
            _ => None,
        }
    }
    pub(crate) fn animations_enabled(&self) -> bool {
        match &self.pending {
            Some(PendingCommand::Wipe { effect, .. }) => effect.animations_enabled(),
            _ => true,
        }
    }
    pub(crate) fn commit_layer_metadata(
        &mut self,
        scenario: &shin_core::format::scenario::higurashi::HigurashiScenario,
        settings: Option<&mut shin_core::format::save::higurashi::SaveData>,
    ) -> Result<()> {
        self.layers.commit_pending_metadata(scenario, settings)
    }
    pub(crate) fn set_route_navigation(&mut self, value: bool) {
        self.layers.set_route_navigation(value);
        if let Some(transition) = &mut self.transition {
            transition.previous.set_route_navigation(value);
        }
    }
    /// The VM must not dispatch another command while this scene owns a wait.
    pub fn layers_mut(&mut self) -> Result<&mut HigurashiLayers> {
        ensure!(
            self.pending.is_none(),
            "Higurashi scene command is still pending"
        );
        Ok(&mut self.layers)
    }
    pub fn transition(&self) -> Option<&HigurashiDefaultTransition> {
        self.transition.as_ref()
    }
    #[cfg(test)]
    pub(crate) fn reference_set_transition(
        &mut self,
        previous: super::layers::HigurashiPreviousFront,
        duration: i32,
    ) {
        assert!(self.pending.is_none() && self.transition.is_none());
        self.transition = Some(HigurashiDefaultTransition::from_previous(
            previous, duration, 0, false,
        ));
    }
    #[cfg(test)]
    pub(crate) fn reference_native_wipe(&mut self, duration: u32) -> Result<()> {
        ensure!(
            self.pending.is_none() && self.transition.is_none(),
            "Native frame probe expects an empty outgoing plane/wiper"
        );
        let previous = self.layers.reference_rotate_planes()?;
        self.transition = Some(HigurashiDefaultTransition::from_previous(
            previous,
            duration as i32,
            0,
            false,
        ));
        Ok(())
    }
    pub fn command_pending(&self) -> bool {
        self.pending.is_some()
    }

    #[cfg(test)]
    pub(crate) fn reference_gate_next_mask(
        &mut self,
        gate: super::wipe::preparation::ReferenceGate,
    ) {
        assert!(self.reference_wipe_gate.is_none() && self.pending.is_none());
        self.reference_wipe_gate = Some(gate);
    }

    /// Returns None for commands owned by another dispatcher. Unsupported
    /// effect types fail before changing the bank state.
    pub fn dispatch(
        &mut self,
        command: &HigurashiRuntimeCommand,
        fast_skip: bool,
    ) -> Result<Option<CommandStatus>> {
        self.dispatch_with_assets(command, fast_skip, None)
    }
    pub(crate) fn dispatch_with_assets(
        &mut self,
        command: &HigurashiRuntimeCommand,
        fast_skip: bool,
        services: Option<(
            std::sync::Arc<crate::asset::system::AssetServer>,
            std::sync::Arc<shin_core::format::scenario::higurashi::HigurashiScenario>,
        )>,
    ) -> Result<Option<CommandStatus>> {
        ensure!(
            self.pending.is_none(),
            "Higurashi scene command is still pending"
        );
        self.pending = Some(match command {
            HigurashiRuntimeCommand::WIPE {
                args: [kind, flags, duration],
                params,
            } => {
                // Native 6a08 dispatches1..4; all other types fall through to
                // the default. Signed nonpositive durations bypass effects.
                ensure!(
                    *duration <= 0 || *kind != 1 || services.is_some(),
                    "Higurashi Mask wipe requires actual asset services"
                );
                let effect = super::wipe::preparation::Preparation::new(*kind, *params, services);
                #[cfg(test)]
                let effect = {
                    let mut effect = effect;
                    if *kind == 1 {
                        effect.reference_gate = self.reference_wipe_gate.take();
                    }
                    effect
                };
                PendingCommand::Wipe {
                    prepared: Some(self.layers.prepare_back()),
                    duration: *duration,
                    flags: *flags as u32,
                    effect,
                }
            }
            HigurashiRuntimeCommand::WIPEWAIT {} => PendingCommand::Wait,
            _ => return Ok(None),
        });
        Ok(Some(self.poll_command(fast_skip)))
    }

    /// Native phase0 waits for the existing root wiper before publishing live
    /// groups. Phase2's bit0 no-wait never bypasses this preparation wait.
    pub fn poll_command(&mut self, fast_skip: bool) -> CommandStatus {
        if let Some(PendingCommand::Wipe {
            prepared,
            duration,
            flags,
            effect,
        }) = &mut self.pending
        {
            if prepared.is_some() {
                if self.transition.is_some() {
                    return CommandStatus::Pending;
                }
                // Actual633c..65a0 loads valid Mask metadata even during fast
                // skip, disables both visual callbacks, then waits for ready2.
                if !effect.poll(*duration) {
                    return CommandStatus::Pending;
                }
                let previous = self.layers.publish_back(prepared.take().unwrap());
                if let Some(wiper) = effect.construct(*duration, fast_skip) {
                    self.transition = Some(HigurashiDefaultTransition::from_wiper(
                        previous, wiper, *flags,
                    ));
                }
            }
        }
        let complete = match &self.pending {
            Some(PendingCommand::Wipe { flags, .. }) => flags & 1 != 0 || self.transition.is_none(),
            Some(PendingCommand::Wait) => self.transition.is_none(),
            None => true,
        };
        if complete {
            self.pending = None;
            CommandStatus::Complete
        } else {
            CommandStatus::Pending
        }
    }

    /// Native ba2a8 updates root, outgoing, incoming and back groups, then the
    /// wiper. It clears outgoing children and releases the wiper on the same
    /// callback that returns false. ADV12da8 gates the entire root update.
    pub fn advance(
        &mut self,
        delta: Ticks,
        animations_enabled: bool,
        paused: bool,
        voice: Option<&dyn Fn(i32, f32) -> Option<u8>>,
    ) {
        if !animations_enabled || paused || !self.animations_enabled() {
            return;
        }
        self.layers.advance_message(delta);
        self.advance_screen(delta, voice);
    }

    pub(super) fn advance_screen(
        &mut self,
        delta: Ticks,
        voice: Option<&dyn Fn(i32, f32) -> Option<u8>>,
    ) {
        self.layers.advance_root(delta);
        if let Some(transition) = &mut self.transition {
            transition.previous.advance(delta, voice);
            if transition.previous.temporal_update_failed() {
                return;
            }
        }
        self.layers.advance_groups(delta, voice);
        if self.layers.temporal_update_failed() {
            return;
        }
        if self
            .transition
            .as_mut()
            .is_some_and(|transition| !transition.advance(delta))
        {
            self.transition = None;
        }
        // Actualba2a8 samples39 only after root/threegroups/wiper retirement.
        // Every enabled callback swaps, including delta0 and no subsequent draw.
        if let Err(error) = self
            .history
            .after_update(self.layers.screen_properties().values()[39])
        {
            self.history_fault
                .get_or_insert_with(|| format!("{error:#}"));
        }
    }

    pub(super) fn advance_message_properties(&mut self, delta: Ticks) {
        self.layers.advance_message(delta);
    }

    /// Root ba210 forwards skip to the wiper but leaves it owned until the
    /// next enabled update. WIPEWAIT therefore remains pending after skip.
    pub fn fast_forward_wiper(&mut self) {
        if let Some(transition) = &mut self.transition {
            transition.fast_forward();
        }
    }
    pub fn fast_forward_screen(&mut self) {
        self.layers.fast_forward_root();
        if let Some(transition) = &mut self.transition {
            transition.previous.fast_forward();
            if transition.previous.temporal_update_failed() {
                return;
            }
        }
        self.layers.fast_forward_groups();
        if self.layers.temporal_update_failed() {
            return;
        }
        self.fast_forward_wiper();
    }
}

#[cfg(test)]
mod tests;
