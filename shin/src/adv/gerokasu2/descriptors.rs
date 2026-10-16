//! Command-current Ger scene targets, separate from live audio/GPU owners.
use std::sync::Arc;

use anyhow::{Result, ensure};
use enum_map::Enum;
use shin_core::vm::{
    command::{RuntimeCommand, types::LayerProperty},
    gerokasu2::{Gerokasu2RuntimeCommand as Cmd, audio::Gerokasu2AudioCommand as Audio},
};

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
    /// Native90F0 is1 for an asynchronous retained message. Negative MSGWAIT
    /// clears it; MSGCLOSE clears text only when this byte is zero.
    pub asynchronous: u8,
    pub style: i32,
    /// Some("") preserves SELECT's non-null source-pointer condition.
    pub text: Option<Arc<str>>,
    pub face: i32,
    /// Gerokasu's last five-operand MSGFACE, re-applied after restore.
    pub ger_face: Option<[i32; 5]>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Descriptors {
    pub music: MusicDescriptor,
    pub effects: [EffectDescriptor; 32],
    pub message: MessageDescriptor,
    /// Original9F10 constructor handles LAYERCTRL−5 independently of the
    /// isolated D8xxx integer-layer helper class.
    pub message_properties: [i32; 90],
    /// Native2DE1: stable read status captured by the MSGSET constructor.
    pub current_was_read: u8,
    /// Native2DE0: EXITarg1==0 sets1; nonzero EXIT leaves it unchanged.
    pub restart_required: u8,
    pub system_controls: bool,
    /// Native37E0: transition type, flags, duration and eight parameter words.
    /// LOAD uses these command-current defaults, independent of live jobs.
    pub default_transition: [i32; 11],
    /// Native9290/9294 are copied checkpoint words; their initialized value
    /// isFFFFFFFF/00000000. Preserve them without inventing a consumer.
    pub opaque_tail: [u32; 2],
    /// Checkpoints copy9298's vector. Its populated producer is not proven;
    /// preserve supplied words without assigning invented behavior.
    pub auxiliary_words: Vec<u32>,
}

impl Default for Descriptors {
    fn default() -> Self {
        Self::new(0, false, -1)
    }
}

impl Descriptors {
    pub fn new(style: i32, asynchronous: bool, face: i32) -> Self {
        // NativeDB80 initializes inactive IDs but leaves audio parameters
        // unspecified. Zero placeholders have no playback effect while id−1.
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
                asynchronous: u8::from(asynchronous),
                style,
                text: None,
                face,
                ger_face: None,
            },
            message_properties: std::array::from_fn(|index| {
                LayerProperty::from_usize(index).initial_value()
            }),
            current_was_read: 0,
            restart_required: 0,
            system_controls: true,
            default_transition: [-1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            opaque_tail: [u32::MAX, 0],
            auxiliary_words: Vec::new(),
        }
    }

    fn effect(&mut self, slot: i32) -> Result<&mut EffectDescriptor> {
        ensure!(
            (0..32).contains(&slot),
            "Ger SE descriptor slot {slot} aliases state outside the32 records"
        );
        Ok(&mut self.effects[slot as usize])
    }

    /// Apply once after any native checkpoint-bearing constructor has captured
    /// the previous scene, before its first live/replay poll. True means this
    /// command has an audited descriptor mutation. Timing and live playback
    /// suppression do not change these raw targets.
    pub fn prepare(&mut self, command: &Cmd, was_read: bool) -> Result<bool> {
        let normalized = |value: i32| value as f32 * 0.001f32;
        match command {
            Cmd::Audio(Audio::BgmPlay {
                id, loops, volume, ..
            }) => {
                self.music = MusicDescriptor {
                    id: if *loops == 0 { *id } else { -1 },
                    volume: normalized(*volume),
                    count: *loops as u32,
                };
            }
            Cmd::Audio(Audio::BgmStop { .. }) => self.music.id = -1,
            Cmd::Audio(Audio::BgmVolume { volume, .. }) => self.music.volume = normalized(*volume),
            Cmd::Audio(Audio::SePlay {
                slot,
                id,
                no_repeat,
                volume,
                pan,
                speed,
                ..
            }) => {
                *self.effect(*slot)? = EffectDescriptor {
                    id: if *no_repeat == 0 { *id } else { -1 },
                    volume: normalized(*volume),
                    pan: normalized(*pan),
                    speed: normalized(*speed),
                };
            }
            Cmd::Audio(Audio::SeStop { slot, .. }) => self.effect(*slot)?.id = -1,
            Cmd::Audio(Audio::SeStopAll { .. }) => {
                for effect in &mut self.effects {
                    effect.id = -1;
                }
            }
            Cmd::Audio(Audio::SeVolume { slot, volume, .. }) => {
                self.effect(*slot)?.volume = normalized(*volume)
            }
            Cmd::Audio(Audio::SePan { slot, pan, .. }) => {
                self.effect(*slot)?.pan = normalized(*pan)
            }
            Cmd::MessageInit { style } => self.message.style = *style,
            Cmd::MessageFace { face } => self.message.face = *face,
            Cmd::GerMessageFace { args } => {
                self.message.face = args[1];
                self.message.ger_face = Some(*args);
            }
            Cmd::MessageSet {
                msg_id,
                auto_wait,
                text,
                ..
            } => {
                self.current_was_read = u8::from(was_read || (msg_id.0 as i32) < 0);
                self.message.asynchronous = u8::from(!auto_wait);
                self.message.text = Some(Arc::from(text.as_str()));
            }
            Cmd::Shared(RuntimeCommand::MSGWAIT(command)) if command.signal_num < 0 => {
                self.message.asynchronous = 0;
            }
            Cmd::Shared(RuntimeCommand::MSGCLOSE(_)) if self.message.asynchronous == 0 => {
                self.message.text = None;
            }
            Cmd::Shared(RuntimeCommand::SYSCALL(command)) if command.call_id == 0 => {
                self.system_controls = command.argument != 0;
            }
            Cmd::Shared(RuntimeCommand::EXIT(command)) if command.arg1 == 0 => {
                self.restart_required = 1;
            }
            Cmd::LayerCtrl {
                layer_id: -5,
                property_id,
                params,
            } => {
                ensure!(
                    (0..90).contains(property_id),
                    "Ger message property {property_id} is outside90 targets"
                );
                let target = &mut self.message_properties[*property_id as usize];
                *target = if params.2 as u32 & 0x80 != 0 {
                    target.wrapping_add(params.0)
                } else {
                    params.0
                };
            }
            Cmd::TransitionSet {
                ty,
                flags,
                duration,
                params,
            } => {
                self.default_transition = [
                    *ty, *flags, *duration, params.0, params.1, params.2, params.3, params.4,
                    params.5, params.6, params.7,
                ];
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests;
