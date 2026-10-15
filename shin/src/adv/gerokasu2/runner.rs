//! Retained VM command driver and native command-before-clock/root/voice order.
use super::{
    audio_commands::{AudioCommandState, AudioCommands},
    gallery::{self, GalleryServices},
    layer_control::{LayerControlPlan, LayerControlServices, LayerWait, TransitionWait},
    layer_edit::{LayerEdit, LayerEditServices},
    layer_load::{LayerLoadController, LoadServices, LoadTransition},
    layers::Gerokasu2Layers,
    message_commands::{MessageCommandContext, MessageCommandStatus},
    message_owner::{Gerokasu2MessageOwner, SharedMessageOwner},
    movie::{MovieServices, MovieWait},
    quiz::{QuizCommand, QuizServices},
    save_commands::{AutosaveCommand, SaveServices},
    scene::{SceneServices, WipeController},
    voice::{Gerokasu2MessageEffects, Gerokasu2VoicePlayer, MessageNotifications},
    voice_commands::VoiceCommandState,
};
use crate::asset::system::AssetServer;
use anyhow::{Context, Result, ensure};
use glam::Mat4;
use shin_audio::AudioManager;
use shin_core::{
    format::scenario::gerokasu2::Gerokasu2Scenario,
    primitives::update::FrameId,
    time::Ticks,
    vm::{
        command::{CommandResult, RuntimeCommand},
        gerokasu2::{
            Gerokasu2RuntimeCommand as Cmd, Gerokasu2Scripter, message_face::PortraitChange,
        },
    },
};
use shin_render::{PassKind, render_pass::RenderPass};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunnerStatus {
    Waiting,
    External,
    CommandBudget,
    Exited,
}
#[derive(Debug, Clone, Copy)]
pub struct FramePolicy {
    pub paused: bool,
    pub scene_changing: bool,
    pub animations_allowed: bool,
    pub autoplay: bool,
    pub fast_skip: bool,
}

/// Initial/restored raw VM message fields remain explicit until native bootstrap
/// and restoration are implemented. Integer layers are supplied by their owner.
pub struct InitialMessage {
    pub style: i32,
    pub asynchronous: bool,
    pub face: i32,
}

pub trait RunnerServices: MessageNotifications {
    fn selection_services(
        &mut self,
    ) -> Option<&mut (dyn super::selection::SelectionServices + '_)> {
        None
    }
    fn movie_services(&mut self) -> Option<&mut (dyn MovieServices + '_)> {
        None
    }
    fn quiz_services(&mut self) -> Option<&mut (dyn QuizServices + '_)> {
        None
    }
    fn gallery_services(&mut self) -> Option<&mut (dyn GalleryServices + '_)> {
        None
    }
    fn save_services(&mut self) -> Option<&mut (dyn SaveServices + '_)> {
        None
    }
    fn layer_edit_services(&mut self) -> Option<&mut (dyn LayerEditServices + '_)> {
        None
    }
    fn audio_commands(&mut self) -> Option<&mut (dyn AudioCommands + '_)> {
        None
    }
    fn attach_message(&mut self, _: SharedMessageOwner) -> Result<()> {
        Ok(())
    }
    fn prepare_message_frame(&mut self, _: FrameId, _: Ticks, _: bool) -> Result<()> {
        Ok(())
    }
    fn prepare_root_with_voice(
        &mut self,
        frame: FrameId,
        delta: Ticks,
        allowed: bool,
        _: &Gerokasu2VoicePlayer,
    ) -> Result<()> {
        self.prepare_message_frame(frame, delta, allowed)
    }
    fn prepare_root_fast_forward(&mut self) -> Result<()> {
        Ok(())
    }
    /// The live scene supplies its drawable tree, jobs and native transition
    /// policy here. Fixture callers may keep explicit external continuations.
    fn layer_load_services(&mut self) -> Option<&mut (dyn LoadServices + '_)> {
        None
    }
    fn scene_services(&mut self) -> Option<&mut (dyn SceneServices + '_)> {
        None
    }
    fn layer_control_services(&mut self) -> Option<&mut (dyn LayerControlServices + '_)> {
        None
    }
    /// Re-evaluate native playback gates after this frame pumps commands.
    fn frame_policy(&mut self, policy: FramePolicy) -> FramePolicy {
        policy
    }
    fn system_call(&mut self, _: i32, _: i32) -> Result<bool> {
        Ok(false)
    }
    fn resume_scene(&mut self) -> Result<bool> {
        Ok(false)
    }
    fn mask_services(&mut self) -> Option<&mut (dyn super::masks::MaskServices + '_)> {
        None
    }
    /// Native constructor/read/settings policy is resolved once for a new
    /// command; later calls may change the skip decision without reinstalling.
    fn command_context(&mut self, command: &Cmd, first_poll: bool)
    -> Result<MessageCommandContext>;
    /// Observe/save after the message owner has submitted its portrait request.
    /// Retried notifications do not resubmit that request.
    fn message_installed(&mut self, id: u32, portrait: PortraitChange) -> Result<()>;
    fn portrait_changed(&mut self, portrait: PortraitChange) -> Result<()>;
    fn advance_layers(
        &mut self,
        layers: &Gerokasu2Layers,
        delta: Ticks,
        animations_allowed: bool,
    ) -> Result<()>;
    fn advance_layers_in_frame(
        &mut self,
        frame: FrameId,
        layers: &Gerokasu2Layers,
        delta: Ticks,
        animations_allowed: bool,
    ) -> Result<()> {
        let _ = frame;
        self.advance_layers(layers, delta, animations_allowed)
    }
    fn advance_layers_with_voice(
        &mut self,
        frame: FrameId,
        layers: &Gerokasu2Layers,
        delta: Ticks,
        animations_allowed: bool,
        _: &Gerokasu2VoicePlayer,
    ) -> Result<()> {
        self.advance_layers_in_frame(frame, layers, delta, animations_allowed)
    }
    fn fast_forward_layers(&mut self, layers: &Gerokasu2Layers) -> Result<()>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct WaitClock {
    remaining: f32,
    allow: u8,
}
impl WaitClock {
    pub fn remaining(&self) -> f32 {
        self.remaining
    }
    pub fn allow_interrupt(&self) -> u8 {
        self.allow
    }
    pub fn install(&mut self, amount: i32, allow: u8) {
        self.remaining = amount.wrapping_abs() as f32;
        self.allow = allow;
    }
    pub fn advance(&mut self, delta: Ticks) {
        if self.remaining > 0.0 {
            self.remaining -= delta.as_f32();
        }
    }
    pub fn poll(&mut self, skip: bool) -> bool {
        if skip {
            self.remaining = 0.0;
            self.allow = 0;
        }
        !(self.remaining > 0.0)
    }
    pub fn interrupt(&mut self) -> bool {
        if self.allow != 1 {
            return false;
        }
        self.remaining = 0.0;
        true
    }
    fn clear_time(&mut self) {
        self.remaining = 0.0;
    }
}
enum Owner {
    Mask(super::masks::MaskController),
    Resume(super::reconstruction_mode::ResumeCommand),
    Selection(super::selection::SelectionCommand),
    Autosave(AutosaveCommand),
    Quiz(QuizCommand),
    Audio(AudioCommandState),
    New,
    Ready,
    Message,
    Voice,
    Wait,
    External,
    LayerLoad(LayerLoadController),
    Wipe(WipeController),
    SceneWait,
    LayerWait(LayerWait),
    TransitionWait(TransitionWait),
    MovieWait(MovieWait),
}
enum Notification {
    Install(u32, PortraitChange),
    Portrait(PortraitChange),
}
struct Pending {
    command: Cmd,
    owner: Owner,
    notification: Option<Notification>,
    save_prepared: bool,
    descriptors_prepared: bool,
    context_prepared: bool,
}

pub struct Gerokasu2Runner {
    vm: Gerokasu2Scripter,
    scenario: Arc<Gerokasu2Scenario>,
    assets: Arc<AssetServer>,
    message: SharedMessageOwner,
    voice: Gerokasu2VoicePlayer,
    voice_commands: VoiceCommandState,
    layers: Gerokasu2Layers,
    wait: WaitClock,
    pending: Option<Pending>,
    response: CommandResult,
    exited: bool,
    vm_fault: Option<String>,
    completed: u64,
    elapsed_seconds: f32,
}
impl Gerokasu2Runner {
    pub(crate) fn require_publication_retry(&mut self, error: &anyhow::Error) {
        self.vm_fault = Some(format!(
            "Ger reconstruction publication requires retry: {error:#}"
        ));
    }
    pub(crate) fn install_replayed_parts(
        &mut self,
        vm: Gerokasu2Scripter,
        layers: Box<Gerokasu2Layers>,
        retained: Option<super::replay::RetainedCommand>,
        response: CommandResult,
    ) {
        self.vm = vm;
        self.layers = *layers;
        self.pending = retained.map(|mut p| {
            let owner = match &p.command {
                Cmd::Selection {
                    choice_set_base,
                    choice_index,
                    visibility,
                    variants,
                    ..
                } => {
                    if p.replay_stop > 0 {
                        Owner::Ready
                    } else {
                        Owner::Selection(super::selection::SelectionCommand::new(
                            super::selection::SelectionRequest {
                                base: *choice_set_base,
                                index: *choice_index,
                                mask: *visibility,
                                variants: variants.clone(),
                            },
                        ))
                    }
                }
                Cmd::Shared(RuntimeCommand::QUIZ(_)) => {
                    if p.replay_stop > 0 {
                        Owner::Ready
                    } else {
                        Owner::Quiz(QuizCommand::new(
                            p.quiz_bank.map(|bank| (self.layers.current_plane(), bank)),
                        ))
                    }
                }
                Cmd::Shared(RuntimeCommand::AUTOSAVE(_)) => {
                    Owner::Autosave(p.autosave.take().expect("replayed AUTOSAVE owner"))
                }
                // Their constructors already checkpointed/advanced the replay.
                Cmd::Shared(
                    RuntimeCommand::EVBEGIN(_)
                    | RuntimeCommand::EVEND(_)
                    | RuntimeCommand::RESUMESET(_),
                ) => Owner::Ready,
                _ => Owner::New,
            };
            Pending {
                command: p.command,
                owner,
                notification: None,
                save_prepared: true,
                descriptors_prepared: true,
                context_prepared: true,
            }
        });
        self.response = response;
        self.wait = Default::default();
        self.exited = false;
        self.vm_fault = None;
        self.completed = 0;
        self.voice_commands = VoiceCommandState::new();
    }
    pub async fn load(
        assets: Arc<AssetServer>,
        scenario: Arc<Gerokasu2Scenario>,
        manager: Arc<AudioManager>,
        init_value: i32,
        seed: u32,
        initial: InitialMessage,
        layers: Gerokasu2Layers,
    ) -> Result<Self> {
        let message =
            Gerokasu2MessageOwner::load(&assets, initial.style, initial.asynchronous, initial.face)
                .await?;
        let voice = Gerokasu2VoicePlayer::new(manager, assets.clone(), &scenario);
        Ok(Self {
            vm: Gerokasu2Scripter::new(scenario.clone(), init_value, seed),
            scenario,
            assets,
            message: SharedMessageOwner::new(message),
            voice,
            voice_commands: VoiceCommandState::new(),
            layers,
            wait: WaitClock::default(),
            pending: None,
            response: CommandResult::None,
            exited: false,
            vm_fault: None,
            completed: 0,
            elapsed_seconds: 0.0,
        })
    }
    pub fn vm(&self) -> &Gerokasu2Scripter {
        &self.vm
    }
    pub fn message(&self) -> parking_lot::MutexGuard<'_, Gerokasu2MessageOwner> {
        self.message.lock()
    }
    pub fn message_owner(&self) -> SharedMessageOwner {
        self.message.clone()
    }
    pub fn voice(&self) -> &Gerokasu2VoicePlayer {
        &self.voice
    }
    pub fn voice_mut(&mut self) -> &mut Gerokasu2VoicePlayer {
        &mut self.voice
    }
    pub fn layers(&self) -> &Gerokasu2Layers {
        &self.layers
    }
    pub fn wait_clock(&self) -> WaitClock {
        self.wait
    }
    pub fn elapsed_seconds(&self) -> f32 {
        self.elapsed_seconds
    }
    pub fn completed_commands(&self) -> u64 {
        self.completed
    }
    pub fn pending_command(&self) -> Option<&Cmd> {
        self.pending.as_ref().map(|p| &p.command)
    }
    pub fn external_pending(&self) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|p| matches!(p.owner, Owner::External))
    }
    pub fn interrupt_wait(&mut self) -> bool {
        self.wait.interrupt()
    }
    pub fn click_message(
        &mut self,
        keep_voice: bool,
        services: &mut impl RunnerServices,
    ) -> Result<bool> {
        self.message.lock().try_advance(
            keep_voice,
            &mut Gerokasu2MessageEffects {
                voice: &mut self.voice,
                notifications: services,
            },
        )
    }
    /// An external drawable/menu/persistence owner acknowledges only its exact
    /// retained continuation. Invalid/double responses leave VM state intact.
    pub fn complete_external(&mut self, response: CommandResult) -> Result<()> {
        let pending = self
            .pending
            .as_ref()
            .context("No Gerokasu2 external command is pending")?;
        ensure!(
            matches!(pending.owner, Owner::External),
            "Gerokasu2 command belongs to a clock/message/voice owner"
        );
        match (self.vm.pending_destination(), &response) {
            (None, CommandResult::None) => {}
            (Some(expected), CommandResult::WriteMemory(actual, _)) if expected == *actual => {}
            _ => {
                anyhow::bail!("Gerokasu2 external response does not match the command destination")
            }
        }
        self.response = response;
        self.pending = None;
        self.completed += 1;
        Ok(())
    }
    /// Poll until an owned wait or external continuation, preserving commands
    /// across callbacks and service errors. Budgets bound host execution; a VM
    /// error is terminal because its partially consumed instruction state cannot
    /// safely be retried with the same feedback.
    pub fn pump(
        &mut self,
        paused: bool,
        max_commands: usize,
        max_instructions: usize,
        services: &mut impl RunnerServices,
    ) -> Result<RunnerStatus> {
        ensure!(
            max_commands > 0 && max_instructions > 0,
            "Gerokasu2 runner budgets must be positive"
        );
        ensure!(
            self.vm_fault.is_none(),
            "Gerokasu2 VM is faulted: {:?}",
            self.vm_fault
        );
        if self.exited {
            return Ok(RunnerStatus::Exited);
        }
        services.attach_message(self.message.clone())?;
        if paused {
            return Ok(RunnerStatus::Waiting);
        }
        for _ in 0..max_commands {
            if self.pending.is_none() {
                let result = std::mem::replace(&mut self.response, CommandResult::None);
                let command = match self.vm.run(result, max_instructions) {
                    Ok(command) => command,
                    Err(error) => {
                        self.vm_fault = Some(format!("{error:#}"));
                        return Err(error);
                    }
                };
                self.pending = Some(Pending {
                    command,
                    owner: Owner::New,
                    notification: None,
                    save_prepared: false,
                    descriptors_prepared: false,
                    context_prepared: false,
                });
            }
            let pending = self.pending.as_mut().unwrap();
            let context = services.command_context(
                &pending.command,
                matches!(pending.owner, Owner::New) && !pending.context_prepared,
            )?;
            pending.context_prepared = true;
            if matches!(pending.owner, Owner::New) {
                if matches!(pending.command, Cmd::MessageSet { .. }) && !pending.save_prepared {
                    if let Some(save) = services.save_services() {
                        save.save_session().timeline.event = context.event_mode;
                        save.save_session().message(&self.vm, &self.layers)?;
                        save.message_started();
                    }
                    pending.save_prepared = true;
                }
                match &pending.command {
                    command @ (Cmd::MaskLoad { .. }
                    | Cmd::Shared(RuntimeCommand::MASKUNLOAD(_))) => {
                        let mut prepared = self.layers.clone();
                        let plan = super::masks::MaskPlan::prepare(&mut prepared, command)?;
                        pending.owner = if services.mask_services().is_some() {
                            Owner::Mask(super::masks::MaskController::new(plan))
                        } else {
                            Owner::External
                        };
                        self.layers = prepared;
                    }
                    command @ Cmd::MovieWait { .. } => {
                        pending.owner = if services.movie_services().is_some() {
                            Owner::MovieWait(MovieWait::new(&mut self.layers, command)?)
                        } else {
                            Owner::External
                        };
                    }
                    Cmd::Shared(RuntimeCommand::SGET(command)) => {
                        if let Some(save) = services.save_services() {
                            let value = save.save_session().persistent_get(command.slot_number);
                            self.response = CommandResult::WriteMemory(
                                self.vm
                                    .pending_destination()
                                    .context("Ger SGET destination missing")?,
                                value,
                            );
                            pending.owner = Owner::Ready;
                        } else {
                            pending.owner = Owner::External;
                        }
                    }
                    Cmd::Shared(RuntimeCommand::SSET(command)) => {
                        if let Some(save) = services.save_services() {
                            save.save_session()
                                .variables
                                .set(command.slot_number, command.value);
                            pending.owner = Owner::Ready;
                        } else {
                            pending.owner = Owner::External;
                        }
                    }
                    Cmd::Shared(RuntimeCommand::EVBEGIN(command)) => {
                        if let Some(save) = services.save_services() {
                            save.save_session()
                                .event_begin(&self.vm, &self.layers, command.arg)?;
                            pending.owner = Owner::Ready;
                        } else {
                            pending.owner = Owner::External;
                        }
                    }
                    Cmd::Shared(RuntimeCommand::EVEND(_)) => {
                        if let Some(save) = services.save_services() {
                            save.save_session().event_end(&self.vm, &self.layers)?;
                            pending.owner = Owner::Ready;
                        } else {
                            pending.owner = Owner::External;
                        }
                    }
                    Cmd::Shared(RuntimeCommand::RESUMESET(_)) => {
                        if let Some(save) = services.save_services() {
                            save.save_session().resume_set(&self.vm, &self.layers)?;
                            pending.owner = Owner::Ready;
                        } else {
                            pending.owner = Owner::External;
                        }
                    }
                    Cmd::Shared(RuntimeCommand::RESUME(_)) => {
                        pending.owner = Owner::Resume(Default::default());
                    }
                    Cmd::Shared(RuntimeCommand::QUIZ(command)) => {
                        if services.quiz_services().is_some() {
                            if !pending.save_prepared {
                                if let Some(save) = services.save_services() {
                                    let has_text = self.message.lock().retained_text().is_some();
                                    save.save_session().interactive(
                                        &self.vm,
                                        &self.layers,
                                        has_text,
                                    )?;
                                }
                                pending.save_prepared = true;
                            }
                            use shin_core::vm::command::types::LayerId;
                            let target = u16::try_from(command.arg)
                                .ok()
                                .and_then(LayerId::try_new)
                                .and_then(|id| self.layers.target(id))
                                .map(|t| (self.layers.current_plane(), t.layerbank));
                            pending.owner = Owner::Quiz(QuizCommand::new(target));
                        } else {
                            pending.owner = Owner::External;
                        }
                    }
                    Cmd::Selection {
                        choice_set_base,
                        choice_index,
                        visibility,
                        variants,
                        ..
                    } => {
                        if services.selection_services().is_some() {
                            if !pending.save_prepared {
                                let has_text = self.message.lock().retained_text().is_some();
                                if let Some(save) = services.save_services() {
                                    save.save_session().interactive(
                                        &self.vm,
                                        &self.layers,
                                        has_text,
                                    )?;
                                }
                                pending.save_prepared = true;
                            }
                            pending.owner =
                                Owner::Selection(super::selection::SelectionCommand::new(
                                    super::selection::SelectionRequest {
                                        base: *choice_set_base,
                                        index: *choice_index,
                                        mask: *visibility,
                                        variants: variants.clone(),
                                    },
                                ));
                        } else {
                            pending.owner = Owner::External;
                        }
                    }
                    Cmd::Shared(RuntimeCommand::SYSCALL(command)) => {
                        pending.owner =
                            if services.system_call(command.call_id, command.argument)? {
                                Owner::Ready
                            } else {
                                Owner::External
                            };
                    }
                    Cmd::Shared(RuntimeCommand::TROPHY(command)) => {
                        gallery::trophy(command.trophy_id);
                        pending.owner = Owner::Ready;
                    }
                    Cmd::Shared(RuntimeCommand::UNLOCK(command)) => {
                        pending.owner = if let Some(gallery) = services.gallery_services() {
                            gallery.unlock_gallery(command.unlock_type, &command.unlock_indices)?;
                            Owner::Ready
                        } else {
                            Owner::External
                        };
                    }
                    Cmd::Shared(RuntimeCommand::SAVEINFO(command)) => {
                        pending.owner = if let Some(save) = services.save_services() {
                            save.save_session()
                                .metadata
                                .set(command.level, &command.info)?;
                            Owner::Ready
                        } else {
                            Owner::External
                        };
                    }
                    Cmd::Shared(RuntimeCommand::AUTOSAVE(_)) => {
                        pending.owner = if let Some(save) = services.save_services() {
                            Owner::Autosave(
                                save.save_session().begin_autosave(&self.vm, &self.layers)?,
                            )
                        } else {
                            Owner::External
                        };
                    }
                    Cmd::Audio(command) => {
                        if let Some(audio) = services.audio_commands() {
                            let mut state = AudioCommandState::default();
                            let complete = state.dispatch(command, context.skip, audio)?;
                            pending.owner = if complete {
                                Owner::Ready
                            } else {
                                Owner::Audio(state)
                            };
                            if !complete {
                                return Ok(RunnerStatus::Waiting);
                            }
                        } else {
                            pending.owner = Owner::External;
                        }
                    }
                    Cmd::Wait {
                        allow_interrupt,
                        amount,
                    } => {
                        self.wait.install(*amount, *allow_interrupt);
                        pending.owner = Owner::Wait;
                    }
                    Cmd::Shared(RuntimeCommand::EXIT(command)) => {
                        if let Some(save) = services.save_services() {
                            save.save_session()
                                .descriptors
                                .prepare(&pending.command, context.was_read)?;
                        }
                        self.exited = command.arg1 == 0;
                        self.pending = None;
                        self.completed += 1;
                        if self.exited {
                            return Ok(RunnerStatus::Exited);
                        }
                        continue;
                    }
                    Cmd::Shared(RuntimeCommand::PAGEBACK(_)) => {
                        if let Some(scene) = services.scene_services() {
                            if !self.layers.back_started() {
                                scene.pageback(false)?;
                                self.layers.begin_back();
                            }
                            pending.owner = Owner::Ready;
                        } else {
                            let mut prepared = self.layers.clone();
                            prepared.apply(&pending.command)?;
                            self.layers = prepared;
                            pending.owner = Owner::External;
                        }
                    }
                    command @ Cmd::Wipe {
                        ty,
                        flags,
                        duration,
                        params,
                    } => {
                        if services.scene_services().is_some() {
                            let had_back = self.layers.back_started();
                            let mut prepared = self.layers.clone();
                            prepared.apply(command)?;
                            self.layers = prepared;
                            pending.owner = Owner::Wipe(WipeController::new(
                                had_back,
                                LoadTransition {
                                    kind: *ty,
                                    flags: *flags,
                                    duration: *duration,
                                    params: [
                                        params.0, params.1, params.2, params.3, params.4, params.5,
                                        params.6, params.7,
                                    ],
                                },
                            ));
                        } else {
                            let mut prepared = self.layers.clone();
                            prepared.apply(command)?;
                            self.layers = prepared;
                            pending.owner = Owner::External;
                        }
                    }
                    Cmd::Shared(RuntimeCommand::WIPEWAIT(_)) => {
                        pending.owner = if services.scene_services().is_some() {
                            Owner::SceneWait
                        } else {
                            Owner::External
                        };
                    }
                    Cmd::TransitionSet {
                        ty,
                        flags,
                        duration,
                        params,
                    } => {
                        if let Some(loader) = services.layer_load_services() {
                            loader.configure_transition(LoadTransition {
                                kind: *ty,
                                flags: *flags,
                                duration: *duration,
                                params: [
                                    params.0, params.1, params.2, params.3, params.4, params.5,
                                    params.6, params.7,
                                ],
                            })?;
                            pending.owner = Owner::Ready;
                        } else {
                            pending.owner = Owner::External;
                        }
                    }
                    command @ (Cmd::LayerUnload { .. }
                    | Cmd::Shared(RuntimeCommand::LAYERSWAP(_))
                    | Cmd::Shared(RuntimeCommand::PLANECLEAR(_))) => {
                        let mut prepared = self.layers.clone();
                        let edit = LayerEdit::prepare(&mut prepared, command)?;
                        if let Some(service) = services.layer_edit_services() {
                            service.edit_layers(&edit)?;
                            pending.owner = Owner::Ready;
                        } else {
                            pending.owner = Owner::External;
                        }
                        self.layers = prepared;
                    }
                    command @ Cmd::LayerCtrl { layer_id, .. } => {
                        let mut prepared = self.layers.clone();
                        let plan = LayerControlPlan::prepare(&mut prepared, command)?;
                        if let Some(control) = services
                            .layer_control_services()
                            .filter(|s| *layer_id != -1 || s.supports_root())
                        {
                            plan.apply(control)?;
                            pending.owner = Owner::Ready;
                        } else {
                            pending.owner = Owner::External;
                        }
                        self.layers = prepared;
                    }
                    Cmd::LayerInit { layer_id } => {
                        let mut prepared = self.layers.clone();
                        if let Some(control) = services
                            .layer_control_services()
                            .filter(|s| *layer_id != -1 || s.supports_root())
                        {
                            super::layer_control::initialize(&mut prepared, *layer_id, control)?;
                            pending.owner = Owner::Ready;
                        } else {
                            prepared.initialize(*layer_id)?;
                            pending.owner = Owner::External;
                        }
                        self.layers = prepared;
                    }
                    Cmd::LayerWait {
                        layer_id,
                        properties,
                    } => {
                        let wait = LayerWait::capture(&mut self.layers, *layer_id, properties)?;
                        pending.owner = if services
                            .layer_control_services()
                            .is_some_and(|s| !wait.needs_root() || s.supports_root())
                        {
                            Owner::LayerWait(wait)
                        } else {
                            Owner::External
                        };
                    }
                    Cmd::TransWait { layer_id } => {
                        let wait = TransitionWait::capture(&mut self.layers, *layer_id)?;
                        pending.owner = if services.layer_control_services().is_some() {
                            Owner::TransitionWait(wait)
                        } else {
                            Owner::External
                        };
                    }
                    command @ (Cmd::LayerSelect { .. } | Cmd::PlaneSelect { .. }) => {
                        let mut prepared = self.layers.clone();
                        prepared.apply(command)?;
                        self.layers = prepared;
                        pending.owner = if services.layer_control_services().is_some() {
                            Owner::Ready
                        } else {
                            Owner::External
                        };
                    }
                    command @ Cmd::LayerLoad { .. } => {
                        if let Some(loader) = services.layer_load_services() {
                            let plan = loader.prepare_plan(&mut self.layers, command)?;
                            pending.owner = Owner::LayerLoad(LayerLoadController::new(plan));
                        } else {
                            let mut prepared = self.layers.clone();
                            prepared.apply(command)?;
                            self.layers = prepared;
                            pending.owner = Owner::External;
                        }
                    }
                    command => {
                        let message_outcome = self.message.lock().dispatch(
                            command,
                            &self.assets.context().wgpu_device,
                            &self.assets.context().wgpu_queue,
                            context,
                            Some(self.voice.timeline()),
                            &self.scenario.bustup_info,
                        )?;
                        if let Some(outcome) = message_outcome {
                            pending.owner = Owner::Message;
                            self.message
                                .lock()
                                .apply_portrait(self.assets.clone(), outcome.portrait.clone());
                            if let Some(id) = outcome.installed_message {
                                pending.notification =
                                    Some(Notification::Install(id, outcome.portrait));
                            } else if outcome.portrait != PortraitChange::Unchanged {
                                pending.notification =
                                    Some(Notification::Portrait(outcome.portrait));
                            }
                        } else if self
                            .voice_commands
                            .dispatch(command, context.skip, &mut self.voice, services)?
                            .is_some()
                        {
                            pending.owner = Owner::Voice;
                        } else {
                            let mut prepared = self.layers.clone();
                            prepared.apply(command)?;
                            self.layers = prepared;
                            pending.owner = Owner::External;
                        }
                    }
                }
            }
            if !pending.descriptors_prepared {
                if let Some(save) = services.save_services() {
                    save.save_session()
                        .descriptors
                        .prepare(&pending.command, context.was_read)?;
                }
                pending.descriptors_prepared = true;
            }
            if let Some(notification) = &pending.notification {
                match notification {
                    Notification::Install(id, portrait) => {
                        services.message_installed(*id, portrait.clone())?
                    }
                    Notification::Portrait(portrait) => {
                        services.portrait_changed(portrait.clone())?
                    }
                }
                pending.notification = None;
            }
            let status = match &mut pending.owner {
                Owner::Quiz(state) => {
                    state.bind(
                        services
                            .quiz_services()
                            .context("Ger QUIZ service disappeared")?,
                    )?;
                    if let Some(save) = services.save_services() {
                        state.capture_before(save)?;
                    }
                    if let Some(result) = state.poll(
                        services
                            .quiz_services()
                            .context("Ger QUIZ service disappeared")?,
                    )? {
                        if let Some(save) = services.save_services() {
                            state.capture_after(result, &mut self.layers, save)?;
                        } else if let Some((_, bank)) = state.target {
                            self.layers.complete_interaction(bank, result);
                        }
                        self.response = CommandResult::WriteMemory(
                            self.vm
                                .pending_destination()
                                .context("QUIZ destination missing")?,
                            result,
                        );
                        services.quiz_services().unwrap().finish_quiz();
                        MessageCommandStatus::Complete
                    } else {
                        MessageCommandStatus::Pending
                    }
                }
                Owner::MovieWait(wait) => {
                    if wait.poll(
                        services
                            .movie_services()
                            .context("Ger movie wait service disappeared")?,
                    )? {
                        MessageCommandStatus::Complete
                    } else {
                        MessageCommandStatus::Pending
                    }
                }
                Owner::Audio(state) => {
                    let audio = services
                        .audio_commands()
                        .context("Ger audio service disappeared")?;
                    if state.poll(audio, context.skip) {
                        MessageCommandStatus::Complete
                    } else {
                        MessageCommandStatus::Pending
                    }
                }
                Owner::Autosave(state) => {
                    if state.poll_live(
                        services
                            .save_services()
                            .context("Ger save service disappeared")?,
                    )? {
                        MessageCommandStatus::Complete
                    } else {
                        MessageCommandStatus::Pending
                    }
                }
                Owner::Selection(state) => {
                    state.bind(
                        services
                            .selection_services()
                            .context("Ger SELECT service disappeared")?,
                    )?;
                    if let Some(save) = services.save_services() {
                        state.capture_before(save)?;
                    }
                    if let Some(result) = state.poll(services.selection_services().unwrap()) {
                        state.record_history(services.selection_services().unwrap(), result)?;
                        if let Some(save) = services.save_services() {
                            state.capture_after(result, save)?;
                        }
                        self.response = CommandResult::WriteMemory(
                            self.vm
                                .pending_destination()
                                .context("Ger SELECT destination missing")?,
                            result,
                        );
                        services.selection_services().unwrap().finish_selection();
                        MessageCommandStatus::Complete
                    } else {
                        MessageCommandStatus::Pending
                    }
                }
                Owner::New => unreachable!(),
                Owner::Ready => MessageCommandStatus::Complete,
                Owner::Wait => {
                    if self.wait.poll(context.skip) {
                        MessageCommandStatus::Complete
                    } else {
                        MessageCommandStatus::Pending
                    }
                }
                Owner::Message => self
                    .message
                    .lock()
                    .poll_command(Some(self.voice.timeline())),
                Owner::Voice => self.voice_commands.poll(&self.voice),
                Owner::Mask(command) => {
                    if command.poll(
                        services
                            .mask_services()
                            .context("Ger retained mask service disappeared")?,
                    )? {
                        MessageCommandStatus::Complete
                    } else {
                        MessageCommandStatus::Pending
                    }
                }
                Owner::Resume(command) => {
                    let mut available = true;
                    command.poll(|| {
                        available = services.resume_scene()?;
                        Ok(())
                    })?;
                    if !available {
                        pending.owner = Owner::External;
                    }
                    return Ok(if available {
                        RunnerStatus::Waiting
                    } else {
                        RunnerStatus::External
                    });
                }
                Owner::External => return Ok(RunnerStatus::External),
                Owner::LayerLoad(controller) => {
                    let loader = services
                        .layer_load_services()
                        .context("Gerokasu2 retained layer loader service disappeared")?;
                    if controller.poll(loader)? {
                        MessageCommandStatus::Complete
                    } else {
                        MessageCommandStatus::Pending
                    }
                }
                Owner::Wipe(controller) => {
                    let scene = services
                        .scene_services()
                        .context("Ger scene service disappeared")?;
                    if controller.poll(scene)? {
                        MessageCommandStatus::Complete
                    } else {
                        MessageCommandStatus::Pending
                    }
                }
                Owner::SceneWait => {
                    let scene = services
                        .scene_services()
                        .context("Ger scene wait service disappeared")?;
                    if scene.scene_wiping()? {
                        MessageCommandStatus::Pending
                    } else {
                        MessageCommandStatus::Complete
                    }
                }
                Owner::LayerWait(wait) => {
                    let services = services
                        .layer_control_services()
                        .context("Ger layer wait service disappeared")?;
                    if wait.poll(services, context.skip)? {
                        MessageCommandStatus::Complete
                    } else {
                        MessageCommandStatus::Pending
                    }
                }
                Owner::TransitionWait(wait) => {
                    let services = services
                        .layer_control_services()
                        .context("Ger transition wait service disappeared")?;
                    if wait.poll(services)? {
                        MessageCommandStatus::Complete
                    } else {
                        MessageCommandStatus::Pending
                    }
                }
            };
            if status == MessageCommandStatus::Pending {
                return Ok(RunnerStatus::Waiting);
            }
            self.pending = None;
            self.completed += 1;
        }
        Ok(RunnerStatus::CommandBudget)
    }
    /// Native105e0: command loop, WAIT clock, root/message update, then voice
    /// service. Pause gates commands/timer/root; native voice pause gates its
    /// own updater. Elapsed seconds still advance with fused tick arithmetic.
    pub fn advance(
        &mut self,
        frame: FrameId,
        delta: Ticks,
        mut policy: FramePolicy,
        services: &mut impl RunnerServices,
    ) -> Result<()> {
        if let Some(loader) = services.layer_load_services() {
            policy.animations_allowed = loader.animations_allowed();
        }
        self.voice.set_paused(policy.paused);
        if !policy.paused {
            if !policy.scene_changing && policy.autoplay {
                self.message.lock().request_autoplay();
            }
            if !policy.scene_changing && policy.fast_skip && policy.animations_allowed {
                services.prepare_root_fast_forward()?;
                self.message
                    .lock()
                    .fast_forward(&mut Gerokasu2MessageEffects {
                        voice: &mut self.voice,
                        notifications: services,
                    })?;
                services.fast_forward_layers(&self.layers)?;
                self.wait.clear_time();
            }
            if !policy.scene_changing {
                self.wait.advance(delta);
            }
            services.prepare_root_with_voice(
                frame,
                delta,
                policy.animations_allowed,
                &self.voice,
            )?;
            self.message.lock().update(
                frame,
                delta,
                policy.animations_allowed,
                &mut Gerokasu2MessageEffects {
                    voice: &mut self.voice,
                    notifications: services,
                },
            )?;
            self.message
                .lock()
                .update_portrait(frame, delta, &self.voice);
            services.advance_layers_with_voice(
                frame,
                &self.layers,
                delta,
                policy.animations_allowed,
                &self.voice,
            )?;
            self.voice.update(delta);
        }
        self.elapsed_seconds = delta
            .as_f32()
            .mul_add(f32::from_bits(0x3c888889), self.elapsed_seconds);
        Ok(())
    }
    pub fn update(
        &mut self,
        frame: FrameId,
        delta: Ticks,
        policy: FramePolicy,
        max_commands: usize,
        max_instructions: usize,
        services: &mut impl RunnerServices,
    ) -> Result<RunnerStatus> {
        self.voice.set_paused(policy.paused);
        let status = self.pump(
            policy.paused || policy.scene_changing,
            max_commands,
            max_instructions,
            services,
        )?;
        let policy = services.frame_policy(policy);
        self.advance(frame, delta, policy, services)?;
        Ok(status)
    }
    /// The drawable owner chooses the native tree's pass/stencil/matrices;
    /// the message owner submits windows, portrait and text in native order.
    pub fn render_message(
        &self,
        pass: &mut RenderPass,
        parent: Mat4,
        local: Mat4,
        stencil: u8,
        kind: PassKind,
    ) -> Result<()> {
        self.message
            .lock()
            .render(pass, parent, local, stencil, kind)
    }
}

#[cfg(test)]
mod tests;
