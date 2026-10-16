//! Gerokasu's `/voice/sys/` clips outside ADV: the splash logo calls
//! (FirstBoot 5f6c8..5f784) and the Title greeting (dbf34..dc038). Both use
//! native voice transport e3180 at volume 1 without character muting or
//! lipsync; the clip name is `sys/{character}{suffix}`.
use std::sync::Arc;

use shin_core::{time::Ticks, vm::command::types::Volume};

use crate::{
    asset::system::AssetServer,
    audio::{VoicePlayFlags, VoicePlayer},
};

/// Native 0x17e288 (splash) and the Title table 0x1c6ec0 order.
pub(super) const CHARACTERS: [&str; 4] = ["fuuka", "karin", "mina", "doremi"];

pub(super) struct SystemVoice {
    player: VoicePlayer,
}
impl SystemVoice {
    pub fn new(audio: Arc<shin_audio::AudioManager>, assets: Arc<AssetServer>, volume: u8) -> Self {
        let mut player = VoicePlayer::new(audio, assets);
        player.set_settings_volume(volume);
        Self { player }
    }
    pub fn play(&mut self, name: &str) {
        self.player
            .play_mapped(&Vec::new(), name, 0, 0, VoicePlayFlags::empty(), Volume(1.0));
    }
    pub fn update(&mut self, delta: Ticks) {
        self.player.update(delta);
    }
}

/// Title dbf34: `rand() & 3` picks the character; with the Extra flag set, a
/// second `rand() & 0xf == 0` selects the `_title2` variant.
pub(super) fn title_clip(extra_unlocked: bool, mut rand: impl FnMut() -> u32) -> String {
    let character = CHARACTERS[(rand() & 3) as usize];
    let variant = if extra_unlocked && rand() & 0xf == 0 {
        "_title2"
    } else {
        "_title"
    };
    format!("sys/{character}{variant}")
}

#[cfg(test)]
mod tests {
    #[test]
    fn title_clip_follows_native_selection() {
        let mut values = [2u32, 0x10].into_iter();
        assert_eq!(super::title_clip(true, || values.next().unwrap()), "sys/mina_title2");
        let mut values = [7u32, 1].into_iter();
        assert_eq!(super::title_clip(true, || values.next().unwrap()), "sys/doremi_title");
        let mut values = [1u32].into_iter();
        assert_eq!(super::title_clip(false, || values.next().unwrap()), "sys/karin_title");
    }
}
