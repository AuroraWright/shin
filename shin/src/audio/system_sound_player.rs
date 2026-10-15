//! Native global system sound player, reusable across ADV and title scenes.
use crate::asset::system::AssetServer;
use shin_audio::{AudioHandle, AudioManager};
use shin_core::vm::command::types::Volume;
use std::sync::Arc;

pub struct SystemSoundPlayer {
    audio_manager: Arc<AudioManager>,
    pub(crate) sounds: Vec<AudioHandle>,
    pub(crate) bank: Option<Arc<shin_core::format::sysse::SysSe>>,
    pub(crate) track: shin_audio::AudioTrack,
    pub(crate) volume_percent: u8,
    pub(crate) retired: Vec<SystemSoundTail>,
}
/// The global native system player outlives ADV scene reconstruction. Keep
/// each retired bus owned until its existing cues finish; no new bus is needed.
pub(crate) struct SystemSoundTail {
    pub(crate) _track: shin_audio::AudioTrack,
    pub(crate) sounds: Vec<AudioHandle>,
}
impl Drop for SystemSoundTail {
    fn drop(&mut self) {
        for sound in &mut self.sounds {
            let _ = sound.stop(shin_core::time::Tween::IMMEDIATE);
        }
    }
}

pub(crate) fn system_cue_volume(
    volume: shin_core::vm::command::types::Volume,
    percent: u8,
) -> shin_core::vm::command::types::Volume {
    // Native0xfeef4..0xfef50 clamps the script gain then captures saved byteB3.
    shin_core::vm::command::types::Volume(volume.0.max(0.0).min(1.0) * (f32::from(percent) * 0.01))
}

impl SystemSoundPlayer {
    pub fn new(audio_manager: Arc<AudioManager>) -> Self {
        let track = audio_manager
            .kira_manager()
            .lock()
            .add_sub_track(kira::track::TrackBuilder::new())
            .expect("Failed to create system sound track")
            .into();
        Self {
            audio_manager,
            sounds: Vec::new(),
            bank: None,
            track,
            volume_percent: 100,
            retired: Vec::new(),
        }
    }
    pub fn set_settings_volume(&mut self, percent: u8) {
        self.volume_percent = percent;
    }
    pub fn update(&mut self) {
        self.sounds
            .retain(|sound| !sound.get_wait_status().is_empty());
        for tail in &mut self.retired {
            tail.sounds
                .retain(|sound| !sound.get_wait_status().is_empty());
        }
        self.retired.retain(|tail| !tail.sounds.is_empty());
    }

    pub fn play(&mut self, server: &AssetServer, name: &str, volume: Volume) {
        // A failed menu cue must not abort its input callback. Keep the bank
        // unpublished on failure so the next cue can retry loading it.
        if self.bank.is_none() {
            match server.load_sync::<shin_core::format::sysse::SysSe>("/sysse.bin") {
                Ok(bank) => self.bank = Some(bank),
                Err(error) => {
                    tracing::warn!(error = ?error, cue = name, "System sound bank could not load");
                    return;
                }
            }
        }
        let Some(sound) = self.bank.as_ref().and_then(|bank| bank.sounds.get(name)) else {
            tracing::warn!(cue = name, "System sound cue is missing");
            return;
        };
        let handle = self.audio_manager.play_on_track(
            &mut self.track,
            shin_audio::AudioData {
            source: sound.decode(),
            settings: shin_audio::AudioSettings {
                paused: false,
                playback_rate: 1.0,

                    fade_in: shin_core::time::Tween::IMMEDIATE,
                loop_start: None,
                loop_count: 0,
                volume: system_cue_volume(volume, self.volume_percent),
                pan: Default::default(),
            },
        },
        );
        self.sounds.push(handle);
    }
    /// Resume keeps its active bus and all in-flight cue identities.
    pub fn preserve_active_from(&mut self, previous: &mut Self) {
        self.sounds = std::mem::take(&mut previous.sounds);
        std::mem::swap(&mut self.track, &mut previous.track);
    }
    /// History/reconstruction retires the old bus until its captured cues finish.
    pub fn absorb_retired(&mut self, mut previous: Self) {
        self.retired.append(&mut previous.retired);
        if !previous.sounds.is_empty() {
            self.retired.push(SystemSoundTail {
                _track: previous.track,
                sounds: previous.sounds,
            });
        }
        if self.bank.is_none() {
            self.bank = previous.bank.take();
        }
    }
}
