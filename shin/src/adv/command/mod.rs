#![allow(clippy::upper_case_acronyms)]

mod prelude {
    pub use std::sync::Arc;

    pub use CommandStartResult::Yield;
    pub use shin_core::{
        format::scenario::Scenario,
        time::Ticks,
        vm::{
            command,
            command::{
                CommandResult,
                types::{VLayerId, VLayerIdRepr},
            },
        },
    };
    pub use tracing::warn;

    pub use crate::{
        adv::{AdvState, CommandStartResult, StartableCommand, UpdatableCommand, VmState},
        layer::DrawableLayer,
        update::UpdateContext,
    };
}

mod autosave;
mod bgmplay;
mod bgmstop;
mod bgmsync;
mod bgmvol;
mod bgmwait;
mod chars;
mod debugout;
mod evbegin;
mod evend;
mod exit;
mod layerctrl;
mod layerinit;
mod layerload;
mod layerselect;
mod layerswap;
mod layerunload;
mod layerwait;
mod maskload;
mod maskunload;
mod moviewait;
mod msgclose;
mod msginit;
mod msgset;
mod msgsignal;
mod msgsync;
mod msgwait;
mod notifyset;
mod pageback;
mod planeclear;
mod planeselect;
mod quiz;
mod resume;
mod resumeset;
mod saveinfo;
mod select;
mod seonce;
mod sepan;
mod seplay;
mod sestop;
mod sestopall;
mod sevol;
mod sewait;
mod sget;
mod showchars;
mod sset;
mod syscall;
mod sysse;
mod tipsget;
mod transset;
mod transwait;
mod trophy;
mod unlock;
mod voiceplay;
mod voicestop;
mod voicewait;
mod wait;
mod wipe;
mod wipewait;

use std::sync::Arc;

use derivative::Derivative;
use enum_dispatch::enum_dispatch;
use shin_core::{
    format::scenario::Scenario,
    vm::command::{CommandResult, RuntimeCommand},
};

use self::quiz::QUIZ;
use self::{
    bgmsync::BGMSYNC, bgmwait::BGMWAIT, layerload::LAYERLOAD, layerwait::LAYERWAIT,
    maskload::MASKLOAD, moviewait::MOVIEWAIT, msgclose::MSGCLOSE, msgset::MSGSET, msgsync::MSGSYNC,
    msgwait::MSGWAIT, resume::RESUME, select::SELECT, seonce::SEONCE, sewait::SEWAIT,
    showchars::SHOWCHARS, transwait::TRANSWAIT, voicewait::VOICEWAIT, wait::WAIT, wipe::WIPE,
    wipewait::WIPEWAIT,
};
use crate::{
    adv::{AdvState, VmState},
    update::UpdateContext,
};

#[enum_dispatch]
pub trait UpdatableCommand {
    /// Consume an advance action when a command explicitly permits interruption.
    fn advance(&mut self) -> bool {
        false
    }

    fn interactive_target(
        &self,
    ) -> Option<(
        shin_core::vm::command::types::PlaneId,
        shin_core::vm::command::types::LayerbankId,
    )> {
        None
    }

    // TODO: provide mutable access to Adv Scene state
    fn update(
        &mut self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &mut VmState,
        adv_state: &mut AdvState,
        is_fast_forwarding: bool,
    ) -> Option<CommandResult>;
}

// all commands that yield to the game loop should have:
// - a type implementing UpdatableCommand
// - a enum variant for that type here
#[enum_dispatch(UpdatableCommand)]
#[derive(Derivative)]
#[derivative(Debug)]
pub enum ExecutingCommand {
    #[derivative(Debug = "transparent")]
    WAIT,
    #[derivative(Debug = "transparent")]
    MSGSET,
    #[derivative(Debug = "transparent")]
    MSGWAIT,
    #[derivative(Debug = "transparent")]
    MSGSYNC,
    #[derivative(Debug = "transparent")]
    MSGCLOSE,
    #[derivative(Debug = "transparent")]
    LAYERLOAD,
    #[derivative(Debug = "transparent")]
    LAYERWAIT,
    #[derivative(Debug = "transparent")]
    SEWAIT,
    #[derivative(Debug = "transparent")]
    SEONCE,
    #[derivative(Debug = "transparent")]
    MOVIEWAIT,
    #[derivative(Debug = "transparent")]
    WIPE,
    #[derivative(Debug = "transparent")]
    MASKLOAD,
    #[derivative(Debug = "transparent")]
    WIPEWAIT,
    #[derivative(Debug = "transparent")]
    TRANSWAIT,
    #[derivative(Debug = "transparent")]
    BGMSYNC,
    #[derivative(Debug = "transparent")]
    BGMWAIT,
    #[derivative(Debug = "transparent")]
    VOICEWAIT,
    #[derivative(Debug = "transparent")]
    SELECT,
    #[derivative(Debug = "transparent")]
    QUIZ,
    #[derivative(Debug = "transparent")]
    SHOWCHARS,
    #[derivative(Debug = "transparent")]
    RESUME,
}

pub fn apply_command_state(command: RuntimeCommand, state: &mut VmState) {
    apply_command_state_ref(&command, state).expect("unsupported state replay command");
}

pub fn apply_command_state_ref(
    command: &RuntimeCommand,
    state: &mut VmState,
) -> anyhow::Result<()> {
    // Reconstruction replays a completed modal command, whose native destructor
    // restores the old gate. Opening a live window belongs to start(), not replay.
    if matches!(command, RuntimeCommand::SHOWCHARS(_)) {
        return Ok(());
    }

    macro_rules! impl_apply_state {
        ($($cmd:ident),*) => {
            match command {
                $(RuntimeCommand::$cmd(v) => {v.apply_state(state);},)*
            }
        };
    }

    impl_apply_state!(
        EXIT,
        SGET,
        SSET,
        WAIT,
        MSGINIT,
        MSGSET,
        MSGWAIT,
        MSGSIGNAL,
        MSGSYNC,
        MSGCLOSE,
        SELECT,
        WIPE,
        WIPEWAIT,
        BGMPLAY,
        BGMSTOP,
        BGMVOL,
        BGMWAIT,
        BGMSYNC,
        SEPLAY,
        SESTOP,
        SESTOPALL,
        SEVOL,
        SEPAN,
        SEWAIT,
        SEONCE,
        VOICEPLAY,
        VOICESTOP,
        VOICEWAIT,
        SYSSE,
        SAVEINFO,
        AUTOSAVE,
        EVBEGIN,
        EVEND,
        RESUMESET,
        RESUME,
        SYSCALL,
        TROPHY,
        UNLOCK,
        LAYERINIT,
        LAYERLOAD,
        LAYERUNLOAD,
        LAYERCTRL,
        LAYERWAIT,
        LAYERSWAP,
        LAYERSELECT,
        MOVIEWAIT,
        TRANSSET,
        TRANSWAIT,
        PAGEBACK,
        PLANESELECT,
        PLANECLEAR,
        MASKLOAD,
        MASKUNLOAD,
        CHARS,
        TIPSGET,
        QUIZ,
        SHOWCHARS,
        NOTIFYSET,
        DEBUGOUT
    );
    Ok(())
}

fn apply_audio_skip_policy(command: &mut RuntimeCommand) -> bool {
    use shin_core::time::Ticks;
    match command {
        RuntimeCommand::BGMPLAY(v) => v.fade_in_time = Ticks::ZERO,
        RuntimeCommand::BGMSTOP(v) => v.fade_out_time = Ticks::ZERO,
        RuntimeCommand::BGMVOL(v) => v.fade_in_time = Ticks::ZERO,
        RuntimeCommand::SEPLAY(v) => {
            v.fade_in_time = Ticks::ZERO;
            // Native6ee8..6f0c stops this slot instead of requesting a
            // non-repeating effect during skip. Repeating effects still start.
            return v.no_repeat;
        }
        RuntimeCommand::SEONCE(_) | RuntimeCommand::VOICEPLAY(_) => return true,
        RuntimeCommand::SESTOP(v) => v.fade_out_time = Ticks::ZERO,
        RuntimeCommand::SESTOPALL(v) => v.fade_out_time = Ticks::ZERO,
        RuntimeCommand::SEVOL(v) => v.fade_in_time = Ticks::ZERO,
        RuntimeCommand::SEPAN(v) => v.fade_in_time = Ticks::ZERO,
        _ => {}
    }
    false
}

pub fn apply_command_state_and_start(
    command: RuntimeCommand,
    context: &mut UpdateContext,
    scenario: &Arc<Scenario>,
    vm_state: &mut VmState,
    adv_state: &mut AdvState,
) -> CommandStartResult {
    apply_command_state_and_start_with_skip(command, context, scenario, vm_state, adv_state, false)
}

/// Native audio polls inspect the current skip policy before starting the
/// command. The ordinary entry point remains suitable for restore/probes.
pub fn apply_command_state_and_start_with_skip(
    command: RuntimeCommand,
    context: &mut UpdateContext,
    scenario: &Arc<Scenario>,
    vm_state: &mut VmState,
    adv_state: &mut AdvState,
    is_fast_forwarding: bool,
) -> CommandStartResult {
    apply_command_state_and_start_with_skip_policy(
        command,
        context,
        scenario,
        vm_state,
        adv_state,
        is_fast_forwarding,
        if is_fast_forwarding {
            NewMessageSkip::Requested
        } else {
            NewMessageSkip::Normal
        },
    )
}

/// The old message determines skip for its pending command. MSGSET replaces
/// that read flag before querying skip, so retain the underlying input request.
#[derive(Clone, Copy)]
pub(super) enum NewMessageSkip {
    Normal,
    Requested,
    Forced,
}

pub(super) fn apply_command_state_and_start_with_skip_policy(
    mut command: RuntimeCommand,
    context: &mut UpdateContext,
    scenario: &Arc<Scenario>,
    vm_state: &mut VmState,
    adv_state: &mut AdvState,
    is_fast_forwarding: bool,
    message_skip: NewMessageSkip,
) -> CommandStartResult {
    command = match command {
        RuntimeCommand::MSGSET(message) => {
            msgset::apply_state_before_install(&message, vm_state);
            let skipping = match message_skip {
                NewMessageSkip::Normal => false,
                NewMessageSkip::Requested => {
                    !vm_state.settings.v11_disallowskipunread || vm_state.messagebox_state.was_read
                }
                NewMessageSkip::Forced => true,
            };
            return msgset::start_with_skip(
                message, context, scenario, vm_state, adv_state, skipping,
            );
        }
        other => other,
    };
    if is_fast_forwarding && apply_audio_skip_policy(&mut command) {
        return match command {
            RuntimeCommand::SEPLAY(se) => {
                se.apply_state(vm_state);
                adv_state
                    .audio
                    .se_player
                    .stop(se.se_slot, shin_core::time::Tween::IMMEDIATE);
                se.token.finish().into()
            }
            RuntimeCommand::SEONCE(se) => {
                se.apply_state(vm_state);
                se.token.finish().into()
            }
            RuntimeCommand::VOICEPLAY(voice) => {
                voice.apply_state(vm_state);
                voice.token.finish().into()
            }
            _ => unreachable!(),
        };
    }
    macro_rules! impl_apply_state {
        ($($cmd:ident),*) => {
            match command {
                $(RuntimeCommand::$cmd(v) => {let info = v.apply_state(vm_state); v.start(context, scenario, vm_state, info, adv_state)},)*
            }
        };
        () => {};
    }

    impl_apply_state!(
        EXIT,
        SGET,
        SSET,
        WAIT,
        MSGINIT,
        MSGSET,
        MSGWAIT,
        MSGSIGNAL,
        MSGSYNC,
        MSGCLOSE,
        SELECT,
        WIPE,
        WIPEWAIT,
        BGMPLAY,
        BGMSTOP,
        BGMVOL,
        BGMWAIT,
        BGMSYNC,
        SEPLAY,
        SESTOP,
        SESTOPALL,
        SEVOL,
        SEPAN,
        SEWAIT,
        SEONCE,
        VOICEPLAY,
        VOICESTOP,
        VOICEWAIT,
        SYSSE,
        SAVEINFO,
        AUTOSAVE,
        EVBEGIN,
        EVEND,
        RESUMESET,
        RESUME,
        SYSCALL,
        TROPHY,
        UNLOCK,
        LAYERINIT,
        LAYERLOAD,
        LAYERUNLOAD,
        LAYERCTRL,
        LAYERWAIT,
        LAYERSWAP,
        LAYERSELECT,
        MOVIEWAIT,
        TRANSSET,
        TRANSWAIT,
        PAGEBACK,
        PLANESELECT,
        PLANECLEAR,
        MASKLOAD,
        MASKUNLOAD,
        CHARS,
        TIPSGET,
        QUIZ,
        SHOWCHARS,
        NOTIFYSET,
        DEBUGOUT
    )
}

pub enum CommandStartResult {
    /// Continue VM execution
    Continue(CommandResult),
    /// Yield to the game loop, run the command to completion, execution continued with the result
    Yield(ExecutingCommand),
    Exit,
}

/// Start a reconstruction stop whose VM effects have already been applied.
pub(super) fn start_reconstructed_command(
    command: RuntimeCommand,
    context: &mut UpdateContext,
    scenario: &Arc<Scenario>,
    state: &VmState,
    adv: &mut AdvState,
) -> anyhow::Result<CommandStartResult> {
    Ok(match command {
        RuntimeCommand::MSGSET(cmd) => cmd.start(context, scenario, state, (), adv),
        RuntimeCommand::SELECT(cmd) => cmd.start(context, scenario, state, (), adv),
        RuntimeCommand::QUIZ(cmd) => {
            use shin_core::vm::command::types::LayerId;
            let plane = state.layers.current_plane;
            let target = u16::try_from(cmd.arg)
                .ok()
                .and_then(LayerId::try_new)
                .and_then(|layer| {
                    state
                        .layers
                        .layerbank_allocator
                        .get_layerbank_id(plane, layer)
                })
                .map(|bank| (plane, bank));
            cmd.start(context, scenario, state, target, adv)
        }
        RuntimeCommand::AUTOSAVE(cmd) => cmd.start(context, scenario, state, (), adv),
        RuntimeCommand::EVBEGIN(cmd) => cmd.start(context, scenario, state, (), adv),
        RuntimeCommand::EVEND(cmd) => cmd.start(context, scenario, state, (), adv),
        RuntimeCommand::RESUMESET(cmd) => cmd.start(context, scenario, state, (), adv),
        other => anyhow::bail!("Unsupported reconstruction stop: {other}"),
    })
}

pub(super) fn replay_choice_result(state: &mut VmState, base: u16, index: u16, value: i32) {
    select::record_choice(state, base, index, value);
}

pub(super) fn replay_quiz_result(
    state: &mut VmState,
    target: Option<(
        shin_core::vm::command::types::PlaneId,
        shin_core::vm::command::types::LayerbankId,
    )>,
    value: i32,
) {
    quiz::record_result(state, target, value);
}

impl From<CommandResult> for CommandStartResult {
    fn from(result: CommandResult) -> Self {
        CommandStartResult::Continue(result)
    }
}

impl From<ExecutingCommand> for CommandStartResult {
    fn from(command: ExecutingCommand) -> Self {
        CommandStartResult::Yield(command)
    }
}

pub trait StartableCommand {
    /// Type of the information the commands needs smuggled from the `apply_state` to the `start` method
    ///
    /// When doing a command replay for scene loading, this information is ignored
    ///
    /// This usually corresponds to non-param data set in the command constructor
    // TODO: for scene loading, this info can actually be paseed to the `RunState` function (or whatever it would be)
    type StateInfo;

    fn apply_state(&self, state: &mut VmState) -> Self::StateInfo;
    fn start(
        self,
        context: &mut UpdateContext,
        scenario: &Arc<Scenario>,
        vm_state: &VmState,
        state_info: Self::StateInfo,
        adv_state: &mut AdvState,
    ) -> CommandStartResult;
}

#[cfg(test)]
pub(crate) use transwait::tests::reference_live_layer_transitions;

#[cfg(test)]
pub(crate) use layerswap::tests::reference_live_layerswap;
#[cfg(test)]
pub(crate) use layerunload::tests::reference_live_delayed_layerunload;
#[cfg(test)]
pub(crate) use maskload::tests::reference_live_maskload_policy;
#[cfg(test)]
pub(crate) use maskunload::tests::reference_live_maskunload;

#[cfg(test)]
pub(crate) use layerload::tests::reference_live_interactive_publication;
#[cfg(test)]
pub(crate) use layerload::tests::reference_live_publication_constructors;

#[cfg(test)]
pub(crate) use layerload::tests::reference_live_movie_publication;

#[cfg(test)]
mod audio_skip_tests;

#[cfg(test)]
pub(crate) use audio_skip_tests::reference_live_audio_skip_dispatch;

#[cfg(test)]
pub(crate) fn reference_hold_asset_request(
    command: &mut ExecutingCommand,
) -> futures::channel::oneshot::Sender<()> {
    match command {
        ExecutingCommand::LAYERLOAD(command) => command.reference_hold_loading(),
        ExecutingCommand::WIPE(command) => command.reference_hold_loading(),
        _ => panic!("Pending asset/wipe command required"),
    }
}
