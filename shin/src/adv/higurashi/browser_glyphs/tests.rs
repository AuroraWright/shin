use super::*;
use shin_core::format::{font::read_font_metrics, text::write_sjis_string};
use std::{
    io::{BufReader, Write},
    process::{Command as Process, Stdio},
};

mod gpu;

struct Draw {
    program: u32,
    glyph: usize,
    matrix: [u32; 16],
    extra: Vec<u32>,
    color: [u32; 4],
    vertices: [u32; 20],
}
struct Packet {
    glyphs: Vec<(char, u32, [u32; 20], [u32; 16])>,
    draws: Vec<Draw>,
    voices: Vec<(u32, String)>,
    voice_count: u8,
    height: u32,
    cached: usize,
    prepared: usize,
}

fn make_row(style: i32, kind: u32, argument: i32, text: &str) -> Row {
    let item = match kind {
        0 => Item::Message {
            text: text.into(),
            style,
            argument: Some(argument),
        },
        1 => Item::Choice {
            title: "選択".into(),
            text: text.into(),
        },
        2 => Item::Checkpoint {
            title: "章".into(),
            text: text.into(),
        },
        3 => Item::CharacterChoice { response: argument },
        _ => panic!("row kind"),
    };
    Row {
        boundary: 1,
        index: 0,
        item,
    }
}
fn input(row: &Row, outline: u8, top: f32, scroll: f32, slide: f32) -> String {
    let (style, kind, argument, text) = match &row.item {
        Item::Message {
            style,
            argument,
            text,
        } => (*style, 0, argument.unwrap_or(9), text.as_str()),
        Item::Choice { text, .. } => (0, 1, 0, text.as_str()),
        Item::Checkpoint { text, .. } => (0, 2, 0, text.as_str()),
        Item::CharacterChoice { response } => (0, 3, *response, ""),
    };
    let mut bytes = Vec::new();
    write_sjis_string(text, &mut bytes).unwrap();
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{style} {kind} {outline} {argument} {top} {scroll} {slide} {}\n",
        if hex.is_empty() { "-" } else { &hex }
    )
}
fn native(input: &str) -> Vec<Packet> {
    let mut child = Process::new(std::env::var("SHIN_HIGURASHI_NATIVE_BROWSER_GLYPHS").unwrap())
        .args([
            std::env::var("SHIN_HIGURASHI_EXE").unwrap(),
            std::env::var("SHIN_HIGURASHI_FONT").unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let data = input.to_owned();
    let writer = std::thread::spawn(move || stdin.write_all(data.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    if let Ok(directory) = std::env::var("SHIN_HIGURASHI_BROWSER_PACKET_CAPTURE") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("input.txt"), input).unwrap();
        std::fs::write(directory.join("native.txt"), &output.stdout).unwrap();
        std::fs::write(directory.join("native.log"), &output.stderr).unwrap();
    }
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("balanced cache/glyph/font ownership")
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| {
            let mut words = line.split_whitespace();
            assert_eq!(words.next(), Some("R"));
            let number =
                |w: &mut std::str::SplitWhitespace<'_>| w.next().unwrap().parse::<u32>().unwrap();
            let mut packet = Packet {
                glyphs: Vec::new(),
                draws: Vec::new(),
                voices: Vec::new(),
                voice_count: 0,
                height: 0,
                cached: 0,
                prepared: 0,
            };
            while let Some(tag) = words.next() {
                match tag {
                    "P" => {
                        let program = number(&mut words);
                        let glyph = number(&mut words) as usize;
                        let matrix = std::array::from_fn(|_| number(&mut words));
                        let extra = (0..if program == 4 { 16 } else { 4 })
                            .map(|_| number(&mut words))
                            .collect();
                        let color = std::array::from_fn(|_| number(&mut words));
                        let vertices = std::array::from_fn(|_| number(&mut words));
                        packet.draws.push(Draw {
                            program,
                            glyph,
                            matrix,
                            extra,
                            color,
                            vertices,
                        });
                    }
                    "S" => {
                        packet.cached = number(&mut words) as usize;
                        packet.voice_count = number(&mut words) as u8;
                        packet.height = number(&mut words);
                    }
                    "G" => {
                        for _ in 0..number(&mut words) {
                            assert_eq!(words.next(), Some("C"));
                            let c = char::from_u32(number(&mut words)).unwrap();
                            let color = number(&mut words);
                            let v = std::array::from_fn(|_| number(&mut words));
                            let b = std::array::from_fn(|_| number(&mut words));
                            packet.glyphs.push((c, color, v, b));
                        }
                    }
                    "A" => {
                        for _ in 0..number(&mut words) {
                            let volume = number(&mut words);
                            let n = number(&mut words);
                            let b = (0..n).map(|_| number(&mut words) as u8).collect();
                            packet.voices.push((volume, String::from_utf8(b).unwrap()));
                        }
                    }
                    "E" => {
                        packet.prepared = number(&mut words) as usize;
                        assert_eq!(number(&mut words) as usize, packet.draws.len());
                        number(&mut words);
                        assert!(words.next().is_none());
                        break;
                    }
                    _ => panic!("tag{tag}"),
                }
            }
            packet
        })
        .collect()
}
fn vertex_words(v: &[TextVertex; 4]) -> [u32; 20] {
    v.iter()
        .flat_map(|v| {
            let position = v.position;
            position.to_array().into_iter().chain([v.color])
        })
        .map(f32::to_bits)
        .collect::<Vec<_>>()
        .try_into()
        .unwrap()
}
fn border_words(v: &[Vec2; 8]) -> [u32; 16] {
    v.iter()
        .flat_map(|v| v.to_array())
        .map(f32::to_bits)
        .collect::<Vec<_>>()
        .try_into()
        .unwrap()
}
fn color_words(c: FloatColor4) -> [u32; 4] {
    c.into_vec4().to_array().map(f32::to_bits)
}

#[test]
#[ignore = "executes updated browser glyph/cache/draw/cull code with supplied single font"]
fn higurashi_browser_glyphs_match_native_geometry_voice_paths_and_cache_packets() {
    let font = read_font_metrics(&mut BufReader::new(
        std::fs::File::open(std::env::var("SHIN_HIGURASHI_FONT").unwrap()).unwrap(),
    ))
    .unwrap();
    let mut texts = vec![
        "".to_owned(),
        "@r".into(),
        "名前@r本文".into(),
        "@r@r本文".into(),
        "@r@c$F00.赤@c$0F0.緑@a50.@c$00F.青".into(),
        "名前@r@bかな.@<漢字@>、続く。".into(),
        "@r@z10.小@z200.大@z.戻".into(),
        "@r@o50.@va.声@x20.次@/後@y同期@|節@v.".into(),
        "@r　字　続".into(),
        format!("@r「{}」@r次", "あいうえお".repeat(12)),
    ];
    let synthetic = texts.len();
    for line in std::fs::read_to_string(std::env::var("SHIN_HIGURASHI_LISTING").unwrap())
        .unwrap()
        .lines()
        .filter(|l| l.contains("MSGSET {"))
    {
        let Some((_, value)) = line.split_once("text: ScriptString(") else {
            continue;
        };
        let Some(value) = value.strip_suffix(") })") else {
            continue;
        };
        if let Ok(text) = serde_json::from_str::<String>(value) {
            if (25..=200).contains(&text.chars().count()) && !texts.contains(&text) {
                texts.push(text);
            }
        }
        if texts.len() == synthetic + 32 {
            break;
        }
    }
    assert_eq!(texts.len(), synthetic + 32, "authored corpus");
    let mut cases = Vec::new();
    let mut data = String::new();
    for (n, text) in texts.iter().enumerate() {
        let styles: &[i32] = if n < synthetic {
            &[0, 3, 4, 0x13, 15]
        } else {
            &[0, 3]
        };
        for &style in styles {
            for outline in [0, 1, 2] {
                for (top, scroll, slide) in [
                    (0.0, 0.0, 1.0),
                    (20.0, 30.0, 0.5),
                    (-500.0, 0.0, 0.0),
                    (1200.0, 0.0, 1.0),
                ] {
                    let row = make_row(style, 0, if n % 2 == 0 { 9 } else { 0 }, text);
                    data.push_str(&input(&row, outline, top, scroll, slide));
                    cases.push((row, outline, top, scroll, slide));
                }
            }
        }
    }
    for kind in 1..=3 {
        for argument in 0..if kind == 3 { 4 } else { 1 } {
            for outline in [0, 1, 2] {
                for (top, scroll, slide) in [(0.0, 0.0, 1.0), (20.0, 30.0, 0.5), (-500.0, 0.0, 0.0)]
                {
                    let row = make_row(0x13, kind, argument, "本文@r@o50.@va.声@c$F00.赤@a50.薄");
                    data.push_str(&input(&row, outline, top, scroll, slide));
                    cases.push((row, outline, top, scroll, slide));
                }
            }
        }
    }
    let packets = native(&data);
    assert_eq!(packets.len(), cases.len());
    let count = cases.len();
    let mut glyph_count = 0;
    for (index, ((row, outline, top, scroll, slide), p)) in
        cases.into_iter().zip(packets).enumerate()
    {
        let measured = PositionedRow::measure(row.clone(), top, &font).unwrap();
        assert_eq!(p.height, measured.height.to_bits(), "case{index} height");
        assert_eq!(
            p.voice_count, measured.voice_count,
            "case{index} voice count"
        );
        let parsed = layout(&row, &font, HigurashiBacklogVoices::Collect).unwrap();
        let chars: Vec<_> = parsed
            .commands
            .into_iter()
            .filter_map(|c| match c {
                Command::Char(c) => Some(c),
                _ => None,
            })
            .collect();
        if !measured.is_visible(scroll) {
            assert_eq!(p.cached, 0, "case{index} culled");
            assert!(p.draws.is_empty() && p.glyphs.is_empty());
            continue;
        }
        assert_eq!(p.cached, 1);
        assert_eq!(
            p.prepared,
            chars.len(),
            "case{index} retained glyph handles"
        );
        assert_eq!(p.glyphs.len(), chars.len(), "case{index} glyphs");
        assert_eq!(
            p.draws.len(),
            4 * chars.len(),
            "case{index} two retained frames"
        );
        assert_eq!(
            p.voices,
            parsed
                .voices
                .into_iter()
                .map(|v| (v.volume.to_bits(), v.path))
                .collect::<Vec<_>>()
        );
        let matrix = native_transform(top, scroll, slide)
            .to_cols_array()
            .map(f32::to_bits);
        for (i, (c, (point, color, v, b))) in chars.iter().zip(&p.glyphs).enumerate() {
            let info = font.get_glyph_info(c.codepoint).unwrap();
            assert_eq!(*point, c.codepoint);
            let rgba = c.color.0;
            let argb = (rgba & 0xff00ff00) | ((rgba & 0xff) << 16) | ((rgba >> 16) & 0xff);
            assert_eq!(*color, argb);
            assert_eq!(
                vertex_words(&glyph_vertices(
                    c,
                    &info,
                    font.get_ascent().into(),
                    font.get_descent().into()
                )),
                *v,
                "case{index} glyph{i} vertices"
            );
            assert_eq!(
                border_words(&outline::distances_with_radius(
                    vec2(c.horizontal_scale, c.scale),
                    info.actual_size_f32(),
                    info.actual_size_normalized(),
                    outline,
                    1.5
                )),
                *b,
                "case{index} glyph{i} borders"
            );
        }
        for (i, draw) in p.draws.iter().enumerate() {
            let border = i % (2 * chars.len()) < chars.len();
            let glyph = i % chars.len();
            let (_, _, v, b) = p.glyphs[glyph];
            assert_eq!(draw.program, if border { 4 } else { 3 });
            assert_eq!(draw.glyph, glyph + 1);
            assert_eq!(draw.matrix, matrix, "case{index} matrix");
            assert_eq!(draw.vertices, v);
            let color = color_words(glyph_color(chars[glyph].color));
            let color1 = if matches!(row.item, Item::Message { .. }) {
                color_words(FloatColor4::from_rgba(1.0, 1.0, 1.0, 1.0))
            } else {
                color
            };
            assert_eq!(
                draw.extra,
                if border { b.to_vec() } else { color1.to_vec() },
                "case{index} color1/border"
            );
            assert_eq!(
                draw.color,
                if border {
                    color_words(FloatColor4::from_rgba(0.0, 0.0, 0.0, 1.0))
                } else {
                    color
                },
                "case{index} color2"
            );
        }
        glyph_count += chars.len();
    }
    println!(
        "Higurashi browser glyphs:{count} native cases/{glyph_count} exact glyph records; retained redraw and cache retirement pass;32 authored messages"
    );
}
