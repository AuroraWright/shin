//! Original8a1b8/8a4b8/8a524 replay queue over the shared voice transport.
//! Replay has its own track and never inherits the story's modal pause.
use super::VoiceFile;
use crate::{
    asset::system::AssetServer,
    audio::{VoicePlayFlags, VoicePlayer},
};
use shin_core::{
    layout::{MessageTextParser, ParsedCommand},
    time::Ticks,
    vm::command::types::{AudioWaitStatus, Volume},
};
use std::sync::Arc;

pub(super) fn files(text: &str) -> Vec<VoiceFile> {
    let mut volume = 1.0;
    MessageTextParser::new(text)
        .filter_map(|command| match command {
            ParsedCommand::VoiceVolume(value) => {
                volume = if value as u32 > 100 {
                    1.0
                } else {
                    value as f32 * 0.01
                };
                None
            }
            ParsedCommand::Voice(path) => Some(VoiceFile { path, volume }),
            _ => None,
        })
        .collect()
}

#[derive(Default)]
pub(super) struct Sequence {
    files: Vec<VoiceFile>,
    next: usize,
    delay: i32,
    active: bool,
}
impl Sequence {
    fn prepare(&mut self, files: Vec<VoiceFile>) -> bool {
        // Original8a298: text with no voice tags leaves the old queue intact.
        if files.is_empty() {
            return false;
        }
        *self = Self {
            files,
            next: 0,
            delay: 0,
            active: true,
        };
        true
    }
    fn stop(&mut self) {
        self.files.clear();
        self.active = false; // Native leaves the index/delay words untouched.
    }
    fn frame(&mut self, playing: bool) -> Option<VoiceFile> {
        if !self.active || playing || self.next >= self.files.len() {
            return None;
        }
        let before = self.delay;
        self.delay = self.delay.wrapping_add(1);
        if before < 9 {
            return None;
        }
        let file = self.files[self.next].clone();
        self.next += 1;
        self.delay = 0;
        Some(file)
    }
}

pub(super) struct BrowserAudio {
    pub(super) voice: VoicePlayer,
    sequence: Sequence,
    pub(super) sounds: super::super::system_menu::GlobalSounds,
    assets: Arc<AssetServer>,
}
impl BrowserAudio {
    pub(super) fn new(
        voice: VoicePlayer,
        sounds: super::super::system_menu::GlobalSounds,
        assets: Arc<AssetServer>,
        volume: u8,
    ) -> Self {
        let mut result = Self {
            voice,
            sequence: Sequence::default(),
            sounds,
            assets,
        };
        result.voice.set_settings_volume(volume);
        result
    }
    pub(super) fn prepare(&mut self, files: Vec<VoiceFile>) {
        if self.sequence.prepare(files) {
            self.voice.stop();
        }
    }
    pub(super) fn stop(&mut self) {
        self.voice.stop();
        self.sequence.stop();
    }
    pub(super) fn playing(&self) -> bool {
        self.voice
            .get_wait_status()
            .contains(AudioWaitStatus::PLAYING)
    }
    pub(super) fn update(&mut self) {
        if let Some(file) = self.sequence.frame(self.playing()) {
            // Originaleb858 receives flags0 and whole-file start/duration0.
            self.voice.play_mapped(
                &Default::default(),
                &file.path,
                0,
                0,
                VoicePlayFlags::empty(),
                Volume(file.volume),
            );
        }
        self.voice.update(Ticks::from_f32(1.0));
    }
    pub(super) fn cue(&mut self, name: &str) {
        let mut sounds = self.sounds.lock();
        sounds.update();
        sounds.play(&self.assets, name, Volume(1.0));
    }
}

#[cfg(test)]
mod tests;
