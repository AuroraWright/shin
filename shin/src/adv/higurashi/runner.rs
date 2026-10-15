//! Older VM command loop with retained continuations and real frame owners.
//! Platform/persistence/menu commands are explicit external continuations;
//! the loop never fabricates their result or silently skips their effects.
use super::{
    effect_commands::{self, EffectCommand, EffectWait},
    frame::HigurashiFrame,
    layers::HigurashiLayers,
    message_owner::HigurashiMessageOwner,
    message_playback::MessageVoice,
    music_commands::{self, MusicCommand, MusicWait},
    read_state::{CommandReadState, ReadPolicy, ReadServices},
    scene::{CommandStatus, HigurashiScene},
    voice_commands::{self, VoiceCommands},
};
use crate::asset::system::AssetServer;
use anyhow::{Context, Result, ensure};
use shin_core::{
    format::scenario::{
        higurashi::{HigurashiScenario, HigurashiVariant},
        instruction_elements::CodeAddress,
    },
    time::Ticks,
    vm::{
        command::CommandResult,
        higurashi::{HigurashiScripter, command::HigurashiRuntimeCommand as Cmd},
    },
};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Waiting,
    External,
    CommandBudget,
    Exited,
}

/// A type used only for the explicitly resolved inspection/test pump. Its
/// services can never be called because that path passes None.
struct NoReadServices;
impl ReadServices for NoReadServices {
    fn checkpoint(
        &mut self,
        _: super::read_state::Checkpoint,
        _: super::input::ReadState,
    ) -> Result<()> {
        unreachable!()
    }
    fn message_is_read(&self, _: u32) -> bool {
        unreachable!()
    }
    fn message_completed(&mut self, _: u32) -> Result<()> {
        unreachable!()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Policy {
    pub animations_enabled: bool,
    pub paused: bool,
    pub fast_skip: bool,
    /// Resolved native137d0(ADV, VM) read/skip/preferences decision for this
    /// command. It is independent of visual/timer fast skip.
    pub suppress_transient_audio: bool,
    pub read_flags: u32,
    pub text_speed: i32,
    pub outline_mode: u8,
}

/// Unlike later Umineko,12bec polls commands before12d68 updates the WAIT
/// clock. Dispatch installs the full duration; that same frame then advances it.
#[derive(Debug, Clone, Copy)]
struct Wait {
    remaining: f32,
    allow_interrupt: u8,
}
impl Wait {
    fn new(amount: i32, allow_interrupt: u8) -> Self {
        Self {
            remaining: amount.wrapping_abs() as f32,
            allow_interrupt,
        }
    }
    fn advance(&mut self, delta: f32) {
        if self.remaining > 0.0 {
            self.remaining -= delta;
        }
    }
    fn poll(&mut self, fast_skip: bool) -> bool {
        if fast_skip {
            self.remaining = 0.0;
            self.allow_interrupt = 0;
        }
        !(self.remaining > 0.0)
    }
    fn interrupt(&mut self) -> bool {
        if self.allow_interrupt != 1 {
            return false;
        }
        self.remaining = 0.0;
        true
    }
}

enum Owner {
    New,
    Frame,
    External,
    Clock(Wait),
    VoiceWait(i32),
    MovieWait(super::layers::MovieWait),
    LayerWait(super::layers::LayerWait),
    LayerUnload(super::layers::LayerUnload),
    Music(MusicWait),
    Effects(EffectWait),
    Asset {
        task: shin_tasks::AsyncTask<Result<super::layers::LoadedLayers>>,
        animate: bool,
    },
    AssetFault {
        message: String,
        animate: bool,
    },
}
struct Pending {
    command: Cmd,
    owner: Owner,
    read_prepared: bool,
    message_copied: bool,
    autosave: Option<super::autosave::AutosaveCommand>,
    choice: Option<super::choice::ChoiceCommand>,
    character: Option<super::character_selection::command::CharacterCommand>,
    tips_get: Option<super::tips_get::TipsGetCommand>,
    notification: Option<super::notification_command::NotificationCommand>,
    kakera: Option<super::kakera::KakeraCommand>,
    scenario_select: Option<super::scenario_select::ScenarioSelectCommand>,
    quiz: Option<super::quiz::QuizCommand>,
    exit: Option<super::exit::ExitCommand>,
    fake_select: Option<super::fake_select::command::Command>,
}
impl Pending {
    fn copy_message_start(
        &mut self,
        state: &CommandReadState,
        services: &mut dyn ReadServices,
    ) -> Result<()> {
        if !self.message_copied && matches!(self.command, Cmd::MSGSET { .. }) {
            services.message_started(state.timeline().boundary)?;
            self.message_copied = true;
        }
        Ok(())
    }
}
impl Owner {
    fn animations_enabled(&self) -> bool {
        match self {
            Self::Asset { animate, .. } | Self::AssetFault { animate, .. } => *animate,
            _ => true,
        }
    }
}

pub struct HigurashiRunner {
    scenario: Arc<HigurashiScenario>,
    assets: Arc<AssetServer>,
    layer_setting: u8,
    vm: Option<HigurashiScripter>,
    reconstruction: Option<Box<reconstruction_mode::Transition>>,
    reconstruction_overlay: Option<reconstruction_mode::Overlay>,
    retired_vm_position: Option<(CodeAddress, CodeAddress, u32)>,
    reconstruction_result: Option<super::replay::ReplaySession>,
    frame: HigurashiFrame,
    pending: Option<Pending>,
    response: CommandResult,
    exited: bool,
    live_exit_services: bool,
    closing: exit::Closing,
    quiz_score: i32,
    vm_fault: Option<String>,
    completed_commands: u64,
    read_state: Option<CommandReadState>,
}
impl HigurashiRunner {
    pub(crate) fn configure_movies(
        &mut self,
        manager: Arc<shin_audio::AudioManager>,
        preferences: [u8; 3],
        clock: Option<crate::layer::user::higurashi::movie::ActiveClock>,
    ) {
        self.frame
            .scene
            .configure_movies(crate::layer::user::higurashi::movie::Services::new(
                self.assets.clone(),
                manager,
                preferences,
                clock,
            ));
    }
    pub(crate) fn set_movie_preferences(&self, preferences: [u8; 3]) {
        self.frame.scene.set_movie_preferences(preferences);
    }
    pub(crate) fn set_layer_setting(&mut self, setting: u8) {
        self.layer_setting = setting;
    }
    pub(crate) fn begin_layer_reload(
        &self,
        setting: u8,
    ) -> shin_tasks::AsyncTask<Result<super::layers::reload::LoadedReload>> {
        let plan = self.frame.scene.layers().state.reload_plan();
        let assets = self.assets.clone();
        let scenario = self.scenario.clone();
        shin_tasks::async_io::spawn_loader(
            async move { plan.load(&assets, &scenario, setting).await },
        )
    }
    pub(crate) fn publish_layer_reload(
        &mut self,
        loaded: &mut super::layers::reload::LoadedReload,
    ) -> Result<()> {
        self.frame.scene.layers_mut()?.publish_reload(loaded, || {
            let mut bytes = [0; 16];
            getrandom::fill(&mut bytes)
                .map_err(|e| anyhow::anyhow!("Higurashi layer entropy: {e}"))?;
            Ok(std::array::from_fn(|i| {
                u32::from_ne_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
            }))
        })
    }
    pub(crate) fn assets(&self) -> &Arc<AssetServer> {
        &self.assets
    }
    pub(crate) fn scenario(&self) -> &Arc<HigurashiScenario> {
        &self.scenario
    }
    pub(crate) fn layer_setting(&self) -> u8 {
        self.layer_setting
    }
    /// Native11cd8 writes its third argument to ADV28. Quiz scenes return their
    /// score here;ordinary constructors preserve the supplied word as well.
    pub(crate) fn set_quiz_score(&mut self, score: i32) {
        self.quiz_score = score;
    }
    /// Full constructor-owned ADV28, independent of read/skip policy.
    pub(crate) fn native_constructor_word(&self) -> u32 {
        self.quiz_score as u32
    }
    #[cfg(test)]
    pub(crate) fn reference_register(
        &self,
        register: shin_core::format::scenario::instruction_elements::Register,
    ) -> Option<i32> {
        self.vm.as_ref().map(|vm| vm.read_register(register))
    }
    /// Build the native forward read-policy/event search from this runner's
    /// current boundary. The source runner and host memory remain unchanged.
    pub fn begin_forward_search(
        &self,
        memory: &super::checkpoints::CheckpointMemory,
        progress: &super::progress::ProgressMemory,
        kind: super::forward_search::Kind,
        read_only: bool,
    ) -> Result<Option<super::forward_search::ForwardSearch>> {
        ensure!(
            self.reconstruction.is_none(),
            "Higurashi reconstruction mode is active"
        );
        ensure!(
            self.vm_fault.is_none(),
            "Cannot search from a faulted Higurashi runner"
        );
        let read = self
            .read_state
            .as_ref()
            .context("Forward search requires native read state")?;
        super::forward_search::ForwardSearch::begin(
            self.scenario.clone(),
            memory,
            progress,
            read.timeline().boundary,
            read.state().flags,
            kind,
            read_only,
        )
    }
    /// Publish an18db0 replay session without decoding its retained command a
    /// second time. The fork keeps progress/archives for the host to install.
    /// An audio failure leaves the session Ready and the old runner faulted,
    /// permitting the same publication retry as final-memory preparation.
    pub async fn reconstruct_replay(
        &mut self,
        replay: &mut super::replay::ReplaySession,
        seeds: impl FnMut() -> [u32; 4],
        audio: &mut impl super::live_reconstruction::ReconstructionAudio,
    ) -> Result<()> {
        ensure!(
            self.reconstruction.is_none(),
            "Higurashi reconstruction mode owns publication"
        );
        replay.check_publication(&self.scenario)?;
        ensure!(
            self.frame.scene.transition().is_none(),
            "Higurashi reconstruction during a scene transition requires its mode owner"
        );
        ensure!(
            !matches!(
                self.pending.as_ref().map(|p| &p.owner),
                Some(Owner::Asset { .. })
            ),
            "Higurashi reconstruction must retire its asset command worker first"
        );
        let prepared = HigurashiLayers::prepare_reconstruction(
            &replay.layers,
            &self.assets,
            &self.scenario,
            self.layer_setting,
        )
        .await?;
        self.frame
            .scene
            .publish_reconstruction_in_place(prepared, seeds, None)?;
        audio.stop();
        self.frame
            .message
            .prepare_reconstruction(&replay.read.descriptors().message, audio);
        if let Err(error) =
            super::live_reconstruction::restore_audio(replay.read.descriptors(), audio)
        {
            self.vm_fault = Some(format!(
                "Higurashi reconstruction publication requires retry: {error:#}"
            ));
            return Err(error);
        }
        self.frame.scene.retire_source_command();
        self.vm = replay.vm.take();
        self.read_state = Some(replay.read.clone());
        self.pending = replay.pending.take().map(|p| Pending {
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
        self.response = std::mem::replace(&mut replay.response, CommandResult::None);
        self.exited = false;
        self.closing = Default::default();
        self.vm_fault = None;
        self.completed_commands = 0;
        replay.mark_published();
        Ok(())
    }
    /// Publish final memory after the native replay owner has reached its
    /// target. This immediate path has no blackout/overlay transition. Existing
    /// scene transitions and in-flight asset commands require their mode owner.
    /// Resource preparation leaves live owners unchanged on error. Audio errors
    /// retain their acknowledged prefix and fault command pumping until the
    /// caller retries publication. Progress/extra words belong to its fork owner.
    pub async fn reconstruct_final_memory(
        &mut self,
        memory: &super::checkpoints::AdvCheckpoint,
        seeds: impl FnMut() -> [u32; 4],
        audio: &mut impl super::live_reconstruction::ReconstructionAudio,
    ) -> Result<()> {
        ensure!(
            self.reconstruction.is_none(),
            "Higurashi reconstruction mode owns publication"
        );
        ensure!(
            memory.vm.matches_scenario(&self.scenario),
            "Higurashi reconstruction scenario mismatch"
        );
        ensure!(
            self.frame.scene.transition().is_none(),
            "Higurashi reconstruction during a scene transition requires its mode owner"
        );
        ensure!(
            !matches!(
                self.pending.as_ref().map(|p| &p.owner),
                Some(Owner::Asset { .. })
            ),
            "Higurashi reconstruction must retire its asset command worker first"
        );
        let prepared = HigurashiLayers::prepare_reconstruction(
            &memory.layers,
            &self.assets,
            &self.scenario,
            self.layer_setting,
        )
        .await?;
        self.frame
            .scene
            .publish_reconstruction_in_place(prepared, seeds, None)?;
        // Native19578 stops ADV voice, then840b8 stops the message voice.
        audio.stop();
        self.frame
            .message
            .prepare_reconstruction(&memory.read.descriptors().message, audio);
        if let Err(error) =
            super::live_reconstruction::restore_audio(memory.read.descriptors(), audio)
        {
            self.vm_fault = Some(format!(
                "Higurashi reconstruction publication requires retry: {error:#}"
            ));
            return Err(error);
        }
        self.frame.scene.retire_source_command();
        self.vm = Some(memory.vm.scripter());
        self.read_state = Some(memory.read.clone());
        self.pending = None;
        self.response = CommandResult::None;
        self.exited = false;
        self.closing = Default::default();
        self.vm_fault = None;
        self.completed_commands = 0;
        Ok(())
    }
    pub async fn load(
        assets: Arc<AssetServer>,
        scenario: Arc<HigurashiScenario>,
        init_val: i32,
        random_seed: u32,
        seeds: impl FnMut() -> [u32; 4],
    ) -> Result<Self> {
        ensure!(
            scenario.variant == HigurashiVariant::Updated,
            "Higurashi runner currently requires the updated scenario/UI resources"
        );
        let vm = HigurashiScripter::new(&scenario, init_val, random_seed)?;
        let message = HigurashiMessageOwner::load(&assets).await?;
        Ok(Self::from_loaded_parts(
            assets,
            scenario,
            Some(vm),
            message,
            seeds,
        ))
    }
    /// The native ADV constructor has no VM until its First resource gate.
    pub(crate) fn fresh_from_loaded_assets(
        assets: Arc<AssetServer>,
        scenario: Arc<HigurashiScenario>,
        message: HigurashiMessageOwner,
        seeds: impl FnMut() -> [u32; 4],
    ) -> Result<Self> {
        ensure!(
            scenario.variant == HigurashiVariant::Updated,
            "Fresh ADV requires updated Higurashi"
        );
        Ok(Self::from_loaded_parts(
            assets, scenario, None, message, seeds,
        ))
    }
    fn from_loaded_parts(
        assets: Arc<AssetServer>,
        scenario: Arc<HigurashiScenario>,
        vm: Option<HigurashiScripter>,
        message: HigurashiMessageOwner,
        seeds: impl FnMut() -> [u32; 4],
    ) -> Self {
        Self {
            scenario,
            assets,
            layer_setting: 0,
            vm,
            reconstruction: None,
            reconstruction_overlay: None,
            retired_vm_position: None,
            reconstruction_result: None,
            frame: HigurashiFrame::new(HigurashiScene::new(HigurashiLayers::new(seeds)), message),
            pending: None,
            response: CommandResult::None,
            exited: false,
            live_exit_services: false,
            closing: Default::default(),
            quiz_score: 0,
            vm_fault: None,
            completed_commands: 0,
            read_state: None,
        }
    }
    pub fn frame(&self) -> &HigurashiFrame {
        &self.frame
    }
    pub(crate) fn set_layer_route_navigation(&mut self, value: bool) {
        self.frame.scene.set_route_navigation(value);
    }
    #[cfg(test)]
    pub(crate) fn reference_frame_mut(&mut self) -> &mut HigurashiFrame {
        &mut self.frame
    }
    pub(crate) fn scene_layers_mut(&mut self) -> Result<&mut HigurashiLayers> {
        ensure!(
            self.reconstruction.is_none(),
            "Higurashi reconstruction mode is active"
        );
        self.frame.scene.layers_mut()
    }
    pub(super) fn set_message_hidden(&mut self, hidden: bool) {
        self.frame.message.playback.hidden = hidden;
    }
    pub fn position(&self) -> CodeAddress {
        self.vm
            .as_ref()
            .map(|vm| vm.position())
            .unwrap_or_else(|| self.retired_vm_position.unwrap().0)
    }
    pub fn next_position(&self) -> CodeAddress {
        self.vm
            .as_ref()
            .map(|vm| vm.next_position())
            .unwrap_or_else(|| self.retired_vm_position.unwrap().1)
    }
    pub fn random_state(&self) -> u32 {
        self.vm
            .as_ref()
            .map(|vm| vm.random_state())
            .unwrap_or_else(|| self.retired_vm_position.unwrap().2)
    }
    pub fn completed_commands(&self) -> u64 {
        self.completed_commands
    }
    pub fn pending_command(&self) -> Option<&Cmd> {
        self.pending.as_ref().map(|p| &p.command)
    }
    pub fn native_read_state(&self) -> Option<super::input::ReadState> {
        self.vm.as_ref()?;
        self.read_state.as_ref().map(CommandReadState::state)
    }
    pub fn native_message_timeline(&self) -> Option<super::read_state::MessageTimeline> {
        self.read_state.as_ref().map(CommandReadState::timeline)
    }
    pub fn native_save_metadata(&self) -> Option<&super::save_metadata::SaveMetadata> {
        self.read_state.as_ref().map(CommandReadState::metadata)
    }
    pub fn native_playback_descriptors(&self) -> Option<&super::descriptors::PlaybackDescriptors> {
        self.read_state.as_ref().map(CommandReadState::descriptors)
    }

    /// Publish queued listener calls before another constructor queries read
    /// history. Only an acknowledged prefix is removed on service failure.
    /// Completing a message never changes its cached command-current read bit.
    pub fn flush_message_completions(&mut self, services: &mut impl ReadServices) -> Result<()> {
        ensure!(
            self.read_state.is_some(),
            "Message read history requires a native VM"
        );
        let queue = &mut self.frame.message.playback.completed_messages;
        let mut acknowledged = 0;
        let result = (|| {
            for &id in queue.iter() {
                // Native16760 ignores signed-negative IDs, including encoded0.
                if (id as i32) >= 0 {
                    services.message_completed(id)?;
                }
                acknowledged += 1;
            }
            Ok(())
        })();
        queue.drain(..acknowledged);
        result
    }

    /// Complete exactly the command yielded as External. VM feedback must
    /// target that command's destination. Failed validation retains ownership.
    pub fn complete_external(&mut self, response: CommandResult) -> Result<()> {
        ensure!(
            self.reconstruction.is_none(),
            "Higurashi reconstruction mode is active"
        );
        let p = self
            .pending
            .as_ref()
            .context("No Higurashi external command is pending")?;
        ensure!(
            matches!(p.owner, Owner::External),
            "Higurashi command belongs to a frame/clock/audio/asset owner"
        );
        match (p.command.destination(), response) {
            (None, CommandResult::None) => {}
            (Some(expected), CommandResult::WriteMemory(actual, _)) if expected == actual => {}
            _ => anyhow::bail!("Higurashi external response does not match command destination"),
        }
        if let Some(character) = self.pending.as_mut().and_then(|p| p.character.as_mut()) {
            character.release(&mut self.frame.scene.layers_mut()?.state);
        }
        if let Some(tips) = self.pending.as_mut().and_then(|p| p.tips_get.as_mut()) {
            tips.release(&mut self.frame.scene.layers_mut()?.state);
        }
        if let Some(notification) = self.pending.as_mut().and_then(|p| p.notification.as_mut()) {
            notification.release(&mut self.frame.scene.layers_mut()?.state);
        }
        self.response = response;
        self.pending = None;
        self.completed_commands += 1;
        Ok(())
    }
    /// Resolve a retained variable command through its concrete memory owner.
    /// Unrelated external commands remain pending; the caller supplies live or
    /// reconstruction execution explicitly and owns checkpoint-time commits.
    pub fn complete_variable_command(
        &mut self,
        variables: &mut super::variables::HigurashiVariables,
        execution: super::variables::Execution,
    ) -> Result<bool> {
        let pending = self
            .pending
            .as_ref()
            .context("No Higurashi command is pending")?;
        ensure!(
            matches!(pending.owner, Owner::External),
            "Variable service requires an external continuation"
        );
        let Some(response) = variables.dispatch(&pending.command, execution)? else {
            return Ok(false);
        };
        self.complete_external(response)?;
        Ok(true)
    }
    /// Poll the retained AUTOSAVE through its concrete native memory owner.
    /// None means another external command; Some(false) retains the command at
    /// a replay target. A completed live poll includes notification acceptance.
    pub fn poll_autosave_command(
        &mut self,
        progress: &mut super::autosave::SaveProgress,
        memory: &mut super::autosave::AutosaveMemory,
        execution: super::autosave::Execution,
        services: &mut impl super::autosave::AutosaveServices,
    ) -> Result<Option<bool>> {
        ensure!(
            self.reconstruction.is_none(),
            "Higurashi reconstruction mode is active"
        );
        let pending = self
            .pending
            .as_mut()
            .context("No Higurashi command is pending")?;
        ensure!(
            matches!(pending.owner, Owner::External),
            "Autosave requires an external continuation"
        );
        if !matches!(pending.command, Cmd::AUTOSAVE {}) {
            return Ok(None);
        }
        let state = self
            .read_state
            .as_ref()
            .context("Autosave requires native preparation")?;
        let command = pending
            .autosave
            .as_mut()
            .context("Autosave constructor has not completed")?;
        let boundary = state.timeline().boundary;
        let complete = match execution {
            super::autosave::Execution::Live => {
                command.poll_live(progress, memory, boundary, state.metadata(), services)?
            }
            super::autosave::Execution::Replay => command.poll_replay(progress, boundary)?,
        };
        if complete {
            self.complete_external(CommandResult::None)?;
        }
        Ok(Some(complete))
    }
    pub fn autosave_flags(&self) -> Option<[u8; 2]> {
        self.pending
            .as_ref()?
            .autosave
            .as_ref()
            .map(|command| command.flags())
    }
    /// Variable and autosave continuations share this session's actual active
    /// response vector. The caller cannot supply a stale separate route copy.
    pub fn poll_progress_command(
        &mut self,
        progress: &mut super::progress::ProgressMemory,
        memory: &mut super::autosave::AutosaveMemory,
        execution: super::autosave::Execution,
        services: &mut impl super::autosave::AutosaveServices,
    ) -> Result<Option<bool>> {
        ensure!(
            self.reconstruction.is_none(),
            "Higurashi reconstruction mode is active"
        );
        let pending = self
            .pending
            .as_ref()
            .context("No Higurashi command is pending")?;
        ensure!(
            matches!(pending.owner, Owner::External),
            "Progress service requires an external continuation"
        );
        let state = self
            .read_state
            .as_ref()
            .context("Progress service requires native preparation")?;
        if matches!(pending.command, Cmd::AUTOSAVE {}) {
            return self.poll_autosave_command(
                progress.progress_mut(),
                memory,
                execution,
                services,
            );
        }
        let Some(response) =
            progress.dispatch_variables(&pending.command, execution, state.timeline().boundary)?
        else {
            return Ok(None);
        };
        self.complete_external(response)?;
        Ok(Some(true))
    }
    /// FakeSelect's first live poll attaches its automatic child and copies
    /// current progress once. Its VOID callback writes no response or boundary.
    pub(crate) fn poll_fake_select_command(
        &mut self,
        progress: &mut super::progress::ProgressMemory,
        memory: &mut super::autosave::AutosaveMemory,
        services: &mut impl super::fake_select::command::Services,
    ) -> Result<Option<bool>> {
        ensure!(self.reconstruction.is_none(), "Higurashi reconstruction mode is active");
        let pending = self.pending.as_mut().context("No Higurashi command is pending")?;
        ensure!(matches!(pending.owner, Owner::External), "FakeSelect requires its external continuation");
        if !matches!(pending.command, Cmd::FAKESELECT {}) { return Ok(None); }
        let boundary = self.read_state.as_ref().context("FakeSelect requires native preparation")?.timeline().boundary;
        let complete = pending.fake_select.as_mut().context("FakeSelect constructor has not completed")?
            .poll_live(progress, memory, boundary, services)?;
        if complete { self.complete_external(CommandResult::None)?; }
        Ok(Some(complete))
    }
    /// Resolve SELECT against the same active response vector as variables and
    /// autosaves. False retains replay stops/UI/clock failures; live handoff
    /// completes a consumed replay choice without creating UI or another copy.
    pub fn poll_choice_command(
        &mut self,
        progress: &mut super::progress::ProgressMemory,
        memory: &mut super::autosave::AutosaveMemory,
        history: &mut super::choice::ChoiceHistory,
        execution: super::autosave::Execution,
        services: &mut impl super::choice::ChoiceServices,
    ) -> Result<Option<bool>> {
        ensure!(
            self.reconstruction.is_none(),
            "Higurashi reconstruction mode is active"
        );
        let pending = self
            .pending
            .as_mut()
            .context("No Higurashi command is pending")?;
        ensure!(
            matches!(pending.owner, Owner::External),
            "Choice requires an external continuation"
        );
        if !matches!(pending.command, Cmd::SELECT { .. }) {
            return Ok(None);
        }
        let state = self
            .read_state
            .as_mut()
            .context("Choice requires native preparation")?;
        let choice = pending
            .choice
            .as_mut()
            .context("Choice constructor has not completed")?;
        let boundary = &mut state.timeline_mut().boundary;
        let publish = |dest, value| {
            self.vm
                .as_mut()
                .context("Higurashi VM is detached during reconstruction")?
                .write_pending_result(dest, value)
        };
        let complete = match execution {
            super::autosave::Execution::Live => {
                choice.poll_live(progress, memory, history, boundary, services, publish)?
            }
            super::autosave::Execution::Replay => {
                choice.poll_replay(progress, boundary, publish)?
            }
        };
        if complete {
            let value = choice.result().context("Completed choice has no result")?;
            let dest = choice.request().destination;
            self.complete_external(CommandResult::WriteMemory(dest, value))?;
        }
        Ok(Some(complete))
    }
    pub fn choice_replay_stop(&self) -> Option<i32> {
        self.pending
            .as_ref()?
            .choice
            .as_ref()
            .map(|choice| choice.replay_stop())
    }
    pub fn character_command(
        &self,
    ) -> Option<&super::character_selection::command::CharacterCommand> {
        self.pending.as_ref()?.character.as_ref()
    }
    pub fn tips_get_command(&self) -> Option<&super::tips_get::TipsGetCommand> {
        self.pending.as_ref()?.tips_get.as_ref()
    }
    pub(crate) fn notification_command(
        &self,
    ) -> Option<&super::notification_command::NotificationCommand> {
        self.pending.as_ref()?.notification.as_ref()
    }
    /// VOID notification callback; app retires both actual identities before
    /// complete_external releases its command reservation.
    pub(crate) fn poll_notification_command(
        &mut self,
        services: &mut impl super::notification_command::Services,
    ) -> Result<Option<bool>> {
        ensure!(
            self.reconstruction.is_none(),
            "Higurashi reconstruction mode is active"
        );
        let pending = self
            .pending
            .as_mut()
            .context("No Higurashi command is pending")?;
        if !matches!(pending.command, Cmd::OTSUGET { .. } | Cmd::KAKERAGET { .. }) {
            return Ok(None);
        }
        ensure!(
            matches!(pending.owner, Owner::External),
            "Notification requires an external continuation"
        );
        let owner = pending
            .notification
            .as_mut()
            .context("Notification constructor has not completed")?;
        Ok(Some(
            owner.poll_live(self.frame.scene.layers_mut()?, services)?,
        ))
    }
    /// Completion is acknowledged only after the app retires its actual input
    /// wrapper and original drawable by identity. Outer1 remains blocked while
    /// its independently copied TIPS scene request replaces the source.
    pub fn poll_tips_get_command(
        &mut self,
        progress: &mut super::progress::ProgressMemory,
        memory: &mut super::autosave::AutosaveMemory,
        execution: super::autosave::Execution,
        services: &mut impl super::tips_get::TipsGetServices,
    ) -> Result<Option<bool>> {
        ensure!(
            self.reconstruction.is_none(),
            "Higurashi reconstruction mode is active"
        );
        let pending = self
            .pending
            .as_mut()
            .context("No Higurashi command is pending")?;
        if !matches!(pending.command, Cmd::TIPSGET { .. }) {
            return Ok(None);
        }
        ensure!(
            matches!(pending.owner, Owner::External),
            "TIPSGET requires an external continuation"
        );
        let state = self
            .read_state
            .as_mut()
            .context("TIPSGET requires native preparation")?;
        let tips = pending
            .tips_get
            .as_mut()
            .context("TIPSGET constructor has not completed")?;
        let complete = match execution {
            super::autosave::Execution::Live => tips.poll_live(
                progress,
                memory,
                &mut state.timeline_mut().boundary,
                self.frame.scene.layers_mut()?,
                services,
            )?,
            super::autosave::Execution::Replay => tips.poll_replay(
                progress,
                &mut state.timeline_mut().boundary,
                self.quiz_score as u32,
            )?,
        };
        Ok(Some(complete))
    }
    pub fn kakera_command(&self) -> Option<&super::kakera::KakeraCommand> {
        self.pending.as_ref()?.kakera.as_ref()
    }
    pub fn scenario_select_command(&self) -> Option<&super::scenario_select::ScenarioSelectCommand> {
        self.pending.as_ref()?.scenario_select.as_ref()
    }
    pub fn quiz_command(&self) -> Option<&super::quiz::QuizCommand> {
        self.pending.as_ref()?.quiz.as_ref()
    }
    pub fn poll_quiz_command(
        &mut self,
        progress: &mut super::progress::ProgressMemory,
        memory: &mut super::autosave::AutosaveMemory,
        execution: super::autosave::Execution,
        services: &mut impl super::quiz::Services,
    ) -> Result<Option<bool>> {
        ensure!(
            self.reconstruction.is_none(),
            "Higurashi reconstruction mode is active"
        );
        let pending = self
            .pending
            .as_mut()
            .context("No Higurashi command is pending")?;
        if !matches!(pending.command, Cmd::QUIZ { .. }) {
            return Ok(None);
        }
        ensure!(
            matches!(pending.owner, Owner::External),
            "Quiz requires an external continuation"
        );
        let state = self
            .read_state
            .as_mut()
            .context("Quiz requires native preparation")?;
        let quiz = pending
            .quiz
            .as_mut()
            .context("Quiz constructor has not completed")?;
        let complete = match execution {
            super::autosave::Execution::Replay => quiz.poll_replay(
                progress,
                state.timeline().boundary,
                self.quiz_score,
                |dest, value| self.vm.as_mut().unwrap().write_pending_result(dest, value),
            )?,
            super::autosave::Execution::Live => {
                quiz.poll_live(progress, memory, state.timeline().boundary, services)?
            }
        };
        if complete {
            let dest = quiz.request().destination;
            let value = quiz
                .result()
                .unwrap_or_else(|| self.vm.as_ref().unwrap().read_register(dest));
            self.complete_external(CommandResult::WriteMemory(dest, value))?;
        }
        Ok(Some(complete))
    }
    pub fn poll_kakera_command(
        &mut self,
        progress: &mut super::progress::ProgressMemory,
        memory: &mut super::autosave::AutosaveMemory,
        execution: super::autosave::Execution,
        services: &mut impl super::kakera::KakeraServices,
    ) -> Result<Option<bool>> {
        ensure!(
            self.reconstruction.is_none(),
            "Higurashi reconstruction mode is active"
        );
        let pending = self
            .pending
            .as_mut()
            .context("No Higurashi command is pending")?;
        ensure!(
            matches!(pending.owner, Owner::External),
            "Kakera requires an external continuation"
        );
        if !matches!(pending.command, Cmd::KAKERA {}) {
            return Ok(None);
        }
        let state = self
            .read_state
            .as_ref()
            .context("Kakera requires native preparation")?;
        let command = pending
            .kakera
            .as_mut()
            .context("Kakera constructor has not completed")?;
        let complete = match execution {
            super::autosave::Execution::Live => {
                command.poll_live(progress, memory, state.timeline().boundary, services)?
            }
            super::autosave::Execution::Replay => {
                command.poll_replay(progress.progress(), state.timeline().boundary)?
            }
        };
        if complete {
            self.complete_external(CommandResult::None)?;
        }
        Ok(Some(complete))
    }
    pub fn poll_scenario_select_command(
        &mut self,
        progress: &mut super::progress::ProgressMemory,
        memory: &mut super::autosave::AutosaveMemory,
        execution: super::autosave::Execution,
        services: &mut impl super::scenario_select::ScenarioSelectServices,
    ) -> Result<Option<bool>> {
        ensure!(self.reconstruction.is_none(), "Higurashi reconstruction mode is active");
        let pending = self.pending.as_mut().context("No Higurashi command is pending")?;
        ensure!(matches!(pending.owner, Owner::External), "ScenarioSelect requires an external continuation");
        if !matches!(pending.command, Cmd::SNRSEL { .. }) {
            return Ok(None);
        }
        let state = self.read_state.as_ref().context("ScenarioSelect requires native preparation")?;
        let command = pending.scenario_select.as_mut().context("ScenarioSelect constructor has not completed")?;
        let complete = match execution {
            super::autosave::Execution::Live => command.poll_live(progress, memory, state.timeline().boundary, services)?,
            super::autosave::Execution::Replay => command.poll_replay(progress.progress(), state.timeline().boundary)?,
        };
        if complete {
            self.complete_external(CommandResult::None)?;
        }
        Ok(Some(complete))
    }
    /// Native character command retains loading before its initial11780 copy.
    /// Positive replay stops complete without calling any live UI/clock service.
    pub fn poll_character_command(
        &mut self,
        progress: &mut super::progress::ProgressMemory,
        memory: &mut super::autosave::AutosaveMemory,
        history: &mut super::choice::ChoiceHistory,
        execution: super::autosave::Execution,
        services: &mut impl super::character_selection::command::CharacterServices,
    ) -> Result<Option<bool>> {
        ensure!(
            self.reconstruction.is_none(),
            "Higurashi reconstruction mode is active"
        );
        let pending = self
            .pending
            .as_mut()
            .context("No Higurashi command is pending")?;
        ensure!(
            matches!(pending.owner, Owner::External),
            "Character selection requires an external continuation"
        );
        if !matches!(pending.command, Cmd::CHARSELECT { .. }) {
            return Ok(None);
        }
        let state = self
            .read_state
            .as_mut()
            .context("Character selection requires native preparation")?;
        let character = pending
            .character
            .as_mut()
            .context("Character constructor has not completed")?;
        let publish = |dest, value| {
            self.vm
                .as_mut()
                .context("Higurashi VM is detached during reconstruction")?
                .write_pending_result(dest, value)
        };
        let complete = match execution {
            super::autosave::Execution::Live => character.poll_live(
                progress,
                memory,
                history,
                &mut state.timeline_mut().boundary,
                self.frame.scene.layers_mut()?,
                services,
                publish,
            )?,
            super::autosave::Execution::Replay => {
                character.poll_replay(progress, &mut state.timeline_mut().boundary, publish)?
            }
        };
        if complete {
            let value = character
                .result()
                .context("Completed character command has no response")?;
            let dest = character.request().destination;
            self.complete_external(CommandResult::WriteMemory(dest, value))?;
        }
        Ok(Some(complete))
    }
    pub fn interrupt_wait(&mut self) -> bool {
        if self.reconstruction.is_some() {
            return false;
        }
        match self.pending.as_mut().map(|p| &mut p.owner) {
            Some(Owner::Clock(wait)) => wait.interrupt(),
            _ => false,
        }
    }
    /// Native12d64 clears only ADV+408 after eligible post-poll skip. The
    /// manual-confirm flag and pending command survive until the next poll.
    pub fn clear_wait_timer_for_skip(&mut self) {
        if self.reconstruction.is_some() {
            return;
        }
        if let Some(Owner::Clock(wait)) = self.pending.as_mut().map(|p| &mut p.owner) {
            wait.remaining = 0.0;
        }
    }
    pub fn wait_allow_interrupt(&self) -> u32 {
        match self.pending.as_ref().map(|p| &p.owner) {
            Some(Owner::Clock(wait)) => u32::from(wait.allow_interrupt),
            _ => 0,
        }
    }
    #[cfg(test)]
    pub(crate) fn reference_wait(&self) -> Option<(f32, u8)> {
        match self.pending.as_ref().map(|p| &p.owner) {
            Some(Owner::Clock(wait)) => Some((wait.remaining, wait.allow_interrupt)),
            _ => None,
        }
    }
    pub fn click_message(&mut self, voice: &mut impl MessageVoice) -> bool {
        if self.reconstruction.is_some() {
            return false;
        }
        self.frame.message.click(voice)
    }
    pub fn request_message_update(&mut self, autoplay: bool, skip: bool) {
        if self.reconstruction.is_some() {
            return;
        }
        self.frame.request_message_update(autoplay, skip);
    }
    pub fn fast_forward_visuals(&mut self, voice: &mut impl MessageVoice) {
        if self.reconstruction.is_some() {
            return;
        }
        self.frame.message.fast_forward(voice);
        self.frame.scene.fast_forward_screen();
    }
    pub fn native_vm_attached(&self) -> bool {
        self.vm.is_some() && self.read_state.is_some()
    }
    pub fn set_message_settings(
        &mut self,
        skip_speed: u8,
        continue_voice: bool,
        window_opacity: u8,
    ) {
        self.frame.message.settings.skip_speed = skip_speed;
        self.frame.message.settings.continue_voice_on_click = continue_voice;
        self.frame.message.ui_settings.window_opacity = window_opacity;
    }

    /// Execute instant commands up to a fairness budget, retaining the current
    /// command on pending external work or asset/unsupported-feature errors.
    /// A VM instruction error is terminal for this runner: replaying a partly
    /// executed instruction stream would otherwise consume feedback twice.
    pub async fn pump(
        &mut self,
        policy: Policy,
        max_commands: usize,
        max_instructions: usize,
        seeds: impl FnMut() -> [u32; 4],
        voice: &mut impl VoiceCommands,
    ) -> Result<Status> {
        ensure!(
            self.read_state.is_none(),
            "Native read state requires pump_native and its services"
        );
        self.pump_resolved(
            policy,
            None::<(ReadPolicy, &mut NoReadServices)>,
            max_commands,
            max_instructions,
            seeds,
            voice,
        )
        .await
    }

    pub async fn pump_native(
        &mut self,
        policy: Policy,
        read_policy: ReadPolicy,
        services: &mut impl ReadServices,
        max_commands: usize,
        max_instructions: usize,
        seeds: impl FnMut() -> [u32; 4],
        voice: &mut impl VoiceCommands,
    ) -> Result<Status> {
        if self.read_state.is_none() {
            ensure!(
                self.completed_commands == 0 && self.pending.is_none() && self.vm_fault.is_none(),
                "Native read state must start before the first command"
            );
            self.read_state = Some(CommandReadState::new());
        }
        self.pump_resolved(
            policy,
            Some((read_policy, services)),
            max_commands,
            max_instructions,
            seeds,
            voice,
        )
        .await
    }

    async fn pump_resolved(
        &mut self,
        policy: Policy,
        mut native: Option<(ReadPolicy, &mut impl ReadServices)>,
        max_commands: usize,
        max_instructions: usize,
        mut seeds: impl FnMut() -> [u32; 4],
        voice: &mut impl VoiceCommands,
    ) -> Result<Status> {
        ensure!(
            max_commands > 0 && max_instructions > 0,
            "Higurashi runner budgets must be positive"
        );
        if let Some(fault) = self.frame.scene.visual_fault() {
            anyhow::bail!("Higurashi visual owner failed: {fault}");
        }
        ensure!(
            self.vm_fault.is_none(),
            "Higurashi VM is faulted: {:?}",
            self.vm_fault
        );
        if let Some((_, services)) = native.as_mut() {
            self.flush_message_completions(*services)?;
            self.frame
                .scene
                .commit_layer_metadata(&self.scenario, services.layer_settings())?;
        }
        if self.reconstruction.is_some() || self.reconstruction_result.is_some() {
            return Ok(Status::Waiting);
        }
        if self.exited {
            return Ok(Status::Exited);
        }
        if policy.paused {
            return Ok(Status::Waiting);
        }
        for _ in 0..max_commands {
            if let Some((_, services)) = native.as_mut() {
                self.flush_message_completions(*services)?;
            }
            if self.pending.is_none() {
                let response = std::mem::replace(&mut self.response, CommandResult::None);
                let command = match self
                    .vm
                    .as_mut()
                    .context("Higurashi VM is detached during reconstruction")?
                    .run(response, max_instructions)
                {
                    Ok(command) => command,
                    Err(error) => {
                        self.vm_fault = Some(format!("{error:#}"));
                        return Err(error);
                    }
                };
                self.pending = Some(Pending {
                    command,
                    owner: Owner::New,
                    read_prepared: false,
                    message_copied: false,
                    autosave: None,
                    choice: None,
                    character: None,
                    tips_get: None,
                    notification: None,
                    kakera: None,
                    scenario_select: None,
                    quiz: None,
                    exit: None,
                    fake_select: None,
                });
            }
            let pending = self.pending.as_mut().unwrap();
            let mut policy = policy;
            if let Some((read_policy, services)) = native.as_mut() {
                let state = self.read_state.as_mut().unwrap();
                if !pending.read_prepared {
                    let choice = if matches!(pending.command, Cmd::SELECT { .. }) {
                        Some(super::choice::ChoiceCommand::new(
                            &pending.command,
                            state.descriptors().message.text.is_none(),
                        )?)
                    } else {
                        None
                    };
                    let before = state.constructor_capture(&pending.command);
                    state.prepare(
                        &pending.command,
                        &mut super::checkpoints::CaptureServices {
                            services: *services,
                            vm: self.vm.as_ref().unwrap(),
                            read: &before,
                            layers: &self.frame.scene.layers().state,
                        },
                    )?;
                    pending.choice = choice;
                    if matches!(pending.command, Cmd::AUTOSAVE {}) {
                        pending.autosave = Some(super::autosave::AutosaveCommand::new());
                    }
                    if matches!(pending.command, Cmd::KAKERA {}) {
                        pending.kakera = Some(super::kakera::KakeraCommand::new());
                    }
                    if matches!(pending.command, Cmd::SNRSEL { .. }) {
                        pending.scenario_select = Some(super::scenario_select::ScenarioSelectCommand::new(&pending.command)?);
                    }
                    if matches!(pending.command, Cmd::QUIZ { .. }) {
                        pending.quiz = Some(super::quiz::QuizCommand::new(&pending.command)?);
                    }
                    pending.read_prepared = true;
                }
                if matches!(pending.command, Cmd::FAKESELECT {}) && pending.fake_select.is_none() {
                    pending.fake_select = Some(super::fake_select::command::Command::new());
                }
                if matches!(pending.command, Cmd::CHARSELECT { .. }) && pending.character.is_none()
                {
                    pending.character =
                        Some(super::character_selection::command::CharacterCommand::new(
                            &pending.command,
                            &mut self.frame.scene.layers_mut()?.state,
                        )?);
                }
                if matches!(pending.command, Cmd::TIPSGET { .. }) && pending.tips_get.is_none() {
                    pending.tips_get = Some(super::tips_get::TipsGetCommand::new(
                        &pending.command,
                        &mut self.frame.scene.layers_mut()?.state,
                    )?);
                }
                if matches!(pending.command, Cmd::OTSUGET { .. } | Cmd::KAKERAGET { .. })
                    && pending.notification.is_none()
                {
                    pending.notification =
                        Some(super::notification_command::NotificationCommand::new(
                            &pending.command,
                            &mut self.frame.scene.layers_mut()?.state,
                        )?);
                }
                policy.suppress_transient_audio = state.suppression(*read_policy);
                policy.read_flags = state.message_flags();
            }
            //9678 captures once after the read constructor prefix;98e4 uses
            // those physical coordinates even after the integer mapping frees.
            if matches!(pending.owner, Owner::New) {
                if let Cmd::LAYERUNLOAD { args: [id, delay] } = &pending.command {
                    pending.owner = Owner::LayerUnload(super::layers::LayerUnload::capture(
                        &mut self.frame.scene.layers_mut()?.state,
                        *id,
                        *delay,
                    )?);
                }
            }
            match &mut pending.owner {
                Owner::External => return Ok(Status::External),
                Owner::Frame => {
                    if let Some((_, services)) = native.as_mut() {
                        pending.copy_message_start(self.read_state.as_ref().unwrap(), *services)?;
                    }
                    let status = self.frame.poll_command(policy.fast_skip, voice);
                    if let Some((_, services)) = native.as_mut() {
                        self.frame
                            .scene
                            .commit_layer_metadata(&self.scenario, services.layer_settings())?;
                    }
                    if status == CommandStatus::Pending {
                        return Ok(Status::Waiting);
                    }
                }
                Owner::Clock(wait) => {
                    if !wait.poll(policy.fast_skip) {
                        return Ok(Status::Waiting);
                    }
                }
                Owner::VoiceWait(mask) => {
                    if voice_commands::wait(*mask, voice) == CommandStatus::Pending {
                        return Ok(Status::Waiting);
                    }
                }
                Owner::MovieWait(wait) => {
                    if !wait.complete(self.frame.scene.layers()) {
                        return Ok(Status::Waiting);
                    }
                }
                Owner::LayerWait(wait) => {
                    if !wait.complete(self.frame.scene.layers()) {
                        return Ok(Status::Waiting);
                    }
                }
                Owner::LayerUnload(unload) => {
                    unload.complete(self.frame.scene.layers_mut()?)?;
                }
                Owner::Music(wait) => {
                    let music = voice
                        .music()
                        .context("Higurashi pending music service is missing")?;
                    if !wait.complete(music) {
                        return Ok(Status::Waiting);
                    }
                }
                Owner::Effects(wait) => {
                    let effects = voice
                        .effects()
                        .context("Higurashi pending effects service is missing")?;
                    if !wait.complete(effects, policy.suppress_transient_audio) {
                        return Ok(Status::Waiting);
                    }
                }
                Owner::Asset { task, animate } => match task.poll_naive() {
                    None => return Ok(Status::Waiting),
                    Some(Ok(loaded)) => {
                        let settings = native
                            .as_mut()
                            .and_then(|(_, services)| services.layer_settings());
                        self.frame
                            .scene
                            .layers_mut()?
                            .publish_load_with_settings(loaded, &mut seeds, settings)?;
                        if let Some((_, services)) = native.as_mut() {
                            self.frame
                                .scene
                                .commit_layer_metadata(&self.scenario, services.layer_settings())?;
                        }
                    }
                    Some(Err(error)) => {
                        let message = format!(
                            "Higurashi layer load at {} failed: {error:#}",
                            self.vm
                                .as_ref()
                                .map(|vm| vm.position())
                                .unwrap_or_else(|| self.retired_vm_position.unwrap().0)
                        );
                        pending.owner = Owner::AssetFault {
                            message: message.clone(),
                            animate: *animate,
                        };
                        anyhow::bail!("{message}");
                    }
                },
                Owner::AssetFault { message, .. } => anyhow::bail!("{message}"),
                Owner::New => match &pending.command {
                    // Updated LOGSET/DEBUGOUT only retain their decoded payload;
                    // all three polls complete immediately. Switch TROPHY's
                    // e8eb0 only locks/unlocks its platform mutex. LOGSET rows
                    // belong to the separate backlog reconstruction provider.
                    Cmd::LOGSET { .. } | Cmd::DEBUGOUT { .. } | Cmd::TROPHY { .. } => {}
                    Cmd::UNLOCK { kind, ids } => {
                        super::unlock::poll(
                            *kind,
                            ids,
                            native.as_mut().and_then(|(_, services)| services.layer_settings()),
                        )?;
                    }
                    // SAVEINFO/CHART polls complete immediately. prepare already
                    // applied their constructor effects; AUTOSAVE retains its external
                    // storage/notification service after native constructor work.
                    Cmd::SAVEINFO { .. }
                    | Cmd::CHART { .. }
                    | Cmd::EVBEGIN { .. }
                    | Cmd::EVEND {}
                        if native.is_some() => {}
                    //420 binds nativeNOPE for every nonzero EXIT mode.
                    Cmd::EXIT { arg1, .. } if *arg1 != 0 => {}
                    Cmd::EXIT { arg1: 0, arg2 } if native.is_some() && self.live_exit_services => {
                        pending.exit = Some(super::exit::ExitCommand::new(0, *arg2));
                        pending.owner = Owner::External;
                        return Ok(Status::External);
                    }
                    Cmd::EXIT { arg1: 0, .. } => {
                        self.exited = true;
                        self.pending = None;
                        self.completed_commands += 1;
                        return Ok(Status::Exited);
                    }
                    Cmd::WAIT {
                        amount,
                        allow_interrupt,
                    } => {
                        let mut wait = Wait::new(*amount, *allow_interrupt);
                        if !wait.poll(policy.fast_skip) {
                            pending.owner = Owner::Clock(wait);
                            return Ok(Status::Waiting);
                        }
                    }
                    Cmd::MOVIEWAIT {
                        args: [selector, mode],
                    } => {
                        let wait = super::layers::MovieWait::capture(
                            self.frame.scene.layers(),
                            *selector,
                            *mode,
                        )?;
                        if !wait.complete(self.frame.scene.layers()) {
                            pending.owner = Owner::MovieWait(wait);
                            return Ok(Status::Waiting);
                        }
                    }
                    Cmd::LAYERWAIT { id, properties } => {
                        let wait = super::layers::LayerWait::capture(
                            self.frame.scene.layers(),
                            *id,
                            properties,
                        )?;
                        if !wait.complete(self.frame.scene.layers()) {
                            pending.owner = Owner::LayerWait(wait);
                            return Ok(Status::Waiting);
                        }
                    }
                    Cmd::LAYERLOAD {
                        args: [id, kind, flags],
                        params,
                    } => {
                        let plan = self
                            .frame
                            .scene
                            .layers_mut()?
                            .plan_load(*id, *kind, *flags, *params)?;
                        if plan.immediate() {
                            let loaded = plan
                                .load_with_setting(&self.assets, &self.scenario, self.layer_setting)
                                .await?;
                            let settings = native
                                .as_mut()
                                .and_then(|(_, services)| services.layer_settings());
                            self.frame
                                .scene
                                .layers_mut()?
                                .publish_load_with_settings(loaded, &mut seeds, settings)?;
                            if let Some((_, services)) = native.as_mut() {
                                self.frame.scene.commit_layer_metadata(
                                    &self.scenario,
                                    services.layer_settings(),
                                )?;
                            }
                        } else {
                            let assets = self.assets.clone();
                            let scenario = self.scenario.clone();
                            let setting = self.layer_setting;
                            let task = shin_tasks::async_io::spawn(async move {
                                plan.load_with_setting(&assets, &scenario, setting).await
                            });
                            pending.owner = Owner::Asset {
                                task,
                                animate: flags & 2 != 0,
                            };
                            return Ok(Status::Waiting);
                        }
                    }
                    command @ Cmd::WIPE { .. } => {
                        let status = self
                            .frame
                            .scene
                            .dispatch_with_assets(
                                command,
                                policy.fast_skip,
                                Some((self.assets.clone(), self.scenario.clone())),
                            )?
                            .unwrap();
                        // Publication can complete immediately. Retain Frame
                        // ownership before metadata callbacks so a failed
                        // acknowledgement retries without publishing twice.
                        pending.owner = Owner::Frame;
                        if let Some((_, services)) = native.as_mut() {
                            self.frame
                                .scene
                                .commit_layer_metadata(&self.scenario, services.layer_settings())?;
                        }
                        if status == CommandStatus::Pending {
                            return Ok(Status::Waiting);
                        }
                    }
                    command => {
                        if let Some(effects) = voice.effects() {
                            if let Some(status) = effect_commands::dispatch(
                                command,
                                policy.suppress_transient_audio,
                                effects,
                            )? {
                                if let EffectCommand::Wait(wait) = status {
                                    if !wait.complete(effects, policy.suppress_transient_audio) {
                                        pending.owner = Owner::Effects(wait);
                                        return Ok(Status::Waiting);
                                    }
                                }
                                self.pending = None;
                                self.completed_commands += 1;
                                continue;
                            }
                        }
                        if let Some(music) = voice.music() {
                            if let Some(status) = music_commands::dispatch(
                                command,
                                policy.suppress_transient_audio,
                                music,
                            )? {
                                if let MusicCommand::Wait(wait) = status {
                                    if !wait.complete(music) {
                                        pending.owner = Owner::Music(wait);
                                        return Ok(Status::Waiting);
                                    }
                                }
                                self.pending = None;
                                self.completed_commands += 1;
                                continue;
                            }
                        }
                        if let Some(status) = voice_commands::dispatch(
                            command,
                            policy.suppress_transient_audio,
                            voice,
                        ) {
                            if status == CommandStatus::Pending {
                                let Cmd::VOICEWAIT { status } = command else {
                                    unreachable!()
                                };
                                pending.owner = Owner::VoiceWait(*status);
                                return Ok(Status::Waiting);
                            }
                            self.pending = None;
                            self.completed_commands += 1;
                            continue;
                        }
                        let context = self.assets.context();
                        let status = self
                            .frame
                            .dispatch(
                                command,
                                &self.assets,
                                &self.scenario,
                                &mut seeds,
                                &context.wgpu_device,
                                &context.wgpu_queue,
                                policy.fast_skip,
                                policy.read_flags,
                                policy.text_speed,
                                policy.outline_mode,
                                voice,
                            )
                            .await
                            .with_context(|| {
                                format!(
                                    "Higurashi command at {}: {command:?}",
                                    self.vm
                                        .as_ref()
                                        .map(|vm| vm.position())
                                        .unwrap_or_else(|| self.retired_vm_position.unwrap().0)
                                )
                            })?;
                        if let Some((_, services)) = native.as_mut() {
                            self.frame
                                .scene
                                .commit_layer_metadata(&self.scenario, services.layer_settings())?;
                        }
                        match status {
                            Some(CommandStatus::Pending) => {
                                pending.owner = Owner::Frame;
                                if let Some((_, services)) = native.as_mut() {
                                    pending.copy_message_start(
                                        self.read_state.as_ref().unwrap(),
                                        *services,
                                    )?;
                                }
                                return Ok(Status::Waiting);
                            }
                            Some(CommandStatus::Complete) => {
                                // Retain the installed message on a failed clock/
                                // publication call; retry cannot reinstall glyphs.
                                pending.owner = Owner::Frame;
                                if let Some((_, services)) = native.as_mut() {
                                    pending.copy_message_start(
                                        self.read_state.as_ref().unwrap(),
                                        *services,
                                    )?;
                                }
                            }
                            None => {
                                pending.owner = Owner::External;
                                return Ok(Status::External);
                            }
                        }
                    }
                },
            }
            self.pending = None;
            self.completed_commands += 1;
        }
        Ok(Status::CommandBudget)
    }

    /// Native12bec command polling precedes12d68 clock update and12da8 frame
    /// update. Pause stops the VM and WAIT clock; animation disable gates only
    /// the message/screen update. Call this after pump, once per displayed frame.
    pub(crate) fn visual_updates_admitted(&self, policy: Policy) -> bool {
        policy.animations_enabled
            && !policy.paused
            && self.frame.scene.animations_enabled()
            && !self.reconstruction.as_ref().is_some_and(|owner| {
                owner.mode.phase() == super::reconstruction_mode::Phase::Preparing
            })
            && self
                .pending
                .as_ref()
                .is_none_or(|p| p.owner.animations_enabled())
    }
    pub fn advance(
        &mut self,
        delta: Ticks,
        policy: Policy,
        voice: &mut impl MessageVoice,
        lipsync: Option<&dyn Fn(i32, f32) -> Option<u8>>,
    ) {
        if !policy.paused {
            if let Some(overlay) = &mut self.reconstruction_overlay {
                overlay.clock.advance(delta);
            }
        }
        if self
            .reconstruction
            .as_ref()
            .is_some_and(|owner| owner.mode.phase() == super::reconstruction_mode::Phase::Preparing)
        {
            return;
        }
        if !policy.paused {
            if let Some(wait) = &mut self.closing.wait {
                wait.advance(delta.as_f32());
            }
            if let Some(Pending {
                owner: Owner::Clock(wait),
                ..
            }) = &mut self.pending
            {
                wait.advance(delta.as_f32());
            }
        }
        // Native8f88 clears ADV3f1 unless load flag bit1 permits animation;
        // 93e8 restores it only after the worker reports ready and publishes.
        let load_animations = self
            .pending
            .as_ref()
            .is_none_or(|p| p.owner.animations_enabled());
        self.frame.advance(
            delta,
            policy.animations_enabled && load_animations,
            policy.paused,
            voice,
            lipsync,
        );
    }
}

#[cfg(test)]
mod autosave_tests;
#[cfg(test)]
mod chapter_tests;
#[cfg(test)]
mod chart_tests;
#[cfg(test)]
mod choice_tests;
#[cfg(test)]
mod descriptor_tests;
#[cfg(test)]
mod progress_tests;

#[cfg(test)]
mod character_tests;
#[cfg(test)]
mod input_tests;
#[cfg(test)]
mod layerwait_tests;
#[cfg(test)]
mod loading_tests;
#[cfg(test)]
mod read_history_tests;
#[cfg(test)]
mod read_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod unload_tests;
#[cfg(test)]
mod variable_tests;
#[cfg(test)]
mod wipe_loading_tests;
#[cfg(test)]
mod wipe_metadata_tests;

#[cfg(test)]
mod forward_search_tests;
#[cfg(test)]
mod live_reconstruction_tests;
#[cfg(test)]
mod replay_tests;
#[cfg(test)]
mod restoration_tests;

mod exit;
pub mod reconstruction_mode;
#[cfg(test)]
mod reconstruction_mode_tests;
#[cfg(test)]
mod reload_tests;
#[cfg(test)]
mod source_command_reconstruction_tests;
