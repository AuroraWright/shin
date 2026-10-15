//! Older-engine VM. Arithmetic/state storage are shared where compatible;
//! Higurashi bytecode and older arithmetic differences stay explicit.
pub mod command;

use anyhow::{Context, Result, bail, ensure};
use smallvec::SmallVec;

use self::command::HigurashiRuntimeCommand;
use super::{VmCtx, command::CommandResult};
use crate::format::scenario::{
    higurashi::{
        HigurashiScenario,
        bytecode::{HigurashiInstruction, HigurashiInstructionReader},
    },
    instruction_elements::{CodeAddress, Register},
    instructions::{BinaryOperationType, Instruction, UnaryOperationType},
};

pub struct HigurashiScripter {
    ctx: VmCtx,
    reader: HigurashiInstructionReader,
    position: CodeAddress,
    previous_random_state: u32,
    pending_destination: Option<Register>,
    halted: bool,
}

/// Arithmetic portion of native1185c/11400 checkpoints. It retains its exact
/// scenario/variant, registers and stacks, and replays the yielded command.
/// ADV layers/audio/metadata, timeline and route restoration are separate state.
#[derive(Clone)]
pub struct HigurashiReplayState {
    ctx: VmCtx,
    reader: HigurashiInstructionReader,
    position: CodeAddress,
    previous_random_state: u32,
}
impl HigurashiReplayState {
    pub fn matches_scenario(&self, scenario: &HigurashiScenario) -> bool {
        self.reader.matches_scenario(scenario)
    }
    pub fn position(&self) -> CodeAddress {
        self.position
    }
    pub fn random_state(&self) -> u32 {
        self.ctx.get_prng_state()
    }
    pub fn scripter(&self) -> HigurashiScripter {
        HigurashiScripter {
            ctx: self.ctx.clone(),
            reader: self.reader.clone(),
            position: self.position,
            previous_random_state: self.previous_random_state,
            // The restored instruction must construct its command again;
            // its eventual feedback destination and EXIT status are decoded.
            pending_destination: None,
            halted: false,
        }
    }
}

impl HigurashiScripter {
    /// Compare the retained reader, without rewinding a constructed command.
    pub fn matches_scenario(&self, scenario: &HigurashiScenario) -> bool {
        self.reader.matches_scenario(scenario)
    }

    pub fn new(scenario: &HigurashiScenario, init_val: i32, random_seed: u32) -> Result<Self> {
        let entry = CodeAddress(scenario.header.code_offset);
        Ok(Self {
            ctx: VmCtx::new(init_val, random_seed),
            reader: scenario.instruction_reader(entry)?,
            position: entry,
            previous_random_state: random_seed,
            pending_destination: None,
            halted: false,
        })
    }

    /// Address of the last executed instruction (the command when yielding).
    pub fn position(&self) -> CodeAddress {
        self.position
    }

    pub fn next_position(&self) -> CodeAddress {
        self.reader.position()
    }

    pub fn random_state(&self) -> u32 {
        self.ctx.get_prng_state()
    }

    pub fn previous_random_state(&self) -> u32 {
        self.previous_random_state
    }

    pub fn read_register(&self, register: Register) -> i32 {
        self.ctx.read_register(register)
    }
    /// Native interactive polls publish feedback before their final memory copy
    /// and may retain the command at a replay boundary. Keep its destination
    /// pending so the eventual run still validates normal command completion.
    pub fn write_pending_result(&mut self, register: Register, value: i32) -> Result<()> {
        ensure!(
            self.pending_destination == Some(register),
            "Higurashi pending result destination mismatch"
        );
        self.ctx.write_register(register, value);
        Ok(())
    }

    /// Native119a4 copies arithmetic storage then119bc replaces its first two
    /// words with current-instruction PC/pre-instruction PRNG.11410 restores it.
    /// Reject initial/control/partly executed states instead of inventing a
    /// command checkpoint. Capturing never consumes feedback or changes the VM.
    pub fn checkpoint_for_replay(&self) -> Result<HigurashiReplayState> {
        let mut reader = self.reader.clone();
        reader.set_position(self.position)?;
        ensure!(
            matches!(reader.read()?, HigurashiInstruction::Command(_))
                && reader.position() == self.reader.position(),
            "Higurashi replay checkpoint requires a yielded command"
        );
        reader.set_position(self.position)?;
        Ok(HigurashiReplayState {
            ctx: self.ctx.clone_with_prng(self.previous_random_state),
            reader,
            position: self.position,
            previous_random_state: self.previous_random_state,
        })
    }

    /// Run to a command with a caller-selected instruction budget. No engine
    /// effects or implicit persistent/menu responses are performed here.
    pub fn run(
        &mut self,
        result: CommandResult,
        max_instructions: usize,
    ) -> Result<HigurashiRuntimeCommand> {
        ensure!(!self.halted, "Higurashi VM has exited");
        ensure!(max_instructions > 0, "Higurashi instruction budget is zero");
        match (self.pending_destination, result) {
            (None, CommandResult::None) => {}
            (Some(expected), CommandResult::WriteMemory(dest, value)) if expected == dest => {
                self.ctx.write_register(dest, value);
                self.pending_destination = None;
            }
            (expected, _) => bail!("Higurashi command feedback mismatch: expected {expected:?}"),
        }
        for _ in 0..max_instructions {
            let pc = self.reader.position();
            let instruction = self
                .reader
                .read()
                .with_context(|| format!("Reading Higurashi instruction at {pc}"))?;
            // The native dispatcher advances the LCG before every instruction,
            // including each command, taken jump, and returning instruction.
            self.previous_random_state = self.ctx.get_prng_state();
            self.ctx.update_prng();
            self.position = pc;
            match instruction {
                HigurashiInstruction::Command(command) => {
                    let command = command.into_runtime_form(&self.ctx);
                    self.pending_destination = command.destination();
                    self.halted = matches!(command, HigurashiRuntimeCommand::EXIT { arg1: 0, .. });
                    return Ok(command);
                }
                HigurashiInstruction::CallTable { index, table } => {
                    let index: i32 = self.ctx.get_number(index);
                    if let Ok(index) = usize::try_from(index) {
                        if let Some(&target) = table.0.get(index) {
                            self.ensure_push(1)?;
                            let continuation = self.reader.position();
                            self.reader.set_position(target)?;
                            // Return to the end of the table, not its chosen slot.
                            self.ctx.push_code_stack(continuation);
                        }
                    }
                }
                HigurashiInstruction::Control(instruction) => self
                    .execute_control(instruction)
                    .with_context(|| format!("Executing Higurashi control at {pc}"))?,
            }
        }
        bail!(
            "Higurashi instruction budget exhausted at {}",
            self.reader.position()
        )
    }

    fn ensure_push(&self, words: usize) -> Result<()> {
        ensure!(
            self.ctx.stack_sizes().0 + words <= 64,
            "Higurashi call stack overflow"
        );
        Ok(())
    }

    fn ensure_pop(&self, words: usize) -> Result<()> {
        ensure!(
            self.ctx.stack_sizes().0 >= words,
            "Higurashi call stack underflow"
        );
        Ok(())
    }

    fn execute_control(&mut self, instruction: Instruction) -> Result<()> {
        match instruction {
            Instruction::uo(op) => {
                let source = self.ctx.get_number(op.source);
                let value = match op.ty {
                    UnaryOperationType::Not16 => source ^ 0xffff,
                    UnaryOperationType::CountOnes => (source as u32 & 0xffff).count_ones() as i32,
                    UnaryOperationType::TrailingZeros => {
                        (source as u32 & 0xffff).trailing_zeros() as i32
                    }
                    _ => self.ctx.evaluate_unary_operation(op.ty, source),
                };
                self.ctx.write_register(op.destination, value);
            }
            Instruction::bo(op) => {
                let left = self.ctx.get_number(op.left);
                let right = self.ctx.get_number(op.right);
                let value = if op.ty == BinaryOperationType::ACursedOperation {
                    // This operator was added later; the older switch's default
                    // writes zero after consuming both operands.
                    0
                } else {
                    self.ctx.evaluate_binary_operation(op.ty, left, right)
                };
                self.ctx.write_register(op.destination, value);
            }
            Instruction::exp { dest, expr } => {
                self.ctx
                    .write_register(dest, self.ctx.evaluate_expression(&expr));
            }
            Instruction::jc {
                cond,
                left,
                right,
                target,
            } => {
                if self.ctx.compute_jump_condition(
                    cond,
                    self.ctx.get_number(left),
                    self.ctx.get_number(right),
                ) {
                    self.reader.set_position(target)?;
                }
            }
            Instruction::j { target } => self.reader.set_position(target)?,
            Instruction::gosub { target } => {
                self.ensure_push(1)?;
                let continuation = self.reader.position();
                self.reader.set_position(target)?;
                self.ctx.push_code_stack(continuation);
            }
            Instruction::retsub {} => {
                self.ensure_pop(1)?;
                let target = self.ctx.pop_code_stack();
                self.reader.set_position(target)?;
            }
            Instruction::jt { index, table } => {
                let index: i32 = self.ctx.get_number(index);
                if let Ok(index) = usize::try_from(index) {
                    if let Some(&target) = table.0.get(index) {
                        self.reader.set_position(target)?;
                    }
                }
            }
            Instruction::rnd { dest, min, max } => {
                let a: i32 = self.ctx.get_number(min);
                let b: i32 = self.ctx.get_number(max);
                let lower = a.min(b);
                let width = (a.max(b) as u32).wrapping_sub(lower as u32).wrapping_add(1);
                let sample = (self.ctx.get_prng_state() >> 8) & 0xffff;
                let value = lower.wrapping_add((sample.wrapping_mul(width) >> 16) as i32);
                self.ctx.write_register(dest, value);
            }
            Instruction::push { values } => {
                self.ensure_push(values.0.len())?;
                let values = values
                    .0
                    .into_iter()
                    .map(|v| CodeAddress(self.ctx.get_number(v) as u32))
                    .collect::<SmallVec<[_; 6]>>();
                for value in values {
                    self.ctx.push_code_stack(value);
                }
            }
            Instruction::pop { dest } => {
                self.ensure_pop(dest.0.len())?;
                for dest in dest.0 {
                    let value = self.ctx.pop_code_stack().0 as i32;
                    self.ctx.write_register(dest, value);
                }
            }
            Instruction::call { target, args } => {
                self.ensure_push(1)?;
                ensure!(
                    self.ctx.stack_sizes().2 + args.0.len() + 1 <= 256,
                    "Higurashi argument stack overflow"
                );
                let args = args
                    .0
                    .into_iter()
                    .map(|v| self.ctx.get_number(v))
                    .collect::<SmallVec<[_; 6]>>();
                let continuation = self.reader.position();
                self.reader.set_position(target)?;
                self.ctx.push_code_stack(continuation);
                self.ctx.push_data_stack_frame(&args);
            }
            Instruction::r#return {} => {
                self.ensure_pop(1)?;
                ensure!(
                    self.ctx.stack_sizes().1 > 0,
                    "Higurashi argument stack underflow"
                );
                self.ctx.pop_data_stack_frame();
                let target = self.ctx.pop_code_stack();
                self.reader.set_position(target)?;
            }
            // Not emitted by the older reader: do not silently execute later
            // dialect opcodes if its representation expands in the future.
            Instruction::gt { .. } | Instruction::Command(_) => {
                bail!("Unexpected Umineko instruction in Higurashi VM")
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
