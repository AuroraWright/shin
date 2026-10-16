use super::*;
use shin_core::format::font::read_font_metrics;
use std::{
    io::{BufReader, Write},
    process::{Command, Stdio},
};
mod live;
mod pixels;
fn hex(text: &str) -> String {
    // BGM metadata is Shift-JIS. Native97020 calls c4990 before SimpleText;
    // the desktop Scenario has already decoded this metadata to Unicode.
    let mut bytes = Vec::new();
    shin_core::format::text::write_sjis_string(text, &mut bytes).unwrap();
    if bytes.is_empty() {
        "-".into()
    } else {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}

fn snapshot(clock: &MenuIndicator, title: Option<&Layout>, note: Option<&Layout>) -> Vec<u32> {
    let a = title.map_or(Vec2::ZERO, |l| l.size);
    let b = note.map_or(Vec2::ZERO, |l| l.size);
    vec![
        clock.indicator.hidden as u32,
        clock.indicator.opacity.to_bits(),
        clock.indicator.transition.value().to_bits(),
        clock.indicator.transition.is_waiting() as u32,
        clock.elapsed.to_bits(),
        a.x.to_bits(),
        a.y.to_bits(),
        b.x.to_bits(),
        b.y.to_bits(),
    ]
}
fn packets(
    clock: &MenuIndicator,
    title: Option<&Layout>,
    note: Option<&Layout>,
    enabled: bool,
) -> Vec<Vec<u32>> {
    if !enabled || clock.indicator.opacity <= 0.0 || clock.indicator.transition.is_idle() {
        return Vec::new();
    }
    let (Some(title), Some(note)) = (title, note) else {
        return Vec::new();
    };
    let (scale, y) = note_pose(clock);
    let mut output = Vec::new();
    for (layout, position, origin, scale) in [
        (
            title,
            vec2(note.size.x + 4.0, clock.indicator.transition.value()),
            vec2(0.0, 1.0),
            Vec2::ONE,
        ),
        (
            note,
            vec2(note.size.x * 0.5 + 4.0, y),
            vec2(0.5, 1.0),
            scale,
        ),
    ] {
        let matrix = transform(layout.size, position, origin, scale);
        for border in [true, false] {
            for g in &layout.glyphs {
                let c = if border {
                    vec4(0.0, 0.0, 0.0, clock.indicator.opacity)
                } else {
                    vec4(0.75, 1.0, 1.0, clock.indicator.opacity)
                };
                let mut p = vec![if border { 4 } else { 3 }, g.point as u32];
                p.extend(matrix.to_cols_array().map(f32::to_bits));
                if border {
                    p.extend(g.border.iter().flat_map(|v| v.to_array()).map(f32::to_bits));
                } else {
                    p.extend(c.to_array().map(f32::to_bits));
                    p.extend([0; 12]);
                }
                p.extend(c.to_array().map(f32::to_bits));
                p.extend([0; 12]);
                for v in &g.vertices {
                    let position = v.position;
                    p.extend(position.to_array().map(f32::to_bits));
                    p.push(v.color.to_bits());
                }
                output.push(p);
            }
        }
    }
    output
}
fn native_packets(line: &str) -> Vec<Vec<u32>> {
    let mut words = line.split_whitespace();
    assert_eq!(words.next(), Some("R"));
    let mut packets = Vec::new();
    loop {
        match words.next() {
            Some("P") => packets.push(
                (0..70)
                    .map(|_| words.next().unwrap().parse().unwrap())
                    .collect(),
            ),
            Some("E") => {
                assert_eq!(
                    words.next().unwrap().parse::<usize>().unwrap(),
                    packets.len()
                );
                assert!(words.next().is_none());
                return packets;
            }
            x => panic!("Native packet token{x:?}"),
        }
    }
}
#[test]
#[ignore = "original Higurashi BGM title show/clock/stop/hide/render/full glyph packets with supplied font, no GPU/audio/save IO"]
fn higurashi_music_title_matches_native_clock_and_glyph_packets() {
    let font = read_font_metrics(&mut BufReader::new(
        std::fs::File::open(std::env::var("SHIN_HIGURASHI_TITLE_FONT").unwrap()).unwrap(),
    ))
    .unwrap();
    let font = std::sync::Arc::new(font);
    let mut input = String::new();
    let mut states = Vec::new();
    let mut expected = Vec::new();
    for text in [
        "日常",
        "見えない何か",
        "Matsuribayashi",
        "@c900赤い曲名@c.",
        "",
    ] {
        for variant in 0..3 {
            let mut clock = MenuIndicator::default();
            input += "N\n";
            states.push(snapshot(&clock, None, None));
            input += &format!("T {}\n", hex(text));
            let title = (!text.is_empty()).then(|| layout(font.clone(), text).unwrap());
            let note = (!text.is_empty()).then(|| layout(font.clone(), "♪").unwrap());
            if let Some(note) = &note {
                start(&mut clock, note.size.y);
            }
            states.push(snapshot(&clock, title.as_ref(), note.as_ref()));
            for (i, delta) in [0.0, 0.5, 10.0, 20.0, 40.0, 60.0, 120.0, 240.0]
                .into_iter()
                .enumerate()
            {
                input += &format!("U {delta}\n");
                clock.advance(Ticks::from_f32(delta));
                states.push(snapshot(&clock, title.as_ref(), note.as_ref()));
                for enabled in [false, true] {
                    input += &format!("R {}\n", enabled as u8);
                    expected.push(packets(&clock, title.as_ref(), note.as_ref(), enabled));
                }
                if variant == 1 && i == 2 {
                    input += "H 1\n";
                    clock.indicator.hidden = true;
                    states.push(snapshot(&clock, title.as_ref(), note.as_ref()));
                }
                if variant == 1 && i == 4 {
                    input += "H 0\n";
                    clock.indicator.hidden = false;
                    states.push(snapshot(&clock, title.as_ref(), note.as_ref()));
                }
                if variant == 2 && i == 3 {
                    input += "S\n";
                    clock.stop();
                    states.push(snapshot(&clock, title.as_ref(), note.as_ref()));
                }
            }
            input += "D\n";
        }
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_BGM_TITLE").unwrap())
        .args([
            "higurashi/uncompressed_executable_update",
            &std::env::var("SHIN_HIGURASHI_TITLE_FONT").unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut pipe = child.stdin.take().unwrap();
    let bytes = input.clone();
    let writer = std::thread::spawn(move || pipe.write_all(bytes.as_bytes()).unwrap());
    let out = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let output = String::from_utf8(out.stdout).unwrap();
    let actual = output
        .lines()
        .filter_map(|l| l.strip_prefix("T "))
        .map(|l| {
            l.split_whitespace()
                .map(|v| v.parse::<u32>().unwrap())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(actual.len(), states.len());
    for (i, (a, e)) in actual.iter().zip(&states).enumerate() {
        assert_eq!(a, e, "state{i}");
    }
    let draws = output
        .lines()
        .filter(|l| l.starts_with("R"))
        .map(native_packets)
        .collect::<Vec<_>>();
    assert_eq!(draws.len(), expected.len());
    let mut packets = 0;
    for (i, (a, e)) in draws.iter().zip(&expected).enumerate() {
        assert_eq!(a.len(), e.len(), "frame{i}");
        for (a, e) in a.iter().zip(e) {
            assert_eq!(&a[..2], &e[..2]);
            for (j, (&a, &e)) in a.iter().zip(e).enumerate().skip(2) {
                if a != e {
                    let x = f32::from_bits(a);
                    let y = f32::from_bits(e);
                    assert!((x - y).abs() <= 0.000002, "frame{i} word{j} {a:x}/{e:x}");
                }
            }
            packets += 1;
        }
    }
    let dir = std::path::PathBuf::from(std::env::var("SHIN_HIGURASHI_TITLE_CAPTURE").unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("input.txt"), input).unwrap();
    std::fs::write(dir.join("output.txt"), output).unwrap();
    std::fs::write(dir.join("stderr.txt"), out.stderr).unwrap();
    println!(
        "Native music title:15 owners/{} states/{} render frames/{packets} complete glyph packets; text/empty/settings/hide/reveal/stop/expiry pass",
        states.len(),
        draws.len()
    );
}
