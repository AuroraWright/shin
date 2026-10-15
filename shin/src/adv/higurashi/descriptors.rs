//! Command-current VM fields at 8a74..8c98 retained by ADV checkpoints.
//! These raw targets/strings are separate from live playback/GPU resources.
use anyhow::{Result, ensure};
use shin_core::vm::higurashi::command::HigurashiRuntimeCommand as Cmd;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MusicDescriptor {
    pub id: i32,
    pub volume: f32,
    pub count: u32,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EffectDescriptor {
    pub id: i32,
    pub volume: f32,
    pub pan: f32,
    pub speed: f32,
}
#[derive(Clone, Debug, PartialEq)]
pub struct MessageDescriptor {
    /// Native8c80: MSGSET stores auto_wait XOR1; negative MSGWAIT clears it.
    /// MSGCLOSE's constructor clears the text only when this byte is zero.
    pub wait_flag: u8,
    pub style: i32,
    pub options: [i32; 2],
    /// Some("") still represents a non-null source pointer for SELECT.
    pub text: Option<Arc<str>>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct PlaybackDescriptors {
    pub music: MusicDescriptor,
    pub effects: [EffectDescriptor; 32],
    pub message: MessageDescriptor,
}
impl PlaybackDescriptors {
    pub fn new() -> Self {
        // Native init sets inactive IDs to -1 but leaves their parameters
        // undefined. Zero is our deterministic placeholder until first use.
        Self {
            music: MusicDescriptor {
                id: -1,
                volume: 0.0,
                count: 0,
            },
            effects: [EffectDescriptor {
                id: -1,
                volume: 0.0,
                pan: 0.0,
                speed: 0.0,
            }; 32],
            message: MessageDescriptor {
                wait_flag: 0,
                style: 0,
                options: [0; 2],
                text: None,
            },
        }
    }
    fn effect(&mut self, slot: i32) -> Result<&mut EffectDescriptor> {
        // Native constructors index unchecked. Outside the authored VM record
        // area they alias neighboring state; keep that unaudited case explicit.
        ensure!(
            (0..32).contains(&slot),
            "Higurashi SE descriptor slot {slot} aliases state outside the 32 records"
        );
        Ok(&mut self.effects[slot as usize])
    }
    /// Constructor effects occur once, before read/skip can suppress live audio.
    /// STOP retains stale parameters and finite PLAY retains targets with id-1.
    pub fn observe(&mut self, command: &Cmd) -> Result<()> {
        let normalized = |value: i32| value as f32 * 0.001f32;
        match command {
            Cmd::BGMPLAY {
                args: [id, _, count, volume],
            } => {
                self.music = MusicDescriptor {
                    id: if *count == 0 { *id } else { -1 },
                    volume: normalized(*volume),
                    count: *count as u32,
                }
            }
            Cmd::BGMSTOP { .. } => self.music.id = -1,
            Cmd::BGMVOL { args: [volume, _] } => self.music.volume = normalized(*volume),
            Cmd::SEPLAY {
                args: [slot, id, _, count, volume, pan, speed],
            } => {
                *self.effect(*slot)? = EffectDescriptor {
                    id: if *count == 0 { *id } else { -1 },
                    volume: normalized(*volume),
                    pan: normalized(*pan),
                    speed: normalized(*speed),
                }
            }
            Cmd::SESTOP { args: [slot, _] } => self.effect(*slot)?.id = -1,
            Cmd::SESTOPALL { .. } => {
                for effect in &mut self.effects {
                    effect.id = -1;
                }
            }
            Cmd::SEVOL {
                args: [slot, volume, _],
            } => self.effect(*slot)?.volume = normalized(*volume),
            Cmd::SEPAN {
                args: [slot, pan, _],
            } => self.effect(*slot)?.pan = normalized(*pan),
            Cmd::MSGINIT {
                style,
                updated_options,
            } => {
                self.message.style = *style;
                self.message.options = updated_options.unwrap_or([0; 2]);
            }
            Cmd::MSGSET {
                auto_wait, text, ..
            } => {
                self.message.wait_flag = *auto_wait ^ 1;
                self.message.text = Some(Arc::from(text.as_str()));
            }
            Cmd::MSGWAIT { signal } if *signal < 0 => self.message.wait_flag = 0,
            Cmd::MSGCLOSE { .. } if self.message.wait_flag == 0 => self.message.text = None,
            _ => {}
        }
        Ok(())
    }
}
