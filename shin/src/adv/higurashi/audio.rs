//! Scenario-specific audio facade over the shared codec, mixer and transports.
use super::{
    effect_commands::{EffectCommands, EffectDescriptors, EffectPlay},
    effects::HigurashiEffects,
    message_playback::MessageVoice,
    music_commands::MusicCommands,
    voice::HigurashiVoicePlayer,
    voice_commands::VoiceCommands,
};
use crate::{
    asset::system::AssetServer,
    audio::{BgmPlayer, native_voice_clock},
};
use anyhow::Result;
use shin_audio::AudioManager;
use shin_core::{
    format::scenario::{
        higurashi::HigurashiScenario, info::BgmId, instruction_elements::FromNumber,
    },
    time::{Ticks, Tween},
    vm::command::types::Volume,
};
use std::{collections::VecDeque, sync::Arc};

/// Successful changed-ID requests expose their metadata to the host. Native
/// title/read/gallery policy and persistent writes remain host responsibilities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BgmStarted {
    pub id: i32,
    pub display_name: String,
    pub linked_id: i16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MusicEvent {
    Started(BgmStarted),
    Stopped,
}

/// Resolved host policy. ADV pause/visual skip do not imply an audio pause.
/// Native d11b8 gates target selection; EC enables voice focus; SE byte0
/// separately gates its polling/focus clock. Music rate resets after its tick.
#[derive(Debug, Clone, Copy)]
pub struct AudioFramePolicy {
    pub focus_mode_active: bool,
    pub voice_focus_enabled: bool,
    pub effects_update_enabled: bool,
    pub music_rate: f32,
}
pub(super) trait AudioFrameServices {
    fn voice_active(&self) -> bool;
    fn set_focus_target(&mut self, active: bool);
    fn advance_voice(&mut self, delta: Ticks);
    fn advance_effects(&mut self, delta: Ticks, enabled: bool);
    fn advance_music(&mut self, delta: Ticks);
}
/// Native12e68 resolves targets before12f00 voice/12f0c SE polling. Global
/// music32e38 runs afterwards. Its rate affects BGM focus, not ADV/voice/SE.
pub(super) fn advance_audio(
    audio: &mut impl AudioFrameServices,
    delta: Ticks,
    policy: AudioFramePolicy,
) {
    if policy.focus_mode_active {
        audio.set_focus_target(policy.voice_focus_enabled && audio.voice_active());
    }
    audio.advance_voice(delta);
    audio.advance_effects(delta, policy.effects_update_enabled);
    audio.advance_music(Ticks::from_f32(delta.as_f32() * policy.music_rate));
}

pub struct HigurashiAudio {
    adv_preferences: (u8, u32, bool, u8),
    manager: Arc<AudioManager>,
    browser_music_target: Option<bool>,
    voice: HigurashiVoicePlayer,
    bgm: BgmPlayer,
    effects: HigurashiEffects,
    effect_descriptors: EffectDescriptors,
    assets: Arc<AssetServer>,
    scenario: Arc<HigurashiScenario>,
    bgm_started: VecDeque<MusicEvent>,
    focus_active: bool,
    modal_effects_paused: bool,
}
impl HigurashiAudio {
    pub fn fast_forward_voice_and_effects(&mut self) -> Result<()> {
        self.voice.stop();
        self.effects.fast_forward()
    }
    pub fn new(
        manager: Arc<AudioManager>,
        assets: Arc<AssetServer>,
        scenario: Arc<HigurashiScenario>,
    ) -> Self {
        Self {
            adv_preferences: (100, u32::MAX, true, 100),
            manager: manager.clone(),
            browser_music_target: None,
            voice: HigurashiVoicePlayer::new(manager.clone(), assets.clone(), &scenario),
            bgm: BgmPlayer::new(manager.clone()),
            effects: HigurashiEffects::new(manager),
            effect_descriptors: EffectDescriptors::default(),
            assets,
            scenario,
            bgm_started: VecDeque::new(),
            focus_active: false,
            modal_effects_paused: false,
        }
    }
    pub fn set_preferences(&mut self, voice_volume: u8, mask: u32, pan: bool, bgm_volume: u8) {
        self.adv_preferences.0 = voice_volume;
        self.adv_preferences.1 = mask;
        self.adv_preferences.2 = pan;
        self.voice.set_preferences(voice_volume, mask, pan);
        self.bgm.set_settings_volume(bgm_volume);
    }
    pub fn set_effects_volume(&mut self, percent: u8) {
        self.adv_preferences.3 = percent;
        self.effects.set_volume(percent);
    }
    pub(super) fn browser_voice_player(&self) -> crate::audio::VoicePlayer {
        crate::audio::VoicePlayer::new(self.manager.clone(), self.assets.clone())
    }
    /// Global BGM and its outstanding metadata events survive ADV replacement.
    /// The empty facade is used only while the source host is being destroyed.
    pub(crate) fn take_for_adv_replacement(&mut self) -> Self {
        let empty = Self::new(self.manager.clone(), self.assets.clone(), self.scenario.clone());
        std::mem::replace(self, empty)
    }
    pub(crate) fn reset_adv_for_replacement(&mut self) {
        self.voice = HigurashiVoicePlayer::new(self.manager.clone(), self.assets.clone(), &self.scenario);
        self.effects = HigurashiEffects::new(self.manager.clone());
        let (volume, mask, pan, effects) = self.adv_preferences;
        self.voice.set_preferences(volume, mask, pan);
        self.effects.set_volume(effects);
        self.effect_descriptors = Default::default();
        self.focus_active = false;
        self.modal_effects_paused = false;
    }
    /// Native12960 retires ADV-local transports without resetting the global
    /// music target before9bb48 installs its independent quiz voice scope.
    pub(crate) fn retire_adv_for_scene(&mut self) {
        let focus = self.focus_active;
        let browser_target = self.browser_music_target;
        self.reset_adv_for_replacement();
        self.focus_active = focus;
        self.browser_music_target = browser_target;
    }
    /// Original32700 writes the global BGM target independently of ADV voice.
    /// Preserve SE's story target and let the next global music tick consume it.
    pub(crate) fn browser_music_multiplier(&mut self, value: f32) {
        self.browser_music_target = Some(value == 0.5);
    }
    /// Pause is an explicit host audio policy, independent of visual gating.
    pub fn set_paused(&mut self, paused: bool) {
        self.voice.set_paused(paused);
        self.bgm.set_paused(paused);
        self.effects.set_paused(paused);
    }
    /// ADV15038 pauses its voice scope; global BGM is owned separately.
    pub(super) fn set_modal_voice_paused(&mut self, paused: bool) {
        self.voice.set_paused(paused);
    }
    /// ADV+a8 owns SE transports and gates the SE polling/focus clock.
    pub(super) fn set_modal_effects_paused(&mut self, paused: bool) {
        self.modal_effects_paused = paused;
        self.effects.set_paused(paused);
    }
    pub(crate) fn modal_effects_paused(&self) -> bool {
        self.modal_effects_paused
    }
    /// Explicit-mode helper for isolated playback checks. The platform host
    /// supplies the complete mode/pause/rate policy through update_frame.
    pub fn update(&mut self, delta: Ticks, voice_focus: bool) {
        self.update_frame(
            delta,
            AudioFramePolicy {
                focus_mode_active: true,
                voice_focus_enabled: voice_focus,
                effects_update_enabled: true,
                music_rate: 1.0,
            },
        );
    }
    pub fn update_frame(&mut self, delta: Ticks, policy: AudioFramePolicy) {
        advance_audio(self, delta, policy);
    }

    /// Native27200 ticks global music after scenes with their own voice scope.
    /// Preserve the existing focus target rather than deriving it from that voice.
    pub(crate) fn update_global_music(&mut self, delta: Ticks) {
        self.advance_music(delta);
    }

    /// Native ADV UI clocks follow voice/SE; global BGM advances afterward.
    pub(super) fn update_frame_with_ui(&mut self, delta: Ticks, policy: AudioFramePolicy, ui: impl FnOnce()) {
        if policy.focus_mode_active {
            self.set_focus_target(policy.voice_focus_enabled && self.voice_active());
        }
        self.advance_voice(delta);
        self.advance_effects(delta, policy.effects_update_enabled);
        ui();
        self.advance_music(Ticks::from_f32(delta.as_f32() * policy.music_rate));
    }
    #[cfg(test)]
    pub(crate) fn reference_focus(&self) -> (bool, f32) {
        (self.focus_active, self.effects.focus_gain())
    }
    #[cfg(test)]
    pub(crate) fn reference_music_focus(&self) -> f32 { self.bgm.reference_focus_gain() }
    pub fn pop_music_event(&mut self) -> Option<MusicEvent> {
        self.bgm_started.pop_front()
    }
}
impl super::live_reconstruction::ReconstructionAudio for HigurashiAudio {
    fn prepare_effect_reconstruction(&mut self) -> Result<()> {
        self.effects.prepare_reconstruction()
    }
    fn install_effect_descriptors(&mut self, effects: &[super::descriptors::EffectDescriptor; 32]) {
        self.effect_descriptors = EffectDescriptors::from_checkpoint(effects);
    }
}
impl AudioFrameServices for HigurashiAudio {
    fn voice_active(&self) -> bool {
        self.voice.is_playing()
    }
    fn set_focus_target(&mut self, active: bool) {
        self.focus_active = active;
        self.browser_music_target = None;
    }
    fn advance_voice(&mut self, delta: Ticks) {
        self.voice.update(delta);
    }
    fn advance_effects(&mut self, delta: Ticks, enabled: bool) {
        if enabled {
            self.effects.update_voice_focus(delta, self.focus_active);
            self.effects.update();
        }
    }
    fn advance_music(&mut self, delta: Ticks) {
        let target = self.browser_music_target.unwrap_or(self.focus_active);
        self.bgm.update_voice_focus(delta, true, target);
        self.bgm.update();
    }
}
fn native_volume(volume: f32) -> Volume {
    Volume(if volume > 0.0 { volume.min(1.0) } else { 0.0 })
}
impl MusicCommands for HigurashiAudio {
    fn play_bgm(&mut self, id: i32, fade: i32, loop_count: u32, volume: f32) -> Result<()> {
        let Some(info) = usize::try_from(id)
            .ok()
            .and_then(|id| self.scenario.bgm_info.get(id))
        else {
            // Native3271c retires the previous entry beforebc434 rejects an ID.
            self.bgm.stop(Tween::linear(Ticks::from_f32(fade as f32)));
            return Ok(());
        };
        let path = format!("/bgm/{}.nxa", info.name.as_str().to_ascii_lowercase());
        if self.bgm.request_with_loop_count(
            self.assets.clone(),
            path,
            BgmId::from_number(id),
            loop_count,
            native_volume(volume),
            Tween::linear(Ticks::from_f32(fade as f32)),
        ) {
            self.bgm_started.push_back(MusicEvent::Started(BgmStarted {
                id,
                display_name: info.display_name.as_str().into(),
                linked_id: info.linked_bgm_id,
            }));
        }
        Ok(())
    }
    fn stop_bgm(&mut self, fade: i32) {
        self.bgm.stop(Tween::linear(Ticks::from_f32(fade as f32)));
        self.bgm_started.push_back(MusicEvent::Stopped);
    }
    fn volume_bgm(&mut self, volume: f32, fade: i32) -> Result<()> {
        self.bgm.enqueue_volume(
            native_volume(volume),
            Tween::linear(Ticks::from_f32(fade as f32)),
        )
    }
    fn bgm_status(&self) -> i32 {
        self.bgm.get_wait_status().bits()
    }
    fn bgm_clock(&self) -> u64 {
        self.bgm
            .sample_clock()
            .map_or(0, |(samples, rate)| native_voice_clock(samples, rate))
    }
}
impl MessageVoice for HigurashiAudio {
    fn is_playing(&self) -> bool {
        self.voice.is_playing()
    }
    fn position_micros(&self) -> u64 {
        self.voice.position_micros()
    }
    fn play(&mut self, name: &str, volume: f32, lipsync: bool, start: u32, duration: u32) -> bool {
        self.voice.play(name, volume, lipsync, start, duration)
    }
    fn stop(&mut self) {
        self.voice.stop();
    }
    fn character_lipsync(&self, character: i32, pan: f32) -> Option<u8> {
        self.voice.character_lipsync(character, pan)
    }
}
impl EffectCommands for HigurashiAudio {
    fn prepare_effect_command(
        &mut self,
        command: &shin_core::vm::higurashi::command::HigurashiRuntimeCommand,
    ) {
        self.effect_descriptors.observe(command);
    }
    fn effect_descriptor_id(&self, slot: i32) -> i32 {
        self.effect_descriptors.id(slot)
    }
    fn play_effect(&mut self, request: EffectPlay) -> Result<()> {
        let path = usize::try_from(request.id)
            .ok()
            .and_then(|id| self.scenario.se_info.get(id))
            .map(|info| format!("/se/{}.nxa", info.name.as_str().to_ascii_lowercase()));
        self.effects.request(self.assets.clone(), path, request)
    }
    fn stop_effect(&mut self, slot: i32, fade: i32) -> Result<()> {
        self.effects.stop(Some(slot), fade)
    }
    fn stop_all_effects(&mut self, fade: i32) -> Result<()> {
        self.effects.stop(None, fade)
    }
    fn volume_effect(&mut self, slot: i32, volume: f32, fade: i32) -> Result<()> {
        self.effects.queue_volume(slot, volume, fade)
    }
    fn pan_effect(&mut self, slot: i32, pan: f32, fade: i32) -> Result<()> {
        self.effects.queue_pan(slot, pan, fade)
    }
    fn effect_status(&self, slot: i32) -> i32 {
        self.effects.status(slot)
    }
    fn active_effect_slots(&self) -> Vec<i32> {
        self.effects.active_slots()
    }
}
impl VoiceCommands for HigurashiAudio {
    fn play_command(&mut self, name: &str, volume: f32, flags: i32) {
        self.voice.play_command(name, volume, flags);
    }
    fn status_bits(&self) -> i32 {
        self.voice.status_bits()
    }
    fn music(&mut self) -> Option<&mut dyn MusicCommands> {
        Some(self)
    }
    fn effects(&mut self) -> Option<&mut dyn EffectCommands> {
        Some(self)
    }
}

#[cfg(test)]
mod tests;
