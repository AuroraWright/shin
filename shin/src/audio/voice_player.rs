use super::SettingsGain;
use std::sync::Arc;

use bitflags::bitflags;
use kira::track::{TrackBuilder, TrackHandle};
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
    fn update_stop(&mut self, ticks: Ticks) -> bool {
        if !self.stopping {
            return false;
        }
        // Native0x11a300/0x11a31c subtract0.2 per tick from the
        // request volume,then multiply by the saved category gain.
        let previous = self.volume;
        self.volume = (self.volume - ticks.as_f32() * f32::from_bits(0x3e4ccccd)).max(0.0);
        if self.volume != previous {
            if let Err(error) = self
                .handle
                .set_volume(Volume(self.volume), Tween::IMMEDIATE)
            {
                warn!(%error, "Failed to fade voice");
            }
        }
        if self.volume == 0.0 {
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

fn voice_speakers(
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
            .expect("Failed to create voice track");
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
        self.stop();
        // VoicePlayer::play at 0x119830 splits on '|'. These files are played
        // together, not concatenated. MessageLayer's caller at 0xb1f50 passes
        // segment boundaries in milliseconds (converted to microseconds).
        let paths: Vec<_> = voice_paths(voicefiles_spec)
            .into_iter()
            .filter(|path| {
                !flags.contains(VoicePlayFlags::ENABLE_CHARACTER_MUTING)
                    || character_voice_enabled(
                        &scenario.info_tables().voice_mapping_info,
                        &path[7..path.len() - 4],
                        self.character_voice_mask,
                    )
            })
            .map(|path| {
                let speakers = voice_speakers(
                    &scenario.info_tables().voice_mapping_info,
                    &path[7..path.len() - 4],
                    flags,
                );
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
            if voice.update_stop(ticks) {
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
                    handle: self.audio_manager.play(AudioData {
                        source,
                        settings: AudioSettings {
                            paused: self.paused,
                            playback_rate: 1.0,
                            track: self.track.id(),
                            fade_in: Tween::IMMEDIATE,
                            loop_start: None,
                            loop_count: 0,
                            volume: pending.volume,
                            pan: Pan::default(),
                        },
                    }),
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
}

/// Native0xea400 uses the first case-insensitive wildcard match;0xea690
/// accepts an unknown speaker or any enabled character in a matched group.
fn character_voice_enabled(
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

fn voice_pattern_matches(pattern: &str, name: &str) -> bool {
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
mod tests {
    use super::*;
    use shin_core::format::audio::AudioBuffer;
    struct ConstantSource {
        position: u32,
        length: u32,
    }
    impl AudioFrameSource for ConstantSource {
        fn max_frame_size(&self) -> usize {
            64
        }
        fn sample_rate(&self) -> u32 {
            1000
        }
        fn pre_skip(&self) -> u32 {
            0
        }
        fn pre_roll(&self) -> u32 {
            0
        }
        fn read_frame(&mut self, destination: &mut AudioBuffer) -> bool {
            let end = (self.position + 64).min(self.length);
            let read = self.position < end;
            for _ in self.position..end {
                destination.push((0.25, 0.25));
            }
            self.position = end;
            read
        }
        fn samples_seek(&mut self, position: u32) -> anyhow::Result<u32> {
            self.position = position.min(self.length);
            Ok(0)
        }
        fn current_sample_position(&self) -> u32 {
            self.position
        }
    }

    #[test]
    fn native_voice_patterns_use_ascii_case_and_full_string_wildcards() {
        for (pattern, name, expected) in [
            ("EP?/KIN*", "ep1/kin0001", true),
            ("a*b*c", "AXBYC", true),
            ("*", "", true),
            ("?", "", false),
            ("a?", "abc", false),
            ("*a", "ab", false),
            ("a**?c", "abc", true),
            ("é?", "é水", true),
            ("é?", "É水", false),
        ] {
            assert_eq!(
                voice_pattern_matches(pattern, name),
                expected,
                "{pattern}/{name}"
            );
        }
    }
    #[test]
    fn character_voice_mask_uses_first_mapping_and_any_enabled_speaker() {
        use shin_core::format::{
            scenario::{info::VoiceMappingInfoItem, types::U8SmallList},
            text::U16String,
        };
        let mapping = |name, ids: Vec<u8>| VoiceMappingInfoItem {
            name_pattern: U16String::new(name),
            lipsync_character_ids: U8SmallList::from_contents(ids),
        };
        let mappings = vec![
            mapping("group/*", vec![2, 31]),
            mapping("group/special", vec![1]),
            mapping("empty", vec![]),
            mapping("wrapped", vec![34]),
        ];
        assert_eq!(
            voice_speakers(
                &mappings,
                "GROUP/special",
                VoicePlayFlags::ENABLE_CHARACTER_LIPSYNC
            ),
            [2, 31]
        );
        assert!(
            voice_speakers(
                &mappings,
                "group/other",
                VoicePlayFlags::ENABLE_CHARACTER_MUTING
            )
            .is_empty()
        );
        assert!(voice_speakers(&mappings, "unknown", VoicePlayFlags::all()).is_empty());
        assert!(speaker_matches(&[2, 255], 255));
        assert!(!speaker_matches(&[2, 255], -1));
        assert!(!speaker_matches(&[2, 255], 258));
        assert!(!character_voice_enabled(&mappings, "GROUP/special", 1 << 1));
        assert!(character_voice_enabled(&mappings, "group/special", 1 << 2));
        assert!(character_voice_enabled(&mappings, "group/other", 1 << 31));
        assert!(!character_voice_enabled(&mappings, "group/other", 0));
        assert!(character_voice_enabled(&mappings, "unknown", 0));
        assert!(!character_voice_enabled(&mappings, "empty", u32::MAX));
        assert!(character_voice_enabled(&mappings, "wrapped", 1 << 2));
    }
    #[test]
    #[ignore = "requires supplied Umineko ROM and executable"]
    fn reference_native_voice_character_mask() {
        use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
        use std::io::Read;
        let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let mut rom =
            RomReader::new(StatelessFile::new(std::fs::File::open(&path).unwrap()).unwrap())
                .unwrap();
        let entry = rom.find_file("/main.snr").unwrap();
        let mut bytes = Vec::new();
        rom.open_file(entry)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let scenario = Scenario::new(bytes.into()).unwrap();
        let mappings = &scenario.info_tables().voice_mapping_info;
        let item = mappings
            .iter()
            .find(|m| voice_pattern_matches(m.name_pattern.as_str(), "19/11900001"))
            .unwrap();
        assert!(!item.lipsync_character_ids.0.is_empty());
        assert!(character_voice_enabled(mappings, "19/11900001", u32::MAX));
        assert!(!character_voice_enabled(mappings, "19/11900001", 0));
        println!(
            "Native voice mappings:{};19/11900001 speakers:{:?}",
            mappings.len(),
            item.lipsync_character_ids
        );
        let exe = std::fs::read(
            std::path::Path::new(&path)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (a, bits) in [
            (0x159974, 0x3e4ccccd),
            (0x159594, 0x3ee66666),
            (0x159ae8, 0x3f0ccccd),
            (0x159570, 0x3dcccccd),
            (0x15946c, 0x3df5c28f),
            (0x1597d4, 0x38000000),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap()),
                bits,
                "native lipsync constant{a:x}"
            );
        }
        let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
        for (a, value) in [
            (0x119e20, 0x7100053f),
            (0x119e24, 0x540000c0),
            (0x119e3c, 0xb90012a8),
            (0x119e64, 0x97ff6aef),
            (0x119eb4, 0xb90012b8),
            (0x11a2cc, 0x35001348),
            (0x11a300, 0xbd497501),
            (0x11a304, 0x1e210809),
            (0x11a310, 0x7100093f),
            (0x11a31c, 0x1e293800),
            (0x11a324, 0xbd001500),
            (0x11a32c, 0xb900151f),
            (0x11a338, 0x1e200900),
            (0x11a33c, 0x97ff6a49),
            (0x11a40c, 0x54000160),
            (0x11a43c, 0x1e202008),
            (0x11a448, 0x97ff6976),
            (0x11a14c, 0x7100011f),
            (0x11a154, 0x320007e0),
            (0x11a160, 0xb9400be8),
            (0x11a16c, 0x531f7900),
            (0xf4ecc, 0xd364fd16),
            (0xf4f78, 0x1b1631ac),
            (0xf4fc4, 0x7c5fc227),
            (0xf4fdc, 0x1e2508e7),
            (0xf4fe0, 0x1e20c0e7),
            (0xf5088, 0x395062ac),
            (0xf50a0, 0x1e2420c0),
            (0xf50a4, 0x5400010c),
            (0xf50ac, 0x1e2220c0),
            (0xf50b0, 0x540000ed),
            (0xf50bc, 0x1e2020c0),
            (0xf50c0, 0x54000125),
            (0xf50cc, 0x1e2320c0),
            (0xf50d0, 0x540000a5),
            (0xf50ec, 0x7100117f),
            (0xf50fc, 0x54000081),
            (0xf5124, 0x390ba1ac),
            (0xf4de8, 0xd344fc08),
            (0xf4df4, 0x9bc97d08),
            (0xf4dfc, 0xd348fd08),
            (0xf4e1c, 0x394ba114),
            (0x1196c0, 0x7100053f),
            (0x1196d0, 0x97ff43dc),
            (0x119714, 0x17ff6da7),
            (0x119724, 0x12800000),
            (0x36adc, 0x52800361),
            (0x36ae4, 0x1e380008),
            (0x36ae8, 0x34000828),
            (0x36b50, 0x1b087e89),
            (0x36b5c, 0x9b2a7d29),
            (0x36b74, 0x1a8ab128),
            (0x36b80, 0xbd40f660),
            (0x36bec, 0xf900767f),
            (0x36ea0, 0xb980ea89),
            (0x36ea4, 0xf8697908),
            (0x119654, 0xf9424908),
            (0x119658, 0x3942d508),
            (0x119668, 0x1e200c28),
            (0x119688, 0x97ff43ee),
            (0x119698, 0x97ff6d8e),
            (0x119960, 0x7200011f),
            (0x119964, 0x9a8003f5),
            (0xea64c, 0x7103fc3f),
            (0xea650, 0x54000168),
            (0xea670, 0x6b01017f),
            (0x14ed0, 0x940086b0),
            (0x14edc, 0x940086b1),
            (0x14ef4, 0x140411cf),
            (0x369fc, 0x1e2e1001),
            (0x36a04, 0x1e215801),
            (0x36a08, 0x1e222000),
            (0x36a0c, 0x1e228c28),
            (0xf4cf8, 0xbd02a688),
            (0x119918, 0x37080de8),
            (0x119c2c, 0x97ff41f5),
            (0x119c34, 0xb4fff560),
            (0x119c40, 0xb940c501),
            (0x119c48, 0x97ff4292),
            (0x119c4c, 0x3707f4a0),
            (0xea414, 0xa951d413),
            (0xea444, 0x9400000f),
            (0xea448, 0x370000a0),
            (0xea4c0, 0x7100a81f),
            (0xea4cc, 0x7100fc1f),
            (0xea4e0, 0x510186a8),
            (0xea4ec, 0x1a953128),
            (0xea4fc, 0x1a803149),
            (0xea51c, 0xaa1403e0),
            (0xea530, 0x97ffffd4),
            (0xea534, 0x37000720),
            (0xea60c, 0x94002385),
            (0xea614, 0x1a9f17f5),
            (0xea690, 0xb40001a0),
            (0xea694, 0xf9400408),
            (0xea698, 0x38401509),
            (0xea6a0, 0x320003e0),
            (0xea6ac, 0x386a690b),
            (0xea6b4, 0x1acb200b),
            (0xea6b8, 0x6a01017f),
            (0xea6bc, 0x54ffff40),
            (0xea6c4, 0x320003e0),
            (0xea6cc, 0x2a1f03e0),
        ] {
            assert_eq!(word(a), value, "Native voice mask{a:x}");
        }
    }

    #[test]
    fn matched_voice_panning_changes_pcm_and_setting_centers_only_matches() {
        use kira::manager::{
            AudioManager as KiraManager, AudioManagerSettings,
            backend::mock::{MockBackend, MockBackendSettings},
        };
        let mut manager = KiraManager::<MockBackend>::new(AudioManagerSettings {
            backend_settings: MockBackendSettings { sample_rate: 1000 },
            ..Default::default()
        })
        .unwrap();
        let handle = manager
            .play(AudioData {
                source: ConstantSource {
                    position: 0,
                    length: 1000,
                },
                settings: AudioSettings {
                    paused: false,
                    playback_rate: 1.0,
                    track: kira::track::TrackId::Main,
                    fade_in: Tween::IMMEDIATE,
                    loop_start: None,
                    loop_count: 0,
                    volume: Volume::default(),
                    pan: Pan::default(),
                },
            })
            .unwrap();
        let mut voice = PlayingVoice {
            handle,
            speakers: vec![19, 42],
            lipsync: vec![],
            volume: 1.0,
            stopping: false,
            pan: Pan::default(),
        };
        let mut sample = |voice: &mut PlayingVoice, id, pan, enabled| {
            voice.pan_for_character(id, pan, enabled);
            let backend = manager.backend_mut();
            let mut frame = kira::Frame::ZERO;
            for _ in 0..16 {
                backend.on_start_processing();
                frame = backend.process();
            }
            (frame.left, frame.right)
        };
        let hard = 0.25 * std::f32::consts::SQRT_2;
        for (id, pan, enabled, left, right) in [
            (19, -1.0, true, hard, 0.0),
            (7, 1.0, true, hard, 0.0),
            (275, 1.0, true, hard, 0.0),
            (42, 1.0, true, 0.0, hard),
            (7, 0.0, false, 0.0, hard),
            (19, 1.0, false, 0.25, 0.25),
        ] {
            let (l, r) = sample(&mut voice, id, pan, enabled);
            assert!(
                (l - left).abs() < 0.00001 && (r - right).abs() < 0.00001,
                "id={id}, enabled={enabled}, PCM={l},{r}"
            );
        }
    }

    #[test]
    fn lipsync_windows_take_both_channels_and_ignore_incomplete_tail() {
        struct Peaks {
            position: u32,
        }
        impl AudioFrameSource for Peaks {
            fn max_frame_size(&self) -> usize {
                37
            }
            fn sample_rate(&self) -> u32 {
                1000
            }
            fn pre_skip(&self) -> u32 {
                0
            }
            fn pre_roll(&self) -> u32 {
                0
            }
            fn read_frame(&mut self, buffer: &mut AudioBuffer) -> bool {
                let peaks = [0.13, 0.6, 0.5, 0.44, 0.11, 0.11, 0.11, 0.11, 0.13, 0.0, 1.0];
                let end = (self.position + 37).min(525);
                let read = self.position < end;
                while self.position < end {
                    buffer.push((0.0, peaks[(self.position / 50) as usize]));
                    self.position += 1;
                }
                read
            }
            fn samples_seek(&mut self, position: u32) -> anyhow::Result<u32> {
                self.position = position;
                Ok(0)
            }
            fn current_sample_position(&self) -> u32 {
                self.position
            }
        }
        assert_eq!(
            lipsync_windows(Peaks { position: 0 }),
            [1, 2, 2, 1, 1, 1, 1, 0, 1, 0]
        );
    }

    #[test]
    fn stopping_voice_fades_absolute_request_volume_while_new_voice_remains_active() {
        use kira::manager::{
            AudioManager as KiraManager, AudioManagerSettings,
            backend::mock::{MockBackend, MockBackendSettings},
        };
        let mut manager = KiraManager::<MockBackend>::new(AudioManagerSettings {
            backend_settings: MockBackendSettings { sample_rate: 1000 },
            ..Default::default()
        })
        .unwrap();
        let mut voices = Vec::new();
        for (volume, stopping, level) in [(0.6, true, 1), (0.5, false, 2)] {
            let handle = manager
                .play(AudioData {
                    source: ConstantSource {
                        position: 0,
                        length: 1000,
                    },
                    settings: AudioSettings {
                        paused: false,
                        playback_rate: 1.0,
                        track: kira::track::TrackId::Main,
                        fade_in: Tween::IMMEDIATE,
                        loop_start: None,
                        loop_count: 0,
                        volume: Volume(volume),
                        pan: Pan::default(),
                    },
                })
                .unwrap();
            voices.push(PlayingVoice {
                handle,
                volume,
                stopping,
                speakers: vec![19],
                lipsync: vec![level],
                pan: Pan::default(),
            });
        }
        assert_eq!(
            character_lipsync(&voices, 19),
            Some(2),
            "The audible stopping tail must not drive mouth animation"
        );
        for (ticks, expected, finished) in [
            (0.0, 0.6, false),
            (1.0, 0.4, false),
            (0.5, 0.3, false),
            (2.0, 0.0, true),
        ] {
            assert_eq!(voices[0].update_stop(Ticks::from_f32(ticks)), finished);
            assert!((voices[0].volume - expected).abs() < 0.000001);
            assert!(!voices[1].update_stop(Ticks::from_f32(ticks)));
            assert_eq!(voices[1].volume, 0.5);
            let backend = manager.backend_mut();
            let mut frame = kira::Frame::ZERO;
            for _ in 0..16 {
                backend.on_start_processing();
                frame = backend.process();
            }
            let expected_pcm = (expected + 0.5) * 0.25;
            assert!(
                (frame.left - expected_pcm).abs() < 0.00001
                    && (frame.right - expected_pcm).abs() < 0.00001,
                "tick{ticks}:PCM{frame:?}"
            );
        }
        assert!(voices[0].handle.get_wait_status().is_empty());
        assert!(
            voices[1]
                .handle
                .get_wait_status()
                .contains(AudioWaitStatus::PLAYING)
        );
    }

    #[test]
    fn native_lipsync_thresholds_are_strict_and_half_open_times_out() {
        let mut detector = VoiceLipSyncDetector::default();
        for (peak, expected) in [
            (0.12, 0),
            (0.13, 1),
            (0.55, 1),
            (0.56, 2),
            (0.45, 2),
            (0.44, 1),
            (0.1, 1),
            (0.11, 1),
            (0.11, 1),
            (0.11, 0),
            (0.13, 1),
            (0.09, 0),
        ] {
            assert_eq!(detector.window(peak), expected, "peak{peak}");
        }
        assert_eq!(detector.window(1.0), 1); // even a loud onset first enters state1
        assert_eq!(detector.window(1.0), 2);
        for _ in 0..20 {
            assert_eq!(detector.window(1.0), 2);
        }
    }

    #[test]
    fn lipsync_uses_first_matching_playing_voice_and_integer_window_positions() {
        use kira::manager::{
            AudioManager as KiraManager, AudioManagerSettings,
            backend::mock::{MockBackend, MockBackendSettings},
        };
        let mut manager = KiraManager::<MockBackend>::new(AudioManagerSettings {
            backend_settings: MockBackendSettings { sample_rate: 1000 },
            ..Default::default()
        })
        .unwrap();
        let mut voices = Vec::new();
        for (speakers, length, lipsync) in [
            (vec![], 1000, vec![2]),
            (vec![19, 42], 200, vec![0, 1, 2]),
            (vec![19], 1000, vec![2]),
        ] {
            let handle = manager
                .play(AudioData {
                    source: ConstantSource {
                        position: 0,
                        length,
                    },
                    settings: AudioSettings {
                        paused: false,
                        playback_rate: 1.0,
                        track: kira::track::TrackId::Main,
                        fade_in: Tween::IMMEDIATE,
                        loop_start: None,
                        loop_count: 0,
                        volume: Volume::default(),
                        pan: Pan::default(),
                    },
                })
                .unwrap();
            voices.push(PlayingVoice {
                handle,
                speakers,
                lipsync,
                volume: 1.0,
                stopping: false,
                pan: Pan::default(),
            });
        }
        assert_eq!(character_lipsync(&voices, 7), None);
        assert_eq!(character_lipsync(&voices, 19), Some(0));
        let backend = manager.backend_mut();
        for (samples, expected) in [(49, 0), (1, 1), (50, 2), (60, 2)] {
            for _ in 0..samples {
                backend.on_start_processing();
                let _ = backend.process();
            }
            backend.on_start_processing();
            assert_eq!(character_lipsync(&voices, 19), Some(expected));
            assert_eq!(character_lipsync(&voices, 42), Some(expected));
        }
        for _ in 0..100 {
            backend.on_start_processing();
            let _ = backend.process();
        }
        backend.on_start_processing();
        assert_eq!(character_lipsync(&voices, 42), None);
        assert_eq!(character_lipsync(&voices, 19), Some(2));
    }

    #[test]
    fn resolves_simultaneous_voice_files() {
        assert_eq!(
            voice_paths("10/10100255|13/10400165"),
            ["/voice/10/10100255.nxa", "/voice/13/10400165.nxa"]
        );
        assert_eq!(voice_paths("41/RG4_00010"), ["/voice/41/rg4_00010.nxa"]);
        assert!(voice_paths("").is_empty());
    }

    #[test]
    #[ignore = "requires the supplied Umineko ROM (SHIN_REFERENCE_ROM)"]
    fn reference_nxa_playback_factories_use_native_pcm16() {
        use shin_core::{
            format::{audio::read_audio, rom::RomReader},
            primitives::stateless_reader::StatelessFile,
        };
        use std::io::Read;
        let mut rom = RomReader::new(
            StatelessFile::new(
                std::fs::File::open(std::env::var_os("SHIN_REFERENCE_ROM").unwrap()).unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        let mut compared = 0usize;
        let mut float_differences = 0usize;
        for path in [
            "/bgm/umib_055.nxa",
            "/se/umise_028.nxa",
            "/voice/19/11900001.nxa",
        ] {
            let mut bytes = Vec::new();
            rom.open_file(rom.find_file(path).unwrap())
                .unwrap()
                .read_to_end(&mut bytes)
                .unwrap();
            let file = Arc::new(read_audio(&bytes).unwrap());
            let settings = AudioSettings {
                paused: false,
                playback_rate: 1.0,
                track: kira::track::TrackId::Main,
                fade_in: Tween::IMMEDIATE,
                loop_start: None,
                loop_count: 0,
                volume: Volume::default(),
                pan: Pan::default(),
            };
            // BGM and SE share this public playback factory. Voice segments
            // use a separate factory, so compare both against the signed API.
            let mut playback =
                AudioSource::new(AudioData::from_audio_file(file.clone(), settings).source);
            let mut voice = AudioSource::new(voice_segment(file.clone(), 0, 0).unwrap());
            let mut expected = AudioSource::new(AudioDecoder::new_pcm16(file.clone()).unwrap());
            let mut floating = AudioSource::new(AudioDecoder::new(file.clone()).unwrap());
            let mut differences = 0usize;
            for position in [
                0,
                123,
                u32::from(file.info().frame_samples) * 3,
                4590,
                4799,
                4800,
            ] {
                playback.samples_seek(position).unwrap();
                voice.samples_seek(position).unwrap();
                expected.samples_seek(position).unwrap();
                floating.samples_seek(position).unwrap();
                for _ in 0..4096 {
                    let pcm = expected.read_sample();
                    assert_eq!(
                        playback.read_sample(),
                        pcm,
                        "BGM/SE factory {path} at {position}"
                    );
                    assert_eq!(
                        voice.read_sample(),
                        pcm,
                        "Voice factory {path} at {position}"
                    );
                    differences += usize::from(floating.read_sample() != pcm);
                    if let Some((left, right)) = pcm {
                        for sample in [left, right] {
                            assert_eq!((sample * 32768.0).trunc(), sample * 32768.0);
                            assert!((-1.0..1.0).contains(&sample));
                        }
                        compared += 1;
                    }
                }
            }
            assert!(
                differences > 0,
                "Fixture must distinguish PCM16 from float decode: {path}"
            );
            float_differences += differences;
            println!(
                "Native PCM16 playback factory: {path}, channels{}, float differences{differences}",
                file.info().channel_count
            );
        }
        println!(
            "Playback factory comparison: {compared} frames, {float_differences} float differences; no audio device opened"
        );
    }

    #[test]
    #[ignore = "requires the supplied Umineko ROM (SHIN_REFERENCE_ROM)"]
    fn reference_voice_segments_decode_and_finish_playback() {
        use kira::manager::{
            AudioManager as KiraManager, AudioManagerSettings,
            backend::mock::{MockBackend, MockBackendSettings},
        };
        use shin_core::{
            format::{
                audio::{AudioSource, read_audio},
                rom::RomReader,
            },
            primitives::stateless_reader::StatelessFile,
        };
        use std::{fs::File, io::Read};

        let path = std::env::var("SHIN_REFERENCE_ROM").expect("Set SHIN_REFERENCE_ROM");
        let mut rom =
            RomReader::new(StatelessFile::new(File::open(path).unwrap()).unwrap()).unwrap();
        let entry = rom.find_file("/voice/19/11900001.nxa").unwrap();
        let mut bytes = Vec::new();
        rom.open_file(entry)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let file = Arc::new(read_audio(&bytes).unwrap());
        let begin = std::time::Instant::now();
        let levels = voice_lipsync(file.clone(), 0, 0).unwrap();
        println!("Reference lipsync timeline load cost {:?}", begin.elapsed());
        assert!(!levels.is_empty());
        assert!(levels.iter().all(|&level| level <= 2));
        println!(
            "Native PCM16 voice lipsync:{} windows, counts{:?}, first{:?}",
            levels.len(),
            [0, 1, 2].map(|level| levels.iter().filter(|&&value| value == level).count()),
            &levels[..levels.len().min(30)]
        );
        assert!(voice_lipsync(file.clone(), 0, 49).unwrap().is_empty());
        assert_eq!(voice_lipsync(file.clone(), 10, 175).unwrap().len(), 3);
        let rate = file.info().sample_rate;
        let length = rate / 50; // 20 milliseconds

        let mut expected = AudioSource::new(AudioDecoder::new_pcm16(file.clone()).unwrap());
        expected.samples_seek(rate / 100).unwrap(); // 10 milliseconds
        let mut segment = AudioSource::new(voice_segment(file.clone(), 10, 20).unwrap());
        for _ in 0..length {
            assert_eq!(segment.read_sample(), expected.read_sample());
        }
        assert_eq!(segment.read_sample(), None);

        // Exercise the same AudioData/AudioSound path used by VoicePlayer,
        // without opening an audio device or constructing a GPU asset server.
        let mut manager = KiraManager::<MockBackend>::new(AudioManagerSettings {
            backend_settings: MockBackendSettings { sample_rate: rate },
            ..Default::default()
        })
        .unwrap();
        let handle = manager
            .play(AudioData {
                source: voice_segment(file, 10, 20).unwrap(),
                settings: AudioSettings {
                    paused: false,
                    playback_rate: 1.0,
                    track: kira::track::TrackId::Main,
                    fade_in: Tween::IMMEDIATE,
                    loop_start: None,
                    loop_count: 0,
                    volume: Volume::default(),
                    pan: Pan::default(),
                },
            })
            .unwrap();
        assert!(handle.get_wait_status().contains(AudioWaitStatus::PLAYING));
        let backend = manager.backend_mut();
        for _ in 0..length + 32 {
            backend.on_start_processing();
            let frame = backend.process();
            assert!(frame.left.is_finite() && frame.right.is_finite());
        }
        assert!(handle.get_wait_status().is_empty());
    }
}
