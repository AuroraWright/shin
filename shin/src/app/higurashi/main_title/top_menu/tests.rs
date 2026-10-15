use super::*;
use std::{
    io::Write,
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};
struct Reference {
    state: State,
    logo: HigurashiTweener,
    black: HigurashiTweener,
    cues: [u32; 4],
    destination: [u32; 3],
}
impl Reference {
    fn new(plus: bool, valid: bool, selected: i32, enabled: u32) -> Self {
        let mut state = State::new(plus, valid, selected);
        state.enabled = enabled;
        Self {
            state,
            logo: HigurashiTweener::new(0.0),
            black: HigurashiTweener::new(0.0),
            cues: [0; 4],
            destination: [0; 3],
        }
    }
    fn actions(&mut self) {
        for action in std::mem::take(&mut self.state.actions) {
            match action {
                Action::Logo(value, ticks) => control(&mut self.logo, value, ticks, 0),
                Action::Cue(cue) => {
                    self.cues[match cue {
                        Cue::Move => 1,
                        Cue::Confirm => 2,
                        Cue::Error => 3,
                    }] += 1
                }
                Action::Destination(destination) => {
                    self.destination = match destination {
                        Destination::Cat { variant, selected } => {
                            [if variant == 0 { 1 } else { 6 }, variant, selected as u32]
                        }
                        Destination::Save { return_id } => [2, return_id as u32, 0],
                        Destination::Config { flags, return_id } => [3, flags, return_id as u32],
                        Destination::Chart {
                            selected,
                            return_id,
                        } => [4, selected as u32, return_id as u32],
                        Destination::Manual { return_id } => [5, return_id as u32, 0],
                        Destination::Tips {
                            selected,
                            alternate,
                        } => [7, selected as u32, alternate as u32],
                        Destination::Quiz => [8, 0, 0],
                        Destination::Origin { selected } => [9, selected as u32, 0],
                        Destination::IdleDemo => [10, 0, 0],
                    };
                    if matches!(
                        destination,
                        Destination::Save { .. }
                            | Destination::Config { .. }
                            | Destination::Chart { .. }
                            | Destination::Manual { .. }
                    ) {
                        control(&mut self.black, 0.5, f32::from_bits(0x3dcccccd), 0x40);
                    }
                    if destination == Destination::IdleDemo {
                        control(&mut self.black, 1.0, 60.0, 0);
                    }
                }
            }
        }
    }
    fn step(&mut self, permission: bool, captured: bool, pressed: u32, repeat: u32, delta: f32) {
        self.state.prepare();
        if self.state.begin_input(permission) {
            self.state.poll_controls(
                captured,
                Buttons {
                    pressed,
                    repeat,
                    ..Default::default()
                },
            );
        }
        self.state.finish();
        self.state.advance(Ticks::from_f32(delta));
        self.actions();
        self.logo.update(Ticks::from_f32(delta));
        self.black.update(Ticks::from_f32(1.0));
    }
    fn snapshot(&self) -> Vec<u32> {
        let s = &self.state;
        let mut words = vec![
            s.phase as u32,
            s.idle as u32,
            s.column as u32,
            s.line as u32,
            s.pointer as u32,
            s.enabled,
            s.idle_demo as u32,
        ];
        words.extend(self.destination);
        words.extend(self.cues[1..].iter());
        for clock in [&self.logo, &self.black] {
            words.extend([clock.value().to_bits(), clock.is_waiting() as u32]);
        }
        words.push(s.rows.len() as u32);
        for row in &s.rows {
            words.push(row.id as u32);
            words.extend(row.rect.to_array().map(f32::to_bits));
            for clock in [&row.opacity, &row.flash] {
                words.extend([clock.value().to_bits(), clock.is_waiting() as u32]);
            }
        }
        words
    }
}
fn native(protocol: String) -> String {
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_TOP_MENU").unwrap())
        .args([
            "higurashi/uncompressed_executable_update",
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
#[test]
#[ignore = "actual updated full top ctor/poll/navigation/confirmation/idle/contact and all child/scene requests"]
fn higurashi_top_menu_matches_native_state_input_and_destinations() {
    let mut protocol = String::new();
    let mut expected = Vec::new();
    let mut owners = 0;
    let add_step = |protocol: &mut String,
                    expected: &mut Vec<Vec<u32>>,
                    model: &mut Reference,
                    p,
                    c,
                    a,
                    r,
                    d| {
        protocol.push_str(&format!("U {} {} {a} {r} {d}\n", p as u32, c as u32));
        model.step(p, c, a, r, d);
        expected.push(model.snapshot());
    };
    for plus in [false, true] {
        for valid in [false, true] {
            for selected in [-9, -1, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9] {
                for enabled in [0, 0x1ff] {
                    protocol.push_str(&format!(
                        "N {} {} {selected} {enabled}\n",
                        plus as u32, valid as u32
                    ));
                    let mut model = Reference::new(plus, valid, selected, enabled);
                    expected.push(model.snapshot());
                    owners += 1;
                    for (p, c, a, r, d) in [
                        (true, false, 0, 0, 1.0),
                        (false, false, 1 << 24, 0, 59.0),
                        (true, true, 1 << 24, 0, 0.0),
                        (true, false, 0, 1 << 6, 0.125),
                        (true, false, 0, 1 << 4, 0.5),
                        (true, false, 1 << 24, 0, 1.0),
                        (true, false, 0, 0, 60.0),
                        (false, false, 0, 0, 40.0),
                        (false, false, 0, 0, 1.0),
                    ] {
                        add_step(&mut protocol, &mut expected, &mut model, p, c, a, r, d);
                    }
                    protocol.push_str("D\n");
                }
            }
        }
    }
    // Every single logical bit,simultaneous direction precedence,grid wrap and
    // every selected row. Confirm preempts navigation;B/shoulders do nothing.
    for pressed in std::iter::once(0)
        .chain((0..32).map(|bit| 1 << bit))
        .chain([0x01000000 | (1 << 25)])
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
                protocol.push_str("N 1 0 8 511\n");
                let mut model = Reference::new(true, false, 8, 511);
                expected.push(model.snapshot());
                owners += 1;
                add_step(
                    &mut protocol,
                    &mut expected,
                    &mut model,
                    true,
                    false,
                    0,
                    0,
                    60.0,
                );
                add_step(
                    &mut protocol,
                    &mut expected,
                    &mut model,
                    true,
                    captured,
                    pressed,
                    repeat,
                    0.0,
                );
                add_step(
                    &mut protocol,
                    &mut expected,
                    &mut model,
                    true,
                    false,
                    0,
                    0,
                    100.0,
                );
                add_step(
                    &mut protocol,
                    &mut expected,
                    &mut model,
                    true,
                    false,
                    0,
                    0,
                    0.0,
                );
                protocol.push_str("D\n");
            }
        }
    }
    for plus in [false, true] {
        protocol.push_str(&format!("N {} 0 -1 511\n", plus as u32));
        let mut model = Reference::new(plus, false, -1, 511);
        expected.push(model.snapshot());
        owners += 1;
        add_step(
            &mut protocol,
            &mut expected,
            &mut model,
            true,
            false,
            0,
            0,
            60.0,
        );
        add_step(
            &mut protocol,
            &mut expected,
            &mut model,
            true,
            false,
            0,
            0,
            0.0,
        );
        for point in [
            vec2(110.0, 410.0),
            vec2(549.999, 436.0),
            vec2(550.0, 436.0),
            vec2(566.0, 436.0),
            vec2(258.0, 856.0),
            vec2(549.0, 863.999),
            vec2(549.0, 864.0),
            vec2(858.0, 968.0),
            vec2(-1.0, -1.0),
        ] {
            protocol.push_str(&format!("V {} {}\n", point.x, point.y));
            model.state.probe(point);
            model.actions();
            expected.push(model.snapshot());
            protocol.push_str("P\nC\nZ\n");
            model.state.press(point);
            model.actions();
            expected.push(model.snapshot());
            model.state.cancel();
            model.actions();
            expected.push(model.snapshot());
            model.state.reset();
            expected.push(model.snapshot());
        }
        protocol.push_str("V 120 550\nP\nE\n");
        model.state.probe(vec2(120.0, 550.0));
        expected.push(model.snapshot());
        model.state.press(Vec2::ZERO);
        model.actions();
        expected.push(model.snapshot());
        model.state.release(Vec2::ZERO);
        model.actions();
        expected.push(model.snapshot());
        for d in [60.0, 40.0, 0.0] {
            add_step(
                &mut protocol,
                &mut expected,
                &mut model,
                true,
                false,
                0,
                0,
                d,
            );
        }
        protocol.push_str("D\n");
    }
    for mode in 0..2 {
        for idle in [3598, 3599, 3600] {
            protocol.push_str("N 1 0 -1 511\n");
            protocol.push_str(&format!("B {mode}\n"));
            let mut model = Reference::new(true, false, -1, 511);
            expected.push(model.snapshot());
            owners += 1;
            add_step(
                &mut protocol,
                &mut expected,
                &mut model,
                true,
                false,
                0,
                0,
                60.0,
            );
            add_step(
                &mut protocol,
                &mut expected,
                &mut model,
                true,
                false,
                0,
                0,
                0.0,
            );
            protocol.push_str(&format!("I {idle}\n"));
            model.state.idle = idle;
            for d in [0.0, 30.0, 0.0] {
                add_step(
                    &mut protocol,
                    &mut expected,
                    &mut model,
                    true,
                    false,
                    0,
                    0,
                    d,
                );
            }
            if model.destination[0] == 10 {
                for _ in 0..61 {
                    protocol.push_str("W\n");
                    if model.black.is_idle() {
                        model.destination = [11, mode + 1, 0];
                    }
                    model.black.update(Ticks::from_f32(1.0));
                    expected.push(model.snapshot());
                }
            }
            protocol.push_str("D\n");
        }
    }
    let output = native(protocol);
    assert_eq!(output.lines().count(), expected.len());
    for (index, (line, words)) in output.lines().zip(&expected).enumerate() {
        let mut values = line.split_whitespace();
        assert_eq!(values.next(), Some("T"));
        assert_eq!(
            values
                .map(|v| v.parse::<u32>().unwrap())
                .collect::<Vec<_>>(),
            *words,
            "native top observation{index}"
        );
    }
    eprintln!(
        "Higurashi top menu:{owners} owners/{} observations/{} exact words;full ctor/auto-slot selection,8/9-cell grid,permission/capture/all pressed bits/repeat precedence,fade/flash/cues,disabled rows,contacts,all9 destinations and3600-call idle fade/demo(+1) pass",
        expected.len(),
        expected.iter().map(Vec::len).sum::<usize>()
    );
}

#[test]
#[ignore = "actual retained title top menu input/draw/confirmation at720p/1080p and exact Loopback Audio"]
fn higurashi_top_menu_real_owner_accepts_pointer_and_queues_config() {
    use crate::{
        adv::higurashi::{
            reference_gpu_assets as gpu_assets, reference_render_to_texture as render_to_texture,
        },
        app::higurashi::{
            load_game::Paths,
            main_title::{Opening, TitleDestination},
            scene_host::SceneHost,
        },
        audio::SystemSoundPlayer,
    };
    use parking_lot::Mutex;
    use shin_core::primitives::update::FrameId;
    let (device, queue, server) = gpu_assets();
    let server = Arc::new(server);
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let sounds = Arc::new(Mutex::new(SystemSoundPlayer::new(manager.clone())));
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut host = SceneHost::new(
        server.clone(),
        manager,
        sounds.clone(),
        Arc::new(Paths {
            current: None,
            trial: None,
        }),
    );
    let mut id = FrameId::default();
    let mut number = 0;
    let transition = loop {
        id.advance();
        number += 1;
        let result = host
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
            .unwrap();
        if let Some(value) = result {
            break value;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    };
    let mut opening = Opening::new(transition, None);
    while !opening
        .top
        .as_ref()
        .is_some_and(|menu| menu.state.phase == 2)
    {
        opening.update(Ticks::from_f32(3.0)).unwrap();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    let count = opening.top.as_ref().unwrap().state.rows.len();
    assert_eq!(count, if server.contains("/append.txt") { 9 } else { 8 });
    let atlas = opening.atlas.as_ref().unwrap();
    assert!(atlas.contains("t_menu") && atlas.contains("o_menu"));
    assert!(opening.top_visible);
    for size in [(1280, 720), (1920, 1080)] {
        let pixels = render_to_texture(&device, &queue, size, |context, target| {
            let mut pass = context.begin_pass(
                shin_render::shaders::types::texture::TextureTarget {
                    view: target,
                    kind: shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
                },
                Some(context.depth_stencil),
                "Higurashi real title top menu",
            );
            opening.render(&mut pass);
        });
        assert!(pixels.chunks_exact(4).all(|p| p[3] == 255));
        assert!(pixels.chunks_exact(4).any(|p| p[..3] != [0, 0, 0]));
    }
    let rect = opening.top.as_ref().unwrap().state.rows[2].rect;
    opening.resize_menu_input((1280, 720));
    for kind in [
        crate::adv::input_area::PointerKind::Down,
        crate::adv::input_area::PointerKind::Up,
    ] {
        number += 1;
        let event = crate::adv::input_area::PointerEvent {
            kind,
            id: 0,
            position: glam::ivec2(
                ((rect.x + 20.0) / 1.5) as i32,
                ((rect.y + 20.0) / 1.5) as i32,
            ),
        };
        opening
            .update_with_input(
                Ticks::from_f32(1.0),
                true,
                Buttons {
                    pressed: 1 << 24,
                    repeat: 1 << 6,
                    ..Default::default()
                },
                &PointerFrame {
                    events: &[event],
                    number,
                    time_seconds: number as f64 / 60.0,
                },
            )
            .unwrap();
        let menu = opening.top.as_ref().unwrap();
        assert_eq!(
            menu.state.selected(),
            if kind == crate::adv::input_area::PointerKind::Down {
                0
            } else {
                2
            }
        );
        assert_eq!(
            menu.state.phase,
            if kind == crate::adv::input_area::PointerKind::Down {
                2
            } else {
                3
            }
        );
    }
    assert!(!sounds.lock().sounds.is_empty());
    assert!(opening.take_destination().is_none());
    loop {
        opening.update(Ticks::from_f32(3.0)).unwrap();
        if let Some(destination) = opening.take_destination() {
            assert_eq!(
                destination,
                TitleDestination::Config {
                    flags: 0,
                    return_id: 2
                }
            );
            break;
        }
        assert!(Instant::now() < deadline);
    }
    assert!(opening.top.as_ref().unwrap().complete());
    assert!(
        opening.top_visible,
        "completed child remains in queueing frame's draw snapshot"
    );
    assert!(opening.frame().black_alpha > 0.0 && opening.frame().black_alpha < 0.5);
    opening.update(Ticks::ZERO).unwrap();
    assert!(opening.top.is_none() && !opening.top_visible);
    assert!(opening.take_destination().is_none());
    // Seed a new active top owner to isolate the idle transition;this does not
    // simulate the still-pending Config child adapter or its return callback.
    opening.top = Some(TopMenu::new(opening.plus, false, 0, (1920, 1080)));
    opening.motion.clocks[4].fast_forward_to(0.0);
    opening.motion.mode = 1;
    opening.update(Ticks::from_f32(60.0)).unwrap();
    opening.update(Ticks::ZERO).unwrap();
    opening.top.as_mut().unwrap().state.idle = 3599;
    opening.update(Ticks::ZERO).unwrap();
    assert_eq!(opening.top.as_ref().unwrap().state.phase, 3);
    opening.update(Ticks::from_f32(30.0)).unwrap();
    opening.update(Ticks::ZERO).unwrap();
    assert_eq!(opening.take_destination(), Some(TitleDestination::IdleDemo));
    assert!(opening.take_title_demo_request().is_none());
    for _ in 0..59 {
        opening.update(Ticks::ZERO).unwrap();
        assert!(opening.take_title_demo_request().is_none());
    }
    assert_eq!(opening.frame().black_alpha, 1.0);
    opening.update(Ticks::ZERO).unwrap();
    assert_eq!(
        opening.take_title_demo_request(),
        Some(crate::app::higurashi::startup::TitleDemoRequest { mode: 2 })
    );
    assert!(opening.take_title_demo_request().is_none());
    eprintln!(
        "Higurashi retained top integration:actual8/9 rows at720p/1080p,1280 pointer mapping/capture over simultaneous A+Down,tap selection on release,confirmation/global cue/100-tick queued fade,owned Config(flags0,return2),parent blackout/next-frame retirement and separate seeded idle-owner60-frame blackout gate/alternating demo(+1) pass"
    );
}

#[test]
#[ignore = "real title atlas and full native top render buffers,including program5 flash,at720p/1080p"]
fn higurashi_top_menu_pixels_match_original_render_packets() {
    use crate::adv::higurashi::{
        reference_gpu_assets as gpu_assets, reference_render_to_texture as render_to_texture,
    };
    let (device, queue, server) = gpu_assets();
    let atlas: Arc<CharacterTextures> = server.load_sync("/title.txa").unwrap();
    let mut pairs = 0;
    for (case, plus, valid, selected, enabled) in [
        (0, false, false, -1, 0xff),
        (1, false, false, 4, 0x7f),
        (2, true, true, 2, 0x1ff),
        (3, true, false, 8, 0x1ff),
    ] {
        let mut menu = TopMenu::new(plus, valid, selected, (1920, 1080));
        menu.state.enabled = enabled;
        let mut protocol = format!("N {} {} {selected} {enabled}\n", plus as u32, valid as u32);
        let mut reference = Reference::new(plus, valid, selected, enabled);
        let steps = match case {
            0 => vec![],
            1 => vec![(0, 0, 17.0)],
            2 => vec![(0, 0, 60.0), (1 << 24, 0, 1.0)],
            _ => vec![(0, 0, 60.0), (0, 0, 0.0)],
        };
        for (a, r, d) in steps {
            protocol.push_str(&format!("U 1 0 {a} {r} {d}\n"));
            reference.step(true, false, a, r, d);
        }
        if case == 3 {
            protocol.push_str("V 600 890\nP\nU 1 1 0 0 3\n");
            reference.state.probe(vec2(600.0, 890.0));
            reference.state.press(Vec2::ZERO);
            reference.actions();
            reference.step(true, true, 0, 0, 3.0);
        }
        menu.state = reference.state;
        protocol.push_str("R\nD\n");
        let output = native(protocol);
        let packets = output
            .lines()
            .filter_map(|line| line.strip_prefix("P "))
            .map(|line| {
                line.split_whitespace()
                    .map(|v| v.parse::<u32>().unwrap())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        assert_eq!(packets.len(), menu.state.rows.len());
        for size in [(1280, 720), (1920, 1080)] {
            let pixels = render_to_texture(&device, &queue, size, |context, target| {
                let mut pass = context.begin_pass(
                    shin_render::shaders::types::texture::TextureTarget {
                        view: target,
                        kind:
                            shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
                    },
                    Some(context.depth_stencil),
                    "Higurashi top-menu composition",
                );
                pass.clear(Some(UnormColor::WHITE), Some(0), Some(1.0));
                menu.render(&mut pass, &atlas);
            });
            let expected = render_to_texture(&device, &queue, size, |context, target| {
                let mut pass = context.begin_pass(
                    shin_render::shaders::types::texture::TextureTarget {
                        view: target,
                        kind:
                            shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
                    },
                    Some(context.depth_stencil),
                    "Original top-menu packet composition",
                );
                pass.clear(Some(UnormColor::WHITE), Some(0), Some(1.0));
                for p in &packets {
                    assert_eq!(p[1], 2);
                    assert!(p[2] == 2 || p[2] == 5);
                    let mut m = [0.0; 16];
                    for (word, bits) in m.iter_mut().zip(&p[3..19]) {
                        *word = f32::from_bits(*bits);
                    }
                    for i in 0..4 {
                        m[4 * i + 2] = 0.5 * m[4 * i + 2] + 0.5 * m[4 * i + 3];
                    }
                    let transform = glam::Mat4::from_cols_array(&m);
                    let vertices = p[23..]
                        .chunks_exact(6)
                        .map(|v| {
                            let bytes = v[3].to_le_bytes();
                            PosColTexVertex {
                                position: vec3(
                                    f32::from_bits(v[0]),
                                    f32::from_bits(v[1]),
                                    f32::from_bits(v[2]),
                                ),
                                color: UnormColor::from_rgba(
                                    bytes[2], bytes[1], bytes[0], bytes[3],
                                ),
                                texture_position: vec2(f32::from_bits(v[4]), f32::from_bits(v[5])),
                            }
                        })
                        .collect::<Vec<_>>();
                    assert_eq!(vertices.len(), 4);
                    let texture = atlas
                        .get(match p[0] {
                            6 => "t_menu",
                            7 => "o_menu",
                            _ => panic!("native menu texture"),
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
                                    .map(|bits| f32::from_bits(*bits))
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
                "top menu/native packet pixels differ forcase{case} at{size:?}"
            );
            assert!(pixels.chunks_exact(4).all(|p| p[3] == 255));
            pairs += 1;
        }
    }
    eprintln!(
        "Higurashi top render:{pairs} byte-identical native/GPU packet pairs at720p/1080p;cold/partial entrance,disabled tint,normal/append atlas/grid,selection UV,program5 float flash and packed opacity pass"
    );
}
