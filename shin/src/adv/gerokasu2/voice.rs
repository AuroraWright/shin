//! Gerokasu2 mappings and microsecond clocks on the shared voice transport.
use super::message_clock::{MessageEffects, VoiceRequest, VoiceTimeline};
use crate::{
    asset::system::AssetServer,
    audio::{VoicePlayFlags, VoicePlayer},
};
use shin_audio::AudioManager;
use shin_core::{
    format::{
        scenario::{
            gerokasu2::Gerokasu2Scenario,
            info::{VoiceMappingInfo, VoiceMappingInfoItem},
        },
        text::U16String,
    },
    time::Ticks,
    vm::command::types::{AudioWaitStatus, Volume},
};
use std::{cell::RefCell, sync::Arc};

pub struct Gerokasu2VoicePlayer {
    player: RefCell<VoicePlayer>,
    mappings: VoiceMappingInfo,
}
pub(super) fn mappings(scenario: &Gerokasu2Scenario) -> VoiceMappingInfo {
    scenario
        .voice_mapping_info
        .iter()
        .map(|row| VoiceMappingInfoItem {
            name_pattern: U16String::new(row.name_pattern.as_str()),
            lipsync_character_ids: row.lipsync_character_ids.clone(),
        })
        .collect()
}

impl Gerokasu2VoicePlayer {
    pub fn new(
        manager: Arc<AudioManager>,
        assets: Arc<AssetServer>,
        scenario: &Gerokasu2Scenario,
    ) -> Self {
        let mut player = VoicePlayer::new(manager, assets);
        player.set_fused_stop_fade(true);
        Self {
            player: RefCell::new(player),
            mappings: mappings(scenario),
        }
    }
    /// Preferences come from the Gerokasu2 scene/settings owner.
    pub fn set_preferences(&mut self, volume: u8, character_mask: u32, character_pan: bool) {
        let player = self.player.get_mut();
        player.set_settings_volume(volume);
        player.set_character_voice_mask(character_mask);
        player.set_character_panning_enabled(character_pan);
    }
    pub fn set_paused(&mut self, paused: bool) {
        self.player.get_mut().set_paused(paused);
    }
    pub fn update(&mut self, delta: Ticks) {
        self.player.get_mut().update(delta);
    }
    pub fn wait_status(&self) -> AudioWaitStatus {
        self.player.borrow().get_wait_status()
    }
    pub fn timeline(&self) -> VoiceTimeline {
        VoiceTimeline {
            playing: self.wait_status().contains(AudioWaitStatus::PLAYING),
            position: self.player.borrow().position_micros(),
        }
    }
    pub fn stop(&mut self) {
        self.player.get_mut().stop();
    }
    pub fn character_lipsync(&self, character: i32, pan: f32) -> Option<u8> {
        let mut player = self.player.borrow_mut();
        player.pan_for_character(character, pan);
        player.lipsync_for_character(character)
    }
    pub fn play(&mut self, request: VoiceRequest) -> bool {
        // Original MessageLayer expands u32 milliseconds into u64 micros.
        // Retain that full domain when entering the shared millisecond transport.
        let Some((start, duration)) = message_segment(&request) else {
            tracing::warn!(
                "Gerokasu2 message voice segment is outside its native u32-millisecond domain"
            );
            return false;
        };
        let mut flags = VoicePlayFlags::ENABLE_CHARACTER_MUTING;
        flags.set(VoicePlayFlags::ENABLE_CHARACTER_LIPSYNC, request.lipsync);
        self.play_segment(&request.filename, request.volume, start, duration, flags)
    }
    pub(super) fn play_command(&mut self, name: &str, volume: f32, flags: i32) -> bool {
        self.play_segment(name, volume, 0, 0, VoicePlayFlags::from_bits_retain(flags))
    }
    fn play_segment(
        &mut self,
        name: &str,
        volume: f32,
        start: u32,
        duration: u32,
        flags: VoicePlayFlags,
    ) -> bool {
        self.player.get_mut().play_mapped(
            &self.mappings,
            name,
            start,
            duration,
            flags,
            Volume(if volume > 0.0 { volume.min(1.0) } else { 0.0 }),
        )
    }
}

fn message_segment(request: &VoiceRequest) -> Option<(u32, u32)> {
    if request.segment_start % 1000 != 0 || request.segment_duration % 1000 != 0 {
        return None;
    }
    Some((
        u32::try_from(request.segment_start / 1000).ok()?,
        u32::try_from(request.segment_duration / 1000).ok()?,
    ))
}

/// Read/backlog/save policy remains with the scene, independently of transport.
pub trait MessageNotifications {
    fn observed_voice(&mut self, filename: &str);
    fn completed(&mut self, message_id: u32);
}
pub struct Gerokasu2MessageEffects<'a, N> {
    pub voice: &'a mut Gerokasu2VoicePlayer,
    pub notifications: &'a mut N,
}
impl<N: MessageNotifications> MessageEffects for Gerokasu2MessageEffects<'_, N> {
    fn voice_is_playing(&self) -> bool {
        self.voice.timeline().playing
    }
    fn stop_voice(&mut self) {
        self.voice.stop();
    }
    fn play_voice(&mut self, request: VoiceRequest) -> bool {
        self.voice.play(request)
    }
    fn observe_voice(&mut self, filename: &str) {
        self.notifications.observed_voice(filename);
    }
    fn completed(&mut self, message_id: u32) {
        self.notifications.completed(message_id);
    }
}

#[cfg(test)]
mod tests;
