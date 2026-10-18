use super::*;
use std::{
    collections::HashMap,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};
fn root() -> PathBuf {
    std::env::var_os("SHIN_GEROKASU2_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("PWD")))
}
fn oracle(variable: &str, fallback: &str, input: &[u8], font_args: bool) -> Vec<u8> {
    let root = root();
    let binary = std::env::var_os(variable)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            root.join("reverse-engineering/gerokasu2/backlog-view")
                .join(fallback)
        });
    let mut command = Command::new(binary);
    command.arg(root.join("game-data/gerokasu2/uncompressed_executable"));
    if font_args {
        command
            .arg(root.join("test-fixtures/gerokasu2/newrodin-medium.fnt"))
            .arg(root.join("test-fixtures/gerokasu2/system.fnt"));
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("build original backlog oracle first");
    let mut stdin = child.stdin.take().unwrap();
    let input = input.to_vec();
    let writer = std::thread::spawn(move || stdin.write_all(&input).unwrap());
    let result = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    result.stdout
}
fn encoded(p: &Packet) -> Vec<u32> {
    let mut words = vec![if p.flash > 0. { 6 } else { 2 }, p.blend, 7, 0, 0, 0, 65535];
    words.extend(p.matrix.map(f32::to_bits));
    words.extend(if p.flash > 0. {
        [
            0,
            0,
            0,
            0,
            1f32.to_bits(),
            1f32.to_bits(),
            1f32.to_bits(),
            p.flash.to_bits(),
        ]
    } else {
        [0; 8]
    });
    words.push(24);
    for i in 0..4 {
        words.extend([
            p.rect[if i & 1 == 0 { 0 } else { 2 }].to_bits(),
            p.rect[if i & 2 == 0 { 1 } else { 3 }].to_bits(),
            0,
            p.color,
            p.uv[if i & 1 == 0 { 0 } else { 2 }].to_bits(),
            p.uv[if i & 2 == 0 { 1 } else { 3 }].to_bits(),
        ]);
    }
    words
}
#[test]
#[ignore = "executes original Ger80DB0 and83620 CPU oracle"]
fn ger_native_backlog_view_packets_and_hover_match() {
    let input =
        std::fs::read(root().join("test-fixtures/gerokasu2/backlog-view/render.input")).unwrap();
    let output = String::from_utf8(oracle(
        "SHIN_GEROKASU2_BACKLOG_VIEW_RENDER_ORACLE",
        "native-view",
        &input,
        true,
    ))
    .unwrap();
    let names: HashMap<_, _> = output
        .lines()
        .filter_map(|l| {
            let w: Vec<_> = l.split_whitespace().collect();
            (w.first() == Some(&"T")).then(|| (w[1].parse::<u32>().unwrap(), w[2]))
        })
        .collect();
    let mut groups = Vec::<Vec<(&str, Vec<u32>)>>::new();
    for line in output.lines() {
        let w: Vec<_> = line.split_whitespace().collect();
        match w[0] {
            "D" => groups.push(vec![]),
            "P" | "S" => groups
                .last_mut()
                .unwrap()
                .push((w[0], w[1..].iter().map(|w| w.parse().unwrap()).collect())),
            "E" => assert_eq!(&w[2..], &["2", "0", "2"]),
            _ => {}
        }
    }
    let profiles = std::str::from_utf8(&input)
        .unwrap()
        .lines()
        .collect::<Vec<_>>();
    assert_eq!(profiles.len(), groups.len());
    let mut packet_count = 0;
    for (profile, (line, native)) in profiles.iter().zip(groups).enumerate() {
        let w: Vec<_> = line.split_whitespace().collect();
        let f = |i: usize| f32::from_bits(u32::from_str_radix(w[i], 16).unwrap());
        let n = |i: usize| w[i].parse::<usize>().unwrap();
        let view = Presentation {
            progress: f(0),
            scroll: f(1),
            close_flash: f(2),
            game: crate::cli::Game::Gerokasu2,
            chapter_open: n(6) != 0,
            swapped: n(8) != 0,
            selected: n(9),
        };
        let row = Row {
            index: 0,
            kind: n(4) as u8,
            voice_count: n(3) as u8,
            message_arg: w[5].parse().unwrap(),
            height: f(10),
            top: f(11),
            flash: f(13),
        };
        let rows = if n(7) != 0 { vec![row] } else { vec![] };
        let frame = frame(
            &view,
            &rows,
            Some(ScrollRange {
                position: 0,
                length: 832,
                minimum: 0,
                maximum: 1200,
            }),
            row.kind == 0 && row.voice_count != 0,
        );
        let mut expected = vec![("P", frame.background.texture, encoded(&frame.background))];
        for r in frame.rows {
            for p in r.frame.into_iter().chain(r.portrait) {
                expected.push(("P", p.texture, encoded(&p)));
            }
        }
        for s in frame.scrollbar {
            let mut words = s.matrix.map(f32::to_bits).to_vec();
            for v in s.vertices {
                words.extend([
                    v.position[0].to_bits(),
                    v.position[1].to_bits(),
                    v.position[2].to_bits(),
                    u32::MAX,
                    v.uv[0].to_bits(),
                    v.uv[1].to_bits(),
                ]);
            }
            expected.push(("S", "scroll", words));
        }
        for p in frame.chrome {
            expected.push(("P", p.texture, encoded(&p)));
        }
        assert_eq!(native.len(), expected.len(), "profile{profile}");
        for ((kind, mut words), (want, texture, expected)) in native.into_iter().zip(expected) {
            let tex = words.remove(if kind == "P" { 2 } else { 0 });
            assert_eq!(kind, want);
            assert_eq!(names[&tex], texture, "profile{profile}");
            assert_eq!(words, expected, "profile{profile} texture{texture}");
            packet_count += 1;
        }
    }
    let input =
        std::fs::read(root().join("test-fixtures/gerokasu2/backlog-view/input.input")).unwrap();
    let input = std::str::from_utf8(&input)
        .unwrap()
        .lines()
        .filter(|l| l.starts_with("P "))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let output = String::from_utf8(oracle(
        "SHIN_GEROKASU2_BACKLOG_VIEW_INPUT_ORACLE",
        "native-input",
        input.as_bytes(),
        true,
    ))
    .unwrap();
    let rows = fixture_rows();
    let mut count = 0;
    for (line, native) in input.lines().zip(output.lines()) {
        let w: Vec<_> = line.split_whitespace().collect();
        let f = |i: usize| f32::from_bits(u32::from_str_radix(w[i], 16).unwrap());
        let ctx = pointer::Context {
            rows: &rows,
            selected: w[5].parse::<isize>().unwrap() as usize,
            scroll: f(3),
            minimum: -496.,
            bottom: 3464.,
        };
        let mut p = pointer::Pointer::default();
        p.fling(pointer::Point { x: 0., y: f(4) });
        let hit = p.hover(pointer::Point { x: f(1), y: f(2) }, &ctx);
        let area = match hit.area {
            pointer::Area::List => -1,
            pointer::Area::None => 0,
            pointer::Area::Close => 1,
        };
        let want = format!(
            "P {} {} {} {}",
            u8::from(hit.captured),
            area,
            hit.row.map_or(-1, |r| r as i32),
            p.movement().to_bits()
        );
        assert_eq!(native, want, "profile{count} {line}");
        count += 1;
    }
    println!(
        "GER_BACKLOG_VIEW_MATCH render_profiles={} packets={} hover_profiles={count}",
        profiles.len(),
        packet_count
    );
}
fn fixture_rows() -> Vec<Row> {
    (0..16)
        .map(|i| Row {
            index: i,
            top: (i as i32 - 2) as f32 * 248.,
            height: 240.,
            kind: 0,
            voice_count: 1,
            message_arg: 0,
            flash: 0.,
        })
        .collect()
}
#[test]
fn ger_backlog_pointer_edges_and_marker_voice_gate() {
    let mut rows = fixture_rows();
    rows[2].kind = 1;
    let ctx = pointer::Context {
        rows: &rows,
        selected: 2,
        scroll: 0.,
        minimum: -496.,
        bottom: 3464.,
    };
    let mut p = pointer::Pointer::default();
    assert_eq!(
        p.hover(pointer::Point { x: 190., y: 178. }, &ctx).row,
        Some(2)
    );
    assert!(matches!(
        p.release(pointer::Point { x: 190., y: 178. }, &ctx)
            .as_slice(),
        [pointer::Event::Select { row: 2, .. }]
    ));
    assert!(!p.hover(pointer::Point { x: 1760., y: 178. }, &ctx).captured);
    assert_eq!(
        p.hover(pointer::Point { x: 1766., y: 46. }, &ctx).area,
        pointer::Area::Close
    );
    assert_eq!(
        p.hover(pointer::Point { x: 1874., y: 46. }, &ctx).area,
        pointer::Area::None
    );
    p.hover(pointer::Point { x: 190., y: 178. }, &ctx);
    assert_eq!(
        p.fling(pointer::Point { x: -21., y: 0. }),
        vec![pointer::Event::ChapterSelector]
    );
}
#[test]
#[ignore = "executes original Ger8133C/81A7C CPU glyph oracle"]
fn ger_native_backlog_view_glyph_vertices_and_sdf_match() {
    let mut cases = Vec::new();
    let mut input = vec![0; 4];
    for i in 0..384 {
        let g = glyph::Geometry {
            origin: [[-333, -3, 0, 127][i % 4], [-75, -44, 0, 17][i / 4 % 4]],
            dimensions: [[1, 8, 45, 513][i / 16 % 4], [2, 17, 53, 257][i / 64 % 4]],
            uv_scale: if i % 3 == 0 { [0.5, 0.75] } else { [1., 1.] },
            font_metrics: if i % 5 == 0 { [120, 20] } else { [60, 20] },
            extra_metric: [1., 40., 80., 127.][i / 3 % 4],
        };
        let position: [f32; 2] = [[0., 6.], [0.25, -0.75], [100.125, 40.5]][i % 3];
        let scale: [f32; 2] = [[0.01, 0.25], [0.55, 0.55], [1., 2.5], [2.5, 1.]][i / 3 % 4];
        let ruby = i % 2 != 0;
        let mut desc = [0u8; 88];
        let mut put = |o: usize, v: u32| desc[o..o + 4].copy_from_slice(&v.to_le_bytes());
        put(0x10, g.origin[0] as u32);
        put(0x14, g.origin[1] as u32);
        put(0x18, g.dimensions[0]);
        put(0x1c, g.dimensions[1]);
        put(0x24, g.font_metrics[0] as u32);
        put(0x28, g.font_metrics[1] as u32);
        put(0x30, g.uv_scale[0].to_bits());
        put(0x34, g.uv_scale[1].to_bits());
        put(0x38, g.extra_metric.to_bits());
        input.extend(desc);
        for v in position.into_iter().chain(scale) {
            input.extend(v.to_le_bytes());
        }
        input.extend(u32::from(ruby).to_le_bytes());
        cases.push((g, position, scale, ruby));
    }
    input[..4].copy_from_slice(&(cases.len() as u32).to_le_bytes());
    let output = oracle(
        "SHIN_GEROKASU2_BACKLOG_VIEW_GLYPH_ORACLE",
        "native-glyph",
        &input,
        false,
    );
    assert_eq!(&output[..4], b"BVG1");
    assert_eq!(
        u32::from_le_bytes(output[4..8].try_into().unwrap()) as usize,
        cases.len()
    );
    assert_eq!(output.len(), 8 + cases.len() * 128);
    for (i, ((g, pos, scale, ruby), native)) in
        cases.iter().zip(output[8..].chunks_exact(128)).enumerate()
    {
        let d = glyph::Draw::new(g, *pos, *scale, *ruby).unwrap();
        let mut expected = Vec::new();
        for v in d.vertices {
            expected.extend(v.position.map(f32::to_bits));
            expected.push(v.color.to_bits());
        }
        expected.extend(d.outline.map(f32::to_bits));
        expected.extend(d.fill.map(f32::to_bits));
        let native = native
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect::<Vec<_>>();
        assert_eq!(&native[..24], &expected, "glyph{i}");
        assert_eq!(&native[24..28], &[0, 0, 0, 0.75f32.to_bits()]);
    }
    println!(
        "GER_BACKLOG_GLYPH_MATCH cases={} vertices={} passes={}",
        cases.len(),
        cases.len() * 4,
        cases.len() * 2
    );
}
#[test]
#[ignore = "executes original Ger80F4C/887C0 cache oracle"]
fn ger_native_backlog_view_cache_boundaries_match() {
    let rows = fixture_rows();
    let mut cases = Vec::new();
    let mut input = String::new();
    for (begin, end) in [(0, 16), (2, 16), (5, 12)] {
        for scroll in [
            -496.25f32, -336., -248., -88., -0.25, 0., 0.25, 160., 248., 408., 736.125, 2632.,
        ] {
            for top in [
                scroll - 160. - 240.,
                scroll - 160.,
                scroll + 920.,
                f32::from_bits((scroll + 920.).to_bits().wrapping_sub(1)),
                f32::from_bits((scroll + 920.).to_bits().wrapping_add(1)),
            ] {
                let height = 240f32;
                input += &format!(
                    "{begin} {end} {:x} {:x} {:x}\n",
                    scroll.to_bits(),
                    top.to_bits(),
                    height.to_bits()
                );
                cases.push((begin..end, scroll, top, height));
            }
        }
    }
    let output = String::from_utf8(oracle(
        "SHIN_GEROKASU2_BACKLOG_VIEW_CACHE_ORACLE",
        "native-cache",
        input.as_bytes(),
        true,
    ))
    .unwrap();
    assert_eq!(output.lines().count(), cases.len());
    for (i, ((allowed, scroll, top, height), native)) in
        cases.iter().zip(output.lines()).enumerate()
    {
        let w: Vec<_> = native.split_whitespace().collect();
        let upper = w[1].parse::<usize>().unwrap();
        assert_eq!(
            cache_start(&rows, allowed.clone(), *scroll),
            upper.saturating_sub(1),
            "cache start{i}"
        );
        let row = Row {
            top: *top,
            height: *height,
            ..rows[0]
        };
        assert_eq!(retain(&row, *scroll), w[2] == "0", "cache retain{i}");
        assert_eq!(should_create(&row, *scroll), w[3] == "1", "cache create{i}");
    }
    println!(
        "GER_BACKLOG_CACHE_MATCH cases={} upper_bound/retirement",
        cases.len()
    );
}
#[test]
#[ignore = "executes original Ger pointer and owner inertia CPU bodies"]
fn ger_native_backlog_view_pointer_callbacks_and_inertia_match() {
    let mut input = String::new();
    let mut expected = Vec::new();
    for i in 0..1440 {
        let action = [b'P', b'R', b'D', b'F', b'A'][i % 5];
        let selected = [0, 2, 5, 15][i / 5 % 4];
        let scroll = [-496f32, 0., 0.75, 248.25, 2632.][i / 20 % 5];
        let movement = [0f32, 0.099, 0.1, -0.25, 20., -20., 1000.][i / 100 % 7];
        let point = pointer::Point {
            x: [
                159.9, 160., 190., 395.99, 396., 1000., 1759.99, 1766., 1800.,
            ][i / 7 % 9],
            y: [46., 153.99, 160., 178., 300., 382., 426., 991.99, 992.][i / 11 % 9],
        };
        let velocity = pointer::Point {
            x: [-100f32, -21., -20., 0., 20., 100.][i / 3 % 6],
            y: [-400f32, -20., -0.1, 0., 0.1, 20., 400.][i / 13 % 7],
        };
        let kind = (i / 17 % 3) as u8;
        let voice = (i / 19 % 2) as u8;
        let delta = [0f32, 0.25, 1., 2.5, 1000.][i / 23 % 5];
        input += &format!(
            "{} {selected} {:x} {:x} {:x} {:x} {:x} {:x} {kind} {voice} {:x}\n",
            action as char,
            scroll.to_bits(),
            movement.to_bits(),
            point.x.to_bits(),
            point.y.to_bits(),
            velocity.x.to_bits(),
            velocity.y.to_bits(),
            delta.to_bits()
        );
        let mut rows = fixture_rows();
        for row in &mut rows {
            row.kind = kind;
            row.voice_count = voice;
        }
        let mut p = pointer::Pointer::default();
        p.fling(pointer::Point { x: 0., y: movement });
        let mut selection = selected;
        let mut current = scroll;
        let mut target = scroll;
        let mut close_flash = 0f32;
        let mut flashes = [0f32; 16];
        let mut actions = Vec::new();
        let context = |selection, current| pointer::Context {
            rows: &rows,
            selected: selection,
            scroll: current,
            minimum: -496.,
            bottom: 3464.,
        };
        let mut apply = |events: Vec<pointer::Event>, selection: &mut usize, current: &mut f32| {
            for e in events {
                match e {
                    pointer::Event::Select {
                        row,
                        scroll,
                        immediate,
                    } => {
                        *selection = row;
                        target = scroll;
                        if immediate {
                            *current = scroll;
                        }
                    }
                    pointer::Event::Close => actions.extend([1, 0]),
                    pointer::Event::ReplayVoice { row } => actions.extend([2, row as u32]),
                    pointer::Event::QuickLoad { row } => actions.extend([3, row as u32]),
                    pointer::Event::ChapterSelector => actions.extend([4, 0]),
                    pointer::Event::CloseFlash { value } => close_flash = value,
                    pointer::Event::VoiceFlash { row, value } => flashes[row] = value,
                }
            }
        };
        if action == b'A' {
            apply(
                p.advance(delta, &context(selection, current)),
                &mut selection,
                &mut current,
            );
        } else {
            p.hover(point, &context(selection, current));
            if [b'P', b'R', b'D'].contains(&action) {
                apply(
                    p.press(point, &context(selection, current)),
                    &mut selection,
                    &mut current,
                );
            }
            let events = match action {
                b'R' => p.release(point, &context(selection, current)),
                b'D' => p.drag(
                    point,
                    pointer::Point {
                        x: point.x + velocity.x,
                        y: point.y + velocity.y,
                    },
                    &context(selection, current),
                ),
                b'F' => p.fling(velocity),
                _ => vec![],
            };
            apply(events, &mut selection, &mut current);
        }
        let area = match p.area() {
            pointer::Area::List => -1,
            pointer::Area::None => 0,
            pointer::Area::Close => 1,
        };
        let mut line = format!(
            "O {selection} {} {} {} {area} {} {}",
            current.to_bits(),
            target.to_bits(),
            p.movement().to_bits(),
            p.row().map_or(-1, |r| r as i32),
            close_flash.to_bits()
        );
        for f in flashes {
            line += &format!(" {}", f.to_bits());
        }
        line += &format!(" {}", actions.len() / 2);
        for a in actions {
            line += &format!(" {a}");
        }
        expected.push(line);
    }
    let output = String::from_utf8(oracle(
        "SHIN_GEROKASU2_BACKLOG_VIEW_POINTER_ORACLE",
        "native-pointer",
        input.as_bytes(),
        true,
    ))
    .unwrap();
    assert_eq!(output.lines().count(), expected.len());
    for (i, (native, want)) in output.lines().zip(&expected).enumerate() {
        assert_eq!(
            native,
            want,
            "pointer profile{i} {}",
            input.lines().nth(i).unwrap()
        );
    }
    println!(
        "GER_BACKLOG_POINTER_MATCH cases={} press/release/drag/fling/inertia",
        expected.len()
    );
}

#[test]
fn gerokasu_chrome_adds_bottom_band_and_own_portraits() {
    let view = Presentation {
        progress: 0.5,
        scroll: 0.,
        selected: 0,
        chapter_open: false,
        swapped: false,
        close_flash: 0.,
        game: crate::cli::Game::Gerokasu,
    };
    let chrome = chrome(&view, true);
    assert_eq!(chrome[0].texture, "top");
    assert_eq!(chrome[0].blend, 2);
    let bottom = chrome.iter().find(|p| p.texture == "bottom").unwrap();
    assert_eq!(bottom.rect, [0., 1036., 1920., 1124.]);
    assert_eq!(bottom.uv, [0., 0., 0.5, 1.]);
    let row = Row {
        index: 0,
        top: 0.,
        height: 300.,
        kind: 0,
        voice_count: 1,
        message_arg: 3,
        flash: 0.,
    };
    assert_eq!(row_packets(&view, row).portrait.unwrap().texture, "voice_doremi");
    let view = Presentation {
        game: crate::cli::Game::Gerokasu2,
        ..view
    };
    let chrome = super::chrome(&view, true);
    assert_eq!(chrome[0].blend, 3);
    assert!(chrome.iter().all(|p| p.texture != "bottom"));
}
