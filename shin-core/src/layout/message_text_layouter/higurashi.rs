//! Updated Higurashi's single-font message layout (816c4/81a14/81a70).
//!
//! Share glyph metrics, ruby, line breaking and command clocks with Umineko;
//! select the older color encoding, parser dialect and first-line policy here.
mod backlog;
pub use backlog::{HigurashiBacklogLayout, HigurashiBacklogLayouter, HigurashiBacklogVoice, HigurashiBacklogVoices};

use glam::Vec2;

use super::{
    ColorEncoding, LayoutParams, LineInfo, MessageTextLayouterDefaults, MessageTextLayouterImpl,
    MessageTextLayouterMixin, MessageTextLayouterWithMixin,
    commands::Command,
    font::FontMetrics,
    mixins::{MessageLayerLayouterMixin, QuotationState},
};
use crate::{
    format::text::decode_string_fixup,
    layout::MessageTextParser,
    vm::command::types::{MessageTextLayout, MessageboxType},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum HigurashiMessageStyle {
    Window = 0,
    // Keep numeric names until the corresponding window resources are traced.
    Mode1 = 1,
    Mode2 = 2,
    Transparent = 3,
    Novel = 4,
}

impl HigurashiMessageStyle {
    /// MSGSET clamps unrecognized low nibbles to the ordinary window.
    pub fn from_msginit(style: i32) -> Self {
        match style & 15 {
            1 => Self::Mode1,
            2 => Self::Mode2,
            3 => Self::Transparent,
            4 => Self::Novel,
            _ => Self::Window,
        }
    }
}

struct HigurashiMessageMixin {
    style: HigurashiMessageStyle,
    common: MessageLayerLayouterMixin,
}

impl<Font: FontMetrics> MessageTextLayouterMixin<Font> for HigurashiMessageMixin {
    fn on_char(&mut self, layouter: &mut MessageTextLayouterImpl<Font>, codepoint: char) {
        if self.common.line_index == 0 {
            if self.style == HigurashiMessageStyle::Window {
                self.common.on_char(layouter, codepoint);
            }
        } else {
            layouter.on_char(codepoint);
        }
    }

    fn on_newline(&mut self, layouter: &mut MessageTextLayouterImpl<Font>) {
        if self.common.line_index == 0 && self.style != HigurashiMessageStyle::Window {
            self.common.line_index += 1;
        } else if self.common.line_index == 0 {
            self.common.on_newline(layouter);
        } else {
            layouter.on_newline(self);
            if self.common.quotation_state == QuotationState::Ignored {
                self.common.quotation_state = QuotationState::Uninit;
            }
            self.common.line_index += 1;
        }
    }

    fn on_voice(&mut self, layouter: &mut MessageTextLayouterImpl<Font>, path: String) {
        layouter.on_voice(path);
    }

    fn finalize_up_to(
        &mut self,
        layouter: &mut MessageTextLayouterImpl<Font>,
        index: usize,
        hard_break: bool,
    ) {
        // Native 817fc skips quote indentation for modes3/4.
        if matches!(
            self.style,
            HigurashiMessageStyle::Transparent | HigurashiMessageStyle::Novel
        ) {
            layouter.finalize_up_to(index, hard_break);
        } else {
            self.common.finalize_up_to(layouter, index, hard_break);
        }
    }
}

pub struct HigurashiMessageLayouter<Font>(
    MessageTextLayouterWithMixin<Font, HigurashiMessageMixin>,
);

///85ff4 base layouter used by older SimpleText, without message-window rules.
pub struct HigurashiTextLayouter<Font>(MessageTextLayouterWithMixin<Font, super::mixins::NoMixin>);
impl<Font: FontMetrics + Clone> HigurashiTextLayouter<Font> {
    pub fn new(font: Font, params: LayoutParams, defaults: MessageTextLayouterDefaults) -> Self {
        Self(MessageTextLayouterWithMixin {
            layouter: MessageTextLayouterImpl::with_color_encoding(
                font.clone(),
                font,
                params,
                defaults,
                ColorEncoding::Hexadecimal,
            ),
            mixin: super::mixins::NoMixin,
        })
    }
    pub fn parse(mut self, text: &str) -> (Vec<Command>, Vec<LineInfo>, Vec2) {
        let text = decode_string_fixup(text);
        MessageTextParser::new_higurashi(&text).parse_into(&mut self.0);
        self.0.finish()
    }
    /// Older9b7a4 SimpleText source emits literal UTF-8 characters, skips
    /// control characters and optionally ignores newlines. Question labels
    /// use this source rather than the script-command parser.
    pub fn parse_plain(mut self, text: &str, ignore_newlines: bool) -> (Vec<Command>, Vec<LineInfo>, Vec2) {
        use crate::layout::text_layouter::TextLayouter;
        self.0.on_message_start();
        for c in text.chars() {
            if c > '\u{1f}' {
                self.0.on_char(c);
            } else if c == '\n' && !ignore_newlines {
                self.0.on_newline();
            }
        }
        self.0.on_message_end();
        self.0.finish()
    }
}

impl<Font: FontMetrics + Clone> HigurashiMessageLayouter<Font> {
    /// Native MessageLayer82838 uses 1920x1080 text units at both output sizes.
    /// MSGINIT packs alignment in the high nibble; mode2 forces instant speed.
    pub fn for_message(font: Font, msginit_style: i32, configured_draw_speed: i32) -> Self {
        let style = HigurashiMessageStyle::from_msginit(msginit_style);
        let alignment = match (msginit_style >> 4) & 15 {
            1 => MessageTextLayout::Left,
            2 => MessageTextLayout::Center,
            3 => MessageTextLayout::Right,
            _ => MessageTextLayout::Justify,
        };
        Self::new(
            font,
            style,
            LayoutParams {
                layout_width: 1500.0,
                text_alignment: alignment,
                line_padding_above: 0.0,
                line_padding_below: 0.0,
                line_padding_between: 4.0,
                rubi_size: 20.0,
                text_size: 50.0,
                base_font_horizontal_scale: 1.0,
                follow_kinsoku_shori_rules: true,
                always_leave_space_for_rubi: true,
                perform_soft_breaks: true,
            },
            MessageTextLayouterDefaults {
                color: 0xfff,
                draw_speed: if style == HigurashiMessageStyle::Mode2 {
                    100
                } else {
                    configured_draw_speed
                },
                fade: 200,
            },
        )
    }

    pub fn new(
        font: Font,
        style: HigurashiMessageStyle,
        params: LayoutParams,
        defaults: MessageTextLayouterDefaults,
    ) -> Self {
        Self(MessageTextLayouterWithMixin {
            layouter: MessageTextLayouterImpl::with_color_encoding(
                font.clone(),
                font,
                params,
                defaults,
                ColorEncoding::Hexadecimal,
            ),
            mixin: HigurashiMessageMixin {
                style,
                common: MessageLayerLayouterMixin::new(MessageboxType::Neutral),
            },
        })
    }

    pub fn parse(mut self, message: &str) -> (Vec<Command>, Vec<LineInfo>, Vec2) {
        // Native parser fixes both literal characters and string arguments. SNR
        // text is already fixed by the bytecode reader; this operation is idempotent.
        let text = decode_string_fixup(message);
        MessageTextParser::new_higurashi(&text).parse_into(&mut self.0);
        self.0.finish()
    }
}
