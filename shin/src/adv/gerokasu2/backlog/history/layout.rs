//! Ger LogMessage display parser and replay voice descriptors.
use super::Entry;
use shin_core::layout::{
    LineInfo, MessageTextParser, ParsedCommand, commands::Command, font::TextFontMetrics,
    gerokasu2::backlog_layouter,
};
#[derive(Clone, Debug, PartialEq)]
pub struct Voice {
    pub path: String,
    pub volume: f32,
}
pub struct Layout {
    pub commands: Vec<Command>,
    pub lines: Vec<LineInfo>,
    pub size: glam::Vec2,
    pub voice_count: u8,
    pub voices: Vec<Voice>,
}
impl Entry {
    ///83030 rejects marker rows, then parses metadata+0x20 of a Message.
    /// Titles affect display measurement but cannot request replay.
    pub fn voices(&self) -> Vec<Voice> {
        let text = match &self.item {
            super::Item::Message { text, .. } => text,
            _ => return vec![],
        };
        voices_in(text)
    }
    ///999 is the worker's measurement color; callers may supply the native
    /// visible row color without changing geometry. No live voice is started.
    pub fn layout<Font: TextFontMetrics + Clone>(
        &self,
        normal: Font,
        bold: Font,
        color: i32,
    ) -> Layout {
        let text = self.display_text();
        let (commands, lines, size) = backlog_layouter(normal, bold, color).parse(&text);
        let voice_count = voices_in(&text)
            .iter()
            .filter(|v| !v.path.is_empty())
            .fold(0u8, |n, _| n.wrapping_add(1));
        Layout {
            commands,
            lines,
            size,
            voice_count,
            voices: self.voices(),
        }
    }
}
fn voices_in(text: &str) -> Vec<Voice> {
    let mut volume = 1.;
    MessageTextParser::new(text)
        .filter_map(|command| match command {
            ParsedCommand::VoiceVolume(v) => {
                volume = if v < 0 { 1. } else { v.min(100) as f32 * 0.01 };
                None
            }
            ParsedCommand::Voice(path) => Some(Voice { path, volume }),
            _ => None,
        })
        .collect()
}
