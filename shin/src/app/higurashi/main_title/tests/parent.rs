use super::*;
use crate::app::higurashi::main_title::{
    particles::Particles,
    view::{self},
};
fn native(protocol: String) -> String {
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_TITLE_PARENT").unwrap())
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
fn state(p: &Particles, warm: bool) -> Vec<u32> {
    let mut words = vec![
        p.seed,
        p.spawn.to_bits(),
        p.retired as u32,
        if warm { p.warmup_steps } else { 0 },
        p.items.len() as u32,
    ];
    for item in &p.items {
        words.extend(item.words());
    }
    words
}
#[test]
#[ignore = "actual updated title seed/MT/particle field and native parent vertex/matrix/tint/draw packets"]
fn higurashi_title_parent_field_and_draw_packets_match_native() {
    let mut protocol = String::from("M 0\nM 1\n");
    let mut expected = vec![("M", vec![0]), ("M", vec![1])];
    let (mut fields, mut observations, mut rendered, mut packets) = (0, 0, 0, 0);
    for seed in [0, 1, 5489, u32::MAX] {
        protocol.push_str(&format!("G {seed} 1300\n"));
        let mut random = crate::engine_random::EngineRandom::from_seed(seed);
        expected.push(("G", (0..1300).map(|_| random.next()).collect()));
    }
    for seed in [
        0,
        1,
        42,
        5489,
        0x7ffffffe,
        0x7fffffff,
        0x80000000,
        0xfffffffe,
        u32::MAX,
    ] {
        for warm in [false, true] {
            protocol.push_str(&format!("N {seed} {}\n", warm as u32));
            let mut particles = if warm {
                Particles::warm(seed)
            } else {
                Particles::cold(seed)
            };
            expected.push(("S", state(&particles, warm)));
            fields += 1;
            observations += 1;
            for (step, delta) in [
                0.0, 0.125, 0.5, 1.0, 3.0, 59.5, 60.0, 0.125, 120.0, 1000.0, 0.0,
            ]
            .into_iter()
            .enumerate()
            {
                protocol.push_str(&format!("U {delta}\n"));
                particles.advance(Ticks::from_f32(delta));
                expected.push(("S", state(&particles, false)));
                observations += 1;
                for (atlas, plus) in [(true, false), (true, true), (false, false)] {
                    let frame = Frame {
                        background_offset: if step % 2 == 0 { -200.0 } else { -73.25 },
                        caption_alpha: 0.37,
                        logo_alpha: [0.0, 0.37, 1.0, 1.7][step % 4],
                        logo_offset: -30.0,
                        black_alpha: if step % 3 == 0 { 0.23 } else { 0.0 },
                    };
                    protocol.push_str(&format!(
                        "R {} {} {} {} {} {} {}\n",
                        atlas as u32,
                        plus as u32,
                        frame.background_offset,
                        frame.caption_alpha,
                        frame.logo_alpha,
                        frame.logo_offset,
                        frame.black_alpha
                    ));
                    let values = view::packets(atlas, plus, &frame, &particles);
                    packets += values.len();
                    for value in &values {
                        expected.push(("P", value.words()));
                    }
                    expected.push(("R", vec![values.len() as u32, 1]));
                    rendered += 1;
                }
            }
            protocol.push_str("D\n");
        }
    }
    let output = native(protocol);
    assert_eq!(output.lines().count(), expected.len());
    for (index, (line, (tag, words))) in output.lines().zip(&expected).enumerate() {
        let mut values = line.split_whitespace();
        assert_eq!(values.next(), Some(*tag));
        assert_eq!(
            values
                .map(|v| v.parse::<u32>().unwrap())
                .collect::<Vec<_>>(),
            *words,
            "native parent observation{index},tag{tag}"
        );
    }
    eprintln!(
        "Higurashi title parent:{fields} fields/{observations} state observations/{rendered} frames/{packets} draw packets/{} exact words;seed normalization,4x1300 MT outputs,warmup,fractional-step full Y movement,spawn/retirement,background/particles/credit/logo/blackout packets and append marker pass",
        expected.iter().map(|(_, w)| w.len()).sum::<usize>()
    );
}

#[test]
#[ignore = "actual title atlas,real startup/movie handoff and original native parent packets at720p/1080p;offscreen Metal and the default audio output"]
fn higurashi_title_parent_real_frames_match_native_packet_composition() {
    use crate::{
        adv::higurashi::{
            input::Buttons, reference_gpu_assets as gpu_assets,
            reference_render_to_texture as render_to_texture,
        },
        app::higurashi::{load_game::Paths, scene_host::SceneHost},
        audio::SystemSoundPlayer,
    };
    use parking_lot::Mutex;
    use shin_core::primitives::{color::UnormColor, update::FrameId};
    use shin_render::{
        ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
        shaders::types::{
            buffer::VertexSource,
            vertices::{PosColTexVertex, PosColVertex},
        },
    };
    let (device, queue, server) = gpu_assets();
    let server = Arc::new(server);
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let sounds = Arc::new(Mutex::new(SystemSoundPlayer::new(manager.clone())));
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
        let result = host
            .update(
                id,
                Ticks::from_f32(3.0),
                Buttons {
                    pressed: 1 << 24,
                    ..Default::default()
                },
                &crate::adv::input_area::PointerFrame {
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
    let calls = crate::engine_random::seed_call_count();
    let mut opening =
        crate::engine_random::with_reference_seed(5489, || Opening::new(transition, None));
    assert_eq!(crate::engine_random::seed_call_count(), calls + 1);
    assert!(opening.particles.warmup_steps > 0);
    assert_eq!(opening.plus, server.contains("/append.txt"));
    assert!(!server.contains("/missing-title-marker-reference.txt"));
    while !opening.ready {
        opening.update(Ticks::ZERO).unwrap();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    let seed = 42;
    let mut pairs = 0;
    for (plus, background, alpha, offset, black) in [
        (false, -200.0, 0.0, -30.0, 0.0),
        (true, -73.25, 0.37, -12.0, 0.0),
        (false, 0.0, 1.0, 0.0, 0.23),
        (true, 0.0, 1.0, 0.0, 1.0),
    ] {
        opening.plus = plus;
        opening.particles = Particles::warm(seed);
        opening.particles.advance(Ticks::from_f32(3.0));
        for (clock, value) in opening
            .motion
            .clocks
            .iter_mut()
            .zip([background, 0.0, alpha, offset, black])
        {
            clock.fast_forward_to(value);
        }
        let output = native(format!(
            "N {seed} 1\nU 3\nR 1 {} {background} 0 {alpha} {offset} {black}\nD\n",
            plus as u32
        ));
        let packets = output
            .lines()
            .filter_map(|line| line.strip_prefix("P "))
            .map(|line| {
                line.split_whitespace()
                    .map(|v| v.parse::<u32>().unwrap())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        assert!(packets.len() > 3);
        for size in [(1280, 720), (1920, 1080)] {
            let pixels = render_to_texture(&device, &queue, size, |context, target| {
                let mut pass = context.begin_pass(
                    shin_render::shaders::types::texture::TextureTarget {
                        view: target,
                        kind:
                            shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
                    },
                    Some(context.depth_stencil),
                    "Higurashi actual title parent",
                );
                opening.render(&mut pass);
            });
            let reference = render_to_texture(&device, &queue, size, |context, target| {
                let mut pass = context.begin_pass(
                    shin_render::shaders::types::texture::TextureTarget {
                        view: target,
                        kind:
                            shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
                    },
                    Some(context.depth_stencil),
                    "Original native title draw packets",
                );
                pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
                for words in &packets {
                    let name = match words[0] {
                        0 => "",
                        1 => "title_bg",
                        2 => "light1",
                        3 => "credit",
                        4 => "logo",
                        5 => "logo_plus",
                        _ => panic!("native texture"),
                    };
                    let mut matrix = [0.0; 16];
                    for (v, bits) in matrix.iter_mut().zip(&words[3..19]) {
                        *v = f32::from_bits(*bits);
                    }
                    // Independently adapt the native clip depth at each column.
                    for i in 0..4 {
                        matrix[i * 4 + 2] = 0.5 * matrix[i * 4 + 2] + 0.5 * matrix[i * 4 + 3];
                    }
                    let transform = glam::Mat4::from_cols_array(&matrix);
                    let builder = RenderRequestBuilder::new().color_blend_type(if words[1] == 1 {
                        ColorBlendType::Opaque
                    } else {
                        ColorBlendType::Layer1
                    });
                    let vertices = words[20..]
                        .chunks_exact(if words[2] == 1 { 4 } else { 6 })
                        .map(|v| {
                            let bytes = v[3].to_le_bytes();
                            PosColTexVertex {
                                position: glam::vec3(
                                    f32::from_bits(v[0]),
                                    f32::from_bits(v[1]),
                                    f32::from_bits(v[2]),
                                ),
                                color: UnormColor::from_rgba(
                                    bytes[2], bytes[1], bytes[0], bytes[3],
                                ),
                                texture_position: if v.len() == 6 {
                                    glam::vec2(f32::from_bits(v[4]), f32::from_bits(v[5]))
                                } else {
                                    glam::Vec2::ZERO
                                },
                            }
                        })
                        .collect::<Vec<_>>();
                    if words[2] == 1 {
                        let fill = vertices
                            .iter()
                            .map(|v| PosColVertex {
                                position: v.position,
                                color: v.color,
                            })
                            .collect::<Vec<_>>();
                        pass.run(builder.build(
                            RenderProgramWithArguments::Fill {
                                vertices: VertexSource::VertexData { vertices: &fill },
                                transform,
                            },
                            DrawPrimitive::TrianglesStrip,
                        ));
                    } else {
                        pass.run(builder.build(
                            RenderProgramWithArguments::Sprite {
                                vertices: VertexSource::VertexData {
                                    vertices: &vertices,
                                },
                                sprite: opening.atlas.as_ref().unwrap().get(name).as_source(),
                                transform,
                            },
                            DrawPrimitive::TrianglesStrip,
                        ));
                    }
                }
            });
            assert_eq!(
                pixels, reference,
                "native parent composition at{size:?},plus{plus},black{black}"
            );
            assert!(pixels.chunks_exact(4).all(|p| p[3] == 255));
            if black == 1.0 {
                assert!(pixels.chunks_exact(4).all(|p| p == [0, 0, 0, 255]));
            } else {
                assert!(pixels.chunks_exact(4).any(|p| p[..3] != [0, 0, 0]));
            }
            pairs += 1;
        }
    }
    eprintln!(
        "Higurashi title parent integration:{pairs} actual/native packet pixel pairs at720p/1080p;real title atlas,prewarmed lights,translated background,both logos,packed alpha/blackout and one shared seed request pass"
    );
}
