use super::*;
use std::{
    fmt::Write as _,
    io::Write as _,
    process::{Command, Stdio},
    sync::Arc,
};
struct Reference {
    state: State,
    black: HigurashiTweener,
    cues: [u32; 3],
    destination: [u32; 2],
    stop: u32,
    gate: bool,
    game: [u32; 2],
    rng: crate::engine_random::EngineRandom,
}
impl Reference {
    fn new() -> Self {
        Self {
            state: State::new(),
            black: HigurashiTweener::new(0.0),
            cues: [0; 3],
            destination: [0; 2],
            stop: 0,
            gate: false,
            game: [0; 2],
            rng: crate::engine_random::EngineRandom::from_seed(5489),
        }
    }
    fn actions(&mut self) {
        for a in std::mem::take(&mut self.state.actions) {
            match a {
                Action::Cue(n) => {
                    self.cues[match n {
                        "move" => 0,
                        "cancel" => 1,
                        "quiz_call" => 2,
                        _ => unreachable!(),
                    }] += 1
                }
                Action::Destination(Destination::Top) => self.destination = [1, 7],
                Action::Destination(Destination::Story(id)) => {
                    self.destination = [5, id as u32];
                    self.stop = 90;
                    self.gate = true;
                    self.game = [id as u32, self.rng.next()];
                    control(&mut self.black, 1.0, 90.0, 0);
                }
            }
        }
    }
    fn step(&mut self, permission: bool, captured: bool, pressed: u32, repeat: u32, delta: f32) {
        if self.state.prepare() && permission {
            self.state.poll_controls(
                captured,
                Buttons {
                    pressed,
                    repeat,
                    ..Default::default()
                },
            );
        }
        self.state.advance(Ticks::from_f32(delta));
        self.actions();
        self.black.update(Ticks::from_f32(1.0));
    }
    fn contact(&mut self, op: char, point: Vec2) {
        match op {
            'V' => {
                self.state.probe(point);
            }
            'P' => self.state.press(point),
            'E' => self.state.release(point),
            'C' => self.state.cancel(),
            'Z' => self.state.reset(),
            _ => unreachable!(),
        };
        self.actions();
    }
    fn wait(&mut self) {
        if self.gate && self.black.is_idle() {
            self.destination = [6, 0];
            self.gate = false;
        }
        self.black.update(Ticks::from_f32(1.0));
    }
    fn snapshot(&self, op: char) -> String {
        let s = &self.state;
        let mut row = format!(
            "Q {op} {} {} {} {} {} F {} {} {} {} G {} {} C {} {} {} B {} {} R {}",
            s.phase,
            s.column,
            s.line,
            s.pointer,
            u32::from(s.cancelled),
            self.destination[0],
            self.destination[1],
            self.stop,
            u32::from(self.gate),
            self.game[0],
            self.game[1],
            self.cues[0],
            self.cues[1],
            self.cues[2],
            self.black.value().to_bits(),
            u32::from(self.black.is_waiting()),
            s.rows.len()
        );
        for r in &s.rows {
            write!(row, " {}", r.id).unwrap();
            for c in [&r.opacity, &r.flash] {
                write!(
                    row,
                    " {} {}",
                    c.value().to_bits(),
                    u32::from(c.is_waiting())
                )
                .unwrap();
            }
        }
        row
    }
}
fn native(protocol: String, tag: &str) -> String {
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_TITLE_QUIZ_MENU").unwrap())
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
    let sent = protocol.clone();
    let writer = std::thread::spawn(move || stdin.write_all(sent.as_bytes()));
    let out = child.wait_with_output().unwrap();
    let sent = writer.join().unwrap();
    if let Ok(dir) = std::env::var("SHIN_HIGURASHI_TITLE_QUIZ_CAPTURE") {
        let dir = std::path::Path::new(&dir).join(tag);
        std::fs::create_dir_all(&dir).unwrap();
        for (name, bytes) in [
            ("input.txt", protocol.as_bytes()),
            ("output.txt", out.stdout.as_slice()),
            ("stderr.txt", out.stderr.as_slice()),
        ] {
            std::fs::write(dir.join(name), bytes).unwrap();
        }
    }
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    sent.unwrap();
    eprintln!("{}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}
fn step(
    protocol: &mut String,
    expected: &mut Vec<String>,
    m: &mut Reference,
    p: bool,
    c: bool,
    a: u32,
    r: u32,
    d: f32,
) {
    writeln!(protocol, "U {} {} {a} {r} {d}", u8::from(p), u8::from(c)).unwrap();
    m.step(p, c, a, r, d);
    expected.push(m.snapshot('U'));
}
fn contact(
    protocol: &mut String,
    expected: &mut Vec<String>,
    m: &mut Reference,
    op: char,
    point: Vec2,
) {
    if op == 'V' {
        writeln!(protocol, "V {} {}", point.x, point.y).unwrap();
    } else {
        writeln!(protocol, "{op}").unwrap();
    }
    m.contact(op, point);
    expected.push(m.snapshot(op));
}
#[test]
#[ignore = "originale3258/e3bec/e40e0/contact/StartADV/e55e8,all logical buttons and8 fresh scenario payloads;input/cue/child/scene boundaries,no GPU/audio/window/saves"]
fn higurashi_title_quiz_menu_matches_native_state_input_and_factories() {
    let (mut protocol, mut expected, mut owners) = (String::new(), Vec::new(), 0);
    for plus in [0, 1] {
        for selected in 0..8 {
            writeln!(protocol, "N {plus}").unwrap();
            let mut m = Reference::new();
            expected.push(m.snapshot('N'));
            owners += 1;
            for d in [0.0, 17.0, 30.0, 1.0, 0.0] {
                step(&mut protocol, &mut expected, &mut m, true, false, 0, 0, d);
            }
            if selected & 1 != 0 {
                step(
                    &mut protocol,
                    &mut expected,
                    &mut m,
                    true,
                    false,
                    0,
                    1 << 5,
                    0.0,
                );
            }
            for _ in 0..selected / 2 {
                step(
                    &mut protocol,
                    &mut expected,
                    &mut m,
                    true,
                    false,
                    0,
                    1 << 6,
                    0.0,
                );
            }
            step(
                &mut protocol,
                &mut expected,
                &mut m,
                false,
                false,
                1 << 24,
                0,
                0.0,
            );
            step(
                &mut protocol,
                &mut expected,
                &mut m,
                true,
                true,
                1 << 24,
                0,
                0.0,
            );
            step(
                &mut protocol,
                &mut expected,
                &mut m,
                true,
                false,
                1 << 24,
                0,
                99.5,
            );
            step(&mut protocol, &mut expected, &mut m, true, false, 0, 0, 0.5);
            step(&mut protocol, &mut expected, &mut m, true, false, 0, 0, 0.0);
            assert_eq!(m.game[0], 5 + selected as u32);
            for _ in 0..91 {
                protocol.push_str("W\n");
                m.wait();
                expected.push(m.snapshot('W'));
            }
            assert_eq!(m.destination, [6, 0]);
            protocol.push_str("D\n");
        }
    }
    for pressed in std::iter::once(0)
        .chain((0..32).map(|bit| 1 << bit))
        .chain([(1 << 24) | (1 << 25)])
    {
        for repeat in [
            0,
            1 << 4,
            1 << 5,
            1 << 6,
            1 << 7,
            (1 << 4) | (1 << 6),
            (1 << 5) | (1 << 7),
            0xf0,
        ] {
            for captured in [false, true] {
                protocol.push_str("N 1\n");
                let mut m = Reference::new();
                expected.push(m.snapshot('N'));
                owners += 1;
                step(
                    &mut protocol,
                    &mut expected,
                    &mut m,
                    true,
                    false,
                    0,
                    0,
                    48.0,
                );
                step(
                    &mut protocol,
                    &mut expected,
                    &mut m,
                    true,
                    captured,
                    pressed,
                    repeat,
                    0.0,
                );
                step(
                    &mut protocol,
                    &mut expected,
                    &mut m,
                    true,
                    false,
                    0,
                    0,
                    100.0,
                );
                step(&mut protocol, &mut expected, &mut m, true, false, 0, 0, 0.0);
                protocol.push_str("D\n");
            }
        }
    }
    for (x, y) in [
        (110.0, 420.0),
        (109.99, 420.0),
        (550.0, 420.0),
        (549.99, 549.99),
        (110.0, 550.0),
        (565.99, 420.0),
        (566.0, 420.0),
        (1006.0, 420.0),
        (1005.99, 420.0),
        (110.0, 940.0),
        (110.0, 939.99),
        (110.0, 419.99),
    ] {
        protocol.push_str("N 0\n");
        let mut m = Reference::new();
        expected.push(m.snapshot('N'));
        owners += 1;
        contact(&mut protocol, &mut expected, &mut m, 'V', glam::vec2(x, y));
        contact(&mut protocol, &mut expected, &mut m, 'P', Vec2::ZERO);
        contact(&mut protocol, &mut expected, &mut m, 'C', Vec2::ZERO);
        contact(&mut protocol, &mut expected, &mut m, 'Z', Vec2::ZERO);
        step(
            &mut protocol,
            &mut expected,
            &mut m,
            true,
            false,
            0,
            0,
            48.0,
        );
        step(&mut protocol, &mut expected, &mut m, true, false, 0, 0, 0.0);
        contact(&mut protocol, &mut expected, &mut m, 'V', glam::vec2(x, y));
        contact(&mut protocol, &mut expected, &mut m, 'P', Vec2::ZERO);
        contact(&mut protocol, &mut expected, &mut m, 'E', Vec2::ZERO);
        step(
            &mut protocol,
            &mut expected,
            &mut m,
            true,
            false,
            0,
            0,
            100.0,
        );
        step(&mut protocol, &mut expected, &mut m, true, false, 0, 0, 0.0);
        protocol.push_str("D\n");
    }
    let output = native(protocol, "state");
    let actual = output.lines().collect::<Vec<_>>();
    assert_eq!(actual.len(), expected.len());
    for (i, (a, e)) in actual.iter().zip(&expected).enumerate() {
        assert_eq!(*a, e, "Title QuizMenu native observation{i}");
    }
    println!(
        "Title QuizMenu:{owners} owners/{} exact native observations;eight unlock-free choices/paired48-tick entrance/all pressed-repeat bits/permission-capture-axes priority/pointer bounds/100-tick chosen exit and48-tick cancel/quiz_call/all fresh extra-word0 scenarios5..12/seed/90-tick pre-clock StartADV gate/flags0 factories/destruction pass",
        expected.len()
    );
}
#[test]
#[ignore = "originale4260 packets with actual title atlas,all8 cells/normal-plus artwork/partial queued entrance/flash/exit at720p/1080p;Metal,no audio/window/saves"]
fn higurashi_title_quiz_menu_pixels_match_original_packets() {
    use crate::adv::higurashi::{
        reference_gpu_assets as gpu_assets, reference_render_to_texture as render_to_texture,
    };
    use glam::{Vec4, vec2, vec3};
    use shin_core::primitives::color::UnormColor;
    use shin_render::{
        ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
        shaders::types::{buffer::VertexSource, vertices::PosColTexVertex},
    };
    let (device, queue, server) = gpu_assets();
    let atlas: Arc<CharacterTextures> = server.load_sync("/title.txa").unwrap();
    let mut frames = 0;
    for plus in [false, true] {
        for profile in 0..8 {
            let mut m = Reference::new();
            let mut protocol = format!("N {}\n", u8::from(plus));
            let apply = |m: &mut Reference, p: &mut String, a: u32, r: u32, d: f32| {
                writeln!(p, "U 1 0 {a} {r} {d}").unwrap();
                m.step(true, false, a, r, d);
            };
            if profile > 0 {
                apply(
                    &mut m,
                    &mut protocol,
                    0,
                    0,
                    if profile == 1 { 17.0 } else { 48.0 },
                );
            }
            if profile >= 2 {
                apply(&mut m, &mut protocol, 0, 0, 0.0);
            }
            if profile >= 3 {
                apply(&mut m, &mut protocol, 0, 1 << 5, 0.0);
                for _ in 0..profile % 4 {
                    apply(&mut m, &mut protocol, 0, 1 << 6, 0.0);
                }
            }
            if profile >= 4 {
                apply(
                    &mut m,
                    &mut protocol,
                    if profile == 7 { 1 << 25 } else { 1 << 24 },
                    0,
                    if profile == 4 { 1.0 } else { 25.0 },
                );
            }
            protocol.push_str("R\nD\n");
            let output = native(protocol, &format!("pixels-{plus}-{profile}"));
            let packets = output
                .lines()
                .filter_map(|l| l.strip_prefix("P "))
                .map(|l| {
                    l.split_whitespace()
                        .map(|v| v.parse::<u32>().unwrap())
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            assert_eq!(packets.len(), 8);
            let mut menu = QuizMenu::new((1920, 1080));
            menu.state = m.state;
            for (i, (actual, p)) in menu.packets(plus).iter().zip(&packets).enumerate() {
                assert_eq!(p.len(), 47);
                assert_eq!(p[0], if plus { 2 } else { 1 });
                assert_eq!(p[1], 2);
                assert_eq!(p[2], if actual.flash > 0.0 { 5 } else { 2 });
                assert_eq!(
                    actual.matrix.to_cols_array().map(f32::to_bits),
                    p[3..19],
                    "profile{profile} row{i} matrix"
                );
                if p[2] == 5 {
                    assert_eq!([1.0, 1.0, 1.0, actual.flash].map(f32::to_bits), p[19..23]);
                }
                for j in 0..4 {
                    let [l, t, r, b] = actual.rect;
                    let [u, v, s, w] = actual.uv;
                    let at = 23 + j * 6;
                    assert_eq!(
                        [
                            if j % 2 == 0 { l } else { r },
                            if j < 2 { t } else { b },
                            0.0
                        ]
                        .map(f32::to_bits),
                        p[at..at + 3]
                    );
                    assert_eq!(actual.color, p[at + 3]);
                    assert_eq!(
                        [if j % 2 == 0 { u } else { s }, if j < 2 { v } else { w }]
                            .map(f32::to_bits),
                        p[at + 4..at + 6]
                    );
                }
            }
            for size in [(1280, 720), (1920, 1080)] {
                let pixels = render_to_texture(&device, &queue, size, |context, target| {
                    let mut pass=context.begin_pass(shin_render::shaders::types::texture::TextureTarget{view:target,kind:shin_render::shaders::types::texture::TextureTargetKind::RenderTexture},Some(context.depth_stencil),"Quiz title menu");
                    pass.clear(Some(UnormColor::WHITE), Some(0), Some(1.0));
                    menu.render(&mut pass, &atlas, plus);
                });
                let expected = render_to_texture(&device, &queue, size, |context, target| {
                    let mut pass=context.begin_pass(shin_render::shaders::types::texture::TextureTarget{view:target,kind:shin_render::shaders::types::texture::TextureTargetKind::RenderTexture},Some(context.depth_stencil),"Original QuizMenu packets");
                    pass.clear(Some(UnormColor::WHITE), Some(0), Some(1.0));
                    for p in &packets {
                        let raw = Mat4::from_cols_array(&std::array::from_fn(|i| {
                            f32::from_bits(p[3 + i])
                        }));
                        let transform=crate::layer::render_params::higurashi::HigurashiTransform::convert_clip_depth(raw);
                        let vertices = p[23..]
                            .chunks_exact(6)
                            .map(|v| {
                                let b = v[3].to_le_bytes();
                                PosColTexVertex {
                                    position: vec3(
                                        f32::from_bits(v[0]),
                                        f32::from_bits(v[1]),
                                        f32::from_bits(v[2]),
                                    ),
                                    color: UnormColor::from_rgba(b[2], b[1], b[0], b[3]),
                                    texture_position: vec2(
                                        f32::from_bits(v[4]),
                                        f32::from_bits(v[5]),
                                    ),
                                }
                            })
                            .collect::<Vec<_>>();
                        let texture = atlas
                            .get(if p[0] == 1 { "q_menu" } else { "q_menu_plus" })
                            .as_source();
                        let vertices = VertexSource::VertexData {
                            vertices: &vertices,
                        };
                        let program = if p[2] == 5 {
                            RenderProgramWithArguments::Button {
                                vertices,
                                texture,
                                transform,
                                flash1: Vec4::from_array(std::array::from_fn(|i| {
                                    f32::from_bits(p[19 + i])
                                })),
                                flash2: Vec4::ZERO,
                            }
                        } else {
                            RenderProgramWithArguments::Sprite {
                                vertices,
                                sprite: texture,
                                transform,
                            }
                        };
                        pass.run(
                            RenderRequestBuilder::new()
                                .color_blend_type(ColorBlendType::Layer1)
                                .build(program, DrawPrimitive::TrianglesStrip),
                        );
                    }
                });
                assert_eq!(
                    pixels, expected,
                    "Title quiz native pixel difference plus{plus} profile{profile} at{size:?}"
                );
                frames += 1;
            }
        }
    }
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    println!(
        "Title QuizMenu:{frames} byte-identical720p/1080p frames;all8 cells/normal-plus sheet,selected UV,queued entrance/confirmed/cancelled exit/flash and exact original sprite packets pass"
    );
}
mod owner;
