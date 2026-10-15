use super::*;
use crate::{
    adv::higurashi::{reference_gpu_assets as gpu_assets, reference_render_to_texture},
    app::higurashi::kakera_menu::tests::{persistent, source},
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
    present: bool,
    pixels: bool,
}
struct Native {
    sizes: [[f32; 2]; 4],
    draws: Vec<NativeDraw>,
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
            "Kakera parent reference",
        );
        pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
        draw(&mut pass);
    })
}
fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for alternate in [false, true] {
        for kind in 0..5 {
            for page in 0..if alternate { 9 } else { 17 } {
                let mut model = Model::new(
                    source(if alternate { 12001 } else { 2001 }, 1, kind.min(3), 64, 0),
                    0,
                    &persistent(kind, 0, 2),
                );
                model.page = page;
                model.selected = page % model.pages[page].len();
                let mut motion = Motion::new(model, false);
                motion.weights =
                    std::array::from_fn(|i| if i == motion.model.selected { 1.0 } else { 0.0 });
                cases.push(Case {
                    motion,
                    present: true,
                    pixels: kind == page % 5,
                });
            }
        }
        for fragment in if alternate { vec![49] } else { vec![60, 62] } {
            for own in [0, 1, 2, -1] {
                for paired in [0, 1, 2, -1] {
                    let mut model = Model::new(
                        source(if alternate { 12001 } else { 2001 }, 1, 1, 64, 0),
                        fragment,
                        &persistent(1, 0, 2),
                    );
                    let other = if alternate {
                        52
                    } else if fragment == 60 {
                        103
                    } else {
                        104
                    };
                    model.statuses[fragment as usize] = own;
                    model.statuses[other] = paired;
                    cases.push(Case {
                        motion: Motion::new(model, false),
                        present: true,
                        pixels: own == 1 && paired == 1,
                    });
                }
            }
        }
        for profile in 0..16 {
            let model = Model::new(
                source(if alternate { 12001 } else { 2001 }, 1, 1, 64, 0),
                if alternate { 49 } else { 60 },
                &persistent(1, 0, 2),
            );
            let mut motion = Motion::new(model, false);
            motion.weights = std::array::from_fn(|i| [0.0, 0.35, 0.7, 1.0][(i + profile) % 4]);
            for (i, clock) in motion.clocks.iter_mut().enumerate() {
                let value = if i == 7 {
                    [-1920.0, -825.5, 0.0, 825.5, 1920.0][profile % 5]
                } else if i == 8 {
                    [0.0, 0.0, 0.37, 1.0][profile % 4]
                } else {
                    [0.0, 0.35, 0.7, 1.0][(i + profile) % 4]
                };
                clock.fast_forward_to(value);
            }
            cases.push(Case {
                motion,
                present: profile != 9 && profile < 12,
                pixels: true,
            });
        }
    }
    cases
}
fn native(cases: &[Case]) -> (Vec<Native>, HashMap<u32, String>) {
    let mut input = String::new();
    for case in cases {
        let m = &case.motion.model;
        input += &format!(
            "R {} {} {} {} {}\n",
            u8::from(m.alternate),
            u8::from(case.present),
            m.page,
            m.selected,
            m.statuses.len()
        );
        for status in &m.statuses {
            input += &format!("{status} ");
        }
        input += "\n";
        for weight in case.motion.weights {
            input += &format!("{:x} ", weight.to_bits());
        }
        input += "\n";
        for clock in &case.motion.clocks {
            input += &format!("{:x} ", clock.value().to_bits());
        }
        input += "\n";
    }
    let mut child =
        Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_KAKERA_MENU_RENDER").unwrap())
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
    let copy = input.clone();
    let writer = std::thread::spawn(move || stdin.write_all(copy.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    let errors = String::from_utf8(output.stderr).unwrap();
    assert!(output.status.success(), "{errors}");
    let output = String::from_utf8(output.stdout).unwrap();
    if let Ok(path) = std::env::var("SHIN_HIGURASHI_KAKERA_RENDER_CAPTURE") {
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
        .filter_map(|line| {
            let (id, name) = line.strip_prefix("Texture")?.split_once(':')?;
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
            if op == "E" {
                assert_eq!(n() as usize, draws.len());
                assert_eq!(n(), 1);
                break;
            }
            assert_eq!(op, "P");
            let (program, blend, texture, point, count) = (n(), n(), n(), n(), n());
            draws.push(NativeDraw {
                program,
                blend,
                texture,
                point,
                matrix: std::array::from_fn(|_| n()),
                extra: std::array::from_fn(|_| n()),
                color: std::array::from_fn(|_| n()),
                vertices: (0..count).map(|_| n()).collect(),
            });
        }
        rows.push(Native { sizes, draws });
    }
    assert_eq!(rows.len(), cases.len());
    (rows, textures)
}
fn check_packets(
    case: &Case,
    native: &Native,
    names: &HashMap<u32, String>,
    index: usize,
) -> usize {
    let actual: Vec<_> = packets(&case.motion, case.present)
        .into_iter()
        .filter_map(|p| if let Draw::Quad(p) = p { Some(p) } else { None })
        .collect();
    let expected: Vec<_> = native
        .draws
        .iter()
        .filter(|d| d.program != 3 && d.program != 4)
        .collect();
    assert_eq!(actual.len(), expected.len(), "case{index} sprite count");
    for (j, (a, e)) in actual.iter().zip(expected).enumerate() {
        assert_eq!(
            if a.texture.is_empty() {
                1
            } else if a.flash > 0.0 {
                5
            } else {
                2
            },
            e.program,
            "case{index} packet{j} program"
        );
        assert_eq!(
            if a.opaque { 1 } else { 2 },
            e.blend,
            "case{index} packet{j} blend"
        );
        if !a.texture.is_empty() {
            assert_eq!(
                &a.texture, &names[&e.point],
                "case{index} packet{j} texture"
            );
        }
        assert_eq!(
            a.matrix.to_cols_array().map(f32::to_bits),
            e.matrix,
            "case{index} packet{j} {} matrix",
            a.texture
        );
        let stride = if e.program == 1 { 4 } else { 6 };
        for vertex in 0..4 {
            let [l, t, r, b] = a.rect;
            let [u, v, s, w] = a.uv;
            let xy = [
                if vertex % 2 == 0 { l } else { r },
                if vertex < 2 { t } else { b },
                0.0,
            ];
            assert_eq!(
                xy.map(f32::to_bits),
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
        if e.program == 5 {
            assert_eq!(e.extra[..4], [1.0f32, 1.0, 1.0, a.flash].map(f32::to_bits));
            // Older program5 has one flash uniform. Captured slot2 may retain
            // prior font data;the original shader never reads that binding.
        }
    }
    actual.len()
}
fn replay(
    pass: &mut RenderPass,
    native: &Native,
    names: &HashMap<u32, String>,
    atlas: &CharacterTextures,
    glyphs: &HashMap<u32, GpuFontGlyphHandle>,
) {
    let color = |words: &[u32]| {
        shin_core::primitives::color::FloatColor4::from_vec4(Vec4::from_array(std::array::from_fn(
            |i| f32::from_bits(words[i]),
        )))
    };
    for d in &native.draws {
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
            let texture = atlas.get(&names[&d.point]).as_source();
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
#[ignore = "executes updated63558 parent/641b0 hints/62d08 labels against guarded native executable and font"]
fn higurashi_kakera_parent_packets_match_native() {
    let cases = cases();
    let (native, names) = native(&cases);
    let mut quads = 0;
    for (i, (case, native)) in cases.iter().zip(&native).enumerate() {
        quads += check_packets(case, native, &names, i);
    }
    assert!(names.values().any(|n| n == "new3"));
    assert!(names.values().any(|n| n == "warning"));
    println!(
        "Higurashi Kakera parent:{} native profiles/{quads} exact sprite/fill packets;all26 authored pages,paired badges,signed locks,selection/flash,page slides,notice,help,blackout and child boundary pass",
        cases.len()
    );
}
#[test]
#[ignore = "compares full original63558/SimpleText packet replay with real Kakera atlas/font at720p/1080p on Metal"]
fn higurashi_kakera_parent_pixels_match_native_at_720p_and_1080p() {
    let cases: Vec<_> = cases().into_iter().filter(|c| c.pixels).collect();
    let (native, names) = native(&cases);
    let (device, queue, assets) = gpu_assets();
    let font: Arc<GpuFontLazy> = assets.load_sync("/newrodin.fnt").unwrap();
    let atlas: Arc<CharacterTextures> = assets.load_sync("/kakera.txa").unwrap();
    let points: Vec<_> = native
        .iter()
        .flat_map(|n| &n.draws)
        .filter(|d| d.texture == 2)
        .map(|d| d.point)
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
        let mut view = View::new(font.clone());
        view.synchronize(&assets, &case.motion.model).unwrap();
        for (j, (text, _)) in view.details.iter().enumerate() {
            assert_eq!(
                text.layout.size.to_array().map(f32::to_bits),
                native.sizes[j].map(f32::to_bits),
                "case{i} detail{j} size"
            );
        }
        for size in [(1280, 720), (1920, 1080)] {
            let actual = render_frame(&device, &queue, size, |pass| {
                view.render(&case.motion, case.present.then_some(&atlas), pass)
            });
            let expected = render_frame(&device, &queue, size, |pass| {
                replay(pass, native, &names, &atlas, &glyphs)
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
        "Higurashi Kakera parent:{frames} byte-identical full RGBA frames at720p/1080p;real atlas,font,dependent glyphs and native63558/641b0/62d08 packet replay pass"
    );
}
