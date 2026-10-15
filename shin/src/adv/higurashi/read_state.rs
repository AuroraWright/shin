//! ADV VM's command-current read fields, separate from arithmetic VM storage.
//! Snapshot/read-history services must be supplied before native policy runs.
use super::input::{self, ReadState};
use anyhow::Result;
use shin_core::vm::higurashi::command::HigurashiRuntimeCommand as Cmd;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Checkpoint {
    EventBegin,
    EventEnd,
    Message,
    Autosave,
    Interactive,
}

pub trait ReadServices {
    /// Live derived constructors access the actual settings bank at publication.
    /// Inspection/checkpoint services can omit it until a TipsGet owner is due.
    fn layer_settings(&mut self) -> Option<&mut shin_core::format::save::higurashi::SaveData> {None}
    /// ec54 mutates checkpointed node history and global unlocks before either
    /// live or replay poll. Explicit capture-only callers need a concrete owner.
    fn chart(&mut self, mode: u8, ids: &[i32]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let (memory, progress) = self.checkpoint_owner().ok_or_else(|| {
            anyhow::anyhow!("Higurashi CHART requires node-history and global-unlock owners")
        })?;
        progress.chart_unlocks.construct(mode, ids, &mut memory.extra_words);
        Ok(())
    }
    /// Concrete memory ownership may request the runner's pre-constructor VM,
    /// read fields and layer banks. Other explicit services keep their callback.
    fn checkpoint_owner(&mut self) -> Option<(&mut super::checkpoints::CheckpointMemory, &mut super::progress::ProgressMemory)> {
        None
    }
    /// Native1185c before the constructor mutates read fields. The caller owns
    /// full VM/ADV snapshot storage; an error must leave that storage unchanged.
    fn checkpoint(&mut self, kind: Checkpoint, before: ReadState) -> Result<()>;
    /// Nativea9fe0 reads the zero-based message bit from settings+28.
    fn message_is_read(&self, native_id: u32) -> bool;
    /// Native16760 publishes completion to settings+28. The runner filters
    /// negative IDs and requires a live native VM before invoking this service.
    /// An error must leave this write unacknowledged; it will be retried.
    fn message_completed(&mut self, native_id: u32) -> Result<()>;
    /// First live MSGSET poll calls11780 after installing its message, before
    /// waiting or continuing. It refreshes progress/autosave memory without a
    /// scene-title notification. Inspection services may omit publication.
    fn message_started(&mut self, _boundary: u32) -> Result<()> { Ok(()) }
}

#[derive(Debug, Clone, Copy)]
pub struct ReadPolicy {
    pub modes: u32,
    pub forced: bool,
    pub read_only: bool,
}

pub fn native_message_id(encoded: u32) -> u32 {
    (encoded & 0x00ff_ffff).wrapping_sub(1)
}

/// Known message/event/chapter counters within native2de8..2e08. Interactive
/// resume ownership includes SELECT's conditional constructor boundary.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MessageTimeline {
    pub boundary: u32,
    pub previous_message: u32,
    pub current_message: u32,
    pub previous_chapter: u32,
    pub current_chapter: u32,
    pub resume_position: u32,
    pub replay_target: u32,
    pub normal_messages: u32,
    pub quick_return_messages: u32,
}
impl MessageTimeline {
    fn chapter(&mut self) {
        self.boundary = self.boundary.wrapping_add(1);
        self.previous_chapter = self.current_chapter;
        self.current_chapter = self.boundary;
        self.replay_target = self.boundary;
        self.quick_return_messages = 0;
    }
    fn event_begin(&mut self) {
        self.boundary = self.boundary.wrapping_add(1);
        self.previous_message = self.current_message;
    }
    fn message(&mut self, normal: bool) {
        self.previous_message = self.current_message;
        if normal {
            self.boundary = self.boundary.wrapping_add(1);
            self.current_message = self.boundary;
            self.replay_target = self.boundary;
            self.normal_messages = self.normal_messages.wrapping_add(1);
            self.quick_return_messages = self.quick_return_messages.wrapping_add(1);
        }
    }
}

#[derive(Clone)]
pub struct CommandReadState {
    state: ReadState,
    timeline: MessageTimeline,
    metadata: super::save_metadata::SaveMetadata,
    descriptors: super::descriptors::PlaybackDescriptors,
}
impl CommandReadState {
    pub fn new() -> Self {
        // Original106a8..10710 resets state+24/+26 and stores -1 at+28.
        Self {
            metadata: super::save_metadata::SaveMetadata::new(),
            descriptors: super::descriptors::PlaybackDescriptors::new(),
            timeline: MessageTimeline::default(),
            state: ReadState {
                flags: u32::MAX,
                fast_forward_blocked: 0,
                is_read: 0,
            },
        }
    }
    pub fn state(&self) -> ReadState {
        self.state
    }
    pub fn timeline(&self) -> MessageTimeline {
        self.timeline
    }
    pub(super) fn timeline_mut(&mut self) -> &mut MessageTimeline {
        &mut self.timeline
    }
    pub fn metadata(&self) -> &super::save_metadata::SaveMetadata {
        &self.metadata
    }
    pub fn descriptors(&self) -> &super::descriptors::PlaybackDescriptors {
        &self.descriptors
    }
    /// d3ec clears8c90 before1185c, so the checkpoint must also retain null
    /// text. Other constructors preserve their ordinary pre-mutation payload.
    pub(super) fn constructor_capture(&mut self, command: &Cmd) -> Self {
        if matches!(command, Cmd::CHARSELECT { .. }) { self.descriptors.message.text = None; }
        self.clone()
    }
    pub fn prepare(&mut self, command: &Cmd, services: &mut impl ReadServices) -> Result<()> {
        match command {
            Cmd::CHART { mode, ids } => services.chart(*mode, ids)?,
            Cmd::KAKERA {} | Cmd::SNRSEL {..} | Cmd::QUIZ {..} | Cmd::TIPSGET {..} => {
                //3b2c/3a84/3d48/c0d0 preserve last text and other timeline positions.
                services.checkpoint(Checkpoint::Interactive,self.state)?;
                self.timeline.boundary=self.timeline.boundary.wrapping_add(1);
            }
            Cmd::CHARSELECT { .. } => {
                self.descriptors.message.text = None;
                services.checkpoint(Checkpoint::Interactive,self.state)?;
                self.timeline.boundary=self.timeline.boundary.wrapping_add(1);
                self.timeline.resume_position=self.timeline.boundary;
                self.timeline.replay_target=self.timeline.boundary;
            }
            Cmd::SELECT {..} if self.descriptors.message.text.is_none() => {
                // Native1134/11e8..1230 snapshots only with a null last-text
                // pointer, then advances the interactive resume/replay target.
                services.checkpoint(Checkpoint::Interactive,self.state)?;
                self.timeline.boundary=self.timeline.boundary.wrapping_add(1);
                self.timeline.resume_position=self.timeline.boundary;
                self.timeline.replay_target=self.timeline.boundary;
            }
            Cmd::SAVEINFO { level, info } => self.metadata.set(*level, info)?,
            Cmd::AUTOSAVE {} => {
                services.checkpoint(Checkpoint::Autosave, self.state)?;
                self.timeline.chapter();
                self.metadata.capture_chapter();
            }
            Cmd::EVBEGIN { argument } => {
                services.checkpoint(Checkpoint::EventBegin, self.state)?;
                self.timeline.event_begin();
                self.state.flags = *argument as u32;
            }
            Cmd::EVEND {} => {
                services.checkpoint(Checkpoint::EventEnd, self.state)?;
                self.timeline.boundary = self.timeline.boundary.wrapping_add(1);
                self.state.flags = u32::MAX;
            }
            Cmd::MSGSET { msg_id, .. } => {
                if self.state.flags & 0x8000_0000 != 0 {
                    services.checkpoint(Checkpoint::Message, self.state)?;
                }
                self.timeline.message(self.state.flags & 0x8000_0000 != 0);
                let id = native_message_id(msg_id.0);
                self.state.is_read = u8::from((id as i32) < 0 || services.message_is_read(id));
            }
            Cmd::EXIT { arg1: 0, .. } => self.state.fast_forward_blocked = 1,
            _ => {}
        }
        self.descriptors.observe(command)?;
        Ok(())
    }
    /// Native4f9c/5174: outside event bit31, every message also blocks input.
    pub fn message_flags(&self) -> u32 {
        u32::from(self.state.is_read != 0)
            | if self.state.flags & 0x8000_0000 == 0 {
                2
            } else {
                0
            }
    }
    pub fn suppression(&self, policy: ReadPolicy) -> bool {
        input::suppress_transient_audio(
            Some(self.state),
            policy.modes,
            policy.forced,
            policy.read_only,
        )
    }
}

#[cfg(test)]
mod chapter_tests;
#[cfg(test)]
mod descriptor_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod timeline_tests;
