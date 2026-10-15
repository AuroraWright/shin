//! Explicit native voice commands; the scene supplies resolved skip policy.
use super::{
    message_commands::MessageCommandStatus,
    voice::{Gerokasu2VoicePlayer, MessageNotifications},
};
use shin_core::vm::{command::RuntimeCommand, gerokasu2::Gerokasu2RuntimeCommand as Cmd};

pub trait VoiceCommands {
    fn play_command(&mut self, name: &str, volume: f32, flags: i32);
    fn stop(&mut self);
    fn status_bits(&self) -> i32;
}
impl VoiceCommands for Gerokasu2VoicePlayer {
    fn play_command(&mut self, name: &str, volume: f32, flags: i32) {
        Gerokasu2VoicePlayer::play_command(self, name, volume, flags);
    }
    fn stop(&mut self) {
        Gerokasu2VoicePlayer::stop(self);
    }
    fn status_bits(&self) -> i32 {
        self.wait_status().bits()
    }
}

pub struct VoiceCommandState {
    pending_mask: Option<i32>,
}
impl VoiceCommandState {
    pub fn new() -> Self {
        Self { pending_mask: None }
    }
    pub fn dispatch(
        &mut self,
        command: &Cmd,
        skip_allowed: bool,
        voice: &mut impl VoiceCommands,
        notifications: &mut impl MessageNotifications,
    ) -> anyhow::Result<Option<MessageCommandStatus>> {
        anyhow::ensure!(
            self.pending_mask.is_none(),
            "Gerokasu2 voice command is still pending"
        );
        match command {
            Cmd::VoicePlay {
                name,
                volume,
                flags,
            } => {
                // Native7290 tests11b00 before filename/read observation/player.
                if !skip_allowed {
                    notifications.observed_voice(name);
                    voice.play_command(name, (*volume as f32) * f32::from_bits(0x3a83126f), *flags);
                }
            }
            Cmd::Shared(RuntimeCommand::VOICESTOP(_)) => voice.stop(),
            Cmd::VoiceWait { statuses } => self.pending_mask = Some(*statuses),
            _ => return Ok(None),
        }
        Ok(Some(self.poll(voice)))
    }
    pub fn poll(&mut self, voice: &impl VoiceCommands) -> MessageCommandStatus {
        if self
            .pending_mask
            .is_some_and(|mask| voice.status_bits() & mask != 0)
        {
            MessageCommandStatus::Pending
        } else {
            self.pending_mask = None;
            MessageCommandStatus::Complete
        }
    }
}
