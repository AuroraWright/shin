mod bgm_player;
mod se_player;
mod voice_player;

pub use bgm_player::BgmPlayer;
pub use se_player::{SE_SLOT_COUNT, SePlayer};
pub(crate) use voice_player::native_voice_clock;
pub use voice_player::{VoicePlayFlags, VoicePlayer};
#[cfg(test)]
pub(crate) use voice_player::{
    character_voice_enabled, voice_pattern_matches, voice_speakers, voice_stop_volume,
};

mod settings_gain;
pub use settings_gain::SettingsGain;

mod voice_focus;
pub(crate) use voice_focus::VoiceFocus;

mod movie_volume;
pub use movie_volume::MovieVolumeSettings;

mod system_sound_player;
pub use system_sound_player::SystemSoundPlayer;
#[cfg(test)]
pub(crate) use system_sound_player::{SystemSoundTail, system_cue_volume};

mod global_audio;
pub use global_audio::{GlobalAudio, GlobalAudioSlot};
