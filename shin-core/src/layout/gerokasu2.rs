//! Gerokasu2 MessageLayer caller defaults and nameplate policy.
//! Native8c918..8c974,8b9f4..8ba10,8ba7c..8bb9c. Font roles are caller inputs.
use super::font::TextFontMetrics;
use super::{LayoutParams, MessageLayerLayouter, MessageTextLayouterDefaults};
use crate::vm::command::types::MessageTextLayout;

pub fn message_layouter<Font: TextFontMetrics>(
    normal: Font,
    bold: Font,
    window_mode: i32,
    alignment: MessageTextLayout,
    message_speed: u8,
) -> MessageLayerLayouter<Font> {
    MessageLayerLayouter::with_name_visibility(
        normal,
        bold,
        window_mode == 1 || window_mode == 4,
        LayoutParams {
            layout_width: 1200.0,
            text_alignment: alignment,
            line_padding_above: 0.0,
            line_padding_below: 0.0,
            line_padding_between: 4.0,
            rubi_size: 20.0,
            text_size: 40.0,
            base_font_horizontal_scale: 1.0,
            follow_kinsoku_shori_rules: true,
            always_leave_space_for_rubi: true,
            perform_soft_breaks: true,
        },
        MessageTextLayouterDefaults {
            color: 999,
            draw_speed: if window_mode == 2 {
                100
            } else {
                i32::from(message_speed)
            },
            fade: 200,
        },
    )
    .with_gerokasu2_rules()
}

#[cfg(test)]
mod tests;

/// Generic SELECT labels use the later parser with48px text and a1000px
/// outlined text box. Layout excludes the native3px outline on both sides.
pub fn choice_layouter<Font: TextFontMetrics + Clone>(
    font: Font,
) -> super::MessageTextLayouter<Font> {
    super::MessageTextLayouter::new(
        font.clone(),
        font,
        LayoutParams {
            layout_width: 994.0,
            text_size: 48.0,
            rubi_size: 0.0,
            perform_soft_breaks: false,
            always_leave_space_for_rubi: false,
            ..Default::default()
        },
        MessageTextLayouterDefaults {
            color: 999,
            draw_speed: 50,
            fade: 200,
        },
    )
    .with_gerokasu2_rules()
}

/// Gerokasu CardSelect messages (46B20 -> 1E0B0): a 1280px outlined box at
/// 48px with the same rules as SELECT labels.
pub fn card_message_layouter<Font: TextFontMetrics + Clone>(
    font: Font,
) -> super::MessageTextLayouter<Font> {
    super::MessageTextLayouter::new(
        font.clone(),
        font,
        LayoutParams {
            layout_width: 1274.0,
            text_size: 48.0,
            rubi_size: 0.0,
            perform_soft_breaks: false,
            always_leave_space_for_rubi: false,
            ..Default::default()
        },
        MessageTextLayouterDefaults {
            color: 999,
            draw_speed: 50,
            fade: 200,
        },
    )
    .with_gerokasu2_rules()
}

#[cfg(test)]
mod selection_tests;

/// GenericText menu and dialog callers supply their native sizes and wrapping
/// policy, while retaining the later engine's parser and layout rules.
pub fn generic_layouter<Font: TextFontMetrics + Clone>(
    font: Font,
    params: LayoutParams,
) -> super::MessageTextLayouter<Font> {
    super::MessageTextLayouter::new(
        font.clone(),
        font,
        params,
        MessageTextLayouterDefaults {
            color: 999,
            draw_speed: 50,
            fade: 200,
        },
    )
    .with_gerokasu2_rules()
}

/// MessageBrowser LogMessage807C0. The modern backlog name/quotation mixin
/// is retained with Ger font roles, parser rules and caller dimensions.
pub fn backlog_layouter<Font: TextFontMetrics + Clone>(
    normal: Font,
    bold: Font,
    color: i32,
) -> super::BacklogTextLayouter<Font> {
    super::BacklogTextLayouter::new(
        normal,
        bold,
        LayoutParams {
            layout_width: 1320.0,
            text_alignment: MessageTextLayout::Justify,
            line_padding_above: 6.0,
            line_padding_below: 4.0,
            line_padding_between: 0.0,
            rubi_size: 0.0,
            text_size: 44.0,
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
    )
    .with_gerokasu2_rules()
}
