//! Private D5A10 preview replay. Register as `gerokasu2::save_preview_replay`.
//! This specialized owner never constructs a retained live replay command.
use super::layers::Gerokasu2Layers;
use anyhow::{Context, Result, ensure};
use shin_core::{
    format::{save::gerokasu2::GameData, scenario::gerokasu2::Gerokasu2Scenario},
    vm::{
        command::{CommandResult, RuntimeCommand, types::LayerId},
        gerokasu2::{Gerokasu2RuntimeCommand as Cmd, Gerokasu2Scripter},
    },
};
use std::sync::Arc;

#[path = "replay/model.rs"]
mod model;
use model::{Callback, Controller};

pub(crate) struct Snapshot {
    pub layers: Arc<Gerokasu2Layers>,
    pub labels: [Arc<str>; 2],
}

/// Collect only preview scene targets and captured SAVEINFO labels. Native
/// SGET reads the saved response stream, SSET is a no-op, and AUTOSAVE/RESUME
/// do not access the ordinary live globals/autosave owner. No unrelated live
/// save descriptors are copied into this private replay owner.
pub(crate) fn collect(
    scenario: Arc<Gerokasu2Scenario>,
    source: GameData,
    mut cancelled: impl FnMut() -> bool,
) -> Result<Snapshot> {
    let entry = source.progress.active_entry();
    let mut vm = Gerokasu2Scripter::new(scenario, entry.scenario_id, entry.random_seed);
    let mut controller = Controller::new(entry.save_position, entry.responses.clone());
    let mut layers = Gerokasu2Layers::default();
    let mut response = CommandResult::None;
    let mut commands = 0usize;
    while !controller.ready {
        // The original worker has no discard/cancel poll. Desktop destruction
        // cancels between commands; bounds also protect corrupt script loops.
        ensure!(!cancelled(), "Ger saved preview replay cancelled");
        ensure!(
            commands < 1_000_000,
            "Ger saved preview command budget exhausted"
        );
        let command = vm.run(
            std::mem::replace(&mut response, CommandResult::None),
            100_000,
        )?;
        commands += 1;
        let callback = match &command {
            Cmd::Shared(RuntimeCommand::EXIT(c)) => Callback::Exit {
                continue_running: c.arg1 != 0,
            },
            Cmd::Shared(RuntimeCommand::SGET(_)) => Callback::PersistentGet,
            Cmd::MessageSet { auto_wait, .. } => Callback::MessageSet {
                auto_wait: *auto_wait,
            },
            Cmd::Shared(RuntimeCommand::MSGWAIT(c)) => Callback::MessageWait {
                signal: c.signal_num,
            },
            Cmd::Shared(RuntimeCommand::MSGCLOSE(_)) => Callback::MessageClose,
            Cmd::Selection { .. } => Callback::Selection,
            Cmd::Shared(RuntimeCommand::QUIZ(_)) => Callback::Quiz,
            Cmd::Shared(RuntimeCommand::SAVEINFO(c)) => Callback::SaveInfo {
                level: usize::try_from(c.level)
                    .context("Ger preview SAVEINFO level is negative")?,
                text: &c.info,
            },
            Cmd::Shared(RuntimeCommand::AUTOSAVE(_)) => Callback::Autosave,
            Cmd::Shared(RuntimeCommand::EVBEGIN(c)) => Callback::EventBegin { event: c.arg },
            Cmd::Shared(RuntimeCommand::EVEND(_)) => Callback::EventEnd,
            Cmd::Shared(RuntimeCommand::RESUMESET(_)) => Callback::ResumeSet,
            Cmd::Shared(RuntimeCommand::RESUME(_)) => Callback::Resume,
            Cmd::Shared(RuntimeCommand::PAGEBACK(_)) => Callback::PageBack,
            Cmd::Wipe { .. } => Callback::Wipe,
            _ => Callback::None,
        };
        let effect = controller.apply(callback).map_err(anyhow::Error::msg)?;
        if let Some(value) = effect.response {
            let destination = vm
                .pending_destination()
                .context("Ger preview response destination missing")?;
            response = CommandResult::WriteMemory(destination, value);
            if effect.complete_quiz
                && let Cmd::Shared(RuntimeCommand::QUIZ(c)) = &command
                && let Some(target) = u16::try_from(c.arg)
                    .ok()
                    .and_then(LayerId::try_new)
                    .and_then(|id| layers.target(id))
            {
                layers.complete_interaction(target.layerbank, value);
            }
        }
        // D8xxx's integer-layer lookup has no Message owner. Address−5 has no
        // matching bank; its properties and reset are ignored in this preview.
        let message_target = matches!(
            &command,
            Cmd::LayerLoad { layer_id: -5, .. }
                | Cmd::LayerInit { layer_id: -5 }
                | Cmd::LayerUnload { layer_id: -5, .. }
                | Cmd::LayerCtrl { layer_id: -5, .. }
        );
        if !message_target {
            layers.apply(&command)?;
        }
    }
    ensure!(!cancelled(), "Ger saved preview replay cancelled");
    Ok(Snapshot {
        layers: Arc::new(layers),
        labels: controller.saved_labels.map(Arc::from),
    })
}

#[cfg(test)]
#[path = "replay/engine_tests.rs"]
mod engine_tests;
