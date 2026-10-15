use super::*;
use std::{
    io::{Read, Write},
    process::{Command as Process, Stdio},
};

use crate::layout::message_text_layouter::{
    LayoutParams, MessageTextLayouterDefaults, MessageTextLayouterImpl, commands::Command,
};

fn defaults() -> MessageTextLayouterDefaults {
    MessageTextLayouterDefaults {
        color: 999,
        draw_speed: 100,
        fade: 0,
    }
}
fn header(words: [u16; 6]) -> Gerokasu2FontHeader {
    Gerokasu2FontHeader {
        version: 1,
        file_size: 0,
        flags: 0,
        page_count: 0,
        glyph_count: 0,
        metric_words: words,
    }
}
fn fixture() -> Gerokasu2Font {
    let mut bytes = vec![0u8; 32 + 4 * 514 + 4 * 24 + 4];
    bytes[..4].copy_from_slice(b"FNT6");
    bytes[4..8].copy_from_slice(&1u32.to_le_bytes());
    let length = bytes.len() as u32;
    bytes[8..12].copy_from_slice(&length.to_le_bytes());
    bytes[16..18].copy_from_slice(&4u16.to_le_bytes());
    bytes[18..20].copy_from_slice(&4u16.to_le_bytes());
    for (i, word) in [20u16, 0, 70, 10, 41, 0].into_iter().enumerate() {
        bytes[20 + i * 2..22 + i * 2].copy_from_slice(&word.to_le_bytes());
    }
    for (i, (tag, low, id)) in [
        (0u16, 65usize, 0u16),
        (0x20, 0x3b, 3),
        (0x30, 0x42, 1),
        (0x1f4, 8, 2),
    ]
    .into_iter()
    .enumerate()
    {
        let at = 32 + i * 514;
        bytes[at..at + 2].copy_from_slice(&tag.to_le_bytes());
        bytes[at + 2..at + 514].fill(0xff);
        bytes[at + 2 + low * 2..at + 4 + low * 2].copy_from_slice(&id.to_le_bytes());
    }
    for (i, advance) in [205u16, 407, 333, 257].into_iter().enumerate() {
        let at = 32 + 4 * 514 + i * 24;
        bytes[at + 4..at + 6].copy_from_slice(&1u16.to_le_bytes());
        bytes[at + 6..at + 8].copy_from_slice(&1u16.to_le_bytes());
        bytes[at + 8..at + 10].copy_from_slice(&advance.to_le_bytes());
        bytes[at + 12..at + 16].copy_from_slice(&(1000 + i as u32).to_le_bytes());
        bytes[at + 20..at + 24].copy_from_slice(&(length - 4 + i as u32).to_le_bytes());
    }
    Gerokasu2Font::new(bytes.into()).unwrap()
}

#[test]
fn gerokasu2_fractional_metrics_reach_shared_layout_and_legacy_adapter_is_exact() {
    let font = Arc::new(fixture());
    assert_eq!(font.glyph_advance('A'), Some(12.8125));
    assert_eq!(font.glyph_advance('あ'), Some(25.4375));
    assert_eq!(font.glyph_advance('🐈'), Some(20.8125));
    assert_eq!(font.glyph_advance('\u{10ffff}'), Some(16.0625));
    assert_eq!(font.ascent(), 70.0);
    assert_eq!(font.descent(), 10.0);
    let mut layout = MessageTextLayouterImpl::new(
        font.clone(),
        font.clone(),
        LayoutParams {
            text_size: 50.0,
            base_font_horizontal_scale: 0.9697,
            ..Default::default()
        },
        defaults(),
    );
    layout.on_message_start();
    layout.on_set_font_scale(90);
    layout.on_char('A');
    let Command::Char(ch) = &layout.commands[0] else {
        panic!("character");
    };
    let scale = 50.0 / 80.0 * layout.font_scale;
    assert_eq!(ch.width.to_bits(), (scale * 0.9697 * 12.8125).to_bits());
    let height = ch.height;
    layout.finalize_up_to(1, true);
    assert_eq!(
        layout.lines[0].baseline_ascent.to_bits(),
        font.baseline(height).to_bits()
    );
    let Command::Char(ch) = &layout.commands[0] else {
        panic!("character");
    };
    assert_eq!(ch.position.y.to_bits(), font.baseline(height).to_bits());
    assert_eq!((&*font).glyph_advance('A'), font.glyph_advance('A'));

    let mut ruby = MessageTextLayouterImpl::new(
        &*font,
        &*font,
        LayoutParams {
            text_size: 50.0,
            rubi_size: 20.0,
            ..Default::default()
        },
        defaults(),
    );
    ruby.on_message_start();
    ruby.on_rubi_content("🐈".into());
    ruby.on_rubi_base_start();
    ruby.on_char('あ');
    ruby.on_rubi_base_end();
    let ruby_chars = ruby
        .commands
        .iter()
        .filter_map(|cmd| match cmd {
            Command::Char(ch) if ch.is_rubi => Some(ch),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(ruby_chars.len(), 1);
    assert_eq!(
        ruby_chars[0].width.to_bits(),
        (20.0f32 / 80.0 * 20.8125).to_bits()
    );
    assert_eq!(ruby_chars[0].codepoint, '🐈');

    // The old API still works without narrowing FNT6 or changing old rounding.
    struct Legacy;
    impl super::super::FontMetrics for Legacy {
        fn get_ascent(&self) -> u32 {
            16777217
        }
        fn get_descent(&self) -> u32 {
            2
        }
        fn get_glyph_info(&self, _: char) -> Option<crate::format::font::GlyphInfo> {
            Some(crate::format::font::GlyphInfo {
                bearing_x: -2,
                bearing_y: 3,
                advance_width: 255,
                actual_width: 4,
                actual_height: 5,
                texture_width: 8,
                texture_height: 8,
            })
        }
    }
    assert_eq!(
        Legacy.line_height().to_bits(),
        (16777219u32 as f32).to_bits()
    );
    assert_eq!(Legacy.glyph_advance('A'), Some(255.0));
    assert_eq!(
        Legacy.baseline(31.23456).to_bits(),
        (31.23456f32 / (16777217u32 as f32 + 2.0) * 16777217u32 as f32).to_bits()
    );
}

#[derive(Clone, Copy)]
struct Synthetic {
    header: Gerokasu2FontHeader,
    advance: f32,
}
impl TextFontMetrics for Synthetic {
    fn ascent(&self) -> f32 {
        Gerokasu2LineMetrics::from_header(&self.header).ascent as f32
    }
    fn descent(&self) -> f32 {
        Gerokasu2LineMetrics::from_header(&self.header).descent as f32
    }
    fn line_height(&self) -> f32 {
        Gerokasu2LineMetrics::from_header(&self.header).line_height()
    }
    fn glyph_advance(&self, _: char) -> Option<f32> {
        Some(self.advance)
    }
    fn baseline(&self, height: f32) -> f32 {
        Gerokasu2LineMetrics::from_header(&self.header).baseline(height)
    }
}

struct Native {
    child: std::process::Child,
    input: Option<std::process::ChildStdin>,
    output: std::process::ChildStdout,
    cases: usize,
    order_differences: usize,
}
impl Native {
    fn new() -> Self {
        let mut child = Process::new(
            std::env::var_os("SHIN_GEROKASU2_FONT_METRICS_ORACLE").expect("metric oracle"),
        )
        .arg(std::env::var_os("SHIN_GEROKASU2_EXECUTABLE").expect("executable"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
        let mut input = child.stdin.take().unwrap();
        input.write_all(b"FMT1").unwrap();
        Self {
            input: Some(input),
            output: child.stdout.take().unwrap(),
            child,
            cases: 0,
            order_differences: 0,
        }
    }
    fn compare<F: TextFontMetrics + Clone>(
        &mut self,
        font: F,
        header: &Gerokasu2FontHeader,
        cp: char,
        text_size: f32,
        horizontal: f32,
        font_scale: f32,
        baseline_height: f32,
    ) {
        let advance = font.glyph_advance(cp).unwrap();
        let mut row = Vec::new();
        for word in header.native_owner_metrics() {
            row.extend(word.to_le_bytes());
        }
        row.extend(0u16.to_le_bytes());
        for value in [advance, text_size, horizontal, font_scale, baseline_height] {
            row.extend(value.to_le_bytes());
        }
        assert_eq!(row.len(), 36);
        self.input.as_mut().unwrap().write_all(&row).unwrap();
        let mut result = [0u8; 24];
        self.output.read_exact(&mut result).unwrap();
        assert_eq!(&result[..4], b"FMT1");
        let actual = result[4..]
            .chunks_exact(4)
            .map(|v| u32::from_le_bytes(v.try_into().unwrap()))
            .collect::<Vec<_>>();
        let mut layout = MessageTextLayouterImpl::new(
            font.clone(),
            font.clone(),
            LayoutParams {
                text_size,
                base_font_horizontal_scale: horizontal,
                ..Default::default()
            },
            defaults(),
        );
        layout.on_message_start();
        layout.font_scale = font_scale;
        layout.on_char(cp);
        let Command::Char(ch) = &layout.commands[0] else {
            panic!("character");
        };
        let expected = [
            ch.width,
            ch.height,
            ch.horizontal_scale,
            ch.scale,
            font.baseline(baseline_height),
        ]
        .map(f32::to_bits);
        assert_eq!(
            actual,
            expected,
            "native metric case{} U+{:x}",
            self.cases,
            u32::from(cp)
        );
        let old_baseline = baseline_height / (font.ascent() + font.descent()) * font.ascent();
        self.order_differences += usize::from(old_baseline.to_bits() != expected[4]);
        self.cases += 1;
    }
    fn finish(mut self) -> (usize, usize) {
        self.input.take();
        assert_eq!(self.output.read(&mut [0; 1]).unwrap(), 0);
        assert!(self.child.wait().unwrap().success());
        (self.cases, self.order_differences)
    }
}

#[test]
#[ignore = "requires supplied executable and guarded CPU font metric oracle"]
fn gerokasu2_shared_character_metrics_match_native_signed_scaling_and_baseline_order() {
    let mut native = Native::new();
    for words in [
        [20, 0, 70, 10, 41, 0],
        [15, 0, 53, 7, 33, 0],
        [65535, 0, 32767, 32768, 1, 2],
        [1, 0, 65535, 2, 3, 4],
        [1, 0, 32768, 32767, 1, 0],
    ] {
        let header = header(words);
        for raw in [0u16, 1, 205, u16::MAX] {
            let font = Synthetic {
                header,
                advance: f32::from(raw) / 16.0,
            };
            for size in [1.0, 20.0, 50.0, 99.5] {
                for horizontal in [0.9697, 0.9, 1.0, 2.0] {
                    for scale in [0.1, 0.9, 1.0, 2.0] {
                        for height in [1.0, 20.0, 50.0, 31.23456] {
                            native.compare(font, &header, 'A', size, horizontal, scale, height);
                        }
                    }
                }
            }
        }
    }
    let (cases, differences) = native.finish();
    assert!(
        differences > 0,
        "Cases must distinguish native baseline rounding order"
    );
    println!(
        "GEROKASU2_FONT_METRICS_SYNTHETIC cases={cases} words=5 baseline_order_differences={differences} original_arithmetic=verified"
    );
}

#[test]
#[ignore = "requires supplied ROM/executable and guarded CPU font metric oracle"]
fn gerokasu2_all_supplied_advances_reach_shared_layout_without_integer_conversion() {
    use crate::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
    let mut rom = RomReader::new(
        StatelessFile::new(
            std::fs::File::open(std::env::var_os("SHIN_GEROKASU2_ROM").unwrap()).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let mut native = Native::new();
    let mut mappings = 0;
    for path in ["/newrodin-medium.fnt", "/system.fnt"] {
        let file = rom.find_file(path).unwrap();
        let mut bytes = Vec::new();
        rom.open_file(file)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let font = Arc::new(Gerokasu2Font::new(bytes.into()).unwrap());
        for (cp, _) in font.codepoints() {
            let cp = char::from_u32(cp).expect("supplied mapping must be a Unicode scalar");
            for (size, horizontal, scale) in
                [(50.0, 0.9697, 1.0), (20.0, 1.0, 0.9), (42.75, 0.9, 1.37)]
            {
                native.compare(
                    font.clone(),
                    font.header(),
                    cp,
                    size,
                    horizontal,
                    scale,
                    size * scale,
                );
            }
            mappings += 1;
        }
        for cp in ['\0', '\u{10ffff}'] {
            for (size, horizontal, scale) in
                [(50.0, 0.9697, 1.0), (20.0, 1.0, 0.9), (42.75, 0.9, 1.37)]
            {
                native.compare(
                    font.clone(),
                    font.header(),
                    cp,
                    size,
                    horizontal,
                    scale,
                    size * scale,
                );
            }
        }
    }
    let (cases, differences) = native.finish();
    println!(
        "GEROKASU2_FONT_METRICS_SUPPLIED mappings={mappings} cases={cases} words=5 baseline_order_differences={differences} shared_character_layout=verified"
    );
}
