//! Older SE callbacks and the VM descriptors used by aggregate waits.
use anyhow::Result;
use shin_core::vm::higurashi::command::HigurashiRuntimeCommand as Cmd;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EffectPlay {
    pub slot: i32,
    pub id: i32,
    pub fade: i32,
    pub count: u32,
    pub volume: f32,
    pub pan: f32,
    pub speed: f32,
}

/// Constructor1ab4 stores -1 for finite effects, or the raw ID for repeats,
/// before the ADV callback can suppress playback. STOPALL clears32 VM records;
/// the media owner's signed slots are a separate dynamic vector.
#[derive(Default)]
pub struct EffectDescriptors(BTreeMap<i32, i32>);
impl EffectDescriptors {
    pub fn from_checkpoint(effects: &[super::descriptors::EffectDescriptor; 32]) -> Self {
        Self(
            effects.iter().enumerate()
                .map(|(slot, effect)| (slot as i32, effect.id)).collect(),
        )
    }
    pub fn id(&self, slot: i32) -> i32 {
        self.0.get(&slot).copied().unwrap_or(-1)
    }
    pub fn observe(&mut self, command: &Cmd) {
        match command {
            Cmd::SEPLAY { args } => {
                self.0
                    .insert(args[0], if args[3] != 0 { -1 } else { args[1] });
            }
            Cmd::SESTOP { args } => {
                self.0.insert(args[0], -1);
            }
            Cmd::SESTOPALL { .. } => {
                for slot in 0..32 {
                    self.0.insert(slot, -1);
                }
            }
            _ => {}
        }
    }
}

pub trait EffectCommands {
    fn prepare_effect_command(&mut self, command: &Cmd);
    fn effect_descriptor_id(&self, slot: i32) -> i32;
    fn play_effect(&mut self, request: EffectPlay) -> Result<()>;
    fn stop_effect(&mut self, slot: i32, fade: i32) -> Result<()>;
    fn stop_all_effects(&mut self, fade: i32) -> Result<()>;
    fn volume_effect(&mut self, slot: i32, volume: f32, fade: i32) -> Result<()>;
    fn pan_effect(&mut self, slot: i32, pan: f32, fade: i32) -> Result<()>;
    fn effect_status(&self, slot: i32) -> i32;
    fn active_effect_slots(&self) -> Vec<i32>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectWait {
    Status { slot: i32, mask: i32 },
    Once { slot: i32, mask: i32 },
}
impl EffectWait {
    pub fn complete(self, effects: &dyn EffectCommands, suppress: bool) -> bool {
        match self {
            Self::Once { slot, mask } => suppress || effects.effect_status(slot) & mask == 0,
            Self::Status { slot, mask } if slot >= 0 => effects.effect_status(slot) & mask == 0,
            Self::Status { mask, .. } => effects
                .active_effect_slots()
                .into_iter()
                .filter(|&slot| slot < 0 || effects.effect_descriptor_id(slot) < 0)
                .all(|slot| effects.effect_status(slot) & mask == 0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectCommand {
    Complete,
    Wait(EffectWait),
}

pub fn dispatch(
    command: &Cmd,
    suppress: bool,
    effects: &mut dyn EffectCommands,
) -> Result<Option<EffectCommand>> {
    effects.prepare_effect_command(command);
    let fade = |time| if suppress { 0 } else { time };
    let result = match command {
        Cmd::SEPLAY {
            args: [slot, id, time, count, volume, pan, speed],
        } => {
            if suppress && *count != 0 {
                effects.stop_effect(*slot, 0)?;
            } else {
                effects.play_effect(EffectPlay {
                    slot: *slot,
                    id: *id,
                    fade: fade(*time),
                    count: *count as u32,
                    volume: *volume as f32 * 0.001,
                    pan: *pan as f32 * 0.001,
                    speed: *speed as f32 * 0.001,
                })?;
            }
            EffectCommand::Complete
        }
        Cmd::SESTOP { args: [slot, time] } => {
            effects.stop_effect(*slot, fade(*time))?;
            EffectCommand::Complete
        }
        Cmd::SESTOPALL { fade: time } => {
            effects.stop_all_effects(fade(*time))?;
            EffectCommand::Complete
        }
        Cmd::SEVOL {
            args: [slot, volume, time],
        } => {
            effects.volume_effect(*slot, *volume as f32 * 0.001, fade(*time))?;
            EffectCommand::Complete
        }
        Cmd::SEPAN {
            args: [slot, pan, time],
        } => {
            effects.pan_effect(*slot, *pan as f32 * 0.001, fade(*time))?;
            EffectCommand::Complete
        }
        Cmd::SEWAIT { args: [slot, mask] } => EffectCommand::Wait(EffectWait::Status {
            slot: *slot,
            mask: *mask,
        }),
        Cmd::SEONCE { .. } if suppress => EffectCommand::Complete,
        Cmd::SEONCE {
            args: [id, mask, volume, pan, _],
        } => {
            let mut slot: i32 = -1;
            while effects.effect_status(slot) & 2 != 0 {
                slot = slot.wrapping_sub(1);
            }
            effects.play_effect(EffectPlay {
                slot,
                id: *id,
                fade: 0,
                count: 1,
                volume: *volume as f32 * 0.001,
                pan: *pan as f32 * 0.001,
                speed: 1.0,
            })?;
            EffectCommand::Wait(EffectWait::Once { slot, mask: *mask })
        }
        _ => return Ok(None),
    };
    Ok(Some(result))
}

#[cfg(test)]
mod tests;
