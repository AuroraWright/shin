//! Native message-browser replay helper b85e0/b87c0/b88c0.
use std::sync::Arc;

use shin_audio::AudioManager;
use shin_core::{
    format::scenario::Scenario,
    layout::{MessageTextParser, ParsedCommand},
    time::Ticks,
    vm::command::types::{AudioWaitStatus, Volume},
};

use crate::{
    asset::system::AssetServer,
    audio::{VoicePlayFlags, VoicePlayer},
};

#[derive(Debug, Clone, PartialEq)]
struct Cue {
    path: String,
    volume: f32,
}

fn cues(text: &str) -> Vec<Cue> {
    let mut volume = 1.0;
    MessageTextParser::new(text)
        .filter_map(|command| match command {
            ParsedCommand::VoiceVolume(value) => {
                // Native b8bd4 is an unsigned comparison: negatives select1 too.
                volume = if value as u32 > 100 {
                    1.0
                } else {
                    value as f32 * 0.01
                };
                None
            }
            ParsedCommand::Voice(path) => Some(Cue { path, volume }),
            _ => None,
        })
        .collect()
}

#[derive(Debug, Default)]
struct Sequence {
    cues: Vec<Cue>,
    next: usize,
    delay: u8,
    active: bool,
}

impl Sequence {
    fn replace(&mut self, text: &str) {
        *self = Self {
            cues: cues(text),
            active: true,
            ..Default::default()
        };
    }
    fn cancel(&mut self) {
        *self = Self::default();
    }
    fn frame(&mut self, playing: bool) -> Option<Cue> {
        if !self.active || playing {
            return None;
        }
        if self.next == self.cues.len() {
            self.active = false;
            return None;
        }
        self.delay += 1;
        if self.delay < 10 {
            return None;
        }
        self.delay = 0;
        let cue = self.cues[self.next].clone();
        self.next += 1;
        Some(cue)
    }
}

/// Owns replay audio separately from the paused story voice player. Its drop
/// releases both current playback and pending loads through VoicePlayer.
pub(super) struct BacklogVoiceReplay {
    voice: VoicePlayer,
    sequence: Sequence,
    scenario: Arc<Scenario>,
}

impl BacklogVoiceReplay {
    pub fn new(
        audio: Arc<AudioManager>,
        assets: Arc<AssetServer>,
        scenario: Arc<Scenario>,
    ) -> Self {
        Self {
            voice: VoicePlayer::new(audio, assets),
            sequence: Sequence::default(),
            scenario,
        }
    }
    pub fn set_settings_volume(&mut self, percent: u8) {
        self.voice.set_settings_volume(percent);
    }
    pub fn play_message(&mut self, text: &str) {
        self.voice.stop();
        self.sequence.replace(text);
    }
    pub fn stop(&mut self) {
        self.voice.stop();
        self.sequence.cancel();
    }
    /// a3fcc/a3fd8 passes literal1 to b88c0. Delays and owned voice stop fades
    /// advance once per browser update, regardless of the frame's elapsed dt.
    pub fn update(&mut self) {
        if let Some(cue) = self.sequence.frame(self.is_playing()) {
            self.voice.play(
                &self.scenario,
                &cue.path,
                0,
                0,
                VoicePlayFlags::empty(),
                Volume(cue.volume),
            );
        }
        // Native queue polls wait status before updating its VoicePlayer.
        self.voice.update(Ticks::from_f32(1.0));
    }
    #[cfg(test)]
    pub(super) fn reference_voice_metadata(
        &self,
    ) -> Vec<(Vec<u8>, shin_core::vm::command::types::Pan)> {
        self.voice.reference_voice_metadata()
    }
    pub fn is_playing(&self) -> bool {
        self.voice
            .get_wait_status()
            .contains(AudioWaitStatus::PLAYING)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
    fn reference_native_backlog_story_voice_and_se_pause_scopes() {
        let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&path)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0x13ab0, 0xb9447668u32),
            (0x13ab4, 0x35000128u32),
            (0x13ac4, 0x940419cbu32),
            (0x13ad0, 0x94037ba4u32),
            (0x13ad8, 0x11000508u32),
            (0x13adc, 0xb9047668u32),
            (0x14584, 0xb9447408u32),
            (0x1458c, 0x71000508u32),
            (0x14590, 0xb9047408u32),
            (0x14594, 0x54000101u32),
            (0x145a4, 0x94041713u32),
            (0x145b0, 0x940378ecu32),
            (0x11a200, 0x39403008u32),
            (0x11a20c, 0x340000c8u32),
            (0x11a24c, 0x97ff6a39u32),
            (0x11a274, 0x97ff6a0bu32),
            (0x11a284, 0x39003274u32),
            (0xf29c0, 0x9400085cu32),
            (0xf29ec, 0x9400082du32),
            (0xf29fc, 0x39000274u32),
            (0xf4b50, 0x390aba9fu32),
            (0xf4b60, 0xd63f0100u32),
            (0xf4ac4, 0x390aba88u32),
            (0xf4b08, 0xd63f0100u32),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native modal audio pause {address:x}"
            );
        }
    }

    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
    fn reference_native_backlog_voice_replay_sequence() {
        let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&path)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0xa3fc8, 0x1e2e1009u32),
            (0xa3fcc, 0x4ea91d20u32),
            (0xa3fd8, 0x9400523au32),
            (0xa7034, 0x9400456bu32),
            (0xa7044, 0x940045dfu32),
            (0xb8624, 0x32091be8u32),
            (0xb8bc0, 0x1e220020u32),
            (0xb8bc8, 0xbd494901u32),
            (0xb8bd4, 0x7101903fu32),
            (0xb8bd8, 0x1e208c20u32),
            (0xb8bdc, 0xbd002000u32),
            (0xb8c10, 0xb9001bf4u32),
            (0xb87e4, 0x94018583u32),
            (0xb8834, 0xf900167fu32),
            (0xb8838, 0xb900327fu32),
            (0xb883c, 0x39008268u32),
            (0xb88f4, 0x940185f3u32),
            (0xb88f8, 0x37080620u32),
            (0xb8914, 0x1e282800u32),
            (0xb8918, 0x1e249001u32),
            (0xb8924, 0x540004cbu32),
            (0xb8970, 0xbd401840u32),
            (0xb897c, 0x2a1f03e3u32),
            (0xb8984, 0x9401836fu32),
            (0xb89ac, 0xf9001668u32),
            (0xb89b0, 0xb900327fu32),
            (0xb89c8, 0x9401863au32),
            (0x1197b0, 0xaa1f03e3u32),
            (0x1197b4, 0xaa1f03e4u32),
            (0x1197c0, 0x9400001cu32),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native replay {address:x}"
            );
        }
        assert_eq!(
            f32::from_le_bytes(exe[0x159948 + 256..0x15994c + 256].try_into().unwrap()),
            0.01
        );
    }

    #[test]
    fn replay_collects_ordered_full_voice_tags_and_authored_volume() {
        // Authored integer parser ignores the minus sign in @o-1.
        let result = cues(
            "戦人@r@vfirst|second.@o25.@vthird.@o150.@vfourth.@o-1.@vfifth.@o0.@v.@x500.@/本文",
        );
        assert_eq!(
            result,
            vec![
                Cue {
                    path: "first|second".into(),
                    volume: 1.0
                },
                Cue {
                    path: "third".into(),
                    volume: 0.25
                },
                Cue {
                    path: "fourth".into(),
                    volume: 1.0
                },
                Cue {
                    path: "fifth".into(),
                    volume: 0.01
                },
                Cue {
                    path: "".into(),
                    volume: 0.0
                },
            ]
        );
    }

    #[test]
    fn replay_waits_ten_frames_and_holds_delay_while_loading_or_playing() {
        let mut sequence = Sequence::default();
        sequence.replace("@vfirst.@vsecond.");
        for _ in 0..9 {
            assert_eq!(sequence.frame(false), None);
        }
        for _ in 0..100 {
            assert_eq!(sequence.frame(true), None);
        }
        assert_eq!(sequence.frame(false).unwrap().path, "first");
        for _ in 0..100 {
            assert_eq!(sequence.frame(true), None);
        }
        for _ in 0..9 {
            assert_eq!(sequence.frame(false), None);
        }
        assert_eq!(sequence.frame(false).unwrap().path, "second");
        assert!(sequence.active);
        assert_eq!(sequence.frame(true), None);
        assert!(sequence.active);
        assert_eq!(sequence.frame(false), None);
        assert!(!sequence.active);
    }

    #[test]
    fn replay_replacement_cancel_empty_and_missing_cues_do_not_stall() {
        let mut sequence = Sequence::default();
        sequence.replace("@vold.@vdiscarded.");
        for _ in 0..9 {
            sequence.frame(false);
        }
        sequence.replace("@v.@vnew.");
        for _ in 0..9 {
            assert_eq!(sequence.frame(false), None);
        }
        assert_eq!(sequence.frame(false).unwrap().path, "");
        for _ in 0..9 {
            assert_eq!(sequence.frame(false), None);
        }
        assert_eq!(sequence.frame(false).unwrap().path, "new");
        sequence.cancel();
        for _ in 0..20 {
            assert_eq!(sequence.frame(false), None);
        }
        sequence.replace("本文");
        assert_eq!(sequence.frame(false), None);
        assert!(!sequence.active);
    }
}
