//! Original MessageBrowser LogBuilder collection and measured-row adapter.
pub mod layout;
mod model;
use anyhow::{Result, ensure};
pub use model::measurement::Measurement;
pub use model::{Entry, Geometry, GeometryRow, Item};
use shin_core::{
    format::{
        save::gerokasu2::{GameData, SaveData},
        scenario::gerokasu2::Gerokasu2Scenario,
    },
    vm::{
        command::{CommandResult, RuntimeCommand},
        gerokasu2::{Gerokasu2RuntimeCommand as Cmd, Gerokasu2Scripter},
    },
};
use std::sync::Arc;

pub struct History {
    pub entries: Vec<Entry>,
    pub chapters: Vec<usize>,
    pub current_row: Option<usize>,
    /// Original7FD00 copies back extended responses but leaves both positions
    /// unchanged. QuickLoad later sets(replay_target,save_position) separately.
    pub jump_source: GameData,
}
impl History {
    /// Replay from scenario entry, as native80530/BDE70. Load ReplaySession
    /// stops at its current publication boundary and cannot collect the future.
    pub fn collect(
        scenario: Arc<Gerokasu2Scenario>,
        mut source: GameData,
        globals: &SaveData,
        command_limit: usize,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Self> {
        ensure!(command_limit > 0, "Ger history command budget is zero");
        let entry = source.progress.active_entry();
        let mut vm = Gerokasu2Scripter::new(scenario, entry.scenario_id, entry.random_seed);
        let mut builder = model::Builder::new(entry.save_position);
        let mut position = 0;
        let mut persist = globals.persist_data.clone();
        let mut response = CommandResult::None;
        for _ in 0..command_limit {
            ensure!(!cancelled(), "Ger history collection cancelled");
            if builder.done {
                break;
            }
            let command = vm.run(
                std::mem::replace(&mut response, CommandResult::None),
                100_000,
            )?;
            match command {
                Cmd::MessageSet {
                    msg_id,
                    auto_wait,
                    message_arg,
                    text,
                } => {
                    let id = msg_id.0;
                    let was_read = globals
                        .save_vectors
                        .seen_messages
                        .get(id as usize / 32)
                        .is_some_and(|w| w & (1 << (id % 32)) != 0);
                    builder.message(
                        id,
                        auto_wait,
                        message_arg,
                        &text,
                        globals.settings.v11_disallowskipunread,
                        was_read,
                    );
                }
                Cmd::Selection {
                    dest,
                    choice_index,
                    visibility,
                    title,
                    variants,
                    ..
                } => {
                    let historical = builder.begin_interactive();
                    let route = &mut source.progress.active_entry_mut().responses;
                    let result = if historical {
                        Some(super::super::responses::read_selection(
                            route,
                            &mut position,
                        ))
                    } else {
                        let remembered = usize::try_from(choice_index)
                            .ok()
                            .and_then(|i| globals.save_vectors.select_last_choices.get(i))
                            .copied()
                            .unwrap_or(0);
                        let value = model::future_choice(
                            choice_index,
                            remembered,
                            variants.len(),
                            visibility,
                        );
                        if let Some(value) = value {
                            super::super::responses::write_bits(
                                route,
                                &mut position,
                                4,
                                value as u32,
                            );
                        }
                        value
                    };
                    builder
                        .choice(result, &title, &variants)
                        .map_err(anyhow::Error::msg)?;
                    if let Some(value) = result {
                        response = CommandResult::WriteMemory(dest, value);
                    }
                }
                Cmd::Shared(RuntimeCommand::SGET(c)) => {
                    let route = &mut source.progress.active_entry_mut().responses;
                    let value = if builder.historical() {
                        super::super::responses::read_persistent(route, &mut position)
                    } else {
                        let value = persist.get(c.slot_number as u32 as usize);
                        super::super::responses::write_persistent(route, &mut position, value);
                        value
                    };
                    response = c.token.finish(value);
                }
                Cmd::Shared(RuntimeCommand::SSET(c)) => {
                    if !builder.historical() {
                        ensure!(
                            (c.slot_number as u32) <= 65535,
                            "Ger future persistent index exceeds bounded adapter"
                        );
                        persist.set(c.slot_number as usize, c.value);
                    }
                }
                Cmd::Shared(RuntimeCommand::QUIZ(c)) => {
                    let historical = builder.begin_interactive();
                    if historical {
                        let value = super::super::responses::read_quiz(
                            &mut source.progress.active_entry_mut().responses,
                            &mut position,
                        );
                        response = c.token.finish(value);
                    }
                    builder.quiz(historical);
                }
                Cmd::Shared(RuntimeCommand::SAVEINFO(c)) => builder
                    .save_info(c.level, &c.info)
                    .map_err(anyhow::Error::msg)?,
                Cmd::Shared(RuntimeCommand::AUTOSAVE(_)) => builder.checkpoint(),
                Cmd::Shared(RuntimeCommand::EVBEGIN(c)) => builder.event_begin(c.arg),
                Cmd::Shared(RuntimeCommand::EVEND(_)) => builder.event_end(),
                Cmd::Shared(RuntimeCommand::RESUMESET(_)) => builder.resume_set(),
                Cmd::Shared(RuntimeCommand::RESUME(_)) => builder.stop(),
                Cmd::Shared(RuntimeCommand::EXIT(c)) if c.arg1 == 0 => builder.stop(),
                Cmd::Shared(RuntimeCommand::MSGWAIT(c)) => builder.message_wait(c.signal_num),
                Cmd::Shared(RuntimeCommand::MSGCLOSE(_)) => builder.message_close(),
                _ => {}
            }
        }
        ensure!(!cancelled(), "Ger history collection cancelled");
        ensure!(builder.done, "Ger history exceeded command budget");
        let current_row = builder.current_row();
        Ok(Self {
            entries: builder.entries,
            chapters: builder.chapters,
            current_row,
            jump_source: source,
        })
    }
    pub fn geometry(&self, measured: &[(f32, u8)]) -> Result<Option<Geometry>> {
        model::geometry(&self.entries, self.current_row, measured).map_err(anyhow::Error::msg)
    }
}

#[cfg(test)]
mod tests;
