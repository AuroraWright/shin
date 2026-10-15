use super::*;
use crate::{
    adv::higurashi::{reference_gpu_assets as gpu_assets, reference_render_to_texture},
    app::higurashi::{
        quiz_scene::{
            QuestionId,
            resources::{Loader, Plan},
        },
        tips_menu::tests::source,
    },
    asset::font::GpuFontGlyphHandle,
};
use glam::{vec2, vec3};
use shin_render::{
    DrawPrimitive, RenderProgramWithArguments,
    shaders::types::{
        buffer::VertexSource,
        vertices::{PosColTexVertex, PosColVertex, TextVertex},
    },
};
use std::{
    collections::HashMap,
    io::Write,
    process::{Command, Stdio},
};

struct Case {
    motion: Motion,
    pixels: bool,
}
struct Native {
    sizes: [[f32; 2]; 4],
    draws: Vec<NativeItem>,
}
enum NativeItem {
    Packet(NativeDraw),
    Bustup {
        index: usize,
        opaque: bool,
        matrix: [u32; 23],
    },
}
struct NativeDraw {
    program: u32,
    blend: u32,
    texture: u32,
    point: u32,
    matrix: [u32; 16],
    extra: [u32; 16],
    color: [u32; 16],
    vertices: Vec<u32>,
}
fn render_frame(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    size: (u32, u32),
    draw: impl FnOnce(&mut RenderPass),
) -> Vec<u8> {
    reference_render_to_texture(device, queue, size, |context, target| {
        let mut pass = context.begin_pass(
            shin_render::shaders::types::texture::TextureTarget {
                view: target,
                kind: shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
            },
            Some(context.depth_stencil),
            "Quiz parent reference",
        );
        pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
        draw(&mut pass);
    })
}
fn case(id: u16, profile: usize) -> Case {
    let mut model = Model::new(
        source(100, profile % 2),
        [
            (id as i32 / 60).min(7),
            (profile % 9) as i32,
            (profile % 10) as i32,
        ],
        17,
    )
    .unwrap();
    model.questions = [QuestionId(id); 10];
    model.current_question = (profile % 10) as i32;
    model.answer_order = std::array::from_fn(|i| (i + profile) % 4);
    model.correct_answer = model.answer_order.iter().position(|&i| i == 0).unwrap() as i32;
    model.chosen_answer = (profile % 5) as i32 - 1;
    model.reaction_variant = (profile % 11) as u32;
    model.marks = std::array::from_fn(|i| ((i + profile) % 2) as i32);
    let mut motion = Motion::new(model, false);
    motion.ready = true;
    motion.question_elapsed =
        [0.0, 1.0, 59.5, 60.0, 2999.0, 3000.0, 3001.0, 3599.0, 3600.0][profile % 9];
    motion.model.clocks[1].fast_forward_to([1.0, 1.125, 1.5][profile % 3]);
    motion.model.clocks[2].fast_forward_to(1.0);
    Case {
        motion,
        pixels: id < 480 && id % 60 == 0 || id >= 480,
    }
}
fn cases() -> Vec<Case> {
    let mut rows: Vec<_> = (0..490).map(|id| case(id, id as usize)).collect();
    for phase in 0..5 {
        for opacity in [0.0, 0.5, 1.0] {
            let mut c = case(0, phase as usize);
            c.motion.model.current_question = -1;
            c.motion.question_elapsed = -1.0;
            c.motion.countdown_phase = phase;
            c.motion.model.clocks[1].fast_forward_to(1.5);
            c.motion.model.clocks[2].fast_forward_to(opacity);
            c.pixels = true;
            rows.push(c);
        }
    }
    for ready in [false, true] {
        for fade in [0.0, 0.37, 1.0] {
            let mut c = case(0, 0);
            c.motion.ready = ready;
            c.motion.model.current_question = -1;
            c.motion.question_elapsed = -1.0;
            c.motion.model.clocks[0].fast_forward_to(fade);
            c.pixels = true;
            rows.push(c);
        }
    }
    for chosen in -1..4 {
        for busy in [false, true] {
            let mut c = case(480, 2);
            c.motion.model.chosen_answer = chosen;
            c.motion.model.correct_answer = chosen.max(0);
            c.motion.model.clocks[2].fast_forward_to(0.5);
            if busy {
                c.motion.model.clocks[2]
                    .control(0.5, shin_core::time::Ticks::from_f32(10.0), 0)
                    .unwrap();
            }
            c.pixels = true;
            rows.push(c);
        }
    }
    rows
}
fn native(cases: &[Case]) -> (Vec<Native>, HashMap<u32, String>) {
    use std::fmt::Write as _;
    let mut input = String::new();
    for c in cases {
        let m = &c.motion.model;
        write!(
            input,
            "R {} {} {} {} {} {} {} {:x} {:x} {:x} {:x} {} {}",
            u8::from(c.motion.ready),
            c.motion.countdown_phase,
            m.current_question,
            m.chosen_answer,
            m.correct_answer,
            m.clamped_argument,
            m.reaction_variant,
            c.motion.question_elapsed.to_bits(),
            m.clocks[1].value().to_bits(),
            m.clocks[2].value().to_bits(),
            m.clocks[0].value().to_bits(),
            u8::from(!m.clocks[2].is_idle()),
            m.questions[0].index()
        )
        .unwrap();
        for v in m.answer_order {
            write!(input, " {v}").unwrap();
        }
        for v in &m.marks[..10] {
            write!(input, " {v}").unwrap();
        }
        input.push('\n');
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_QUIZ_SCENE_RENDER").unwrap())
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
    let sent = input.clone();
    let writer = std::thread::spawn(move || stdin.write_all(sent.as_bytes()));
    let output = child.wait_with_output().unwrap();
    let sent = writer.join().unwrap();
    let errors = String::from_utf8(output.stderr).unwrap();
    assert!(output.status.success(), "{errors}");
    sent.unwrap();
    let output = String::from_utf8(output.stdout).unwrap();
    if let Ok(path) = std::env::var("SHIN_HIGURASHI_QUIZ_RENDER_CAPTURE") {
        std::fs::create_dir_all(&path).unwrap();
        for (name, text) in [
            ("input.txt", &input),
            ("output.txt", &output),
            ("stderr.txt", &errors),
        ] {
            std::fs::write(format!("{path}/{name}"), text).unwrap();
        }
    }
    let textures = errors
        .lines()
        .filter_map(|s| {
            let (id, name) = s.strip_prefix("Texture")?.split_once(':')?;
            Some((id.parse().unwrap(), name.to_owned()))
        })
        .collect();
    let mut words = output.split_whitespace();
    let mut rows = Vec::new();
    while let Some(op) = words.next() {
        assert_eq!(op, "R");
        let sizes = std::array::from_fn(|_| {
            std::array::from_fn(|_| f32::from_bits(words.next().unwrap().parse().unwrap()))
        });
        let mut draws = Vec::new();
        loop {
            let op = words.next().unwrap();
            let mut n = || words.next().unwrap().parse::<u32>().unwrap();
            match op {
                "E" => {
                    assert_eq!(
                        n() as usize,
                        draws
                            .iter()
                            .filter(|d| matches!(d, NativeItem::Packet(_)))
                            .count()
                    );
                    assert_eq!(n(), 1);
                    assert_eq!(
                        n() as usize,
                        draws
                            .iter()
                            .filter(|d| matches!(d, NativeItem::Bustup { .. }))
                            .count()
                    );
                    break;
                }
                "B" => {
                    let (index, stencil, pass) = (n(), n(), n());
                    assert_eq!(stencil, 1);
                    assert!(pass <= 1);
                    draws.push(NativeItem::Bustup {
                        index: index as usize,
                        opaque: pass == 1,
                        matrix: std::array::from_fn(|_| n()),
                    });
                }
                "P" => {
                    let (program, blend, texture, point, count) = (n(), n(), n(), n(), n());
                    draws.push(NativeItem::Packet(NativeDraw {
                        program,
                        blend,
                        texture,
                        point,
                        matrix: std::array::from_fn(|_| n()),
                        extra: std::array::from_fn(|_| n()),
                        color: std::array::from_fn(|_| n()),
                        vertices: (0..count).map(|_| n()).collect(),
                    }));
                }
                _ => panic!("Unexpected native renderer op {op}"),
            }
        }
        rows.push(Native { sizes, draws });
    }
    assert_eq!(rows.len(), cases.len());
    (rows, textures)
}
fn check_quad(
    a: &Packet,
    e: &NativeDraw,
    names: &HashMap<u32, String>,
    name: &str,
    index: usize,
    j: usize,
) {
    assert_eq!(
        if name.is_empty() { 1 } else { 2 },
        e.program,
        "case{index} packet{j} program"
    );
    assert_eq!(
        if a.opaque { 1 } else { 2 },
        e.blend,
        "case{index} packet{j} blend"
    );
    if !name.is_empty() {
        assert_eq!(name, names[&e.point], "case{index} packet{j} texture");
    }
    assert_eq!(
        a.matrix.to_cols_array().map(f32::to_bits),
        e.matrix,
        "case{index} packet{j} {name} matrix"
    );
    let stride = if e.program == 1 { 4 } else { 6 };
    for vertex in 0..4 {
        let [l, t, r, b] = a.rect;
        let [u, v, s, w] = a.uv;
        assert_eq!(
            [
                if vertex % 2 == 0 { l } else { r },
                if vertex < 2 { t } else { b },
                0.0
            ]
            .map(f32::to_bits),
            e.vertices[stride * vertex..stride * vertex + 3],
            "case{index} packet{j} position"
        );
        assert_eq!(
            a.color,
            e.vertices[stride * vertex + 3],
            "case{index} packet{j} color"
        );
        if stride == 6 {
            assert_eq!(
                [
                    if vertex % 2 == 0 { u } else { s },
                    if vertex < 2 { v } else { w }
                ]
                .map(f32::to_bits),
                e.vertices[stride * vertex + 4..stride * vertex + 6],
                "case{index} packet{j} uv"
            );
        }
    }
}
fn check_packets(
    case: &Case,
    native: &Native,
    names: &HashMap<u32, String>,
    index: usize,
) -> usize {
    let actual = packets(&case.motion);
    assert!(matches!(actual.last(), Some(Draw::Children)));
    let actual: Vec<_> = actual
        .iter()
        .filter(|d| !matches!(d, Draw::Answers(_) | Draw::Children))
        .collect();
    let expected: Vec<_> = native
        .draws
        .iter()
        .filter(|d| !matches!(d,NativeItem::Packet(p) if p.program==3||p.program==4))
        .collect();
    assert_eq!(actual.len(), expected.len(), "case{index} sprite/BUP count");
    for (j, (a, e)) in actual.iter().zip(expected).enumerate() {
        match (a, e) {
            (Draw::Quad(a), NativeItem::Packet(e)) => check_quad(a, e, names, &a.texture, index, j),
            (
                Draw::Question {
                    index: slot,
                    packet: a,
                },
                NativeItem::Packet(e),
            ) => check_quad(a, e, names, &format!("question:{slot}"), index, j),
            (
                Draw::Bustup { index: i, opaque },
                NativeItem::Bustup {
                    index: k,
                    opaque: pass,
                    matrix,
                },
            ) => {
                assert_eq!(i, k);
                assert_eq!(opaque, pass);
                let mut expected = [0u32; 23];
                expected[..16].copy_from_slice(&Mat4::IDENTITY.to_cols_array().map(f32::to_bits));
                assert_eq!(*matrix, expected);
            }
            _ => panic!("case{index} item{j} native draw order"),
        }
    }
    let has_text = native
        .draws
        .iter()
        .any(|d| matches!(d,NativeItem::Packet(p) if p.program==3));
    assert_eq!(
        has_text,
        case.motion.ready
            && case.motion.model.current_question >= 0
            && case.motion.model.clocks[2].is_idle()
    );
    actual.len()
}
fn replay(
    pass: &mut RenderPass,
    native: &Native,
    names: &HashMap<u32, String>,
    atlas: &CharacterTextures,
    prepared: &Prepared,
    glyphs: &HashMap<u32, GpuFontGlyphHandle>,
) {
    let color = |words: &[u32]| {
        shin_core::primitives::color::FloatColor4::from_vec4(Vec4::from_array(std::array::from_fn(
            |i| f32::from_bits(words[i]),
        )))
    };
    for item in &native.draws {
        let d = match item {
            NativeItem::Packet(d) => d,
            NativeItem::Bustup { index, opaque, .. } => {
                prepared.bustups[*index]
                    .render(
                        pass,
                        &Default::default(),
                        1,
                        if *opaque {
                            PassKind::Opaque
                        } else {
                            PassKind::Transparent
                        },
                    )
                    .unwrap();
                continue;
            }
        };
        let transform = HigurashiTransform::convert_clip_depth(Mat4::from_cols_array(
            &d.matrix.map(f32::from_bits),
        ));
        let builder = RenderRequestBuilder::new().color_blend_type(if d.blend == 1 {
            ColorBlendType::Opaque
        } else {
            ColorBlendType::Layer1
        });
        if d.program == 1 {
            let vertices: [PosColVertex; 4] = std::array::from_fn(|i| PosColVertex {
                position: vec3(
                    f32::from_bits(d.vertices[4 * i]),
                    f32::from_bits(d.vertices[4 * i + 1]),
                    f32::from_bits(d.vertices[4 * i + 2]),
                ),
                color: UnormColor(d.vertices[4 * i + 3]),
            });
            pass.run(builder.build(
                RenderProgramWithArguments::Fill {
                    vertices: VertexSource::VertexData {
                        vertices: &vertices,
                    },
                    transform,
                },
                DrawPrimitive::TrianglesStrip,
            ));
        } else if d.texture == 0 {
            let vertices: [PosColTexVertex; 4] = std::array::from_fn(|i| PosColTexVertex {
                position: vec3(
                    f32::from_bits(d.vertices[6 * i]),
                    f32::from_bits(d.vertices[6 * i + 1]),
                    f32::from_bits(d.vertices[6 * i + 2]),
                ),
                color: UnormColor(d.vertices[6 * i + 3]),
                texture_position: vec2(
                    f32::from_bits(d.vertices[6 * i + 4]),
                    f32::from_bits(d.vertices[6 * i + 5]),
                ),
            });
            let vertices = VertexSource::VertexData {
                vertices: &vertices,
            };
            let name = &names[&d.point];
            let texture = if let Some(slot) = name.strip_prefix("question:") {
                prepared.questions[slot.parse::<usize>().unwrap()]
                    .first_texture()
                    .as_source()
            } else {
                atlas.get(name).as_source()
            };
            let program = if d.program == 2 {
                RenderProgramWithArguments::Sprite {
                    vertices,
                    sprite: texture,
                    transform,
                }
            } else {
                assert_eq!(d.program, 5);
                RenderProgramWithArguments::Button {
                    vertices,
                    texture,
                    transform,
                    flash1: Vec4::from_array(std::array::from_fn(|i| f32::from_bits(d.extra[i]))),
                    flash2: Vec4::ZERO,
                }
            };
            pass.run(builder.build(program, DrawPrimitive::TrianglesStrip));
        } else {
            assert_eq!(d.texture, 2);
            let vertices: [TextVertex; 4] = std::array::from_fn(|i| TextVertex {
                position: Vec4::from_array(std::array::from_fn(|j| {
                    f32::from_bits(d.vertices[5 * i + j])
                })),
                color: f32::from_bits(d.vertices[5 * i + 4]),
            });
            let vertices = VertexSource::VertexData {
                vertices: &vertices,
            };
            let glyph = glyphs[&d.point].as_texture_source().unwrap();
            let program = if d.program == 3 {
                RenderProgramWithArguments::Font {
                    vertices,
                    glyph,
                    transform,
                    color1: color(&d.extra),
                    color2: color(&d.color),
                }
            } else {
                assert_eq!(d.program, 4);
                RenderProgramWithArguments::FontBorder {
                    vertices,
                    glyph,
                    transform,
                    distances: std::array::from_fn(|i| {
                        vec2(
                            f32::from_bits(d.extra[2 * i]),
                            f32::from_bits(d.extra[2 * i + 1]),
                        )
                    }),
                    color: color(&d.color),
                }
            };
            pass.run(builder.build(program, DrawPrimitive::TrianglesStrip));
        }
    }
}
#[test]
#[ignore = "original updated9d1b0/SimpleText with guarded native executable/font;explicit BUP/atlas/GPU/child boundaries,no audio/window/saves"]
fn higurashi_quiz_parent_packets_match_native() {
    let cases = cases();
    let (native, names) = native(&cases);
    let mut draws = 0;
    for (i, (case, native)) in cases.iter().zip(&native).enumerate() {
        draws += check_packets(case, native, &names, i);
    }
    for name in [
        "count",
        "start",
        "timeup",
        "ok",
        "ng",
        "sig10_on",
        "sig10_off",
    ] {
        assert!(names.values().any(|v| v == name));
    }
    println!(
        "Quiz parent:{} native profiles/{draws} exact sprite/fill/BUP-call packets;490 authored descriptors,all11 BUP selections/countdown/question/end/answer gates/marks/fade,final child boundary and balanced native font/buffer owners pass",
        cases.len()
    );
}
#[test]
#[ignore = "real quiz/question/BUP/font offscreen Metal with original9d1b0/SimpleText packet replay at720p/1080p;BUP/GPU/child services,no audio/window/saves"]
fn higurashi_quiz_parent_pixels_match_native_at_720p_and_1080p() {
    let cases: Vec<_> = cases().into_iter().filter(|c| c.pixels).collect();
    let (native, names) = native(&cases);
    let (device, queue, assets) = gpu_assets();
    let assets = Arc::new(assets);
    let font: Arc<GpuFontLazy> = assets.load_sync("/newrodin.fnt").unwrap();
    let atlas: Arc<CharacterTextures> = assets.load_sync("/quiz.txa").unwrap();
    let scenario: Arc<shin_core::format::scenario::higurashi::HigurashiScenario> =
        assets.load_sync("/main.snr").unwrap();
    let points: Vec<_> = native
        .iter()
        .flat_map(|n| &n.draws)
        .filter_map(|d| match d {
            NativeItem::Packet(d) if d.texture == 2 => Some(d.point),
            _ => None,
        })
        .collect();
    let glyphs: HashMap<_, _> = points
        .iter()
        .copied()
        .zip(
            font.clone().load_glyphs(
                device.clone(),
                queue.clone(),
                &points
                    .iter()
                    .map(|&p| char::from_u32(p).unwrap())
                    .collect::<Vec<_>>(),
            ),
        )
        .collect();
    let mut frames = 0;
    for (i, (case, native)) in cases.iter().zip(&native).enumerate() {
        check_packets(case, native, &names, i);
        let mut loader = Loader::new(
            assets.clone(),
            scenario.clone(),
            Plan::new(&case.motion.model, &scenario).unwrap(),
            || 17,
        );
        let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while loader.poll().unwrap() != 2 {
            assert!(
                std::time::Instant::now() < end,
                "Quiz pixel resources timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        loader.publish().unwrap();
        loader.retire_first();
        let prepared = loader.prepared.as_ref().unwrap();
        let mut view = View::new(font.clone());
        view.synchronize(&assets, &case.motion.model).unwrap();
        for (j, text) in view.answers.iter().enumerate() {
            assert_eq!(
                text.layout.size.to_array().map(f32::to_bits),
                native.sizes[j].map(f32::to_bits),
                "case{i} answer{j} layout"
            );
        }
        for size in [(1280, 720), (1920, 1080)] {
            let actual = render_frame(&device, &queue, size, |pass| {
                view.render(&case.motion, Some(&atlas), Some(prepared), pass)
                    .unwrap()
            });
            let expected = render_frame(&device, &queue, size, |pass| {
                replay(pass, native, &names, &atlas, prepared, &glyphs)
            });
            let errors: Vec<_> = actual
                .iter()
                .zip(&expected)
                .enumerate()
                .filter(|(_, (a, b))| a != b)
                .collect();
            assert!(
                errors.is_empty(),
                "case{i} at{size:?}:{} differing RGBA channels;first{:?}",
                errors.len(),
                errors.first()
            );
            frames += 1;
        }
    }
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    println!(
        "Quiz parent:{frames} byte-identical full RGBA frames at720p/1080p;real quiz/question atlases/BUP expressions/font and original9d1b0/SimpleText packet replay pass;BUP rendering/GPU/child boundaries,no audio/window/saves"
    );
}
