//! Loaded SaveData/GameData binding for the updated109bc/10b28 VM constructor.
use super::super::runtime_game_data::RuntimeGameData;
use crate::adv::higurashi::{
    autosave::{AutosaveMemory, Execution, SaveProgress},
    chart_progress::ChartUnlocks,
    checkpoints::CheckpointMemory,
    choice::ChoiceHistory,
    input::ReadState,
    progress::ProgressMemory,
    read_history::MessageReadHistory,
    read_state::{Checkpoint, ReadServices},
    runner::HigurashiRunner,
};
use anyhow::{Context, Result, bail};
use shin_core::format::save::{
    higurashi::{GameData, SaveData, SaveDate},
    index_set::IndexSet,
};

fn progress(data: &GameData) -> Result<SaveProgress> {
    Ok(RuntimeGameData::from(data.clone()).progress)
}
pub(in crate::app::higurashi) fn packed_date(date: SaveDate) -> u64 {
    u64::from(date.year)
        | (u64::from(date.month) << 16)
        | (u64::from(date.day) << 24)
        | (u64::from(date.hour) << 32)
        | (u64::from(date.minute) << 40)
        | (u64::from(date.second) << 48)
}
pub(in crate::app::higurashi) fn unpacked_date(word: u64) -> SaveDate {
    SaveDate {
        year: word as u16,
        month: (word >> 16) as u8,
        day: (word >> 24) as u8,
        hour: (word >> 32) as u8,
        minute: (word >> 40) as u8,
        second: (word >> 48) as u8,
    }
}
pub(super) fn game_from_progress(
    progress: &SaveProgress,
    date: SaveDate,
    trailing_values: Vec<u16>,
    byte_variables: Vec<u8>,
) -> GameData {
    RuntimeGameData::from_progress(progress.clone(), date, trailing_values, byte_variables)
        .save_data()
}
pub(super) fn publish_autosave(memory: &AutosaveMemory, saved: &mut SaveData) {
    //11780 replaces+0c..+67 only;retain the target's+68/+80 vectors.
    let (trailing, bytes) = saved
        .auto_save_slot
        .as_ref()
        .map(|slot| (slot.trailing_values.clone(), slot.byte_variables.clone()))
        .unwrap_or_default();
    saved.auto_save_slot = Some(game_from_progress(
        &memory.progress,
        unpacked_date(memory.packed_time),
        trailing,
        bytes,
    ));
}
pub(in crate::app::higurashi) struct Bindings {
    pub date: SaveDate,
    pub progress: ProgressMemory,
    pub memory: CheckpointMemory,
    pub history: MessageReadHistory,
    pub choices: ChoiceHistory,
    pub autosave: AutosaveMemory,
}
impl Bindings {
    pub fn new(saved: &SaveData, data: &GameData) -> Result<Self> {
        Self::new_runtime(saved, &RuntimeGameData::from(data.clone()))
    }
    pub fn new_runtime(saved: &SaveData, data: &RuntimeGameData) -> Result<Self> {
        let current = data.progress.clone();
        let autosave = if let Some(slot) = &saved.auto_save_slot {
            AutosaveMemory {
                valid: true,
                packed_time: packed_date(slot.date),
                progress: progress(slot)?,
            }
        } else {
            AutosaveMemory {
                valid: false,
                packed_time: 0,
                progress: current.clone(),
            }
        };
        Ok(Self {
            date: data.date,
            progress: ProgressMemory::new(
                saved.persist_data.clone(),
                current,
                data.byte_variables.clone(),
            )
            .with_trailing_values(data.trailing_values.clone())
            .with_chart_unlocks(ChartUnlocks::from_saved_words(
                saved.save_vectors.extra_words_1.clone(),
            )),
            memory: CheckpointMemory::new(Vec::new()),
            history: MessageReadHistory::from_saved_words(saved.save_vectors.seen_messages.clone()),
            choices: ChoiceHistory::new(
                IndexSet::from_words(saved.save_vectors.select_seen_choices.clone()),
                saved.save_vectors.select_last_choices.clone(),
            ),
            autosave,
        })
    }
    pub fn game_data(&self) -> Result<GameData> {
        Ok(self.runtime_game_data()?.save_data())
    }
    pub fn runtime_game_data(&self) -> Result<RuntimeGameData> {
        Ok(RuntimeGameData::from_progress(
            self.progress.progress().clone(),
            self.date,
            self.progress.trailing_values()?,
            self.progress.bytes().to_vec(),
        ))
    }
    /// A browser closure copies GameData; settings/persist/read/choice owners
    /// still belong to the application. Do not import the history worker's
    /// temporary persistent variables or its response cursor into a fresh VM.
    pub fn replacement(&self, saved: &SaveData, source: &ProgressMemory) -> Result<Self> {
        let data = RuntimeGameData::from_progress(
            source.progress().clone(),
            self.date,
            source.trailing_values()?,
            source.bytes().to_vec(),
        );
        let mut replacement = Self::new_runtime(saved, &data)?;
        replacement.history = self.history.clone();
        replacement.choices = self.choices.clone();
        replacement.progress = replacement
            .progress
            .with_chart_unlocks(source.chart_unlocks.clone());
        Ok(replacement)
    }
    pub fn poll_commands(
        &mut self,
        runner: &mut HigurashiRunner,
        services: &mut impl crate::adv::higurashi::autosave::AutosaveServices,
    ) -> Result<bool> {
        Ok(runner
            .poll_progress_command(
                &mut self.progress,
                &mut self.autosave,
                Execution::Live,
                services,
            )?
            .unwrap_or(false))
    }
    pub fn poll_variables(&mut self, runner: &mut HigurashiRunner) -> Result<bool> {
        let command = runner
            .pending_command()
            .context("Story variable service requires a pending command")?;
        let boundary = runner
            .native_message_timeline()
            .context("Story variable service requires VM read state")?
            .boundary;
        let Some(response) =
            self.progress
                .dispatch_variables(command, Execution::Live, boundary)?
        else {
            return Ok(false);
        };
        runner.complete_external(response)?;
        Ok(true)
    }
    pub fn publish_globals(&self, saved: &mut SaveData) {
        saved.persist_data = self.progress.persistent().clone();
        saved.save_vectors.seen_messages = self.history.bits().words().to_vec();
        saved.save_vectors.select_seen_choices = self.choices.seen().words().to_vec();
        saved.save_vectors.select_last_choices = self.choices.last().to_vec();
        saved.save_vectors.extra_words_1 = self.progress.chart_unlocks.saved_words();
    }
}
impl ReadServices for Bindings {
    fn checkpoint_owner(&mut self) -> Option<(&mut CheckpointMemory, &mut ProgressMemory)> {
        Some((&mut self.memory, &mut self.progress))
    }
    fn checkpoint(&mut self, _: Checkpoint, _: ReadState) -> Result<()> {
        bail!("Story checkpoint requires its runner constructor context")
    }
    fn message_is_read(&self, id: u32) -> bool {
        self.history.contains(id)
    }
    fn message_completed(&mut self, id: u32) -> Result<()> {
        self.history.record_completion(true, id);
        Ok(())
    }
}

/// Live message polls need the same loaded slot and clock used by AUTOSAVE.
pub(super) struct LiveReads<'a> {
    pub bindings: &'a mut Bindings,
    pub saved: &'a mut SaveData,
    pub clock: &'a mut Box<dyn FnMut() -> Result<SaveDate> + Send>,
}
impl ReadServices for LiveReads<'_> {
    fn layer_settings(&mut self) -> Option<&mut SaveData> {
        Some(self.saved)
    }
    fn checkpoint_owner(&mut self) -> Option<(&mut CheckpointMemory, &mut ProgressMemory)> {
        self.bindings.checkpoint_owner()
    }
    fn checkpoint(&mut self, kind: Checkpoint, before: ReadState) -> Result<()> {
        self.bindings.checkpoint(kind, before)
    }
    fn message_is_read(&self, id: u32) -> bool {
        self.bindings.message_is_read(id)
    }
    fn message_completed(&mut self, id: u32) -> Result<()> {
        self.bindings.message_completed(id)
    }
    fn message_started(&mut self, boundary: u32) -> Result<()> {
        struct Copy<'a> {
            saved: &'a mut SaveData,
            clock: &'a mut Box<dyn FnMut() -> Result<SaveDate> + Send>,
        }
        impl crate::adv::higurashi::autosave::AutosaveServices for Copy<'_> {
            fn packed_time(&mut self) -> Result<u64> {
                Ok(packed_date((self.clock)()?))
            }
            fn captured(&mut self, memory: &AutosaveMemory) -> Result<()> {
                publish_autosave(memory, self.saved);
                Ok(())
            }
            fn notify(&mut self, _: &str) -> Result<()> {
                bail!("MSGSET progress copy must not construct a notification")
            }
        }
        self.bindings.autosave.capture_message_start(
            &mut self.bindings.progress,
            boundary,
            &mut Copy {
                saved: self.saved,
                clock: self.clock,
            },
        )
    }
}
