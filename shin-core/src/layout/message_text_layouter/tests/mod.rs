mod dumps;
mod snapshots;

use std::{fs::File, io::BufReader, sync::LazyLock};

use glam::Vec2;

use crate::{
    format::font::FontInfo,
    layout::{
        MessageTextParser,
        message_text_layouter::{
            LayoutParams, LineInfo, MessageLayerLayouter, MessageTextLayouterDefaults,
            commands::Command,
        },
    },
    vm::command::types::{MessageTextLayout, MessageboxType},
};

pub struct TestFonts {
    bold_font: FontInfo,
    normal_font: FontInfo,
}

const FONT_PATHS: [&str; 2] = [
    "test_assets/newrodin-medium.fnt",
    "test_assets/newrodin-bold.fnt",
];

/// The test fonts are game data and are not distributed with the repository;
/// tests that lay out text are skipped without them.
pub fn fonts_available() -> bool {
    let available = FONT_PATHS
        .iter()
        .all(|path| std::path::Path::new(path).is_file());
    if !available {
        eprintln!("skipping: the test fonts are not in shin-core/test_assets");
    }
    available
}

pub fn read_fonts() -> TestFonts {
    fn read_font(path: &str) -> FontInfo {
        let font = File::open(path).unwrap();
        let mut font = BufReader::new(font);
        shin_core::format::font::read_font_metrics(&mut font).unwrap()
    }

    let normal = read_font(FONT_PATHS[0]);
    let bold = read_font(FONT_PATHS[1]);
    // let system = read_font("test_assets/system.fnt");

    TestFonts {
        bold_font: bold,
        normal_font: normal,
    }
}

pub fn make_snapshot(
    text_alignment: MessageTextLayout,
    messagebox_type: MessageboxType,
    text: &str,
) -> (Vec<Command>, Vec<LineInfo>, Vec2) {
    // share fonts between invocations in the same process
    static FONTS: LazyLock<TestFonts> = LazyLock::new(|| read_fonts());

    let normal = &FONTS.normal_font;
    let bold = &FONTS.bold_font;

    // layout params used by the MessageLayer
    let layout_params = LayoutParams {
        layout_width: 1500.0,
        text_alignment,
        line_padding_above: 0.0,
        line_padding_below: 0.0,
        line_padding_between: 4.0,
        rubi_size: 20.0,
        text_size: 50.0,
        base_font_horizontal_scale: 0.9697,
        follow_kinsoku_shori_rules: true,
        always_leave_space_for_rubi: true,
        perform_soft_breaks: true,
    };
    let defaults = MessageTextLayouterDefaults {
        color: 999,
        draw_speed: 80,
        fade: 200,
    };

    let mut layouter =
        MessageLayerLayouter::new(normal, bold, messagebox_type, layout_params, defaults);
    let parser = MessageTextParser::new(text);
    parser.parse_into(&mut layouter);

    let (mut commands, lines, size) = layouter.finish();
    // Native layout captures store ARGB words. Production UnormColor stores
    // RGBA bytes for GPU consumption; convert only this fixture representation.
    // Keep the independently captured snapshots and CBOR payloads unchanged.
    for command in &mut commands {
        if let Command::Char(character) = command {
            let rgba = character.color.0;
            character.color.0 = (rgba & 0xff00ff00) | ((rgba & 0xff) << 16) | ((rgba >> 16) & 0xff);
        }
    }
    (commands, lines, size)
}

#[test]
fn disabled_kinsoku_allows_a_ruby_base_to_cross_a_soft_break() {
    if !fonts_available() {
        return;
    }
    use crate::layout::MessageTextLayouter;
    let fonts = read_fonts();
    let layout = |kinsoku| {
        let mut layouter = MessageTextLayouter::new(
            &fonts.normal_font,
            &fonts.bold_font,
            LayoutParams {
                layout_width: 150.0,
                text_size: 50.0,
                follow_kinsoku_shori_rules: kinsoku,
                ..Default::default()
            },
            MessageTextLayouterDefaults {
                color: 999,
                draw_speed: 80,
                fade: 200,
            },
        );
        MessageTextParser::new("前前@bかな.@<漢字@>後").parse_into(&mut layouter);
        layouter
            .finish()
            .0
            .into_iter()
            .filter_map(|command| match command {
                Command::Char(c) if !c.is_rubi => Some((c.codepoint, c.line_index)),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        layout(false),
        [('前', 0), ('前', 0), ('漢', 0), ('字', 1), ('後', 1)]
    );
    assert_eq!(
        layout(true),
        [('前', 0), ('前', 0), ('漢', 1), ('字', 1), ('後', 1)]
    );
}

#[test]
fn zero_message_color_and_default_reset_preserve_native_command_words() {
    if !fonts_available() {
        return;
    }
    use crate::layout::MessageTextLayouter;
    let fonts = read_fonts();
    for (default, expected_default) in [(-1, 0), (0, 0), (900, 0xff0000ff)] {
        let mut layouter = MessageTextLayouter::new(
            &fonts.normal_font,
            &fonts.bold_font,
            LayoutParams::default(),
            MessageTextLayouterDefaults {
                color: default,
                draw_speed: 80,
                fade: 200,
            },
        );
        MessageTextParser::new("既@c0.零@c900.赤@c.既").parse_into(&mut layouter);
        let colors: Vec<_> = layouter
            .finish()
            .0
            .into_iter()
            .filter_map(|command| match command {
                Command::Char(c) => Some(c.color.0),
                _ => None,
            })
            .collect();
        assert_eq!(colors, [expected_default, 0, 0xff0000ff, expected_default]);
    }
}

#[test]
fn click_wait_resumes_after_the_longest_parallel_branch() {
    if !fonts_available() {
        return;
    }
    let fonts = read_fonts();
    let (commands, _, _) = MessageLayerLayouter::new(
        &fonts.normal_font,
        &fonts.bold_font,
        MessageboxType::Ushiromiya,
        LayoutParams {
            layout_width: 1500.0,
            text_size: 50.0,
            ..Default::default()
        },
        MessageTextLayouterDefaults {
            color: 999,
            draw_speed: 80,
            fade: 200,
        },
    )
    .parse("@rあいうえお@t短@k次");
    let wait = commands
        .iter()
        .find_map(|command| match command {
            Command::Wait(wait) if !wait.is_last_wait => Some(wait),
            _ => None,
        })
        .unwrap();
    let next = commands
        .iter()
        .find_map(|command| match command {
            Command::Char(c) if c.codepoint == '次' => Some(c),
            _ => None,
        })
        .unwrap();
    assert_eq!(next.time.to_bits(), (wait.time + 0.001).to_bits());
    assert!(
        wait.time
            > commands
                .iter()
                .find_map(|command| match command {
                    Command::Char(c) if c.codepoint == '短' => Some(c.time),
                    _ => None,
                })
                .unwrap()
    );
}

#[test]
fn backlog_finalization_keeps_the_closing_bracket_at_the_final_wait_time() {
    if !fonts_available() {
        return;
    }
    use crate::layout::BacklogTextLayouter;
    let fonts = read_fonts();
    let (commands, _, _) = BacklogTextLayouter::new(
        &fonts.normal_font,
        &fonts.bold_font,
        LayoutParams {
            layout_width: 1480.0,
            text_size: 40.0,
            base_font_horizontal_scale: f32::from_bits(0x3f783e42),
            ..Default::default()
        },
        MessageTextLayouterDefaults {
            color: 999,
            draw_speed: 50,
            fade: 200,
        },
    )
    .parse("名前");
    let closing = commands
        .iter()
        .find_map(|command| match command {
            Command::Char(c) if c.codepoint == '】' => Some(c),
            _ => None,
        })
        .unwrap();
    let wait = commands
        .iter()
        .find_map(|command| match command {
            Command::Wait(wait) if wait.is_last_wait => Some(wait),
            _ => None,
        })
        .unwrap();
    assert_eq!(closing.time.to_bits(), wait.time.to_bits());
    assert_eq!(
        commands
            .iter()
            .filter(|c| matches!(c, Command::Wait(_)))
            .count(),
        1
    );
}
