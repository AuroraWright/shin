use std::sync::Arc;

use anyhow::{Context, Result, bail, ensure};
use smallvec::SmallVec;

use crate::{
    format::scenario::{
        gerokasu2::{
            Gerokasu2Scenario,
            bytecode::{Instruction as GerInstruction, InstructionReader},
        },
        instruction_elements::{CodeAddress, MessageId, NumberSpec, Register},
        instructions::Instruction,
    },
    vm::{
        IntoRuntimeForm, VmCtx,
        command::{CommandResult, CompiletimeCommand, RuntimeCommand},
    },
};

/// Common evaluated command types are retained where their representation is
/// compatible. Later message and layer values remain explicit/raw so the ADV
/// bridge can choose this game's styles, CardSelect and resource policy.
#[derive(Debug)]
pub enum Gerokasu2RuntimeCommand {
    Audio(super::audio::Gerokasu2AudioCommand),
    Wait {
        allow_interrupt: u8,
        amount: i32,
    },
    Shared(RuntimeCommand),
    MessageSet {
        msg_id: MessageId,
        auto_wait: bool,
        message_arg: i32,
        text: String,
    },
    MessageFace {
        face: i32,
    },
    MessageInit {
        style: i32,
    },
    /// Native consumer multiplies raw volume by0.001f before clipping.
    VoicePlay {
        name: String,
        volume: i32,
        flags: i32,
    },
    VoiceWait {
        statuses: i32,
    },
    Selection {
        dest: Register,
        choice_set_base: i32,
        choice_index: i32,
        visibility: i32,
        title: String,
        variants: Vec<String>,
    },
    LayerLoad {
        layer_id: i32,
        layer_type: i32,
        flags: i32,
        params: (i32, i32, i32, i32, i32, i32, i32, i32),
    },
    LayerInit {
        layer_id: i32,
    },
    LayerUnload {
        layer_id: i32,
        delay_time: i32,
    },
    LayerCtrl {
        layer_id: i32,
        property_id: i32,
        params: (i32, i32, i32, i32, i32, i32, i32, i32),
    },
    LayerWait {
        layer_id: i32,
        properties: Vec<i32>,
    },
    LayerSelect {
        start: i32,
        end: i32,
    },
    MovieWait {
        layer_id: i32,
        statuses: i32,
    },
    TransWait {
        layer_id: i32,
    },
    Wipe {
        ty: i32,
        flags: i32,
        duration: i32,
        params: (i32, i32, i32, i32, i32, i32, i32, i32),
    },
    TransitionSet {
        ty: i32,
        flags: i32,
        duration: i32,
        params: (i32, i32, i32, i32, i32, i32, i32, i32),
    },
    PlaneSelect {
        plane: i32,
    },
    MaskLoad {
        mask_id: i32,
        flags: i32,
        animation_permission_arg: i32,
    },
}

#[derive(Clone)]
pub struct Gerokasu2Scripter {
    scenario: Arc<Gerokasu2Scenario>,
    ctx: VmCtx,
    reader: InstructionReader,
    position: CodeAddress,
    previous_random_state: u32,
    pending_destination: Option<Register>,
    halted: bool,
    checkpoint_ready: bool,
}

mod replay_state;
pub use replay_state::GerReplayState;

impl Gerokasu2Scripter {
    pub fn new(scenario: Arc<Gerokasu2Scenario>, init_value: i32, random_seed: u32) -> Self {
        let position = CodeAddress(scenario.header.code_offset);
        Self {
            reader: scenario.instruction_reader(position),
            scenario,
            ctx: VmCtx::new(init_value, random_seed),
            position,
            previous_random_state: random_seed,
            pending_destination: None,
            halted: false,
            checkpoint_ready: false,
        }
    }

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
    pub fn pending_destination(&self) -> Option<Register> {
        self.pending_destination
    }
    pub fn is_halted(&self) -> bool {
        self.halted
    }

    /// Run to the next ADV command without substituting menu/persistent results
    /// or executing engine effects. A budget bounds loops while preserving the
    /// state reached so far, including native per-instruction PRNG advancement.
    pub fn run(
        &mut self,
        result: CommandResult,
        max_instructions: usize,
    ) -> Result<Gerokasu2RuntimeCommand> {
        ensure!(!self.halted, "Gerokasu2 VM has exited");
        ensure!(
            max_instructions != 0,
            "Gerokasu2 instruction budget is zero"
        );
        match (self.pending_destination, result) {
            (None, CommandResult::None) => {}
            (Some(expected), CommandResult::WriteMemory(destination, value))
                if expected == destination =>
            {
                self.ctx.write_register(destination, value);
                self.pending_destination = None;
            }
            (expected, _) => bail!("Gerokasu2 command feedback mismatch: expected {expected:?}"),
        }
        self.checkpoint_ready = false;
        for _ in 0..max_instructions {
            let pc = self.reader.position();
            let instruction = self
                .reader
                .read()
                .with_context(|| format!("Reading Gerokasu2 instruction at {pc}"))?;
            self.previous_random_state = self.ctx.get_prng_state();
            self.ctx.update_prng();
            self.position = pc;
            match instruction {
                GerInstruction::Wait {
                    allow_interrupt,
                    amount,
                } => {
                    self.checkpoint_ready = true;
                    return Ok(Gerokasu2RuntimeCommand::Wait {
                        allow_interrupt,
                        amount: self.ctx.get_number(amount),
                    });
                }
                GerInstruction::MessageSet {
                    msg_id,
                    auto_wait,
                    message_arg,
                    text,
                } => {
                    self.checkpoint_ready = true;
                    return Ok(Gerokasu2RuntimeCommand::MessageSet {
                        // Native bfbe4: zero wire ID becomes the signed -1 sentinel.
                        msg_id: MessageId(msg_id.0.wrapping_sub(1)),
                        auto_wait: auto_wait.0,
                        message_arg: self.ctx.get_number(message_arg),
                        text: text.0,
                    });
                }
                GerInstruction::MessageFace { face } => {
                    self.checkpoint_ready = true;
                    return Ok(Gerokasu2RuntimeCommand::MessageFace {
                        face: self.ctx.get_number(face),
                    });
                }
                GerInstruction::Shared(Instruction::Command(command)) => {
                    let result = self.command(command);
                    self.checkpoint_ready = result.is_ok();
                    return result;
                }
                GerInstruction::Shared(control) => self.control(control)?,
                GerInstruction::Noop { .. } => {}
                GerInstruction::StoreMany {
                    value,
                    destinations,
                } => {
                    let value = self.ctx.get_number(value);
                    for destination in destinations.0 {
                        self.ctx.write_register(destination, value);
                    }
                }
                GerInstruction::StoreTable {
                    value,
                    index,
                    destinations,
                } => {
                    let value = self.ctx.get_number(value);
                    let index: i32 = self.ctx.get_number(index);
                    if let Ok(index) = usize::try_from(index) {
                        if let Some(&destination) = destinations.0.get(index) {
                            self.ctx.write_register(destination, value);
                        }
                    }
                }
                GerInstruction::CallTable { index, targets } => {
                    let index: i32 = self.ctx.get_number(index);
                    if let Ok(index) = usize::try_from(index) {
                        if let Some(&target) = targets.0.get(index) {
                            self.call(target)?;
                        }
                    }
                }
                GerInstruction::SearchTable {
                    equal,
                    dest,
                    value,
                    start,
                    table,
                } => {
                    let value: i32 = self.ctx.get_number(value);
                    let start: i32 = self.ctx.get_number(start);
                    let found = usize::try_from(start)
                        .ok()
                        .and_then(|start| {
                            table
                                .0
                                .iter()
                                .enumerate()
                                .skip(start)
                                .find_map(|(index, slot)| {
                                    ((self.ctx.get_number::<i32>(slot.0) == value) == equal)
                                        .then_some(index as i32)
                                })
                        })
                        .unwrap_or(-1);
                    self.ctx.write_register(dest, found);
                }
                GerInstruction::ResourceQuery {
                    dest,
                    selector,
                    index,
                } => {
                    let selector: i32 = self.ctx.get_number(selector);
                    let index: i32 = self.ctx.get_number(index);
                    let value = if selector == 100 {
                        usize::try_from(index)
                            .ok()
                            .and_then(|index| self.scenario.bustup_info.get(index))
                            .map(|record| i32::from(record.lipsync_character_id as i16))
                            .unwrap_or(0)
                    } else {
                        0
                    };
                    self.ctx.write_register(dest, value);
                }
            }
        }
        bail!(
            "Gerokasu2 instruction budget exhausted at {}",
            self.reader.position()
        )
    }

    fn command(&mut self, command: CompiletimeCommand) -> Result<Gerokasu2RuntimeCommand> {
        match command {
            CompiletimeCommand::BGMPLAY(c) => Ok(Gerokasu2RuntimeCommand::Audio(
                super::audio::Gerokasu2AudioCommand::BgmPlay {
                    id: self.raw(c.bgm_data_id),
                    fade: self.raw(c.fade_in_time),
                    loops: self.raw(c.no_repeat),
                    volume: self.raw(c.volume),
                },
            )),
            CompiletimeCommand::BGMSTOP(c) => Ok(Gerokasu2RuntimeCommand::Audio(
                super::audio::Gerokasu2AudioCommand::BgmStop {
                    fade: self.raw(c.fade_out_time),
                },
            )),
            CompiletimeCommand::BGMVOL(c) => Ok(Gerokasu2RuntimeCommand::Audio(
                super::audio::Gerokasu2AudioCommand::BgmVolume {
                    volume: self.raw(c.volume),
                    fade: self.raw(c.fade_in_time),
                },
            )),
            CompiletimeCommand::BGMWAIT(c) => Ok(Gerokasu2RuntimeCommand::Audio(
                super::audio::Gerokasu2AudioCommand::BgmWait {
                    mask: self.raw(c.unwanted_statuses),
                },
            )),
            CompiletimeCommand::BGMSYNC(c) => Ok(Gerokasu2RuntimeCommand::Audio(
                super::audio::Gerokasu2AudioCommand::BgmSync {
                    ticks: self.raw(c.sync_time),
                },
            )),
            CompiletimeCommand::SEPLAY(c) => Ok(Gerokasu2RuntimeCommand::Audio(
                super::audio::Gerokasu2AudioCommand::SePlay {
                    slot: self.raw(c.se_slot),
                    id: self.raw(c.se_data_id),
                    fade: self.raw(c.fade_in_time),
                    no_repeat: self.raw(c.no_repeat),
                    volume: self.raw(c.volume),
                    pan: self.raw(c.pan),
                    speed: self.raw(c.play_speed),
                },
            )),
            CompiletimeCommand::SESTOP(c) => Ok(Gerokasu2RuntimeCommand::Audio(
                super::audio::Gerokasu2AudioCommand::SeStop {
                    slot: self.raw(c.se_slot),
                    fade: self.raw(c.fade_out_time),
                },
            )),
            CompiletimeCommand::SESTOPALL(c) => Ok(Gerokasu2RuntimeCommand::Audio(
                super::audio::Gerokasu2AudioCommand::SeStopAll {
                    fade: self.raw(c.fade_out_time),
                },
            )),
            CompiletimeCommand::SEVOL(c) => Ok(Gerokasu2RuntimeCommand::Audio(
                super::audio::Gerokasu2AudioCommand::SeVolume {
                    slot: self.raw(c.se_slot),
                    volume: self.raw(c.volume),
                    fade: self.raw(c.fade_in_time),
                },
            )),
            CompiletimeCommand::SEPAN(c) => Ok(Gerokasu2RuntimeCommand::Audio(
                super::audio::Gerokasu2AudioCommand::SePan {
                    slot: self.raw(c.se_slot),
                    pan: self.raw(c.pan),
                    fade: self.raw(c.fade_in_time),
                },
            )),
            CompiletimeCommand::SEWAIT(c) => Ok(Gerokasu2RuntimeCommand::Audio(
                super::audio::Gerokasu2AudioCommand::SeWait {
                    slot: self.raw(c.se_slot),
                    mask: self.raw(c.unwanted_statuses),
                },
            )),
            CompiletimeCommand::SEONCE(c) => Ok(Gerokasu2RuntimeCommand::Audio(
                super::audio::Gerokasu2AudioCommand::SeOnce {
                    id: self.raw(c.arg1),
                    mask: self.raw(c.arg2),
                    volume: self.raw(c.arg3),
                    pan: self.raw(c.arg4),
                    speed: self.raw(c.arg5),
                },
            )),
            CompiletimeCommand::VOICEPLAY(c) => Ok(Gerokasu2RuntimeCommand::VoicePlay {
                name: c.name.0,
                volume: self.raw(c.volume),
                flags: self.raw(c.flags),
            }),
            CompiletimeCommand::VOICEWAIT(c) => Ok(Gerokasu2RuntimeCommand::VoiceWait {
                statuses: self.raw(c.unwanted_statuses),
            }),
            CompiletimeCommand::WIPE(c) => Ok(Gerokasu2RuntimeCommand::Wipe {
                ty: self.raw(c.ty),
                flags: self.raw(c.flags),
                duration: self.raw(c.duration),
                params: c.params.into_runtime_form(&self.ctx),
            }),
            CompiletimeCommand::TRANSSET(c) => Ok(Gerokasu2RuntimeCommand::TransitionSet {
                ty: self.raw(c.ty),
                flags: self.raw(c.flags),
                duration: self.raw(c.duration),
                params: c.params.into_runtime_form(&self.ctx),
            }),
            CompiletimeCommand::PLANESELECT(c) => Ok(Gerokasu2RuntimeCommand::PlaneSelect {
                plane: self.raw(c.plane_id),
            }),
            CompiletimeCommand::MASKLOAD(c) => Ok(Gerokasu2RuntimeCommand::MaskLoad {
                mask_id: self.raw(c.mask_id),
                flags: self.raw(c.mask_flags),
                animation_permission_arg: self.raw(c.smth_smth_transition),
            }),
            CompiletimeCommand::LAYERINIT(c) => Ok(Gerokasu2RuntimeCommand::LayerInit {
                layer_id: self.raw(c.layer_id),
            }),
            CompiletimeCommand::LAYERUNLOAD(c) => Ok(Gerokasu2RuntimeCommand::LayerUnload {
                layer_id: self.raw(c.layer_id),
                delay_time: self.raw(c.delay_time),
            }),
            CompiletimeCommand::LAYERCTRL(c) => Ok(Gerokasu2RuntimeCommand::LayerCtrl {
                layer_id: self.raw(c.layer_id),
                property_id: self.raw(c.property_id),
                params: c.params.into_untyped().into_runtime_form(&self.ctx),
            }),
            CompiletimeCommand::LAYERWAIT(c) => Ok(Gerokasu2RuntimeCommand::LayerWait {
                layer_id: self.raw(c.layer_id),
                properties: c
                    .wait_properties
                    .0
                    .into_iter()
                    .map(|value| self.raw(value))
                    .collect(),
            }),
            CompiletimeCommand::LAYERSELECT(c) => Ok(Gerokasu2RuntimeCommand::LayerSelect {
                start: self.raw(c.selection_start_id),
                end: self.raw(c.selection_end_id),
            }),
            CompiletimeCommand::MOVIEWAIT(c) => Ok(Gerokasu2RuntimeCommand::MovieWait {
                layer_id: self.raw(c.layer_id),
                statuses: self.raw(c.unwanted_statuses),
            }),
            CompiletimeCommand::TRANSWAIT(c) => Ok(Gerokasu2RuntimeCommand::TransWait {
                layer_id: self.raw(c.layer_id),
            }),
            CompiletimeCommand::EXIT(mut command) => {
                command.arg1 = u8::from(command.arg1 != 0);
                self.halted = command.arg1 == 0;
                Ok(Gerokasu2RuntimeCommand::Shared(RuntimeCommand::EXIT(
                    command.into_runtime_form(&self.ctx),
                )))
            }
            CompiletimeCommand::SELECT(command) => {
                self.pending_destination = Some(command.dest);
                Ok(Gerokasu2RuntimeCommand::Selection {
                    dest: command.dest,
                    // Native bfdf4/bfe14 subtract one after zero extension.
                    choice_set_base: i32::from(command.choice_set_base) - 1,
                    choice_index: i32::from(command.choice_index) - 1,
                    visibility: self.ctx.get_number(command.choice_visibility_mask),
                    title: command.choice_title.0,
                    variants: command.variants.0.into_vec(),
                })
            }
            CompiletimeCommand::MSGINIT(command) => Ok(Gerokasu2RuntimeCommand::MessageInit {
                style: self.ctx.get_number(NumberSpec::<i32>::new(
                    command.messagebox_style.into_untyped(),
                )),
            }),
            CompiletimeCommand::LAYERLOAD(command) => Ok(Gerokasu2RuntimeCommand::LayerLoad {
                layer_id: self
                    .ctx
                    .get_number(NumberSpec::<i32>::new(command.layer_id.into_untyped())),
                layer_type: self
                    .ctx
                    .get_number(NumberSpec::<i32>::new(command.layer_type.into_untyped())),
                flags: self
                    .ctx
                    .get_number(NumberSpec::<i32>::new(command.flags.into_untyped())),
                params: command.params.into_runtime_form(&self.ctx),
            }),
            CompiletimeCommand::MSGSET(_) => bail!("Unexpected Umineko MSGSET in Gerokasu2 VM"),
            command => {
                self.pending_destination = match &command {
                    CompiletimeCommand::SGET(command) => Some(command.dest),
                    CompiletimeCommand::QUIZ(command) => Some(command.dest),
                    _ => None,
                };
                Ok(Gerokasu2RuntimeCommand::Shared(
                    command.into_runtime_form(&self.ctx),
                ))
            }
        }
    }

    fn raw<T>(&self, number: NumberSpec<T>) -> i32 {
        self.ctx
            .get_number(NumberSpec::<i32>::new(number.into_untyped()))
    }

    fn jump(&mut self, target: CodeAddress) -> Result<()> {
        ensure!(
            target.0 >= self.scenario.header.code_offset,
            "Gerokasu2 branch target is inside metadata"
        );
        self.reader.set_position(target)
    }
    fn ensure_push(&self, count: usize) -> Result<()> {
        ensure!(
            self.ctx.stack_sizes().0 + count <= 64,
            "Gerokasu2 call stack overflow"
        );
        Ok(())
    }
    fn ensure_pop(&self, count: usize) -> Result<()> {
        ensure!(
            self.ctx.stack_sizes().0 >= count,
            "Gerokasu2 call stack underflow"
        );
        Ok(())
    }
    fn call(&mut self, target: CodeAddress) -> Result<()> {
        self.ensure_push(1)?;
        let continuation = self.reader.position();
        self.jump(target)?;
        self.ctx.push_code_stack(continuation);
        Ok(())
    }

    fn control(&mut self, instruction: Instruction) -> Result<()> {
        match instruction {
            Instruction::uo(operation) => {
                let source = self.ctx.get_number(operation.source);
                let value = self.ctx.evaluate_unary_operation(operation.ty, source);
                self.ctx.write_register(operation.destination, value);
            }
            Instruction::bo(operation) => {
                let left = self.ctx.get_number(operation.left);
                let right = self.ctx.get_number(operation.right);
                let value = self
                    .ctx
                    .evaluate_binary_operation(operation.ty, left, right);
                self.ctx.write_register(operation.destination, value);
            }
            Instruction::exp { dest, expr } => self
                .ctx
                .write_register(dest, self.ctx.evaluate_expression(&expr)),
            Instruction::gt { dest, index, table } => {
                let index: i32 = self.ctx.get_number(index);
                let value = usize::try_from(index)
                    .ok()
                    .and_then(|index| table.0.get(index))
                    .map(|value| self.ctx.get_number(value.0))
                    .unwrap_or(0);
                self.ctx.write_register(dest, value);
            }
            Instruction::j { target } => self.jump(target)?,
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
                    self.jump(target)?;
                }
            }
            Instruction::jt { index, table } => {
                let index: i32 = self.ctx.get_number(index);
                if let Ok(index) = usize::try_from(index) {
                    if let Some(&target) = table.0.get(index) {
                        self.jump(target)?;
                    }
                }
            }
            Instruction::gosub { target } => self.call(target)?,
            Instruction::retsub {} => {
                self.ensure_pop(1)?;
                let target = self.ctx.pop_code_stack();
                self.jump(target)?;
            }
            Instruction::rnd { dest, min, max } => self.ctx.write_register(
                dest,
                self.ctx
                    .run_prng(self.ctx.get_number(min), self.ctx.get_number(max)),
            ),
            Instruction::push { values } => {
                self.ensure_push(values.0.len())?;
                let values = values
                    .0
                    .iter()
                    .map(|&number| CodeAddress(self.ctx.get_number(number) as u32))
                    .collect::<SmallVec<[CodeAddress; 6]>>();
                for value in values {
                    self.ctx.push_code_stack(value);
                }
            }
            Instruction::pop { dest } => {
                self.ensure_pop(dest.0.len())?;
                for destination in dest.0 {
                    let value = self.ctx.pop_code_stack().0 as i32;
                    self.ctx.write_register(destination, value);
                }
            }
            Instruction::call { target, args } => {
                self.ensure_push(1)?;
                ensure!(
                    self.ctx.stack_sizes().2 + args.0.len() + 1 <= 256,
                    "Gerokasu2 argument stack overflow"
                );
                let values = args
                    .0
                    .iter()
                    .map(|&number| self.ctx.get_number(number))
                    .collect::<SmallVec<[i32; 6]>>();
                self.call(target)?;
                self.ctx.push_data_stack_frame(&values);
            }
            Instruction::r#return {} => {
                self.ensure_pop(1)?;
                ensure!(
                    self.ctx.stack_sizes().1 != 0,
                    "Gerokasu2 argument stack underflow"
                );
                self.ctx.pop_data_stack_frame();
                let target = self.ctx.pop_code_stack();
                self.jump(target)?;
            }
            Instruction::Command(_) => unreachable!(),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod route_explorer;
