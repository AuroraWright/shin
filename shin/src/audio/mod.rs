mod bgm_player;
mod se_player;
mod voice_player;

pub use bgm_player::BgmPlayer;
pub use se_player::{SE_SLOT_COUNT, SePlayer};
pub use voice_player::{VoicePlayFlags, VoicePlayer};

mod settings_gain;
pub use settings_gain::SettingsGain;

mod voice_focus;

mod movie_volume;
pub use movie_volume::MovieVolumeSettings;

mod system_sound_player;
pub use system_sound_player::SystemSoundPlayer;
#[cfg(test)]
pub(crate) use system_sound_player::{SystemSoundTail, system_cue_volume};

mod global_audio;
pub use global_audio::{GlobalAudio, GlobalAudioSlot};
