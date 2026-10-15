//! Native MSG command construction/poll policy; independent of GPU resources.
use anyhow::{Result, ensure};
use shin_core::vm::{
    command::{RuntimeCommand, types::MessageTextLayout},
    gerokasu2::Gerokasu2RuntimeCommand,
};

use super::message_clock::{MessageClock, MessageParams, VoiceTimeline};

#[derive(Debug, Clone, Copy)]
pub struct MessageCommandContext {
    pub was_read: bool,
    /// VM2de4; nonnegative event modes disable ordinary message input.
    pub event_mode: i32,
    pub skip: bool,
    pub message_speed: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageCommandStatus {
    Pending,
    Complete,
}

#[derive(Debug, Clone, Copy)]
pub struct MessageInstall {
    pub params: MessageParams,
    pub alignment: MessageTextLayout,
}

pub enum MessageAction {
    Install(MessageInstall),
    Accepted,
}

#[derive(Debug, Clone, Copy)]
enum Pending {
    AutoWait,
    Wait(i32),
    VoiceSync(i32, i32),
    Close,
}

pub struct MessageCommandState {
    style: i32,
    async_message_active: bool,
    retained_text: Option<String>,
    pending: Option<Pending>,
}

impl MessageCommandState {
    pub(crate) fn restore_descriptor(
        &mut self,
        descriptor: &super::descriptors::MessageDescriptor,
    ) {
        self.style = descriptor.style;
        self.async_message_active = descriptor.asynchronous != 0;
        self.retained_text = descriptor.text.as_deref().map(str::to_owned);
        self.pending = None;
    }
    /// The scene supplies initialized/restored VM90f4 and VM90f0 values.
    pub fn new(style: i32, async_message_active: bool) -> Self {
        Self {
            style,
            async_message_active,
            retained_text: None,
            pending: None,
        }
    }
    pub fn style(&self) -> i32 {
        self.style
    }
    pub fn async_message_active(&self) -> bool {
        self.async_message_active
    }
    pub fn retained_text(&self) -> Option<&str> {
        self.retained_text.as_deref()
    }
    pub fn command_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// Evaluated Ger commands keep their raw style/extra MSGSET operand. Shared
    /// command tokens remain owned by the scene until this state completes.
    pub fn begin(
        &mut self,
        command: &Gerokasu2RuntimeCommand,
        clock: Option<&mut MessageClock>,
        context: MessageCommandContext,
    ) -> Result<Option<MessageAction>> {
        ensure!(
            self.pending.is_none(),
            "Gerokasu2 message command is still pending"
        );
        match command {
            Gerokasu2RuntimeCommand::MessageInit { style } => self.style = *style,
            Gerokasu2RuntimeCommand::MessageSet {
                msg_id,
                auto_wait,
                message_arg,
                text,
            } => {
                // Native constructor4dd0 changes these before first-poll install.
                self.async_message_active = !auto_wait;
                self.retained_text = Some(text.clone());
                self.pending = auto_wait.then_some(Pending::AutoWait);
                let mode = self.style & 15;
                let mode = if mode <= 4 { mode } else { 0 };
                let alignment = match (self.style >> 4) & 15 {
                    1 => MessageTextLayout::Left,
                    2 => MessageTextLayout::Center,
                    3 => MessageTextLayout::Right,
                    _ => MessageTextLayout::Justify,
                };
                return Ok(Some(MessageAction::Install(MessageInstall {
                    params: MessageParams {
                        flags: u32::from(context.was_read || (msg_id.0 as i32) < 0)
                            | (u32::from(context.event_mode >= 0) << 1),
                        mode,
                        // Parserbfbe4 already converted wire ID to zero-based.
                        // Poll4fec forwards that same word to layer/listener.
                        message_id: msg_id.0,
                        message_arg: *message_arg,
                        animate_open: !context.skip,
                    },
                    alignment,
                })));
            }
            Gerokasu2RuntimeCommand::Shared(command) => match command {
                RuntimeCommand::MSGWAIT(command) => {
                    if command.signal_num < 0 {
                        self.async_message_active = false;
                    }
                    self.pending = Some(Pending::Wait(command.signal_num));
                }
                RuntimeCommand::MSGSIGNAL(_) => {
                    if let Some(clock) = clock {
                        clock.send_sync();
                    }
                }
                RuntimeCommand::MSGSYNC(command) => {
                    self.pending = Some(Pending::VoiceSync(command.voice_index, command.sync_time));
                }
                RuntimeCommand::MSGCLOSE(command) => {
                    // Factoryd40 captures VM90f0. An inhibited close completes
                    // before obtaining the message layer or testing skip.
                    if !self.async_message_active {
                        self.retained_text = None;
                        if let Some(clock) = clock {
                            clock.close(!context.skip);
                        }
                        if command.wait_for_close {
                            self.pending = Some(Pending::Close);
                        }
                    }
                }
                _ => return Ok(None),
            },
            _ => return Ok(None),
        }
        Ok(Some(MessageAction::Accepted))
    }

    pub fn poll(
        &mut self,
        clock: Option<&MessageClock>,
        voice: Option<VoiceTimeline>,
    ) -> MessageCommandStatus {
        let waiting = clock.is_some_and(|clock| match self.pending {
            Some(Pending::AutoWait) => clock.sync_is_waiting(-1),
            Some(Pending::Wait(section)) => {
                (section < 0 && clock.busy()) || clock.sync_is_waiting(section)
            }
            Some(Pending::VoiceSync(index, time)) => {
                clock.voice_sync_is_waiting(index, (time as i64) as u64, voice)
            }
            Some(Pending::Close) => clock.busy(),
            None => false,
        });
        if waiting {
            MessageCommandStatus::Pending
        } else {
            self.pending = None;
            MessageCommandStatus::Complete
        }
    }
}

#[cfg(test)]
mod tests;
