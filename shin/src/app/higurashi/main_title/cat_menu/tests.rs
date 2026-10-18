use super::*;
use std::{
    io::Write,
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};
struct Reference {
    state: State,
    black: HigurashiTweener,
    cues: [u32; 6],
    destination: [u32; 2],
    stop_ticks: u32,
    gate: bool,
    game: [u32; 38],
    rng: crate::engine_random::EngineRandom,
}
impl Reference {
    fn new(variant: u32, word: i16, selected: i32) -> Self {
        Self {
            state: State::new(variant, word, selected),
            black: HigurashiTweener::new(0.0),
            cues: [0; 6],
            destination: [0; 2],
            stop_ticks: 0,
            gate: false,
            game: [0; 38],
            rng: crate::engine_random::EngineRandom::from_seed(5489),
        }
    }
    fn actions(&mut self) {
        for action in std::mem::take(&mut self.state.actions) {
            match action {
                Action::Cue(name) => {
                    self.cues[match name {
                        "move" => 1,
                        "decide" => 2,
                        "error" => 3,
                        "cancel" => 4,
                        "start" => 5,
                        _ => unreachable!(),
                    }] += 1
                }
                Action::Destination(Destination::Top(selected)) => {
                    self.destination = [1, selected as u32]
                }
                Action::Destination(Destination::Cg) => self.destination = [2, 0],
                Action::Destination(Destination::Music) => self.destination = [3, 0],
                Action::Destination(Destination::Movie) => self.destination = [4, 0],
                Action::Destination(Destination::Story(id)) => {
                    self.destination = [5, id as u32];
                    self.stop_ticks = 90;
                    self.gate = true;
                    self.game[0] = 1;
                    self.game[4] = id as u32;
                    self.game[6] = id as u32;
                    self.game[7] = self.rng.next();
                    control(&mut self.black, 1.0, 90.0, 0);
                }
            }
        }
    }
    fn step(&mut self, p: bool, c: bool, a: u32, r: u32, d: f32) {
        if self.state.prepare() && p {
            self.state.poll_controls(
                c,
                Buttons {
                    pressed: a,
                    repeat: r,
                    ..Default::default()
                },
            );
        }
        self.state.advance(Ticks::from_f32(d));
        self.actions();
        self.black.update(Ticks::from_f32(1.0));
    }
    fn wait(&mut self) {
        if self.gate && self.black.is_idle() {
            self.destination = [6, 0];
            self.gate = false;
        }
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
        }
        self.actions();
    }
    fn snapshot(&self) -> Vec<u32> {
        let s = &self.state;
        let mut v = vec![
            s.variant,
            0,
            s.enabled,
            s.phase as u32,
            s.selected as u32,
            s.base_y as u32,
            s.pending as u32,
            s.pointer as u32,
        ];
        v.extend(self.destination);
        v.extend([
            self.stop_ticks,
            self.gate as u32,
            self.black.value().to_bits(),
            self.black.is_waiting() as u32,
        ]);
        v.extend(&self.cues[1..]);
        v.extend(self.game);
        v.push(s.rows.len() as u32);
        for row in &s.rows {
            v.push(row.id as u32);
            for clock in [&row.opacity, &row.flash] {
                v.extend([clock.value().to_bits(), clock.is_waiting() as u32]);
            }
        }
        v
    }
}
fn native(protocol: String) -> String {
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_CAT_MENU").unwrap())
        .args([
            "game-data/higurashi/uncompressed_executable_update",
            "/tmp/higurashi-newrodin.fnt",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || input.write_all(protocol.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    eprintln!("{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap()
}
fn step(
    protocol: &mut String,
    expected: &mut Vec<Vec<u32>>,
    model: &mut Reference,
    p: bool,
    c: bool,
    a: u32,
    r: u32,
    d: f32,
) {
    protocol.push_str(&format!("U {} {} {a} {r} {d}\n", p as u32, c as u32));
    model.step(p, c, a, r, d);
    expected.push(model.snapshot());
}
#[test]
#[ignore = "original updated Cat ctor/poll/contact/confirm/render boundaries,all destinations and owned story blackout gate"]
fn higurashi_cat_menu_matches_native_state_and_scene_factories() {
    let mut protocol = String::new();
    let mut expected = Vec::new();
    let mut owners = 0;
    for variant in [0, 1] {
        for plus in [false, true] {
            for word in [i16::MIN, -1, 0, 2, 4, 16, 31, i16::MAX] {
                for selected in [-2, -1, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9] {
                    protocol.push_str(&format!("N {variant} {} {word} {selected}\n", plus as u32));
                    owners += 1;
                    let mut m = Reference::new(variant, word, selected);
                    expected.push(m.snapshot());
                    for d in [0.0, 0.125, 0.5, 3.0, 60.0] {
                        step(&mut protocol, &mut expected, &mut m, true, false, 0, 0, d);
                    }
                    step(
                        &mut protocol,
                        &mut expected,
                        &mut m,
                        false,
                        false,
                        (1 << 24) | (1 << 25),
                        1 << 6,
                        0.0,
                    );
                    step(
                        &mut protocol,
                        &mut expected,
                        &mut m,
                        true,
                        true,
                        (1 << 24) | (1 << 25),
                        1 << 6,
                        0.0,
                    );
                    step(
                        &mut protocol,
                        &mut expected,
                        &mut m,
                        true,
                        false,
                        (1 << 24) | (1 << 25),
                        1 << 6,
                        0.0,
                    );
                    for d in [0.0, 0.125, 3.0, 17.0, 90.0, 0.0] {
                        step(&mut protocol, &mut expected, &mut m, true, false, 0, 0, d);
                    }
                    protocol.push_str("D\n");
                }
            }
        }
    }
    for variant in [0, 1] {
        for bit in 0..32 {
            for repeat in [false, true] {
                protocol.push_str(&format!("N {variant} 0 0 1\n"));
                owners += 1;
                let mut m = Reference::new(variant, 0, 1);
                expected.push(m.snapshot());
                step(
                    &mut protocol,
                    &mut expected,
                    &mut m,
                    true,
                    false,
                    0,
                    0,
                    60.0,
                );
                step(
                    &mut protocol,
                    &mut expected,
                    &mut m,
                    true,
                    false,
                    if repeat { 0 } else { 1 << bit },
                    if repeat { 1 << bit } else { 0 },
                    1.0,
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
    for variant in [0, 1] {
        for word in [0, 31] {
            for at in 0..if variant == 0 { 5 } else { 3 } {
                let base = if variant == 0 { 360.0 } else { 480.0 };
                for (x, y) in [
                    (247.999, base + 120.0 * at as f32),
                    (248.0, base + 120.0 * at as f32),
                    (847.999, base + 120.0 * at as f32 + 119.999),
                    (848.0, base + 120.0 * at as f32),
                    (500.0, base - 0.001),
                    (500.0, 840.0),
                    (500.0, base + 120.0 * at as f32 + 120.0),
                ] {
                    for release in [false, true] {
                        protocol.push_str(&format!("N {variant} 0 {word} -1\n"));
                        owners += 1;
                        let mut m = Reference::new(variant, word, -1);
                        expected.push(m.snapshot());
                        protocol.push_str(&format!("V {x} {y}\n"));
                        m.contact('V', vec2(x, y));
                        expected.push(m.snapshot());
                        step(
                            &mut protocol,
                            &mut expected,
                            &mut m,
                            true,
                            false,
                            0,
                            0,
                            60.0,
                        );
                        step(
                            &mut protocol,
                            &mut expected,
                            &mut m,
                            true,
                            false,
                            0,
                            (1 << 6) | (1 << 4),
                            0.0,
                        );
                        protocol.push_str(&format!("V {x} {y}\nP\n"));
                        m.contact('V', vec2(x, y));
                        expected.push(m.snapshot());
                        m.contact('P', Vec2::ZERO);
                        expected.push(m.snapshot());
                        step(
                            &mut protocol,
                            &mut expected,
                            &mut m,
                            true,
                            true,
                            (1 << 24) | (1 << 25),
                            1 << 6,
                            3.0,
                        );
                        let op = if release { 'E' } else { 'C' };
                        protocol.push_str(&format!("{op}\nZ\n"));
                        m.contact(op, Vec2::ZERO);
                        expected.push(m.snapshot());
                        m.contact('Z', Vec2::ZERO);
                        expected.push(m.snapshot());
                        for d in [0.0, 100.0, 0.0] {
                            step(&mut protocol, &mut expected, &mut m, true, false, 0, 0, d);
                        }
                        protocol.push_str("D\n");
                    }
                }
            }
        }
    }
    for id in 0..5 {
        protocol.push_str(&format!("N 0 0 31 {id}\n"));
        owners += 1;
        let mut m = Reference::new(0, 31, id);
        expected.push(m.snapshot());
        step(
            &mut protocol,
            &mut expected,
            &mut m,
            true,
            false,
            0,
            0,
            60.0,
        );
        step(
            &mut protocol,
            &mut expected,
            &mut m,
            true,
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
            false,
            0,
            0,
            100.0,
        );
        step(&mut protocol, &mut expected, &mut m, true, false, 0, 0, 0.0);
        for _ in 0..92 {
            protocol.push_str("W\n");
            m.wait();
            expected.push(m.snapshot());
        }
        protocol.push_str("D\n");
    }
    let output = native(protocol);
    let actual = output
        .lines()
        .filter_map(|l| l.strip_prefix("T "))
        .map(|l| {
            l.split_whitespace()
                .map(|v| v.parse::<u32>().unwrap())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(actual.len(), expected.len());
    let mut words = 0;
    for (i, (a, e)) in actual.iter().zip(&expected).enumerate() {
        assert_eq!(a, e, "Cat native observation{i}");
        words += e.len();
    }
    eprintln!(
        "Higurashi CatMenu:{owners} owners/{} observations/{words} exact words;ctor/unlock mask/grid,all pressed/repeat bits,permission/capture/precedence,queued entrance/closing/flash,cues/contacts/retreat,all8 categories,Top return IDs and original90-tick owned story gate/ADV factory pass",
        expected.len()
    );
}

#[test]
#[ignore = "original Cat renderer buffers and actual title atlas,locked/append artwork and flash,at720p/1080p"]
fn higurashi_cat_menu_pixels_match_original_render_packets() {
    use crate::adv::higurashi::{
        reference_gpu_assets as gpu_assets, reference_render_to_texture as render_to_texture,
    };
    use shin_core::primitives::color::UnormColor;
    let (device, queue, server) = gpu_assets();
    let atlas: Arc<CharacterTextures> = server.load_sync("/title.txa").unwrap();
    let mut pairs = 0;
    for (case, variant, plus, word, selected) in [
        (0, 0, false, 0, 0),
        (1, 0, true, 0, 3),
        (2, 0, false, 0, 2),
        (3, 0, true, 31, 4),
        (4, 1, false, 0, 6),
        (5, 1, true, 0, 7),
    ] {
        let mut menu = CatMenu::new(variant, word, selected, (1920, 1080));
        let mut protocol = format!("N {variant} {} {word} {selected}\n", plus as u32);
        let mut model = Reference::new(variant, word, selected);
        let steps = match case {
            0 => vec![],
            1 => vec![(0, 17.0)],
            2 | 3 => vec![(0, 60.0), (1 << 24, 1.0)],
            _ => vec![(0, 60.0), (0, 0.0)],
        };
        for (a, d) in steps {
            protocol.push_str(&format!("U 1 0 {a} 0 {d}\n"));
            model.step(true, false, a, 0, d);
        }
        if case == 5 {
            protocol.push_str("V 600 780\nP\nU 1 1 0 0 3\n");
            model.contact('V', vec2(600.0, 780.0));
            model.contact('P', Vec2::ZERO);
            model.step(true, true, 0, 0, 3.0);
        }
        menu.state = model.state;
        protocol.push_str("R\nD\n");
        let output = native(protocol);
        let packets = output
            .lines()
            .filter_map(|l| l.strip_prefix("P "))
            .map(|l| {
                l.split_whitespace()
                    .map(|v| v.parse::<u32>().unwrap())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            packets.len(),
            menu.state.rows.len()
                + menu
                    .state
                    .rows
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| menu.state.enabled & (1 << i) == 0)
                    .count()
        );
        for size in [(1280, 720), (1920, 1080)] {
            let pixels = render_to_texture(&device, &queue, size, |context, target| {
                let mut pass = context.begin_pass(
                    shin_render::shaders::types::texture::TextureTarget {
                        view: target,
                        kind:
                            shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
                    },
                    Some(context.depth_stencil),
                    "Higurashi Cat menu",
                );
                pass.clear(Some(UnormColor::WHITE), Some(0), Some(1.0));
                menu.render(&mut pass, &atlas, plus);
            });
            let expected = render_to_texture(&device, &queue, size, |context, target| {
                let mut pass = context.begin_pass(
                    shin_render::shaders::types::texture::TextureTarget {
                        view: target,
                        kind:
                            shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
                    },
                    Some(context.depth_stencil),
                    "Original Cat menu packets",
                );
                pass.clear(Some(UnormColor::WHITE), Some(0), Some(1.0));
                for p in &packets {
                    assert_eq!(p[1], 2);
                    assert!(p[2] == 2 || p[2] == 5);
                    let mut m = [0.0; 16];
                    for (v, bits) in m.iter_mut().zip(&p[3..19]) {
                        *v = f32::from_bits(*bits);
                    }
                    for i in 0..4 {
                        m[4 * i + 2] = 0.5 * m[4 * i + 2] + 0.5 * m[4 * i + 3];
                    }
                    let transform = glam::Mat4::from_cols_array(&m);
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
                                texture_position: vec2(f32::from_bits(v[4]), f32::from_bits(v[5])),
                            }
                        })
                        .collect::<Vec<_>>();
                    assert_eq!(vertices.len(), 4);
                    let texture = atlas
                        .get(match p[0] {
                            1 => "s_menu",
                            2 => "s_menu_plus",
                            3 => "s_menu_lock",
                            4 => "s_menu_lock_plus",
                            5 => "lock",
                            _ => panic!("cat native texture"),
                        })
                        .as_source();
                    let program = if p[2] == 5 {
                        RenderProgramWithArguments::Button {
                            vertices: VertexSource::VertexData {
                                vertices: &vertices,
                            },
                            texture,
                            transform,
                            flash1: Vec4::from_array(
                                p[19..23]
                                    .iter()
                                    .map(|b| f32::from_bits(*b))
                                    .collect::<Vec<_>>()
                                    .try_into()
                                    .unwrap(),
                            ),
                            flash2: Vec4::ZERO,
                        }
                    } else {
                        RenderProgramWithArguments::Sprite {
                            vertices: VertexSource::VertexData {
                                vertices: &vertices,
                            },
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
            assert!(
                pixels == expected,
                "Cat/native pixel differencecase{case} at{size:?}"
            );
            assert!(pixels.chunks_exact(4).all(|p| p[3] == 255));
            pairs += 1;
        }
    }
    eprintln!(
        "Higurashi Cat render:{pairs} byte-identical original-packet/GPU pairs at720p/1080p;cold/partial entrance,locked/append artwork+lock icon,selected UV and enabled/disabled/held flash pass"
    );
}

#[test]
#[ignore = "real startup/title-owned Top-Cat-Top navigation,physical pointer capture/gallery request and story90-tick handoff;exact Loopback Audio/offscreen Metal"]
fn higurashi_cat_menu_real_owner_navigates_and_gates_story() {
    use super::super::{CategorySceneRequest, Opening, Phase};
    use crate::{
        adv::higurashi::{
            reference_gpu_assets as gpu_assets, reference_render_to_texture as render_to_texture,
        },
        app::higurashi::{
            load_game::Paths,
            scene_host::{SceneHost, TitleTransition},
            title_demo::TitleRequest,
        },
        audio::SystemSoundPlayer,
    };
    use shin_core::{
        format::save::higurashi::{GameDataEntry, SaveDate},
        primitives::update::FrameId,
    };
    let (device, queue, server) = gpu_assets();
    let server = Arc::new(server);
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let sounds = Arc::new(parking_lot::Mutex::new(SystemSoundPlayer::new(
        manager.clone(),
    )));
    let mut host = SceneHost::new(
        server.clone(),
        manager,
        sounds,
        Arc::new(Paths {
            current: None,
            trial: None,
        }),
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut id = FrameId::default();
    let mut number = 0;
    let transition = loop {
        id.advance();
        number += 1;
        if let Some(t) = host
            .update(
                id,
                Ticks::from_f32(3.0),
                Buttons {
                    pressed: 1 << 24,
                    ..Default::default()
                },
                &PointerFrame {
                    events: &[],
                    number,
                    time_seconds: number as f64 / 60.0,
                },
                &queue,
            )
            .unwrap()
        {
            break t;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    };
    let mut opening = Opening::new(transition, None);
    let ready = |opening: &mut Opening| {
        while opening.motion.phase != Phase::Menu {
            opening.update(Ticks::from_f32(3.0)).unwrap();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        opening.update(Ticks::from_f32(120.0)).unwrap();
        opening.update(Ticks::ZERO).unwrap();
    };
    ready(&mut opening);
    let poll = |opening: &mut Opening,
                number: &mut u32,
                a: u32,
                r: u32,
                events: &[crate::adv::input_area::PointerEvent]| {
        *number += 1;
        opening
            .update_with_input(
                Ticks::ZERO,
                true,
                Buttons {
                    pressed: a,
                    repeat: r,
                    ..Default::default()
                },
                &PointerFrame {
                    events,
                    number: *number,
                    time_seconds: *number as f64 / 600.0,
                },
            )
            .unwrap();
    };
    let enter_cat = |opening: &mut Opening, number: &mut u32| {
        poll(opening, number, 1 << 24, 0, &[]);
        opening.update(Ticks::from_f32(100.0)).unwrap();
        opening.update(Ticks::ZERO).unwrap();
        assert!(opening.cat.is_some() && opening.top_visible && !opening.cat_visible);
        opening.update(Ticks::from_f32(60.0)).unwrap();
        opening.update(Ticks::ZERO).unwrap();
        assert!(opening.top.is_none() && opening.cat_visible);
        assert_eq!(opening.cat.as_ref().unwrap().state.phase, 2);
    };
    enter_cat(&mut opening, &mut number);
    assert_eq!(opening.cat.as_ref().unwrap().state.enabled, 1);
    for size in [(1280, 720), (1920, 1080)] {
        let pixels = render_to_texture(&device, &queue, size, |context, target| {
            let mut pass = context.begin_pass(
                shin_render::shaders::types::texture::TextureTarget {
                    view: target,
                    kind: shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
                },
                Some(context.depth_stencil),
                "Actual locked categories",
            );
            opening.render(&mut pass);
        });
        assert!(pixels.chunks_exact(4).all(|p| p[3] == 255));
        assert!(pixels.chunks_exact(4).any(|p| p[..3] != [0, 0, 0]));
    }
    for _ in 0..2 {
        poll(&mut opening, &mut number, 0, 1 << 6, &[]);
    }
    poll(&mut opening, &mut number, 1 << 24, 0, &[]);
    assert_eq!(opening.cat.as_ref().unwrap().state.phase, 2);
    assert!(opening.story_gate.is_none() && opening.take_category_scene_request().is_none());
    poll(&mut opening, &mut number, 1 << 25, 0, &[]);
    opening.update(Ticks::from_f32(60.0)).unwrap();
    opening.update(Ticks::ZERO).unwrap();
    assert!(opening.top.is_some() && opening.cat_visible && !opening.top_visible);
    opening.update(Ticks::from_f32(60.0)).unwrap();
    opening.update(Ticks::ZERO).unwrap();
    assert!(opening.cat.is_none());
    for _ in 0..2 {
        poll(&mut opening, &mut number, 0, 1 << 6, &[]);
    }
    enter_cat(&mut opening, &mut number);
    assert_eq!(opening.cat.as_ref().unwrap().state.variant, 1);
    opening.resize_menu_input((1280, 720));
    use crate::adv::input_area::{PointerEvent, PointerKind};
    for down in [true, false] {
        let event = PointerEvent {
            kind: if down {
                PointerKind::Down
            } else {
                PointerKind::Up
            },
            id: 0,
            position: glam::ivec2(40, 40),
        };
        poll(&mut opening, &mut number, 0, 0, &[event]);
    }
    assert_eq!(opening.cat.as_ref().unwrap().state.pending, -1);
    opening.update(Ticks::from_f32(60.0)).unwrap();
    opening.update(Ticks::ZERO).unwrap();
    opening.update(Ticks::from_f32(60.0)).unwrap();
    opening.update(Ticks::ZERO).unwrap();
    // Confirming immediately after gallery Back proves the Top focus returned to ID5.
    enter_cat(&mut opening, &mut number);
    assert_eq!(opening.cat.as_ref().unwrap().state.variant, 1);
    for size in [(1280, 720), (1920, 1080)] {
        let pixels = render_to_texture(&device, &queue, size, |context, target| {
            let mut pass = context.begin_pass(
                shin_render::shaders::types::texture::TextureTarget {
                    view: target,
                    kind: shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
                },
                Some(context.depth_stencil),
                "Actual gallery categories",
            );
            opening.render(&mut pass);
        });
        assert!(pixels.chunks_exact(4).all(|p| p[3] == 255));
    }
    for down in [true, false] {
        let event = PointerEvent {
            kind: if down {
                PointerKind::Down
            } else {
                PointerKind::Up
            },
            id: 0,
            position: glam::ivec2(400, 520),
        };
        poll(
            &mut opening,
            &mut number,
            (1 << 24) | (1 << 25),
            1 << 6,
            &[event],
        );
        assert_eq!(
            opening.cat.as_ref().unwrap().state.selected,
            if down { 0 } else { 2 }
        );
    }
    assert_eq!(opening.cat.as_ref().unwrap().state.phase, 3);
    opening.update(Ticks::from_f32(88.0)).unwrap();
    opening.update(Ticks::ZERO).unwrap();
    assert_eq!(
        opening.take_category_scene_request(),
        Some(CategorySceneRequest::Movie)
    );
    assert!(opening.take_category_scene_request().is_none());
    opening.update(Ticks::ZERO).unwrap();
    assert!(opening.cat.is_none());
    // Separate title owner: the movie-gallery scene itself is still a pending adapter.
    let mut globals = opening.into_globals();
    globals.data.persist_data.set(0, 31);
    let mut opening = Opening::new(
        TitleTransition {
            request: TitleRequest {
                selected: 0,
                mode: 0,
            },
            globals,
        },
        None,
    );
    ready(&mut opening);
    enter_cat(&mut opening, &mut number);
    for _ in 0..4 {
        poll(&mut opening, &mut number, 0, 1 << 6, &[]);
    }
    poll(&mut opening, &mut number, 1 << 24, 0, &[]);
    opening.update(Ticks::from_f32(100.0)).unwrap();
    let calls = crate::engine_random::seed_call_count();
    crate::engine_random::with_reference_seed(5489, || opening.update(Ticks::ZERO).unwrap());
    assert_eq!(crate::engine_random::seed_call_count(), calls + 1);
    assert!(opening.take_category_scene_request().is_none());
    assert!(opening.frame().black_alpha > 0.0 && opening.frame().black_alpha < 1.0);
    for _ in 0..89 {
        opening.update(Ticks::ZERO).unwrap();
        assert!(opening.take_category_scene_request().is_none());
    }
    assert_eq!(opening.frame().black_alpha, 1.0);
    opening.update(Ticks::ZERO).unwrap();
    assert_eq!(
        opening.take_category_scene_request(),
        Some(CategorySceneRequest::Story {
            flags: 0,
            data: GameData {
                date: SaveDate {
                    year: 0,
                    month: 0,
                    day: 0,
                    hour: 0,
                    minute: 0,
                    second: 0
                },
                extra_word: 4,
                entry: GameDataEntry {
                    scenario_id: 4,
                    random_seed: 3499211612,
                    save_position: 0,
                    replay_target: 0,
                    response_data: Vec::new()
                },
                primary_entry: None,
                trailing_values: Vec::new(),
                byte_variables: Vec::new(),
            }
        })
    );
    assert!(opening.take_category_scene_request().is_none());
    let bank = opening.globals.sounds.lock().bank.clone().unwrap();
    for name in ["start", "decide", "move", "cancel", "error"] {
        assert!(bank.sounds.contains_key(name));
    }
    // The direct return factory uses10+rowID rather than a zero-based index.
    // This is an independent title construction,not a simulated gallery child.
    let mut returned = Opening::new(
        TitleTransition {
            request: TitleRequest {
                selected: 16,
                mode: 1,
            },
            globals: opening.into_globals(),
        },
        None,
    );
    ready(&mut returned);
    assert!(returned.top.is_none() && returned.cat_visible);
    let cat = returned.cat.as_ref().unwrap();
    assert_eq!(
        (cat.state.variant, cat.state.selected, cat.state.phase),
        (1, 1, 2)
    );
    assert_eq!(
        returned.take_menu_request(),
        Some(super::super::MenuRequest::Extra {
            variant: 1,
            selected: 6
        })
    );
    assert_eq!(returned.motion.mode, 1);
    eprintln!(
        "Higurashi retained Cat integration:actual title/locked rows/gallery at720p/1080p,Top-story-B-Top-gallery/outside-tap-Top(ID5),1280 pointer capture over A+B+Down,tap-release Movie request,next-poll retirement,separate title story seed/category payload and90 one-tick blackout/flags0 ADV handoff,direct title16/mode1 return selects gallery rowID6 pass"
    );
}
