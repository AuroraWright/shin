//! Older scenario speaker mappings connected to shared asynchronous NXA audio.
use std::{cell::RefCell, sync::Arc};

use shin_audio::AudioManager;
use shin_core::{
    format::scenario::{
        higurashi::HigurashiScenario,
        info::{VoiceMappingInfo, VoiceMappingInfoItem},
    },
    format::text::U16String,
    time::Ticks,
    vm::command::types::{AudioWaitStatus, Volume},
};

use super::message_playback::MessageVoice;
use crate::{
    asset::system::AssetServer,
    audio::{VoicePlayFlags, VoicePlayer},
};

pub struct HigurashiVoicePlayer {
    // Character callbacks run after message callbacks, and mutate panning
    // through a shared feedback reference. No borrow spans an async operation.
    player: RefCell<VoicePlayer>,
    mappings: VoiceMappingInfo,
}

pub(super) fn mappings(scenario: &HigurashiScenario) -> VoiceMappingInfo {
    scenario
        .voice_mapping_info
        .iter()
        .map(|mapping| VoiceMappingInfoItem {
            name_pattern: U16String::new(mapping.name_pattern.as_str()),
            lipsync_character_ids: mapping.lipsync_character_ids.clone(),
        })
        .collect()
}

impl HigurashiVoicePlayer {
    pub fn new(
        manager: Arc<AudioManager>,
        assets: Arc<AssetServer>,
        scenario: &HigurashiScenario,
    ) -> Self {
        Self {
            player: RefCell::new(VoicePlayer::new(manager, assets)),
            mappings: mappings(scenario),
        }
    }
    /// Preferences are explicit host inputs, not Umineko save/settings defaults.
    pub fn set_preferences(&mut self, volume: u8, character_mask: u32, character_pan: bool) {
        let player = self.player.get_mut();
        player.set_settings_volume(volume);
        player.set_character_voice_mask(character_mask);
        player.set_character_panning_enabled(character_pan);
    }
    pub fn set_paused(&mut self, paused: bool) {
        self.player.get_mut().set_paused(paused);
    }
    /// Poll asynchronous loads and retire completed/stopping voices each host
    /// frame. The host separately controls modal/native audio pause policy.
    pub fn update(&mut self, delta: Ticks) {
        self.player.get_mut().update(delta);
    }
    pub fn wait_status(&self) -> AudioWaitStatus {
        self.player.borrow().get_wait_status()
    }
}

impl MessageVoice for HigurashiVoicePlayer {
    fn is_playing(&self) -> bool {
        self.wait_status().contains(AudioWaitStatus::PLAYING)
    }
    fn position_micros(&self) -> u64 {
        self.player.borrow().position_micros()
    }
    fn play(
        &mut self,
        filename: &str,
        volume: f32,
        lipsync: bool,
        start: u32,
        duration: u32,
    ) -> bool {
        let mut flags = VoicePlayFlags::ENABLE_CHARACTER_MUTING;
        flags.set(VoicePlayFlags::ENABLE_CHARACTER_LIPSYNC, lipsync);
        self.player.get_mut().play_mapped(
            &self.mappings,
            filename,
            start,
            duration,
            flags,
            Volume(if volume > 0.0 { volume.min(1.0) } else { 0.0 }),
        )
    }
    fn stop(&mut self) {
        self.player.get_mut().stop();
    }
    fn character_lipsync(&self, character: i32, pan: f32) -> Option<u8> {
        let mut player = self.player.borrow_mut();
        player.pan_for_character(character, pan);
        player.lipsync_for_character(character)
    }
}

impl super::voice_commands::VoiceCommands for HigurashiVoicePlayer {
    fn play_command(&mut self, filename: &str, volume: f32, flags: i32) {
        // Nativeeb858 supplies zero segment start/duration toeb944. Failure
        // to find/load a voice does not keep VOICEPLAY pending.
        self.player.get_mut().play_mapped(
            &self.mappings,
            filename,
            0,
            0,
            VoicePlayFlags::from_bits_retain(flags),
            Volume(if volume > 0.0 { volume.min(1.0) } else { 0.0 }),
        );
    }
    fn status_bits(&self) -> i32 {
        self.wait_status().bits()
    }
}

#[cfg(test)]
mod tests;
