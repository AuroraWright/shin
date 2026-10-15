//! Updated SNRSEL3a84/ee10/ee50/ee58 and the b6be0 scene factory.
//! Its checkpoint, replay stop and11780 transport match KAKERA. The factory
//! additionally retains the signed scenario-picker kind from the command.
use super::{
    autosave::{AutosaveMemory, SaveProgress},
    kakera::{KakeraCommand, KakeraServices},
    progress::ProgressMemory,
};
use anyhow::{Result, bail};
use shin_core::vm::higurashi::command::HigurashiRuntimeCommand as Cmd;

pub trait ScenarioSelectServices {
    fn packed_time(&mut self) -> Result<u64>;
    /// b6be0 owns a deep copy of both records and all four GameData vectors.
    /// Source date and scene/global bindings are retained by the caller.
    fn queue_scene(&mut self, kind: i32, source: &ProgressMemory) -> Result<()>;
}

#[derive(Debug)]
pub struct ScenarioSelectCommand {
    kind: i32,
    transport: KakeraCommand,
}

impl ScenarioSelectCommand {
    pub fn new(command: &Cmd) -> Result<Self> {
        let Cmd::SNRSEL { argument } = command else {
            bail!("ScenarioSelect constructor requires SNRSEL");
        };
        //3a84 copies the raw signed argument. The concrete picker factory
        // validates its supported kinds; command decoding does not clamp it.
        Ok(Self {
            kind: *argument,
            transport: KakeraCommand::new(),
        })
    }

    pub fn kind(&self) -> i32 {
        self.kind
    }

    pub fn replay_stop(&self) -> i32 {
        self.transport.replay_stop()
    }

    /// ee50 always blocks forward search, including after a replay stop.
    pub fn forward_complete(&self) -> bool {
        false
    }

    pub fn poll_replay(&mut self, progress: &SaveProgress, boundary: u32) -> Result<bool> {
        self.transport.poll_replay(progress, boundary)
    }

    pub fn poll_live(
        &mut self,
        progress: &mut ProgressMemory,
        memory: &mut AutosaveMemory,
        boundary: u32,
        services: &mut impl ScenarioSelectServices,
    ) -> Result<bool> {
        struct Factory<'a, S> {
            kind: i32,
            services: &'a mut S,
        }
        impl<S: ScenarioSelectServices> KakeraServices for Factory<'_, S> {
            fn packed_time(&mut self) -> Result<u64> {
                self.services.packed_time()
            }

            fn queue_scene(&mut self, mode: i32, source: &ProgressMemory) -> Result<()> {
                debug_assert_eq!(mode, 0);
                self.services.queue_scene(self.kind, source)
            }
        }
        self.transport.poll_live(
            progress,
            memory,
            boundary,
            &mut Factory {
                kind: self.kind,
                services,
            },
        )
    }
}

#[cfg(test)]
mod tests;
