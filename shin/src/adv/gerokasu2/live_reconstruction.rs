//! Publication of replayed audio targets after scene resources are ready.
use super::{audio_commands::AudioCommands, descriptors::Descriptors};
use anyhow::{Result, ensure};

/// Check the entire restored request set before a prepared app destination
/// gains access to the shared audible players.
pub(crate) fn check_audio_metadata(
    descriptors: &Descriptors,
    bgm_count: usize,
    se_count: usize,
) -> Result<()> {
    ensure!(
        descriptors.music.id < 0 || (descriptors.music.id as usize) < bgm_count,
        "Invalid Ger restored BGM {}",
        descriptors.music.id
    );
    for (slot, effect) in descriptors.effects.iter().enumerate() {
        ensure!(
            effect.id < 0 || (effect.id as usize) < se_count,
            "Invalid Ger restored SE {} in slot {slot}",
            effect.id
        );
    }
    Ok(())
}

pub trait ReconstructionAudio: AudioCommands {
    /// Original C3980 shortens already stopping effects to four ticks. Active
    /// negative one-shot slots remain outside the subsequent 32-slot loop.
    fn prepare_effect_reconstruction(&mut self) -> Result<()>;
}

/// Original 1AC3C..1ACE8. Retained IDs represent infinite repeats; their saved
/// finite count and stale inactive parameters do not control publication.
pub fn restore_audio(
    descriptors: &Descriptors,
    audio: &mut impl ReconstructionAudio,
) -> Result<()> {
    let music = descriptors.music;
    if music.id >= 0 {
        audio.bgm_play(music.id, 0, music.volume, 0)?;
    } else {
        audio.bgm_stop(0)?;
    }
    audio.prepare_effect_reconstruction()?;
    for (slot, effect) in descriptors.effects.iter().enumerate() {
        if effect.id >= 0 {
            audio.se_play(
                slot as i32,
                effect.id,
                0,
                effect.volume,
                effect.pan,
                effect.speed,
                0,
            )?;
        } else {
            audio.se_stop(slot as i32, 0)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
