//! Explicit older voice commands; skip/read policy is resolved by the host.
use super::{message_playback::MessageVoice, scene::CommandStatus};
use shin_core::vm::higurashi::command::HigurashiRuntimeCommand as Cmd;

/// Message playback and explicit commands share one voice owner. The raw flag
/// and status words are preserved; unknown status bits simply do not intersect
/// the native player's loading/playing bits.
pub trait VoiceCommands: MessageVoice {
    fn play_command(&mut self, filename: &str, volume: f32, flags: i32);
    fn status_bits(&self) -> i32;
    /// A voice-only host yields music commands as external work. The combined
    /// Higurashi audio facade supplies the ordinary BGM service here.
    fn music(&mut self) -> Option<&mut dyn super::music_commands::MusicCommands> {
        None
    }
    fn effects(&mut self) -> Option<&mut dyn super::effect_commands::EffectCommands> {
        None
    }
}

pub fn dispatch(
    command: &Cmd,
    suppress_transient_audio: bool,
    voice: &mut impl VoiceCommands,
) -> Option<CommandStatus> {
    match command {
        Cmd::VOICEPLAY { name, args } => {
            // Native7bd8 tests137d0(ADV, VM) before touching the filename or
            // player. This differs from timer/visual fast skip: the host must
            // supply the resolved command/read/preferences decision.
            if !suppress_transient_audio {
                voice.play_command(name, args[0] as f32 * 0.001, args[1]);
            }
            Some(CommandStatus::Complete)
        }
        Cmd::VOICESTOP {} => {
            voice.stop();
            Some(CommandStatus::Complete)
        }
        Cmd::VOICEWAIT { status } => Some(wait(*status, voice)),
        _ => None,
    }
}

pub fn wait(mask: i32, voice: &impl VoiceCommands) -> CommandStatus {
    if voice.status_bits() & mask == 0 {
        CommandStatus::Complete
    } else {
        CommandStatus::Pending
    }
}

#[cfg(test)]
mod tests;
