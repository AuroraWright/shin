//! Browser-private transport; cue ordering lives in the native serial scheduler.
use crate::{
    adv::gerokasu2::{
        backlog::{history::layout::Voice, voice_queue::Sequence},
        voice::Gerokasu2VoicePlayer,
    },
    asset::system::AssetServer,
};
use shin_audio::AudioManager;
use shin_core::{
    format::{save::Settings, scenario::gerokasu2::Gerokasu2Scenario},
    time::Ticks,
    vm::command::types::AudioWaitStatus,
};
use std::sync::Arc;

pub(super) struct Replay {
    player: Gerokasu2VoicePlayer,
    queue: Sequence,
}

#[cfg(test)]
mod tests;
impl Replay {
    #[cfg(test)]
    pub(super) fn reference_position(&self) -> u64 {
        self.player.timeline().position
    }
    pub fn new(
        audio: Arc<AudioManager>,
        assets: Arc<AssetServer>,
        scenario: &Gerokasu2Scenario,
        settings: &Settings,
    ) -> Self {
        let mut player = Gerokasu2VoicePlayer::new(audio, assets, scenario);
        player.set_preferences(settings.v2_voicevol, settings.v19, settings.v5_voicepanapot);
        Self {
            player,
            queue: Sequence::default(),
        }
    }
    pub fn start(&mut self, cues: Vec<Voice>) {
        self.player.stop();
        self.queue.restart(cues);
    }
    pub fn stop(&mut self) {
        self.queue.stop();
        self.player.stop();
    }
    pub fn playing(&self) -> bool {
        self.player.wait_status().contains(AudioWaitStatus::PLAYING)
    }
    pub fn update(&mut self) {
        if let Some(cue) = self.queue.advance(self.playing()) {
            self.player.play_backlog(&cue.path, cue.volume);
        }
        self.player.update(Ticks::from_f32(1.));
    }
}
