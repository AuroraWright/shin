use super::*;
use crate::adv::higurashi::{reference_gpu_assets, reference_render_to_texture};
use crate::layer::higurashi_properties::INITIAL_VALUES;
use shin_core::primitives::color::UnormColor;
use shin_render::{
    LayerShaderOutputKind,
    gpu_texture::GpuTexture,
    shaders::types::texture::{TextureTarget, TextureTargetKind},
};
use std::{
    io::Write,
    process::{Command, Stdio},
};

pub(in crate::app::higurashi::save_menu::preview) mod gpu;
use gpu::NativeShader;

struct Case {
    canvas: (u32, u32),
    values: [[i32; 65]; 3],
    tile: i32,
}
impl Case {
    fn new(canvas: (u32, u32)) -> Self {
        let mut values = [INITIAL_VALUES; 3];
        values[2][0] = -640;
        values[2][1] = 720;
        Self {
            canvas,
            values,
            tile: 0xf123,
        }
    }
    fn scene(&self) -> Scene {
        let mut owner = HigurashiUserLayer::tile(
            FloatColor4::from_4bpp_property(self.tile),
            Vec4::new(0., 0., 1280., 720.),
            [0xabc00011, 0xabc00012, 0xabc00013, 0xabc00014],
        );
        owner.properties = targets(
            &self.values[2],
            [0xabc00011, 0xabc00012, 0xabc00013, 0xabc00014],
        );
        Scene {
            layers: vec![(0, 101, Layer::Basic(owner))],
            root: targets(
                &self.values[0],
                [0xabc00001, 0xabc00002, 0xabc00003, 0xabc00004],
            ),
            screen: targets(&self.values[1], [1, 2, 0xabc0000b, 0xabc0000c]),
        }
    }
}
fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for canvas in [(1280, 720), (1920, 1080)] {
        for owner in 0..2 {
            for operation in 0..8 {
                for (variant, param) in [
                    [1000; 4],
                    [700, 200, 1600, 650],
                    [0; 4],
                    [1300, 900, 1100, 0],
                ]
                .into_iter()
                .enumerate()
                {
                    let mut c = Case::new(canvas);
                    c.values[owner][6] = 300 + variant as i32 * 400;
                    c.values[owner][7] = 1700;
                    c.values[owner][8] = 800;
                    c.values[owner][21] = operation;
                    c.values[owner][23..27].copy_from_slice(&param);
                    if variant == 3 {
                        c.tile = 0x8123;
                    }
                    cases.push(c);
                }
            }
        }
        for (owner, property, value) in [
            (0, 9, 0),
            (0, 9, -1000),
            (0, 9, 500),
            (0, 12, 0),
            (0, 13, 0),
            (0, 19, 0),
            (0, 20, 2),
            (1, 9, 0),
            (1, 9, 500),
            (1, 12, 0),
            (1, 13, 0),
            (1, 19, 0),
        ] {
            let mut c = Case::new(canvas);
            c.values[owner][property] = value;
            cases.push(c);
        }
        for shader in [0, 3] {
            let mut c = Case::new(canvas);
            c.values[0][6] = 700;
            c.values[1][6] = 300;
            c.values[1][21] = shader;
            c.values[1][23..27].copy_from_slice(&[200, 700, 400, 800]);
            cases.push(c);
        }
        for operation in 0..8 {
            let mut c = Case::new(canvas);
            c.values[0][21] = operation;
            c.values[0][26] = 0;
            if operation == 3 {
                c.values[0][23..26].fill(0);
            }
            cases.push(c);
        }
        let mut c = Case::new(canvas);
        c.values[0][39] = 600; // Fresh b8454 has no previous afterimage resource.
        cases.push(c);
    }
    cases
}

#[derive(Clone)]
struct Draw {
    program: u32,
    blend: u32,
    texture: u32,
    primitive: u32,
    count: u32,
    vertex_bytes: u32,
    words: [u32; 72],
}
fn native(cases: &[Case]) -> Vec<Vec<Draw>> {
    let mut input = String::new();
    for c in cases {
        let (w, h) = resources::thumbnail_size(c.canvas);
        input += &format!("G {w} {h} {}", c.tile);
        for values in c.values {
            for value in values {
                input += &format!(" {value}");
            }
        }
        input.push('\n');
    }
    let mut child =
        Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_RESTORED_FRAMEBUFFER").unwrap())
            .args([
                std::env::var("SHIN_HIGURASHI_EXE").unwrap(),
                std::env::var("SHIN_HIGURASHI_FONT").unwrap(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
    let copy = input.clone();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let result = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    if let Ok(dir) = std::env::var("SHIN_HIGURASHI_POSTPROCESS_CAPTURE") {
        let dir = std::path::Path::new(&dir);
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("input.txt"), copy).unwrap();
        std::fs::write(dir.join("output.txt"), &result.stdout).unwrap();
        std::fs::write(dir.join("stderr.txt"), &result.stderr).unwrap();
    }
    let output = String::from_utf8(result.stdout).unwrap();
    assert_eq!(output.lines().count(), cases.len());
    output
        .lines()
        .map(|line| {
            let tokens = line.split_whitespace().collect::<Vec<_>>();
            assert_eq!(&tokens[..3], &["G", "20", "0"]);
            let mut draws = Vec::new();
            let mut at = 3;
            while tokens[at] != "E" {
                match tokens[at] {
                    "C" => at += 5,
                    "P" => {
                        let head = (1..7)
                            .map(|i| tokens[at + i].parse::<u32>().unwrap())
                            .collect::<Vec<_>>();
                        draws.push(Draw {
                            program: head[0],
                            blend: head[1],
                            texture: head[2],
                            primitive: head[3],
                            count: head[4],
                            vertex_bytes: head[5],
                            words: std::array::from_fn(|i| tokens[at + 7 + i].parse().unwrap()),
                        });
                        at += 79;
                    }
                    _ => panic!("unexpected original packet token:{}", tokens[at]),
                }
            }
            let tail = tokens[at + 1..]
                .iter()
                .map(|s| s.parse::<u32>().unwrap())
                .collect::<Vec<_>>();
            let copies = draws.iter().filter(|d| d.program >= 6).count() as u32;
            assert_eq!(tail, [draws.len() as u32, copies, 20, 0, copies + 2]);
            draws
        })
        .collect()
}
fn packet_words(packet: &Packet) -> Vec<u32> {
    let mut values = packet.projection.to_cols_array().to_vec();
    values.extend(packet.color.to_array());
    values.extend(packet.param.to_array());
    for v in packet.vertices {
        let position = v.position;
        let texture_position = v.texture_position;
        values.extend(position.to_array());
        values.extend(texture_position.to_array());
    }
    values.into_iter().map(f32::to_bits).collect()
}
fn copied_draw_words(draw: &Draw) -> Vec<u32> {
    [
        &draw.words[..16],
        &draw.words[16..20],
        &draw.words[32..36],
        &draw.words[48..60],
    ]
    .concat()
}
fn present(pass: &mut RenderPass, source: TextureSource) {
    let vertices = [Vec2::new(2., 0.), Vec2::new(0., 2.), Vec2::ZERO].map(|v| PosTexVertex {
        position: v,
        texture_position: v,
    });
    pass.run(RenderRequestBuilder::new().build(
        RenderProgramWithArguments::Layer {
            output_kind: LayerShaderOutputKind::Layer,
            fragment_shader: LayerFragmentShader::Default,
            vertices: VertexSource::VertexData {
                vertices: &vertices,
            },
            texture: source,
            transform: HigurashiTransform::convert_clip_depth(Mat4::from_cols_array(&[
                2., 0., 0., 0., 0., -2., 0., 0., 0., 0., -1., 0., -1., 1., -0., 1.,
            ])),
            color_multiplier: FloatColor4::WHITE,
            fragment_shader_param: Vec4::ONE,
        },
        DrawPrimitive::TrianglesStrip,
    ));
}

#[test]
#[ignore = "original restored65-target framebuffer/plane/Tile packets and six extracted GLSL operations; full capture pixels at228x128/342x192, no audio/windows/user saves"]
fn higurashi_saved_preview_framebuffer_postprocessing_matches_native_packets_and_pixels() {
    let cases = cases();
    assert_eq!(cases.len(), 174);
    let original = native(&cases);
    let (device, queue, _) = reference_gpu_assets();
    let dummy = GpuTexture::new_static_with_data(
        &device,
        &queue,
        None,
        winit::dpi::PhysicalSize::new(1, 1),
        shin_render::TEXTURE_FORMAT,
        &[255; 4],
    );
    let shaders = (0..6)
        .map(|op| NativeShader::new(&device, Some(op), 1))
        .collect::<Vec<_>>();
    let fill_opaque = NativeShader::new(&device, None, 1);
    let fill_alpha = NativeShader::new(&device, None, 2);
    let mut packets = 0;
    for (index, (case, draws)) in cases.iter().zip(&original).enumerate() {
        let scene = case.scene();
        let expected = draws.iter().filter(|d| d.program >= 6).collect::<Vec<_>>();
        let packets_here = [&scene.screen, &scene.root]
            .into_iter()
            .filter_map(Packet::new)
            .collect::<Vec<_>>();
        assert_eq!(
            packets_here.len(),
            expected.len(),
            "original identity/copy count case{index}"
        );
        for (packet, draw) in packets_here.iter().zip(expected) {
            assert_eq!(
                (
                    draw.program,
                    draw.blend,
                    draw.texture,
                    draw.primitive,
                    draw.count,
                    draw.vertex_bytes
                ),
                (packet.program, 1, 1, 1, 3, 48)
            );
            assert_eq!(
                packet_words(packet),
                copied_draw_words(draw),
                "original postprocess packet case{index}"
            );
            packets += 1;
        }
        let size = resources::thumbnail_size(case.canvas);
        let actual = reference_render_to_texture(&device, &queue, size, |ctx, view| {
            let captured = scene.capture(ctx, case.canvas).unwrap();
            let mut pass = ctx.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view,
                },
                None,
                "Actual saved framebuffer pixels",
            );
            present(&mut pass, captured.as_texture_source());
        });
        let expected = reference_render_to_texture(&device, &queue, size, |ctx, view| {
            let mut texture =
                ctx.new_render_texture("Original restored framebuffer reference".into());
            {
                let mut pass =
                    ctx.begin_pass(texture.as_texture_target(), None, "Original black clear");
                pass.clear(Some(UnormColor::BLACK), None, None);
            }
            for draw in draws {
                if draw.blend == 0 {
                    continue;
                } // Bounded stencil-only clip packet.
                if draw.program == 1 {
                    let shader = if draw.blend == 1 {
                        &fill_opaque
                    } else {
                        assert_eq!(draw.blend, 2);
                        &fill_alpha
                    };
                    shader.draw(ctx, dummy.as_source(), texture.as_texture_target(), draw);
                } else {
                    let op = [6, 9, 7, 8, 10, 11]
                        .iter()
                        .position(|&p| p == draw.program)
                        .unwrap();
                    let snapshot = texture.copy_snapshot(&mut ctx.render_clone_ctx());
                    shaders[op].draw(
                        ctx,
                        snapshot.as_texture_source(),
                        texture.as_texture_target(),
                        draw,
                    );
                }
            }
            let mut pass = ctx.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view,
                },
                None,
                "Original capture publication",
            );
            present(&mut pass, texture.as_texture_source());
        });
        assert_eq!(
            actual, expected,
            "original restored framebuffer pixels case{index},canvas{:?}",
            case.canvas
        );
        if case.values[0][19] == 0 || case.values[1][19] == 0 {
            assert!(
                actual.chunks_exact(4).any(|p| p[..3] != [0; 3]),
                "external hidden flag must not discard captured contents"
            );
        }
    }
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    eprintln!(
        "Saved framebuffer postprocessing:174 original restored-target owners/{packets} exact copy/shader packets and174 byte-identical228x128/342x192 capture frames; six primary GLSL operations,older Fill2,screen-before-root order,identity fast paths,external alpha/visibility and fresh afterimage behavior pass; no audio/windows/user saves"
    );
}
