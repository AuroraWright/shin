//! Stream the production message layouter in the native oracle's bitwise format.
use std::{
    fs::File,
    io::{self, BufRead, BufReader, Write},
};

use shin_core::{
    format::font::read_font_metrics,
    layout::{
        BacklogTextLayouter, LayoutParams, MessageLayerLayouter, MessageTextLayouter,
        MessageTextLayouterDefaults,
        commands::{CharFontType, Command},
    },
    vm::command::types::{MessageTextLayout, MessageboxType},
};

fn main() -> anyhow::Result<()> {
    let paths: Vec<_> = std::env::args().skip(1).collect();
    anyhow::ensure!(
        paths.len() == 2 || paths.len() == 3,
        "Expected regular/bold and optional system FNT paths"
    );
    let regular = read_font_metrics(&mut BufReader::new(File::open(&paths[0])?))?;
    let bold = read_font_metrics(&mut BufReader::new(File::open(&paths[1])?))?;
    let system = paths
        .get(2)
        .map(|path| read_font_metrics(&mut BufReader::new(File::open(path)?)))
        .transpose()?;
    let mut output = io::BufWriter::new(io::stdout().lock());
    for input in io::stdin().lock().lines() {
        let input = input?;
        let mut words = input.split_whitespace();
        let style_number: u32 = words.next().unwrap().parse()?;
        let style = match style_number {
            0 => MessageboxType::Neutral,
            1 => MessageboxType::WitchSpace,
            2 => MessageboxType::Ushiromiya,
            3 => MessageboxType::Transparent,
            4 => MessageboxType::Novel,
            5 => MessageboxType::NoText,
            6..=9 => MessageboxType::Neutral,
            _ => anyhow::bail!("Invalid style"),
        };
        let flags: u32 = words.next().unwrap().parse()?;
        let mut values = [0u32; 8];
        for value in &mut values {
            *value = u32::from_str_radix(words.next().unwrap(), 16)?;
        }
        let alignment = match values[1] {
            0 => MessageTextLayout::Justify,
            1 => MessageTextLayout::Left,
            2 => MessageTextLayout::Center,
            3 => MessageTextLayout::Right,
            _ => anyhow::bail!("Invalid alignment"),
        };
        let color = words.next().unwrap().parse()?;
        let draw_speed = words.next().unwrap().parse()?;
        let fade = words.next().unwrap().parse()?;
        let hex = words.next().unwrap_or("");
        anyhow::ensure!(hex.len() % 2 == 0 && hex.len() <= 16384, "Text bound");
        let text: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&hex[at..at + 2], 16))
            .collect::<Result<_, _>>()?;
        let text = std::str::from_utf8(&text)?;
        let parameters = LayoutParams {
            layout_width: f32::from_bits(values[0]),
            text_alignment: alignment,
            line_padding_above: f32::from_bits(values[2]),
            line_padding_below: f32::from_bits(values[3]),
            line_padding_between: f32::from_bits(values[4]),
            rubi_size: f32::from_bits(values[5]),
            text_size: f32::from_bits(values[6]),
            base_font_horizontal_scale: f32::from_bits(values[7]),
            follow_kinsoku_shori_rules: flags & 1 != 0,
            always_leave_space_for_rubi: flags & 2 != 0,
            perform_soft_breaks: flags & 4 != 0,
        };
        let defaults = MessageTextLayouterDefaults {
            color,
            draw_speed,
            fade,
        };
        let (commands, lines, size) = if style_number == 6 {
            BacklogTextLayouter::new(&regular, &bold, parameters, defaults).parse(text)
        } else if style_number >= 7 {
            let font = match style_number {
                7 => &regular,
                8 => &bold,
                9 => system
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("System font required"))?,
                _ => unreachable!(),
            };
            MessageTextLayouter::new(font, font, parameters, defaults).parse(text)
        } else {
            MessageLayerLayouter::new(&regular, &bold, style, parameters, defaults).parse(text)
        };
        let mut row = vec![
            commands.len() as u32,
            lines.len() as u32,
            size.x.to_bits(),
            size.y.to_bits(),
        ];
        for command in commands {
            let time = command.time().to_bits();
            match command {
                Command::Wait(v) => row.extend([
                    0,
                    time,
                    v.line_index as u32,
                    v.is_last_wait as u32,
                    v.is_auto_click as u32,
                ]),
                Command::Char(v) => {
                    // Native stores ARGB; production uses little-endian RGBA.
                    let color = v.color.0;
                    let argb =
                        (color & 0xff00ff00) | ((color & 0xff) << 16) | ((color >> 16) & 0xff);
                    row.extend([
                        1,
                        time,
                        v.line_index as u32,
                        // Generic native text shares one font in both roles.
                        // Compare the actual font resource, not the logical enum.
                        if style_number >= 7 {
                            style_number - 7
                        } else {
                            (v.font == CharFontType::Bold) as u32
                        },
                        v.codepoint as u32,
                        v.is_rubi as u32,
                        v.cant_be_at_line_start as u32,
                        v.cant_be_at_line_end as u32,
                        v.has_rubi as u32,
                        v.width.to_bits(),
                        v.height.to_bits(),
                        v.position.x.to_bits(),
                        v.position.y.to_bits(),
                        v.horizontal_scale.to_bits(),
                        v.scale.to_bits(),
                        argb,
                        v.fade.to_bits(),
                    ]);
                }
                Command::Section(v) => row.extend([2, time, v.line_index as u32, v.index]),
                Command::Sync(v) => row.extend([3, time, v.line_index as u32, v.index]),
                Command::Voice(v) => {
                    row.extend([4, time, v.line_index as u32, v.filename.len() as u32]);
                    row.extend(v.filename.bytes().map(u32::from));
                    row.extend([
                        v.volume.to_bits(),
                        v.lipsync_enabled as u32,
                        v.segment_duration,
                    ]);
                }
                Command::VoiceSync(v) => row.extend([
                    5,
                    time,
                    v.line_index as u32,
                    v.segment_start,
                    v.segment_duration,
                ]),
                Command::VoiceWait(v) => row.extend([6, time, v.line_index as u32]),
            }
        }
        for line in lines {
            row.extend([
                line.width.to_bits(),
                line.y_position.to_bits(),
                line.line_height.to_bits(),
                line.baseline_ascent.to_bits(),
                line.rubi_height.to_bits(),
            ]);
        }
        for (index, value) in row.iter().enumerate() {
            if index != 0 {
                write!(output, " ")?;
            }
            write!(output, "{value}")?;
        }
        writeln!(output)?;
    }
    Ok(())
}
