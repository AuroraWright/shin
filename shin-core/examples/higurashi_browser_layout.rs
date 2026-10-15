//! Stream the older backlog layout and separately collected voice paths.
use shin_core::{
    format::font::read_font_metrics,
    layout::{HigurashiBacklogLayouter, HigurashiBacklogVoices, commands::Command},
};
use std::{
    fs::File,
    io::{self, BufRead, BufReader, Write},
};
fn main() -> anyhow::Result<()> {
    let font = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("Expected FNT path"))?;
    let font = read_font_metrics(&mut BufReader::new(File::open(font)?))?;
    let mut output = io::BufWriter::new(io::stdout().lock());
    for input in io::stdin().lock().lines() {
        let input = input?;
        let mut words = input.split_whitespace();
        let style: i32 = words.next().unwrap().parse()?;
        let flags: u32 = words.next().unwrap().parse()?;
        anyhow::ensure!(
            (0..=255).contains(&style) && flags <= 9,
            "Input owner/style bound"
        );
        let mut params = [0u32; 8];
        for value in &mut params {
            *value = u32::from_str_radix(words.next().unwrap(), 16)?;
        }
        anyhow::ensure!(
            params
                == [
                    0x44b40000, 0, 0x40c00000, 0x40800000, 0, 0, 0x42340000, 0x3f800000
                ],
            "Native backlog parameters"
        );
        let color: i32 = words.next().unwrap().parse()?;
        let speed: i32 = words.next().unwrap().parse()?;
        let fade: i32 = words.next().unwrap().parse()?;
        let initial: u8 = words.next().unwrap().parse()?;
        anyhow::ensure!(speed == 50 && fade == 200, "Native backlog defaults");
        let hex = words.next().unwrap_or("");
        anyhow::ensure!(hex.len() % 2 == 0 && hex.len() <= 16384, "Text bound");
        let text: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&hex[at..at + 2], 16))
            .collect::<Result<_, _>>()?;
        let text = std::str::from_utf8(&text)?;
        let voices = if flags & 1 != 0 {
            HigurashiBacklogVoices::Count(initial)
        } else {
            HigurashiBacklogVoices::Collect
        };
        let layout = HigurashiBacklogLayouter::new(
            &font,
            if flags >> 1 == 0 { style } else { 0 },
            color,
            voices,
        )
        .parse(text);
        let (commands, lines, size) = (layout.commands, layout.lines, layout.size);
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
                        0,
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
        row.extend([
            if flags & 1 != 0 {
                u32::from(layout.voice_count)
            } else {
                u32::from(initial)
            },
            layout.voices.len() as u32,
        ]);
        for voice in layout.voices {
            row.extend([voice.volume.to_bits(), voice.path.len() as u32]);
            row.extend(voice.path.bytes().map(u32::from));
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
