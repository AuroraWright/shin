//! Native app-global players retained across scene destruction.
use super::{BgmPlayer, SePlayer, SystemSoundPlayer};
use shin_audio::AudioManager;
use std::{
    ops::{Deref, DerefMut},
    sync::Arc,
};

pub struct GlobalAudio {
    pub manager: Arc<AudioManager>,
    pub bgm_player: BgmPlayer,
    pub se_player: SePlayer,
    pub system_player: SystemSoundPlayer,
}
impl GlobalAudio {
    pub fn new(manager: Arc<AudioManager>) -> Self {
        Self {
            bgm_player: BgmPlayer::new(manager.clone()),
            se_player: SePlayer::new(manager.clone()),
            system_player: SystemSoundPlayer::new(manager.clone()),
            manager,
        }
    }
}
/// A scene can detach its players without allocating placeholder audio tracks.
/// An emptied slot belongs to a retired scene and must not be updated again.
pub struct GlobalAudioSlot(Option<GlobalAudio>);
impl GlobalAudioSlot {
    pub fn take(&mut self) -> Option<GlobalAudio> {
        self.0.take()
    }
}
impl From<GlobalAudio> for GlobalAudioSlot {
    fn from(audio: GlobalAudio) -> Self {
        Self(Some(audio))
    }
}
impl Deref for GlobalAudioSlot {
    type Target = GlobalAudio;
    fn deref(&self) -> &GlobalAudio {
        self.0.as_ref().expect("Retired scene audio accessed")
    }
}
impl DerefMut for GlobalAudioSlot {
    fn deref_mut(&mut self) -> &mut GlobalAudio {
        self.0.as_mut().expect("Retired scene audio accessed")
    }
}
