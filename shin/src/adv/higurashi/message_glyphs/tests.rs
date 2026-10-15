use super::*;
use glam::Mat4;
use shin_core::{format::font::read_font_metrics, layout::font::FontMetrics};
use std::{
    io::{BufReader, Write},
    process::{Command as Process, Stdio},
};

pub(in crate::adv::higurashi) mod gpu;

pub(in crate::adv::higurashi) struct Packet {
    vertices: Vec<[u32; 20]>,
    borders: Vec<[u32; 16]>,
    draws: Vec<Draw>,
}
struct Draw {
    program: u32,
    glyph: usize,
    start: usize,
    matrix: [u32; 16],
    extra: Vec<u32>,
    color: [u32; 4],
}

pub(in crate::adv::higurashi) fn native(input: &str) -> Vec<Packet> {
    let mut child = Process::new(std::env::var("SHIN_HIGURASHI_NATIVE_MESSAGE_RENDER").unwrap())
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
    let input = input.to_owned();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let result = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        result.status.success(),
        "native message renderer: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stderr).contains("balanced ownership"));
    parse_packets(&String::from_utf8(result.stdout).unwrap())
}

fn parse_packets(output: &str) -> Vec<Packet> {
    output
        .lines()
        .map(|line| {
            let mut words = line.split_whitespace();
            assert_eq!(words.next(), Some("R"));
            let number = |words: &mut std::str::SplitWhitespace<'_>| {
                words.next().unwrap().parse::<u32>().unwrap()
            };
            let count = number(&mut words);
            number(&mut words);
            number(&mut words);
            let mut packet = Packet {
                vertices: Vec::new(),
                borders: Vec::new(),
                draws: Vec::new(),
            };
            for _ in 0..count {
                assert_eq!(words.next(), Some("C"));
                for _ in 0..16 {
                    number(&mut words);
                }
                packet
                    .borders
                    .push(std::array::from_fn(|_| number(&mut words)));
                assert_eq!(words.next(), Some("V"));
                packet
                    .vertices
                    .push(std::array::from_fn(|_| number(&mut words)));
            }
            loop {
                match words.next().unwrap() {
                    "D" => {
                        let program = number(&mut words);
                        let glyph = number(&mut words) as usize;
                        let start = number(&mut words) as usize;
                        let matrix = std::array::from_fn(|_| number(&mut words));
                        let extra = (0..if program == 4 { 16 } else { 4 })
                            .map(|_| number(&mut words))
                            .collect();
                        let color = std::array::from_fn(|_| number(&mut words));
                        packet.draws.push(Draw {
                            program,
                            glyph,
                            start,
                            matrix,
                            extra,
                            color,
                        });
                    }
                    "E" => {
                        assert_eq!(number(&mut words), 1);
                        assert_eq!(number(&mut words), packet.draws.len() as u32);
                        assert_eq!(number(&mut words), 0);
                        break;
                    }
                    tag => panic!("unexpected native tag{tag}"),
                }
            }
            assert!(words.next().is_none());
            packet
        })
        .collect()
}

pub(in crate::adv::higurashi) fn row(
    style: i32,
    outline: u8,
    options: [i32; 2],
    progress: f32,
    text: &str,
) -> String {
    let hex = text
        .as_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    format!(
        "{style} {outline} {} {} {progress} {hex}\n",
        options[0], options[1]
    )
}
fn vertex_words(vertices: &[TextVertex; 4]) -> [u32; 20] {
    let words: Vec<_> = vertices
        .iter()
        .flat_map(|v| {
            let position = v.position;
            position.to_array().into_iter().chain([v.color])
        })
        .map(f32::to_bits)
        .collect();
    words.try_into().unwrap()
}

#[test]
#[ignore = "executes updated CPU message record/vertex/draw paths with supplied newrodin"]
fn higurashi_native_message_vertices_outlines_and_packets() {
    let font = read_font_metrics(&mut BufReader::new(
        std::fs::File::open(std::env::var("SHIN_HIGURASHI_FONT").unwrap()).unwrap(),
    ))
    .unwrap();
    let texts = [
        "@r本文",
        "名前@r@bかな.@<漢字@>、続く。",
        "@r@c$F00.赤@c$0F0.緑@c$00F.青",
        "@r@z10.小@z200.大@z.戻",
        "@r@vfoo.声@x20.次@/後@y同期@|節",
        "@r……もやがかったような意識のすぐそばで、いびき交じりの寝息が聞こえてくる。",
    ];
    let mut cases = Vec::new();
    let mut input = String::new();
    for text in texts {
        for style in 0..5 {
            for outline in [0, 1, 2] {
                for progress in [0.0, 0.5, 1.0] {
                    for options in [[0, 0], [-1, 2], [i32::MAX, i32::MIN]] {
                        input.push_str(&row(style, outline, options, progress, text));
                        cases.push((text, style, outline, progress, options));
                    }
                }
            }
        }
    }
    let packets = native(&input);
    assert_eq!(packets.len(), cases.len());
    for (index, ((text, style, outline, progress, options), packet)) in
        cases.into_iter().zip(packets).enumerate()
    {
        let (commands, lines, size) =
            HigurashiMessageLayouter::for_message(&font, style, 50).parse(text);
        let chars: Vec<_> = commands
            .iter()
            .filter_map(|command| match command {
                Command::Char(c) => Some(c),
                _ => None,
            })
            .collect();
        assert_eq!(packet.vertices.len(), chars.len());
        for (i, c) in chars.iter().enumerate() {
            let info = font.get_glyph_info(c.codepoint).unwrap();
            assert_eq!(
                vertex_words(&glyph_vertices(c, &lines[c.line_index], &info)),
                packet.vertices[i],
                "case{index} glyph{i} vertices"
            );
            let border = outline::distances_with_radius(
                vec2(c.horizontal_scale, c.scale),
                info.actual_size_f32(),
                info.actual_size_normalized(),
                outline,
                1.5,
            );
            let words: Vec<_> = border
                .iter()
                .flat_map(|v| v.to_array())
                .map(f32::to_bits)
                .collect();
            assert_eq!(
                words.as_slice(),
                packet.borders[i],
                "case{index} glyph{i} outline"
            );
        }
        let style = HigurashiMessageStyle::from_msginit(style);
        let expected_draws = if progress == 0.0 || style == HigurashiMessageStyle::Mode2 {
            0
        } else {
            chars.len()
                * if style == HigurashiMessageStyle::Novel {
                    1
                } else {
                    2
                }
        };
        assert_eq!(packet.draws.len(), expected_draws, "case{index} draw count");
        let matrix = HigurashiTransform::default()
            .compute_native_message_transform(message_offset(style, options, 1.0, 357.0, size))
            .to_cols_array()
            .map(f32::to_bits);
        for (i, draw) in packet.draws.iter().enumerate() {
            let border = style != HigurashiMessageStyle::Novel && i < chars.len();
            let glyph = i % chars.len();
            assert_eq!(draw.program, if border { 4 } else { 3 });
            assert_eq!(draw.glyph, glyph + 1);
            assert_eq!(draw.start, glyph * 4);
            assert_eq!(draw.matrix, matrix, "case{index} draw matrix");
            let color = if border {
                FloatColor4::from_rgba(0.0, 0.0, 0.0, progress)
            } else {
                FloatColor4::from_unorm(chars[glyph].color).with_alpha(progress)
            };
            let words = [
                color.r.to_bits(),
                color.g.to_bits(),
                color.b.to_bits(),
                color.a.to_bits(),
            ];
            assert_eq!(draw.color, words, "case{index} draw color");
            if border {
                assert_eq!(draw.extra.as_slice(), packet.borders[glyph]);
            } else {
                assert_eq!(draw.extra.as_slice(), words);
            }
        }
    }
    println!(
        "Higurashi native message:810 cases match glyph vertices,1.5px outlines, style/offset transforms, draw order and color/progress packets"
    );
}
