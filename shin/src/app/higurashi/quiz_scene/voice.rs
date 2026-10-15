//! Quiz owns its own eb5f4 voice player. Original9e41c selects the S99 namespace.
use crate::{
    adv::higurashi::{
        message_playback::MessageVoice, voice::HigurashiVoicePlayer, voice_commands::VoiceCommands,
    },
    asset::system::AssetServer,
};
use shin_audio::AudioManager;
use shin_core::{format::scenario::higurashi::HigurashiScenario, time::Ticks};
use std::sync::Arc;

fn request(name: &str, reaction: bool) -> (String, i32) {
    // Native9e500 masks the reaction argument to bit0. Unlike ADV message
    // playback, this request never enables character muting (bit1).
    (format!("S99/{name}"), i32::from(reaction))
}

pub(super) struct Voice {
    player: HigurashiVoicePlayer,
}
impl Voice {
    pub fn new(
        manager: Arc<AudioManager>,
        assets: Arc<AssetServer>,
        scenario: &HigurashiScenario,
    ) -> Self {
        Self {
            player: HigurashiVoicePlayer::new(manager, assets, scenario),
        }
    }
    /// Originalec440 reads the global voice gain every frame. The owning
    /// scene supplies its current saved v2_voicevol, including changed settings.
    pub fn set_volume(&mut self, percent: u8) {
        self.player.set_preferences(percent, u32::MAX, false);
    }
    pub fn play(&mut self, name: &str, reaction: bool) {
        let (name, flags) = request(name, reaction);
        self.player.play_command(&name, 1.0, flags);
    }
    pub fn advance(&mut self, delta: Ticks) {
        self.player.update(delta);
    }
    pub fn status(&self) -> u32 {
        self.player.status_bits() as u32
    }
    pub fn character_lipsync(&self, character: i32, _pan: f32) -> Option<u8> {
        // Original9e928 and scene+10 thunk9e95c always pass zero to eb738.
        self.player.character_lipsync(character, 0.0)
    }
}

#[cfg(test)]
mod tests;
