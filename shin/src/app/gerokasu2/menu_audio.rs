//! Ger menu modes retain the native global music owner and accepted-source
//! gallery notifications across asynchronous mode factories.
use crate::{asset::system::AssetServer, audio::GlobalAudio};
use shin_core::{
    format::scenario::{
        gerokasu2::Gerokasu2Scenario, info::BgmId,
    },
    time::{Ticks, Tween},
    vm::command::types::{AudioWaitStatus, Volume},
};
use std::sync::Arc;

pub(crate) struct MenuMusic {
    audio: Arc<parking_lot::Mutex<GlobalAudio>>,
    links: Vec<i16>,
    unlocks: Vec<u32>,
    focus_target: f32,
}
impl MenuMusic {
    pub fn new(audio: Arc<shin_audio::AudioManager>, scenario: &Gerokasu2Scenario) -> Self {
        Self {
            audio: Arc::new(parking_lot::Mutex::new(GlobalAudio::new(audio))),
            links: scenario.bgm_info.iter().map(|b| b.linked_bgm_id).collect(),
            unlocks: vec![],
            focus_target: 1.,
        }
    }
    /// ADV borrows the same app-global tracks only when its prepared scene is
    /// published. Background restoration must not mutate the retained menu.
    pub fn shared_audio(&self) -> Arc<parking_lot::Mutex<GlobalAudio>> {
        self.audio.clone()
    }
    pub fn set_voice_focus_target(&mut self, enabled: bool, active: bool) {
        self.focus_target = if enabled && active { 0.5 } else { 1. };
    }
    pub fn restore_voice_focus_target(&mut self) {
        self.focus_target = 1.;
    }
    pub fn update(&mut self, delta: Ticks) {
        let mut audio = self.audio.lock();
        audio
            .bgm_player
            .update_gerokasu2_focus(delta, self.focus_target);
        audio.bgm_player.update();
        audio.se_player.update();
        audio.system_player.update();
    }
    pub fn set_settings_volume(&mut self, percent: u8) {
        self.audio.lock().bgm_player.set_settings_volume(percent);
    }
    pub fn stop(&mut self, fade: Tween) {
        self.audio.lock().bgm_player.stop(fade);
    }
    pub fn get_wait_status(&self) -> AudioWaitStatus {
        self.audio.lock().bgm_player.get_wait_status()
    }
    pub fn request(
        &mut self,
        assets: Arc<AssetServer>,
        path: String,
        id: BgmId,
        repeat: bool,
        volume: Volume,
        fade: Tween,
    ) -> bool {
        self.request_with_loop_count(assets, path, id, if repeat { 0 } else { 1 }, volume, fade)
    }
    pub fn request_with_loop_count(
        &mut self,
        assets: Arc<AssetServer>,
        path: String,
        id: BgmId,
        count: u32,
        volume: Volume,
        fade: Tween,
    ) -> bool {
        let accepted = self
            .audio
            .lock()
            .bgm_player
            .request_with_loop_count(assets, path, id, count, volume, fade);
        // Original38368..3838C commits these before IO readiness, only when a
        // new source was accepted. Same-ID retargets have no notification.
        self.unlocks
            .extend(accepted_unlocks(accepted, id.into_i32(), &self.links));
        accepted
    }
    pub fn take_unlocks(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.unlocks)
    }
}
fn accepted_unlocks(accepted: bool, id: i32, links: &[i16]) -> Vec<u32> {
    if !accepted || id < 0 {
        return vec![];
    }
    let mut out = vec![id as u32];
    if let Some(&link) = links.get(id as usize) {
        if link >= 0 {
            out.push(link as u32);
        }
    }
    out
}
pub(super) fn merge_unlocks(words: &mut Vec<u32>, ids: impl IntoIterator<Item = u32>) -> bool {
    let mut changed = false;
    for id in ids {
        let word = id as usize / 32;
        if word >= words.len() {
            words.resize((word + 1).next_multiple_of(256), 0);
        }
        let bit = 1 << (id % 32);
        changed |= words[word] & bit == 0;
        words[word] |= bit;
    }
    changed
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepted_sources_preserve_direct_link_and_same_id_retarget_rules() {
        assert_eq!(accepted_unlocks(true, 0, &[2, -1, 2]), vec![0, 2]);
        assert_eq!(accepted_unlocks(true, 2, &[2, -1, 2]), vec![2, 2]);
        assert_eq!(accepted_unlocks(true, 1, &[2, -1, 2]), vec![1]);
        assert!(accepted_unlocks(false, 0, &[2]).is_empty());
        assert!(accepted_unlocks(true, -1, &[2]).is_empty());
    }
    #[test]
    fn music_unlock_merge_preserves_physical_tail_and_coalesces_new_bits() {
        let mut bank = vec![0, 0, 0x80000000];
        assert!(merge_unlocks(&mut bank, [0, 0, 31, 32]));
        assert_eq!(bank, vec![0x80000001, 1, 0x80000000]);
        assert!(!merge_unlocks(&mut bank, [0, 31, 32]));
        assert!(merge_unlocks(&mut bank, [96]));
        assert_eq!(bank.len(), 256);
        assert_eq!(&bank[..4], &[0x80000001, 1, 0x80000000, 1]);
    }
}
