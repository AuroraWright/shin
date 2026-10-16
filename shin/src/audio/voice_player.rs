use super::SettingsGain;
use std::sync::Arc;

use bitflags::bitflags;
use kira::track::TrackBuilder;
use shin_audio::AudioTrack as TrackHandle;
use shin_audio::{AudioData, AudioFile, AudioHandle, AudioManager, AudioSegment, AudioSettings};
use shin_core::{
    format::audio::{AudioDecoder, AudioFrameSource, AudioSource},
    format::scenario::Scenario,
    time::{Ticks, Tween},
    vm::command::types::{AudioWaitStatus, Pan, Volume},
};
use shin_tasks::AsyncTask;
use tracing::warn;

use crate::asset::system::AssetServer;

bitflags! {
    #[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Clone, Copy)]
    pub struct VoicePlayFlags: i32 {
        const ENABLE_CHARACTER_LIPSYNC = 1;
        const ENABLE_CHARACTER_MUTING = 2;
    }
}

pub struct VoicePlayer {
    audio_manager: Arc<AudioManager>,
    settings_gain: SettingsGain,
    asset_server: Arc<AssetServer>,
    track: TrackHandle,
    pending: Option<PendingVoice>,
    voices: Vec<PlayingVoice>,
    segment_start_millis: u32,
    character_panning_enabled: bool,
    paused: bool,
    character_voice_mask: u32,
    fused_stop_fade: bool,
}

struct PlayingVoice {
    handle: AudioHandle,
    speakers: Vec<u8>,
    pan: Pan,
    lipsync: Vec<u8>,
    volume: f32,
    stopping: bool,
}

impl PlayingVoice {
    fn update_stop(&mut self, ticks: Ticks, fused: bool) -> bool {
        if !self.stopping {
            return false;
        }
        // Native0x11a300/0x11a31c subtract0.2 per tick from the
        // request volume,then multiply by the saved category gain.
        let previous = self.volume;
        self.volume = voice_stop_volume(self.volume, ticks.as_f32(), fused);
        if self.volume != previous {
            if let Err(error) = self
                .handle
                .set_volume(Volume(self.volume), Tween::IMMEDIATE)
            {
                warn!(%error, "Failed to fade voice");
            }
        }
        if if fused {
            !(self.volume > 0.0)
        } else {
            self.volume == 0.0
        } {
            if let Err(error) = self.handle.stop(Tween::IMMEDIATE) {
                warn!(%error,"Failed to stop faded voice");
            }
            true
        } else {
            false
        }
    }

    fn pan_for_character(&mut self, id: i32, pan: f32, enabled: bool) {
        if !speaker_matches(&self.speakers, id) {
            return;
        }
        let pan = Pan(if enabled { pan } else { 0.0 });
        if pan != self.pan {
            match self.handle.set_panning(pan, Tween::IMMEDIATE) {
                Ok(()) => self.pan = pan,
                Err(error) => warn!(%error, "Failed to pan voice"),
            }
        }
    }
}

impl Drop for VoicePlayer {
    fn drop(&mut self) {
        // A player can disappear during scene reconstruction before another
        // ADV tick advances its stop fade. Its handles must not outlive it.
        for voice in &mut self.voices {
            let _ = voice.handle.stop(Tween::IMMEDIATE);
        }
    }
}

fn character_lipsync(voices: &[PlayingVoice], id: i32) -> Option<u8> {
    voices
        .iter()
        .find(|voice| {
            !voice.stopping
                && speaker_matches(&voice.speakers, id)
                && voice
                    .handle
                    .get_wait_status()
                    .contains(AudioWaitStatus::PLAYING)
        })
        .map(|voice| {
            let index = voice.handle.position_millis() as usize / 50;
            voice
                .lipsync
                .get(index)
                .or_else(|| voice.lipsync.last())
                .copied()
                .unwrap_or(0)
        })
}

pub(crate) fn voice_speakers(
    mappings: &shin_core::format::scenario::info::VoiceMappingInfo,
    name: &str,
    flags: VoicePlayFlags,
) -> Vec<u8> {
    if !flags.contains(VoicePlayFlags::ENABLE_CHARACTER_LIPSYNC) {
        return Vec::new();
    }
    mappings
        .iter()
        .find(|mapping| voice_pattern_matches(mapping.name_pattern.as_str(), name))
        .map(|mapping| mapping.lipsync_character_ids.0.to_vec())
        .unwrap_or_default()
}

fn speaker_matches(speakers: &[u8], id: i32) -> bool {
    u8::try_from(id).is_ok_and(|id| speakers.contains(&id))
}

struct PendingVoice {
    task: AsyncTask<Vec<(Arc<AudioFile>, Vec<u8>, Vec<u8>)>>,
    segment_start: u32,
    segment_duration: u32,
    volume: Volume,
}

impl VoicePlayer {
    pub fn new(audio_manager: Arc<AudioManager>, asset_server: Arc<AssetServer>) -> Self {
        let track = audio_manager
            .kira_manager()
            .lock()
            .add_sub_track(TrackBuilder::new())
            .expect("Failed to create voice track")
            .into();
        Self {
            settings_gain: SettingsGain::default(),
            audio_manager,
            asset_server,
            track,
            pending: None,
            voices: Vec::new(),
            segment_start_millis: 0,
            character_voice_mask: u32::MAX,
            character_panning_enabled: true,
            paused: false,
            fused_stop_fade: false,
        }
    }

    pub fn set_paused(&mut self, paused: bool) {
        if self.paused == paused {
            return;
        }
        self.paused = paused;
        for voice in &self.voices {
            voice.handle.set_paused(paused);
        }
    }

    pub fn set_settings_volume(&mut self, percent: u8) {
        self.settings_gain.apply(&mut self.track, percent);
    }
    pub fn set_character_voice_mask(&mut self, mask: u32) {
        self.character_voice_mask = mask;
    }

    pub fn set_character_panning_enabled(&mut self, enabled: bool) {
        self.character_panning_enabled = enabled;
    }

    pub(crate) fn set_fused_stop_fade(&mut self, enabled: bool) {
        self.fused_stop_fade = enabled;
    }

    pub fn pan_for_character(&mut self, id: i32, pan: f32) {
        for voice in &mut self.voices {
            voice.pan_for_character(id, pan, self.character_panning_enabled);
        }
    }

    pub fn lipsync_for_character(&self, id: i32) -> Option<u8> {
        character_lipsync(&self.voices, id)
    }

    pub fn play(
        &mut self,
        scenario: &Scenario,
        voicefiles_spec: &str,
        segment_start: u32,
        segment_duration: u32,
        flags: VoicePlayFlags,
        volume: Volume,
    ) -> bool {
        self.play_mapped(
            &scenario.info_tables().voice_mapping_info,
            voicefiles_spec,
            segment_start,
            segment_duration,
            flags,
            volume,
        )
    }

    /// Shared transport; callers supply their game's decoded speaker mappings.
    pub(crate) fn play_mapped(
        &mut self,
        mappings: &shin_core::format::scenario::info::VoiceMappingInfo,
        voicefiles_spec: &str,
        segment_start: u32,
        segment_duration: u32,
        flags: VoicePlayFlags,
        volume: Volume,
    ) -> bool {
        self.stop();
        // VoicePlayer::play at 0x119830 splits on '|'. These files are played
        // together, not concatenated. MessageLayer's caller at 0xb1f50 passes
        // segment boundaries in milliseconds (converted to microseconds).
        let paths: Vec<_> = voice_paths(voicefiles_spec)
            .into_iter()
            .filter(|path| {
                !flags.contains(VoicePlayFlags::ENABLE_CHARACTER_MUTING)
                    || character_voice_enabled(
                        mappings,
                        &path[7..path.len() - 4],
                        self.character_voice_mask,
                    )
            })
            .map(|path| {
                let speakers = voice_speakers(mappings, &path[7..path.len() - 4], flags);
                (path, speakers)
            })
            .collect();
        if paths.is_empty() {
            return false;
        }
        let asset_server = self.asset_server.clone();
        let task = shin_tasks::async_io::spawn(async move {
            let mut voices = Vec::with_capacity(paths.len());
            for (path, speakers) in paths {
                match asset_server.load::<AudioFile, _>(&path).await {
                    Ok(audio) => {
                        let lipsync = if speakers.is_empty() {
                            Vec::new()
                        } else {
                            match voice_lipsync(audio.clone(), segment_start, segment_duration) {
                                Ok(levels) => levels,
                                Err(error) => {
                                    warn!(%path, %error, "Failed to decode voice lipsync");
                                    Vec::new()
                                }
                            }
                        };
                        voices.push((audio, speakers, lipsync));
                    }
                    Err(error) => warn!(%path, %error, "Failed to load voice"),
                }
            }
            voices
        });
        self.pending = Some(PendingVoice {
            task,
            segment_start,
            segment_duration,
            volume,
        });
        self.segment_start_millis = segment_start;
        true
    }

    pub fn update(&mut self, ticks: Ticks) {
        self.voices.retain_mut(|voice| {
            if voice.update_stop(ticks, self.fused_stop_fade) {
                return false;
            }
            voice
                .handle
                .get_wait_status()
                .contains(AudioWaitStatus::PLAYING)
        });
        let Some(pending) = self.pending.as_mut() else {
            return;
        };
        let Some(files) = pending.task.poll_naive() else {
            return;
        };
        let pending = self.pending.take().unwrap();
        for (file, speakers, lipsync) in files {
            let source = voice_segment(file, pending.segment_start, pending.segment_duration);
            match source {
                Ok(source) => self.voices.push(PlayingVoice {
                    speakers,
                    lipsync,
                    volume: pending.volume.0,
                    stopping: false,
                    pan: Pan::default(),
                    handle: self.audio_manager.play_on_track(
                        &mut self.track,
                        AudioData {
                            source,
                            settings: AudioSettings {
                                paused: self.paused,
                                playback_rate: 1.0,

                                fade_in: Tween::IMMEDIATE,
                                loop_start: None,
                                loop_count: 0,
                                volume: pending.volume,
                                pan: Pan::default(),
                            },
                        },
                    ),
                }),
                Err(error) => warn!(%error, "Failed to decode voice"),
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn reference_pause_state(&self) -> (bool, Vec<u32>) {
        (
            self.paused,
            self.voices
                .iter()
                .filter(|voice| !voice.stopping)
                .map(|voice| voice.handle.position_millis())
                .collect(),
        )
    }

    #[cfg(test)]
    pub(crate) fn reference_voice_metadata(&self) -> Vec<(Vec<u8>, Pan)> {
        self.voices
            .iter()
            .filter(|voice| !voice.stopping)
            .map(|voice| (voice.speakers.clone(), voice.pan))
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn reference_stopping_volumes(&self) -> Vec<f32> {
        self.voices
            .iter()
            .filter(|voice| voice.stopping)
            .map(|voice| voice.volume)
            .collect()
    }

    pub fn stop(&mut self) {
        self.pending = None;
        for voice in &mut self.voices {
            voice.stopping = true;
        }
    }

    pub fn get_wait_status(&self) -> AudioWaitStatus {
        // Native0x11a0c0 counts loading=3 and active playing=2;
        // stopping tails contribute no wait bits.
        if self.pending.is_some() {
            return AudioWaitStatus::LOADING | AudioWaitStatus::PLAYING;
        }
        self.voices
            .iter()
            .fold(AudioWaitStatus::empty(), |status, voice| {
                status
                    | if voice.stopping {
                        AudioWaitStatus::empty()
                    } else {
                        voice.handle.get_wait_status() & AudioWaitStatus::PLAYING
                    }
            })
    }

    /// Native11a180 returns the first active voice clock plus segment origin.
    /// Loading entries expose the origin before their playback clock exists;
    /// stopping tails contribute no clock.
    pub fn position_millis(&self) -> u64 {
        if self.pending.is_some() {
            return u64::from(self.segment_start_millis);
        }
        self.voices
            .iter()
            .find(|voice| {
                !voice.stopping
                    && voice
                        .handle
                        .get_wait_status()
                        .contains(AudioWaitStatus::PLAYING)
            })
            .map_or(0, |voice| {
                u64::from(self.segment_start_millis) + u64::from(voice.handle.position_millis())
            })
    }

    /// Gerokasu2 native2ad34/c6dc8/e59f0 (also used by older Higurashi).
    /// Keep the native sample fraction and double-precision rounding while
    /// retaining Umineko's existing millisecond API and behavior.
    pub(crate) fn position_micros(&self) -> u64 {
        let origin = u64::from(self.segment_start_millis) * 1000;
        if self.pending.is_some() {
            return origin;
        }
        self.voices
            .iter()
            .find(|voice| {
                !voice.stopping
                    && voice
                        .handle
                        .get_wait_status()
                        .contains(AudioWaitStatus::PLAYING)
            })
            .map_or(0, |voice| {
                origin.wrapping_add(native_voice_clock(
                    u64::from(voice.handle.position_samples()),
                    voice.handle.sample_rate(),
                ))
            })
    }
}

pub(crate) fn native_voice_clock(samples: u64, sample_rate: u32) -> u64 {
    (samples as f64 / (f64::from(sample_rate) * 0.000001)) as u64
}

pub(crate) fn voice_stop_volume(volume: f32, ticks: f32, fused: bool) -> f32 {
    if fused {
        let volume = ticks.mul_add(f32::from_bits(0xbe4ccccd), volume);
        if volume < 0.0 { 0.0 } else { volume }
    } else {
        (volume - ticks * f32::from_bits(0x3e4ccccd)).max(0.0)
    }
}

/// Native0xea400 uses the first case-insensitive wildcard match;0xea690
/// accepts an unknown speaker or any enabled character in a matched group.
pub(crate) fn character_voice_enabled(
    mappings: &shin_core::format::scenario::info::VoiceMappingInfo,
    name: &str,
    mask: u32,
) -> bool {
    mappings
        .iter()
        .find(|mapping| voice_pattern_matches(mapping.name_pattern.as_str(), name))
        .is_none_or(|mapping| {
            mapping
                .lipsync_character_ids
                .0
                .iter()
                .any(|id| mask & (1 << (*id & 31)) != 0)
        })
}

pub(crate) fn voice_pattern_matches(pattern: &str, name: &str) -> bool {
    let pattern: Vec<_> = pattern.chars().map(|c| c.to_ascii_uppercase()).collect();
    let mut previous = vec![false; pattern.len() + 1];
    previous[0] = true;
    for (i, c) in pattern.iter().enumerate() {
        previous[i + 1] = previous[i] && *c == '*';
    }
    for c in name.chars().map(|c| c.to_ascii_uppercase()) {
        let mut next = vec![false; pattern.len() + 1];
        for (i, p) in pattern.iter().enumerate() {
            next[i + 1] = if *p == '*' {
                next[i] || previous[i + 1]
            } else {
                previous[i] && (*p == '?' || *p == c)
            };
        }
        previous = next;
    }
    previous[pattern.len()]
}

fn voice_paths(spec: &str) -> Vec<String> {
    spec.split('|')
        .filter(|name| !name.is_empty())
        .map(|name| format!("/voice/{}.nxa", name.to_ascii_lowercase()))
        .collect()
}

// Native audio0xf4ea0 accumulates a PCM16 peak for each sample_rate/20
// window.0xf5088..0xf5108 implements hysteresis and the half-open timeout.
#[derive(Default)]
struct VoiceLipSyncDetector {
    level: u8,
    unchanged: u32,
}

impl VoiceLipSyncDetector {
    fn window(&mut self, peak: f32) -> u8 {
        let next = match self.level {
            0 if peak > f32::from_bits(0x3df5c28f) => 1,
            1 if peak > f32::from_bits(0x3f0ccccd) => 2,
            1 if peak < f32::from_bits(0x3dcccccd) => 0,
            2 if peak < f32::from_bits(0x3ee66666) => 1,
            _ => self.level,
        };
        if next != self.level {
            self.level = next;
            self.unchanged = 0;
        } else {
            self.unchanged = self.unchanged.wrapping_add(1);
            if self.unchanged >= 4 && self.level == 1 {
                self.level = 0;
                self.unchanged = 0;
            }
        }
        self.level
    }
}

fn voice_lipsync(file: Arc<AudioFile>, start_ms: u32, duration_ms: u32) -> anyhow::Result<Vec<u8>> {
    let rate = u64::from(file.info().sample_rate);
    let start = (u64::from(start_ms) * rate / 1000).min(u64::from(u32::MAX)) as u32;
    let length = (duration_ms != 0)
        .then(|| (u64::from(duration_ms) * rate / 1000).min(u64::from(u32::MAX)) as u32);
    Ok(lipsync_windows(AudioSegment::new(
        AudioDecoder::new_pcm16(file)?,
        start,
        length,
    )?))
}

fn lipsync_windows<S: AudioFrameSource>(source: S) -> Vec<u8> {
    let window = (source.sample_rate() / 20).max(1);
    let mut source = AudioSource::new(source);
    let mut detector = VoiceLipSyncDetector::default();
    let mut levels = Vec::new();
    let mut peak = 0.0_f32;
    let mut samples = 0;
    while let Some((left, right)) = source.read_sample() {
        peak = peak.max(left.abs()).max(right.abs());
        samples += 1;
        if samples == window {
            levels.push(detector.window(peak));
            samples = 0;
            peak = 0.0;
        }
    }
    // Native playback lookup clamps to the last completed window; a partial
    // trailing window does not append another state.
    levels
}

fn voice_segment(
    file: Arc<AudioFile>,
    start_ms: u32,
    duration_ms: u32,
) -> anyhow::Result<AudioSegment<AudioDecoder<Arc<AudioFile>>>> {
    let rate = u64::from(file.info().sample_rate);
    let start = (u64::from(start_ms) * rate / 1000).min(u64::from(u32::MAX)) as u32;
    let length = (duration_ms != 0)
        .then(|| (u64::from(duration_ms) * rate / 1000).min(u64::from(u32::MAX)) as u32);
    AudioSegment::new(AudioDecoder::new_pcm16(file)?, start, length)
}

#[cfg(test)]
mod tests;
