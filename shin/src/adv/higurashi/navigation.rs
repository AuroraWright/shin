//! Updated14974 rewind and14140/14414/17d1c/17f04 chapter decisions.
use super::{
    input::{self, ReadState},
    question::Callback,
    read_state::{MessageTimeline, ReadPolicy},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chapter {
    Previous,
    Next,
}
impl Chapter {
    pub fn prompt(self) -> &'static str {
        match self {
            Self::Previous => "前のチャプターに戻ります。@rよろしいですか？",
            Self::Next => "次のチャプターか選択肢までスキップします。@rよろしいですか？",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviousMessage {
    Suppressed,
    Unavailable,
    Restore(u32),
}
pub fn previous_message(
    vm: ReadState,
    timeline: MessageTimeline,
    policy: ReadPolicy,
    has_text: bool,
) -> PreviousMessage {
    if input::suppress_transient_audio(Some(vm), policy.modes, policy.forced, policy.read_only) {
        PreviousMessage::Suppressed
    } else if timeline.normal_messages <= 1 {
        PreviousMessage::Unavailable
    } else {
        PreviousMessage::Restore(if has_text {
            timeline.previous_message
        } else {
            timeline.current_message
        })
    }
}
pub fn previous_chapter(timeline: MessageTimeline) -> u32 {
    if timeline.quick_return_messages > 1 {
        timeline.current_chapter
    } else {
        timeline.previous_chapter
    }
}
pub fn accepts_chapter(disabled: bool, callback: Callback) -> bool {
    !disabled && callback.stage == 1 && callback.button == 0
}
#[cfg(test)]
mod tests;
