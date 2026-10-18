//! Compare renderer packets with the executable at the CPU submission boundary.
//! Font preparation/pixels retain their separate FNT6 native draw evidence.
use super::*;
use std::{
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

struct Case {
    prompt: Vec2,
    answers: Vec<Vec2>,
    selected: i32,
    origin: Vec2,
    scale: f32,
    opacity: f32,
    focus: [f32; 2],
    flash: [f32; 2],
}
struct Words<'a>(std::str::SplitWhitespace<'a>);
impl Words<'_> {
    fn token(&mut self, value: &str) {
        assert_eq!(self.0.next(), Some(value));
    }
    fn number(&mut self) -> u32 {
        self.0.next().unwrap().parse().unwrap()
    }
    fn floats(&mut self, expected: &[f32]) {
        for &v in expected {
            let actual = self.number();
            // Ignore only signed zero, as the vertex/palette values are finite
            // and this sign has no effect at a GPU submission boundary.
            assert!(
                actual == v.to_bits() || v == 0. && f32::from_bits(actual) == 0.,
                "native {} ({actual:08x}) != port {v} ({:08x})",
                f32::from_bits(actual),
                v.to_bits()
            );
        }
    }
    fn packet(
        &mut self,
        program: u32,
        textures: &[u32],
        matrix: Mat4,
        tint: Option<Vec4>,
        vertices: &[f32],
        indexed: bool,
    ) {
        self.token("P");
        assert_eq!(self.number(), program);
        for index in 0..3 {
            let actual = self.number();
            if let Some(expected) = textures.get(index) {
                assert_eq!(actual, *expected);
            }
        }
        assert_eq!(self.number() as usize, vertices.len());
        assert_eq!(self.number(), if indexed { 22 } else { 0 });
        if program == 0 {
            for _ in 0..16 {
                self.number();
            }
        } else {
            self.floats(&matrix.to_cols_array());
        }
        if let Some(tint) = tint {
            self.floats(&tint.to_array());
        }
        self.floats(vertices);
        if indexed {
            for index in INDICES {
                assert_eq!(self.number(), u32::from(index));
            }
        }
    }
    fn text(
        &mut self,
        index: u32,
        matrix: Mat4,
        position: Vec2,
        anchor: Vec2,
        fill: Vec4,
        outline: Vec4,
    ) {
        self.token("T");
        assert_eq!(self.number(), index);
        self.floats(&matrix.to_cols_array());
        self.floats(&[position.x, position.y, anchor.x, anchor.y, 1., 1.]);
        self.floats(&fill.to_array());
        self.floats(&fill.to_array());
        self.floats(&outline.to_array());
        self.floats(&[0., 0.]);
    }
}
fn vertices(vertices: &[PosTexVertex]) -> Vec<f32> {
    vertices
        .iter()
        .flat_map(|v| {
            [
                v.position.x,
                v.position.y,
                v.texture_position.x,
                v.texture_position.y,
            ]
        })
        .collect()
}

#[test]
#[ignore = "executes original Ger Question cache/frame/button/palette/composite; CPU only"]
fn ger_native_question_cached_draw_packets_and_palette_match() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    let out = root.join("reverse-engineering/gerokasu2/question-render");
    std::fs::create_dir_all(&out).unwrap();
    let binary = out.join("native-question-render");
    assert!(
        Command::new("clang")
            .args(["-std=c11", "-O2"])
            .arg(root.join("reverse-engineering/gerokasu2/native-question-render.c"))
            .arg("-o")
            .arg(&binary)
            .status()
            .unwrap()
            .success()
    );
    let mut cases = Vec::new();
    let mut input = String::new();
    for count in [0, 1, 2] {
        for width in [1., 599.25, 600., 601.75, 800., 1446.] {
            for (a, b) in [
                (vec2(80., 50.), vec2(120., 50.)),
                (vec2(480.5, 50.25), vec2(300., 90.)),
                (vec2(720., 85.), vec2(1050., 35.)),
                (Vec2::ZERO, Vec2::ZERO),
            ] {
                for state in 0..8 {
                    let answers = [a, b][..count].to_vec();
                    let c = Case {
                        prompt: vec2(width, 110.25),
                        answers,
                        selected: if count == 0 {
                            -2
                        } else if state == 6 {
                            -1
                        } else if state == 7 {
                            -2
                        } else {
                            (state % count) as i32
                        },
                        origin: vec2(123.25 + state as f32 * 11.5, -40.25 + state as f32 * 79.5),
                        scale: [0.5, 0.75, 1., 1.4][state % 4],
                        opacity: [0., 0.25, 0.7, 1.][state % 4],
                        focus: [
                            [0., 1.],
                            [1., 0.],
                            [0.25, 0.75],
                            [0.5, 0.5],
                            [0.75, 0.25],
                            [1., 1.],
                            [0., 0.],
                            [0.125, 0.875],
                        ][state],
                        flash: [[0., 0.], [0.5, 0.], [0., 0.25], [0.75, 1.]][state % 4],
                    };
                    input += &format!(
                        "{count} {} {} {} {} {} {} {} {:x} {:x} {:x} {:x} {:x} {:x} {:x} {:x}\n",
                        c.prompt.x,
                        c.prompt.y,
                        a.x,
                        a.y,
                        b.x,
                        b.y,
                        c.selected,
                        c.origin.x.to_bits(),
                        c.origin.y.to_bits(),
                        c.scale.to_bits(),
                        c.opacity.to_bits(),
                        c.focus[0].to_bits(),
                        c.flash[0].to_bits(),
                        c.focus[1].to_bits(),
                        c.flash[1].to_bits()
                    );
                    cases.push(c);
                }
            }
        }
    }
    std::fs::write(out.join("native-question-render.input"), &input).unwrap();
    let mut child = Command::new(&binary)
        .args([
            root.join("game-data/gerokasu2/uncompressed_executable"),
            root.join("test-fixtures/gerokasu2/newrodin-medium.fnt"),
            root.join("test-fixtures/gerokasu2/system.fnt"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let result = child.wait_with_output().unwrap();
    writer.join().unwrap();
    std::fs::write(out.join("native-question-render.output"), &result.stdout).unwrap();
    std::fs::write(out.join("native-question-render.stderr"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let text = String::from_utf8(result.stdout).unwrap();
    let mut words = Words(text.split_whitespace());
    let mut packets = 0;
    let mut texts = 0;
    for c in &cases {
        let g = geometry(c.prompt, &c.answers);
        words.token("R");
        words.floats(&g.outer.to_array());
        words.token("A");
        assert_eq!(words.number(), g.outer.x.round() as u32);
        assert_eq!(words.number(), g.outer.y.round() as u32);
        words.token("C");
        words.token("K");
        let frame: Vec<f32> = frame_vertices(g.outer)
            .iter()
            .flat_map(|v| {
                [
                    v.position.x,
                    v.position.y,
                    v.position.z,
                    v.position.w,
                    v.texture_position.x,
                    v.texture_position.y,
                    v.texture_position.z,
                    v.texture_position.w,
                ]
            })
            .collect();
        words.packet(8, &[0, 1, 2], projection(g.outer), None, &frame, true);
        let content = content_transform(g.outer);
        let mut n = 1;
        words.text(
            0,
            content,
            Vec2::ZERO,
            Vec2::ZERO,
            vec4(0.8, 0.8, 0.8, 1.),
            vec4(0.2, 0.2, 0.2, 1.),
        );
        for i in 0..c.answers.len() {
            let rect = g.buttons[i];
            let (fill, outline, background) = palette(c.focus[i]);
            words.packet(
                9,
                &[3],
                content,
                Some(background),
                &vertices(&button_vertices(0, rect)),
                true,
            );
            n += 1;
            words.text(
                i as u32 + 1,
                content,
                vec2(rect.x, rect.y) + vec2(rect.z, rect.w) * 0.5,
                Vec2::splat(0.5),
                fill,
                outline,
            );
            if c.selected == i as i32 {
                words.packet(
                    9,
                    &[3],
                    content,
                    Some(vec4(0.8, 0.8, 0.8, 1.)),
                    &vertices(&button_vertices(1, rect)),
                    true,
                );
                n += 1;
            }
            if c.flash[i] > 0. {
                words.packet(
                    9,
                    &[3],
                    content,
                    Some(vec4(0.8, 0.8, 0.8, c.flash[i])),
                    &vertices(&button_vertices(2, rect)),
                    true,
                );
                n += 1;
            }
        }
        words.token("S");
        words.packet(
            0,
            &[],
            Mat4::IDENTITY,
            Some(vec4(0., 0., 0., 0.5 * c.opacity)),
            &backdrop_vertices()
                .iter()
                .flat_map(|v| {
                    let position = v.position;
                    position.to_array()
                })
                .collect::<Vec<_>>(),
            false,
        );
        n += 1;
        words.packet(
            9,
            &[4],
            composite_transform(g.outer, c.origin, c.scale),
            Some(vec4(1., 1., 1., c.opacity)),
            &vertices(&composite_vertices(g.outer)),
            false,
        );
        n += 1;
        words.token("E");
        assert_eq!(words.number(), n);
        assert_eq!(words.number() as usize, c.answers.len() + 1);
        packets += n;
        texts += c.answers.len() + 1;
    }
    assert!(words.0.next().is_none());
    println!(
        "GEROKASU2_QUESTION_RENDER cases={} packets={packets} text_submissions={texts} native_cache_frame_buttons_palette_composite=exact CPU_boundaries=explicit",
        cases.len()
    );
}
