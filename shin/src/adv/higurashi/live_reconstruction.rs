//! Final replayed-memory publication. Native replay/mode/overlay owners supply
//! the final payload; this does not interpret a raw save as a replayed scene.
use super::{
    descriptors::{EffectDescriptor, PlaybackDescriptors},
    effect_commands::{EffectCommands, EffectPlay},
    music_commands::MusicCommands,
    voice_commands::VoiceCommands,
};
use anyhow::Result;

pub trait ReconstructionAudio: VoiceCommands + MusicCommands + EffectCommands {
    fn prepare_effect_reconstruction(&mut self) -> Result<()>;
    fn install_effect_descriptors(&mut self, effects: &[EffectDescriptor; 32]);
}

/// Original19f40..19ff0. Native repeats use count0 and raw float targets, even
/// if an inactive descriptor retains a finite count or stale parameters.
pub fn restore_audio(
    descriptors: &PlaybackDescriptors,
    audio: &mut impl ReconstructionAudio,
) -> Result<()> {
    let music = descriptors.music;
    if music.id >= 0 {
        audio.play_bgm(music.id, 0, 0, music.volume)?;
    } else {
        audio.stop_bgm(0);
    }
    audio.prepare_effect_reconstruction()?;
    for (slot, effect) in descriptors.effects.iter().enumerate() {
        if effect.id >= 0 {
            audio.play_effect(EffectPlay {
                slot: slot as i32,
                id: effect.id,
                fade: 0,
                count: 0,
                volume: effect.volume,
                pan: effect.pan,
                speed: effect.speed,
            })?;
        } else {
            audio.stop_effect(slot as i32, 0)?;
        }
    }
    audio.install_effect_descriptors(&descriptors.effects);
    Ok(())
}

#[cfg(test)]
pub(super) mod tests;
