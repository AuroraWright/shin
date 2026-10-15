use super::*;
use crate::adv::higurashi::{
    message_glyphs::tests::gpu::{gpu_assets, render_frame},
    system_menu::{Confirmation, Cue, MenuServices},
};
use shin_core::primitives::color::UnormColor;
use std::{
    collections::HashMap,
    io::Write,
    process::{Command as Process, Stdio},
};

struct Services;
impl MenuServices for Services {
    fn enter_modal(&mut self) {}
    fn exit_modal(&mut self) {}
    fn cue(&mut self, _: Cue) {}
    fn push_confirmation(&mut self, _: Confirmation) {}
    fn push_layer_reloader(&mut self) {}
    fn dispatch_item(&mut self, _: u32) {}
}
struct Case {
    model: SystemMenu,
    title: &'static str,
    ticks: f64,
}
struct Packet {
    title_width: f32,
    time_width: f32,
    draws: Vec<Draw>,
}
struct Draw {
    program: u32,
    texture: u32,
    point: u32,
    matrix: [u32; 16],
    extra: [u32; 16],
    color: [u32; 16],
    vertices: Vec<u32>,
}
fn native(cases: &[Case]) -> Vec<Packet> {
    let hex = |text: &str| {
        if text.is_empty() {
            "-".to_owned()
        } else {
            text.bytes().map(|b| format!("{b:02x}")).collect()
        }
    };
    let mut input = String::new();
    for (id, case) in [5, 0, 11001, 12001].into_iter().cycle().zip(cases) {
        input += &format!(
            "R {id} {} {} {:x} {:x} {} {}\n",
            case.model.phase(),
            case.model.selected(),
            case.model.detail_slide().to_bits(),
            case.model.detail_fade().to_bits(),
            hex(case.title),
            hex(&super::super::play_time_text(case.ticks))
        );
        for row in case.model.rows() {
            input += &format!(
                "{:x} {:x}\n",
                row.slide.value().to_bits(),
                row.flash.value().to_bits()
            );
        }
    }
    let mut child = Process::new(std::env::var("SHIN_HIGURASHI_NATIVE_MENU_RENDER").unwrap())
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
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = String::from_utf8(output.stdout).unwrap();
    let mut words = output.split_whitespace();
    let mut packets = Vec::new();
    while let Some(op) = words.next() {
        assert_eq!(op, "R");
        let mut number = || words.next().unwrap().parse::<u32>().unwrap();
        let mut packet = Packet {
            title_width: f32::from_bits(number()),
            time_width: f32::from_bits(number()),
            draws: Vec::new(),
        };
        loop {
            let op = words.next().unwrap();
            let mut number = || words.next().unwrap().parse::<u32>().unwrap();
            if op == "E" {
                assert_eq!(number() as usize, packet.draws.len());
                break;
            }
            assert_eq!(op, "P");
            let program = number();
            let texture = number();
            let point = number();
            let count = number();
            packet.draws.push(Draw {
                program,
                texture,
                point,
                matrix: std::array::from_fn(|_| number()),
                extra: std::array::from_fn(|_| number()),
                color: std::array::from_fn(|_| number()),
                vertices: (0..count).map(|_| number()).collect(),
            });
        }
        packets.push(packet);
    }
    assert_eq!(packets.len(), cases.len());
    packets
}
fn replay(
    pass: &mut RenderPass,
    packet: &Packet,
    atlas: &CharacterTextures,
    glyphs: &HashMap<u32, GpuFontGlyphHandle>,
) {
    let color = |words: &[u32]| {
        FloatColor4::from_rgba(
            f32::from_bits(words[0]),
            f32::from_bits(words[1]),
            f32::from_bits(words[2]),
            f32::from_bits(words[3]),
        )
    };
    for draw in &packet.draws {
        let clip = Mat4::from_cols_array(&[
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.5, 1.0,
        ]);
        let transform = clip * Mat4::from_cols_array(&draw.matrix.map(f32::from_bits));
        let builder = RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1);
        if draw.texture < 2 {
            assert_eq!(draw.vertices.len(), 24);
            let vertices: [PosColTexVertex; 4] = std::array::from_fn(|i| PosColTexVertex {
                position: vec3(
                    f32::from_bits(draw.vertices[6 * i]),
                    f32::from_bits(draw.vertices[6 * i + 1]),
                    f32::from_bits(draw.vertices[6 * i + 2]),
                ),
                color: UnormColor(draw.vertices[6 * i + 3]),
                texture_position: vec2(
                    f32::from_bits(draw.vertices[6 * i + 4]),
                    f32::from_bits(draw.vertices[6 * i + 5]),
                ),
            });
            let texture = atlas
                .get(if draw.texture == 0 { "sysmenu" } else { "info" })
                .as_source();
            let vertices = VertexSource::VertexData {
                vertices: &vertices,
            };
            let program = match draw.program {
                2 => RenderProgramWithArguments::Sprite {
                    vertices,
                    sprite: texture,
                    transform,
                },
                5 => RenderProgramWithArguments::Button {
                    vertices,
                    texture,
                    transform,
                    flash1: Vec4::from_array(std::array::from_fn(|i| {
                        f32::from_bits(draw.extra[i])
                    })),
                    flash2: Vec4::from_array(std::array::from_fn(|i| {
                        f32::from_bits(draw.color[i])
                    })),
                },
                _ => panic!("unexpected atlas program"),
            };
            pass.run(builder.build(program, DrawPrimitive::TrianglesStrip));
        } else {
            assert_eq!(draw.texture, 2);
            assert_eq!(draw.vertices.len(), 20);
            let vertices: [TextVertex; 4] = std::array::from_fn(|i| TextVertex {
                position: Vec4::from_array(std::array::from_fn(|j| {
                    f32::from_bits(draw.vertices[5 * i + j])
                })),
                color: f32::from_bits(draw.vertices[5 * i + 4]),
            });
            let vertices = VertexSource::VertexData {
                vertices: &vertices,
            };
            let glyph = glyphs[&draw.point].as_texture_source().unwrap();
            let program = match draw.program {
                3 => RenderProgramWithArguments::Font {
                    vertices,
                    glyph,
                    transform,
                    color1: color(&draw.extra),
                    color2: color(&draw.color),
                },
                4 => RenderProgramWithArguments::FontBorder {
                    vertices,
                    glyph,
                    transform,
                    distances: std::array::from_fn(|i| {
                        vec2(
                            f32::from_bits(draw.extra[2 * i]),
                            f32::from_bits(draw.extra[2 * i + 1]),
                        )
                    }),
                    color: color(&draw.color),
                },
                _ => panic!("unexpected text program"),
            };
            pass.run(builder.build(program, DrawPrimitive::TrianglesStrip));
        }
    }
}

#[test]
#[ignore = "replays original updated menu draw and SimpleText packets with real assets on Metal"]
fn higurashi_system_menu_pixels_match_native_at_720p_and_1080p() {
    let mut cases = Vec::new();
    for i in 0..20 {
        let id = [5, 0, 11001, 12001][i % 4];
        let mut model = SystemMenu::new(id, 0, &mut Services);
        model.state.phase = match i {
            0 => 0,
            1 => -1,
            2 => 2,
            _ => 1,
        };
        model.state.selected = i % model.state.rows.len();
        model
            .state
            .detail_slide
            .fast_forward_to([0.0, 0.37, 1.0][i % 3]);
        model
            .state
            .detail_fade
            .fast_forward_to([1.0, 0.37, 0.0, 1.0][i % 4]);
        for (index, row) in model.state.rows.iter_mut().enumerate() {
            row.slide.fast_forward_to(if i < 4 {
                1.0
            } else {
                [0.0, 0.37, 1.0][(i + index) % 3]
            });
            row.flash.fast_forward_to(if index == model.state.selected {
                [0.0, 0.35, 0.7][i % 3]
            } else {
                0.0
            });
        }
        cases.push(Case {
            model,
            title: [
                "",
                "鬼隠し編",
                "@c$F00.色と 空白@r二行",
                "長い題名の文字列を並べて幅の制限と縮小を確かめる",
            ][i % 4],
            ticks: if i % 2 == 0 { 0.0 } else { 60.0 * 3661.0 },
        });
    }
    let packets = native(&cases);
    assert!(
        packets
            .iter()
            .any(|p| p.draws.iter().any(|d| d.program == 3))
    );
    assert!(
        packets
            .iter()
            .any(|p| p.draws.iter().any(|d| d.program == 4))
    );
    let (device, queue, assets) = gpu_assets();
    let font: Arc<GpuFontLazy> = shin_tasks::block_on(assets.load("/newrodin.fnt")).unwrap();
    let points: Vec<_> = packets
        .iter()
        .flat_map(|p| p.draws.iter())
        .filter(|d| d.texture == 2)
        .map(|d| d.point)
        .collect();
    let glyphs: HashMap<_, _> = points
        .iter()
        .copied()
        .zip(
            font.load_glyphs(
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
    for (index, (case, packet)) in cases.iter().zip(&packets).enumerate() {
        let mut view = shin_tasks::block_on(View::load(&assets, case.title, 0.0)).unwrap();
        view.update_time(&assets, case.ticks);
        let title_width = view.title.as_ref().map_or(0.0, |t| t.size.x);
        assert!(
            (title_width - packet.title_width).abs() < 0.001,
            "title extent case{index}: {title_width} vs {}",
            packet.title_width
        );
        assert!(
            (view.time.size.x - packet.time_width).abs() < 0.001,
            "time extent case{index}: {} vs {}",
            view.time.size.x,
            packet.time_width
        );
        for size in [(1280, 720), (1920, 1080)] {
            let actual = render_frame(&device, &queue, size, |pass| view.render(&case.model, pass));
            let expected = render_frame(&device, &queue, size, |pass| {
                replay(pass, packet, &view.atlas, &glyphs)
            });
            let errors: Vec<_> = actual
                .iter()
                .zip(&expected)
                .enumerate()
                .filter(|(_, (a, b))| a != b)
                .collect();
            assert!(
                errors.is_empty(),
                "case{index} at{size:?}: {} differing channels; first {:?}",
                errors.len(),
                errors.first()
            );
            if case.model.phase() < 1 {
                assert!(actual.chunks_exact(4).all(|p| p == [0, 0, 0, 255]));
            } else {
                assert!(actual.chunks_exact(4).any(|p| p[..3] != [0, 0, 0]));
            }
            frames += 1;
        }
    }
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    assert_eq!(frames, 40);
    println!(
        "Higurashi SystemMenu:40 whole RGBA frames match original draw/SimpleText packets at720p/1080p; rows, panels, glyphs, selected flash, slide, fade, phases and ownership pass"
    );
}
