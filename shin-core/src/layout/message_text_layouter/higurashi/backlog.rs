//! Updated MessageBrowser::LogMessage76900/78e30 and callbacks7ed00..7f03c.
use super::super::{
    ColorEncoding, LayoutParams, LineInfo, MessageTextLayouterDefaults, MessageTextLayouterImpl,
    MessageTextLayouterMixin, MessageTextLayouterWithMixin,
    commands::Command,
    font::FontMetrics,
    mixins::{MessageLayerLayouterMixin, QuotationState},
};
use crate::{
    format::text::decode_string_fixup,
    layout::{MessageTextParser, TextLayouter},
    vm::command::types::{MessageTextLayout, MessageboxType},
};
use glam::Vec2;

/// Measurement increments the row's byte counter; glyph construction records
/// paths and their current volume instead. Neither mode produces voice commands.
pub enum HigurashiBacklogVoices {
    Count(u8),
    Collect,
}

#[derive(Debug, PartialEq)]
pub struct HigurashiBacklogVoice {
    pub volume: f32,
    pub path: String,
}

pub struct HigurashiBacklogLayout {
    pub commands: Vec<Command>,
    pub lines: Vec<LineInfo>,
    pub size: Vec2,
    pub voice_count: u8,
    pub voices: Vec<HigurashiBacklogVoice>,
}

struct BacklogMixin {
    style: i32,
    in_name: bool,
    name_chars: u32,
    body_chars: u32,
    quotation: MessageLayerLayouterMixin,
    voice_mode: HigurashiBacklogVoices,
    voices: Vec<HigurashiBacklogVoice>,
}

impl<Font: FontMetrics> MessageTextLayouterMixin<Font> for BacklogMixin {
    fn on_char(&mut self, layouter: &mut MessageTextLayouterImpl<Font>, codepoint: char) {
        if self.in_name {
            if self.name_chars == 0 {
                layouter.on_char('【');
            }
            self.name_chars = self.name_chars.wrapping_add(1);
        } else {
            self.body_chars = self.body_chars.wrapping_add(1);
        }
        layouter.on_char(codepoint);
    }

    fn on_newline(&mut self, layouter: &mut MessageTextLayouterImpl<Font>) {
        if self.in_name {
            if self.name_chars != 0 {
                layouter.on_char('】');
                layouter.finalize_up_to(layouter.commands.len(), true);
            }
            self.in_name = false;
        } else if self.style & 15 != 3 || self.body_chars != 0 {
            layouter.on_newline(self);
            if self.quotation.quotation_state == QuotationState::Ignored {
                self.quotation.quotation_state = QuotationState::Uninit;
            }
            self.body_chars = 0;
        }
    }

    fn on_voice(&mut self, layouter: &mut MessageTextLayouterImpl<Font>, path: String) {
        match &mut self.voice_mode {
            HigurashiBacklogVoices::Count(count) => {
                if !path.is_empty() {
                    *count = count.wrapping_add(1);
                }
            }
            HigurashiBacklogVoices::Collect => self.voices.push(HigurashiBacklogVoice {
                volume: layouter.voice_volume,
                path,
            }),
        }
    }

    fn finalize_up_to(
        &mut self,
        layouter: &mut MessageTextLayouterImpl<Font>,
        index: usize,
        hard_break: bool,
    ) {
        if self.style & 15 == 3 {
            layouter.finalize_up_to(index, hard_break);
        } else {
            self.quotation.finalize_up_to(layouter, index, hard_break);
        }
    }
}

pub struct HigurashiBacklogLayouter<Font>(MessageTextLayouterWithMixin<Font, BacklogMixin>);

impl<Font: FontMetrics + Clone> HigurashiBacklogLayouter<Font> {
    /// Both original constructors use1440 units,45-point body text,6/4 padding,
    /// derived18-point ruby,50 draw speed,200 fade and hexadecimal colors.
    /// Nonmessage rows pass style0 regardless of the previous MSGINIT.
    pub fn new(font: Font, style: i32, color: i32, voices: HigurashiBacklogVoices) -> Self {
        let mut quotation = MessageLayerLayouterMixin::new(MessageboxType::Transparent);
        quotation.line_index = 1;
        Self(MessageTextLayouterWithMixin {
            layouter: MessageTextLayouterImpl::with_color_encoding(
                font.clone(),
                font,
                LayoutParams {
                    layout_width: 1440.0,
                    text_alignment: MessageTextLayout::Justify,
                    line_padding_above: 6.0,
                    line_padding_below: 4.0,
                    line_padding_between: 0.0,
                    rubi_size: 0.0,
                    text_size: 45.0,
                    base_font_horizontal_scale: 1.0,
                    follow_kinsoku_shori_rules: true,
                    always_leave_space_for_rubi: false,
                    perform_soft_breaks: true,
                },
                MessageTextLayouterDefaults {
                    color,
                    draw_speed: 50,
                    fade: 200,
                },
                ColorEncoding::Hexadecimal,
            ),
            mixin: BacklogMixin {
                style,
                in_name: true,
                name_chars: 0,
                body_chars: 0,
                quotation,
                voice_mode: voices,
                voices: Vec::new(),
            },
        })
    }

    pub fn parse(mut self, text: &str) -> HigurashiBacklogLayout {
        let text = decode_string_fixup(text);
        MessageTextParser::new_higurashi(&text).parse_into(&mut self);
        let voice_count = match self.0.mixin.voice_mode {
            HigurashiBacklogVoices::Count(count) => count,
            HigurashiBacklogVoices::Collect => 0,
        };
        let voices = std::mem::take(&mut self.0.mixin.voices);
        let (commands, lines, size) = self.0.finish();
        HigurashiBacklogLayout {
            commands,
            lines,
            size,
            voice_count,
            voices,
        }
    }
}

macro_rules! forward {
    ($($name:ident($($arg:ident: $ty:ty),*);)*) => {
        $(fn $name(&mut self, $($arg: $ty),*) { self.0.$name($($arg),*); })*
    };
}
impl<Font: FontMetrics> TextLayouter for HigurashiBacklogLayouter<Font> {
    forward! {
        on_message_start(); on_message_end(); on_char(codepoint: char); on_newline();
        on_click_wait(); on_auto_click(); on_set_font_scale(scale: i32);
        on_set_color(color: i32); on_set_draw_speed(speed: i32); on_set_fade(fade: i32);
        on_wait(delay: i32); on_start_parallel(); on_section(); on_sync();
        on_instant_start(); on_instant_end(); on_lipsync_enabled(); on_lipsync_disabled();
        on_set_voice_volume(volume: i32); on_voice(path: String); on_voice_wait();
        on_rubi_content(content: String); on_rubi_base_start(); on_rubi_base_end();
        on_bold_start(); on_bold_end();
    }
    // Original7f038 returns immediately, including while a ruby base is open.
    fn on_voice_sync(&mut self, _target_instant: i32) {}
}
