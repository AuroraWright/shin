//! Numeric part of original EEC0 checkpoints. ADV descriptors, layers, timeline,
//! response cursor and current global progress belong to the scene checkpoint.
use super::*;

/// Immutable arithmetic checkpoint retaining its exact Ger scenario and all
/// registers/stacks. Restoration re-decodes the yielded command from its entry
/// PC and pre-instruction seed, rather than retaining an engine continuation.
#[derive(Clone)]
pub struct GerReplayState {
    scenario: Arc<Gerokasu2Scenario>,
    ctx: Box<VmCtx>,
    reader: InstructionReader,
    position: CodeAddress,
    previous_random_state: u32,
}

impl GerReplayState {
    pub fn matches_scenario(&self, scenario: &Gerokasu2Scenario) -> bool {
        self.scenario.raw() == scenario.raw()
    }

    pub fn position(&self) -> CodeAddress {
        self.position
    }

    /// Seed immediately before the yielded command's instruction.
    pub fn random_state(&self) -> u32 {
        self.ctx.get_prng_state()
    }

    /// Rebuild an unbound interpreter. Command decoding restores its eventual
    /// feedback destination and EXIT status; registers/stacks are never reset.
    pub fn scripter(&self) -> Gerokasu2Scripter {
        Gerokasu2Scripter {
            scenario: self.scenario.clone(),
            ctx: (*self.ctx).clone(),
            reader: self.reader.clone(),
            position: self.position,
            previous_random_state: self.previous_random_state,
            pending_destination: None,
            halted: false,
            checkpoint_ready: false,
        }
    }
}

impl Gerokasu2Scripter {
    pub fn matches_scenario(&self, scenario: &Gerokasu2Scenario) -> bool {
        self.scenario.raw() == scenario.raw()
    }

    /// Native interactive replay callbacks publish feedback before retaining or
    /// retiring their command. Retain the destination so normal run completion
    /// still checks the explicit feedback token against the same register.
    pub fn write_pending_result(&mut self, destination: Register, value: i32) -> Result<()> {
        ensure!(
            self.pending_destination == Some(destination),
            "Gerokasu2 pending result destination mismatch"
        );
        self.ctx.write_register(destination, value);
        Ok(())
    }

    /// Native EEC0 copies VM+8 then EFD8..EFEC replaces the first two checkpoint
    /// words with command-entry PC/pre-instruction PRNG. Capture is immutable.
    ///
    /// Requires a successful yielded command (including EXIT), with its full
    /// instruction still matching the reader's next position. Initial state,
    /// control-loop budgets, malformed/unsupported instructions and restored
    /// states not yet decoded return errors; a rejected feedback token leaves
    /// the preceding valid yield available. Engine continuation state is never
    /// included in this numeric payload.
    pub fn checkpoint_for_replay(&self) -> Result<GerReplayState> {
        ensure!(
            self.checkpoint_ready,
            "Gerokasu2 replay checkpoint requires a successfully yielded command"
        );
        let mut reader = self.reader.clone();
        reader.set_position(self.position)?;
        let instruction = reader.read()?;
        ensure!(
            matches!(
                instruction,
                GerInstruction::Wait { .. }
                    | GerInstruction::MessageSet { .. }
                    | GerInstruction::MessageFace { .. }
                    | GerInstruction::Shared(Instruction::Command(_))
            ) && reader.position() == self.reader.position(),
            "Gerokasu2 replay checkpoint has an inconsistent command boundary"
        );
        reader.set_position(self.position)?;
        Ok(GerReplayState {
            scenario: self.scenario.clone(),
            ctx: Box::new(self.ctx.clone_with_prng(self.previous_random_state)),
            reader,
            position: self.position,
            previous_random_state: self.previous_random_state,
        })
    }
}

#[cfg(test)]
mod tests;
