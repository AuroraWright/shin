use super::*;
use crate::{
    adv::higurashi::{reference_gpu_assets as gpu_assets, reference_render_to_texture},
    asset::font::GpuFontGlyphHandle,
    layer::render_params::higurashi::HigurashiTransform,
};
use glam::vec2;
use shin_core::{format::scenario::higurashi::HigurashiScenario, primitives::color::UnormColor};
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments,
    shaders::types::{
        buffer::VertexSource,
        vertices::{PosColVertex, TextVertex},
    },
};
use std::{
    collections::HashMap,
    fmt::Write as _,
    io::Write as _,
    process::{Command, Stdio},
};

struct Case {
    model: Model,
    present: bool,
    scrollbar: Scrollbar,
    pixels: bool,
}
struct Native {
    size: [f32; 2],
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
fn cases() -> Vec<Case> {
    let snr = HigurashiScenario::new(
        std::fs::read(std::env::var("SHIN_HIGURASHI_SNR").unwrap())
            .unwrap()
            .into(),
    )
    .unwrap();
    let mut cases = vec![];
    for alt in [false, true] {
        for pattern in [0, 1, 3, 4] {
            for (i, selected) in [0, 31, 32, 127, 187, 188, 244, 314].into_iter().enumerate() {
                let mut model = Model::new(
                    &snr,
                    Some(super::super::tests::source(
                        if alt { 11000 } else { 100 },
                        0,
                    )),
                    selected,
                    0,
                    &super::super::tests::vectors(pattern),
                )
                .unwrap();
                model.background_phase = [0.0, 0.25, 1.125][i % 3];
                model.feedback.fast_forward_to([0.0, 0.35, 0.7][i % 3]);
                for (j, row) in model.rows.iter_mut().enumerate() {
                    if j % 13 == i % 13 {
                        row.flash.fast_forward_to([0.0, 0.2, 0.7][j % 3]);
                    }
                }
                if i == 2 {
                    model.scroll.fast_forward_to(model.scroll.value() + 0.375);
                }
                let scrollbar = Scrollbar {
                    position: model.scroll.value() as i32,
                    visible: 818,
                    first: 0,
                    end: model.content_height,
                };
                cases.push(Case {
                    model,
                    present: true,
                    scrollbar,
                    pixels: i == pattern % 8,
                });
            }
        }
    }
    // Native rows can extend beyond the list viewport;top/bottom obi obscure
    // them. Include a fractional scroll at each row-culling boundary.
    for scroll in [69.999, 70.0, 253.999, 254.0, 413.375] {
        let mut model = Model::new(&snr, None, 0, 0, &super::super::tests::vectors(4)).unwrap();
        model.scroll.fast_forward_to(scroll);
        cases.push(Case {
            scrollbar: Scrollbar {
                position: scroll as i32,
                visible: 818,
                first: 0,
                end: model.content_height,
            },
            model,
            present: true,
            pixels: true,
        });
    }
    let model = Model::new(&snr, None, 0, 0, &super::super::tests::vectors(4)).unwrap();
    cases.push(Case {
        scrollbar: Scrollbar {
            position: 0,
            visible: 818,
            first: 0,
            end: model.content_height,
        },
        model,
        present: false,
        pixels: true,
    });
    cases
}
fn native(cases: &[Case]) -> (Vec<Native>, HashMap<u32, String>) {
    let mut input = String::new();
    for case in cases {
        let m = &case.model;
        writeln!(
            input,
            "R {} {} {} {} {:x} {:x} {:x} {}",
            u32::from(case.present),
            m.selected,
            m.rows.len(),
            m.content_height,
            m.scroll.value().to_bits(),
            m.background_phase.to_bits(),
            m.feedback.value().to_bits(),
            m.completion_text
                .bytes()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        )
        .unwrap();
        for row in &m.rows {
            writeln!(
                input,
                "{} {} {} {:x}",
                row.source_index,
                u32::from(row.current),
                row.status,
                row.flash.value().to_bits()
            )
            .unwrap();
        }
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_TIPS_RENDER").unwrap())
        .args([
            std::env::var("SHIN_HIGURASHI_EXE").unwrap(),
            std::env::var("SHIN_HIGURASHI_FONT").unwrap(),
            std::env::var("SHIN_HIGURASHI_SNR").unwrap(),
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
    if let Ok(dir) = std::env::var("SHIN_HIGURASHI_TIPS_RENDER_CAPTURE") {
        std::fs::create_dir_all(&dir).unwrap();
        for (name, data) in [
            ("input.txt", input.as_bytes()),
            ("output.txt", output.stdout.as_slice()),
            ("stderr.txt", output.stderr.as_slice()),
        ] {
            std::fs::write(std::path::Path::new(&dir).join(name), data).unwrap();
        }
    }
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    sent.unwrap();
    let errors = String::from_utf8(output.stderr).unwrap();
    let names = errors
        .lines()
        .filter_map(|s| {
            let (id, name) = s.strip_prefix("Texture")?.split_once(':')?;
            Some((id.parse().unwrap(), name.into()))
        })
        .collect();
    let text = String::from_utf8(output.stdout).unwrap();
    let mut words = text.split_whitespace();
    let mut rows = vec![];
    while let Some(op) = words.next() {
        assert_eq!(op, "R");
        let size = std::array::from_fn(|_| f32::from_bits(words.next().unwrap().parse().unwrap()));
        let mut draws = vec![];
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
        rows.push(Native { size, draws });
    }
    assert_eq!(rows.len(), cases.len());
    (rows, names)
}
fn check_packets(
    case: &Case,
    native: &Native,
    names: &HashMap<u32, String>,
    index: usize,
) -> usize {
    let actual = packets(&case.model, case.present, case.scrollbar)
        .into_iter()
        .filter(|p| !matches!(p, Draw::Completion(_)))
        .collect::<Vec<_>>();
    let expected = native
        .draws
        .iter()
        .filter(|d| d.program != 3 && d.program != 4)
        .collect::<Vec<_>>();
    assert_eq!(actual.len(), expected.len(), "case{index} sprite count");
    for (j, (a, e)) in actual.iter().zip(expected).enumerate() {
        let (name, matrix, program, blend, vertices, flash) = match a {
            Draw::Quad(p) => {
                let [l, t, r, b] = p.rect;
                let [u, v, s, w] = p.uv;
                let vertices = (0..4)
                    .flat_map(|i| {
                        [
                            if i % 2 == 0 { l } else { r },
                            if i < 2 { t } else { b },
                            0.0,
                        ]
                        .map(f32::to_bits)
                        .into_iter()
                        .chain([p.color])
                        .chain(
                            [if i % 2 == 0 { u } else { s }, if i < 2 { v } else { w }]
                                .map(f32::to_bits),
                        )
                    })
                    .collect::<Vec<_>>();
                (
                    p.texture.as_str(),
                    p.matrix,
                    if p.flash > 0.0 { 5 } else { 2 },
                    if p.opaque { 1 } else { 2 },
                    vertices,
                    p.flash,
                )
            }
            Draw::Scroll { matrix, vertices } => {
                let words = vertices
                    .iter()
                    .flat_map(|v| {
                        {
                            let position = v.position;
                            position.to_array()
                        }
                        .map(f32::to_bits)
                        .into_iter()
                        .chain([v.color.0])
                        .chain({
                            let uv = v.texture_position;
                            uv.to_array().map(f32::to_bits)
                        })
                    })
                    .collect();
                ("scroll", *matrix, 2, 2, words, 0.0)
            }
            Draw::Completion(_) => unreachable!(),
        };
        assert_eq!(
            (program, blend),
            (e.program, e.blend),
            "case{index} packet{j} {name} program/blend"
        );
        assert_eq!(name, names[&e.point], "case{index} packet{j} name");
        assert_eq!(
            matrix.to_cols_array().map(f32::to_bits),
            e.matrix,
            "case{index} packet{j} {name} matrix"
        );
        assert_eq!(
            vertices, e.vertices,
            "case{index} packet{j} {name} vertices"
        );
        if program == 5 {
            assert_eq!(e.extra[..4], [1.0f32, 1.0, 1.0, flash].map(f32::to_bits));
        }
    }
    actual.len()
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
            "TIPS native parent reference",
        );
        pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
        draw(&mut pass);
    })
}

#[test]
#[ignore = "actual daf38/db88c/c0574 full TIPS parent packet emitter and completion layout"]
fn higurashi_tips_parent_packets_match_native() {
    let cases = cases();
    let (native, names) = native(&cases);
    let mut count = 0;
    for (i, (case, native)) in cases.iter().zip(&native).enumerate() {
        count += check_packets(case, native, &names, i);
    }
    eprintln!(
        "TIPS parent:{} native profiles/{count} exact sprite packets;source SNR IDs,plates/new badges/title sheets/selection pulse/flashes,integer scrollbar,header/back/help and parent traversal pass",
        cases.len()
    );
}
#[test]
#[ignore = "real TIPS/system atlases,font and original full packet replay at720p/1080p"]
fn higurashi_tips_parent_pixels_match_native_at_720p_and_1080p() {
    let cases = cases().into_iter().filter(|c| c.pixels).collect::<Vec<_>>();
    let (native, names) = native(&cases);
    let (device, queue, assets) = gpu_assets();
    let font: Arc<GpuFontLazy> = assets.load_sync("/newrodin.fnt").unwrap();
    let atlas: Arc<CharacterTextures> = assets.load_sync("/tipsmode.txa").unwrap();
    let system: Arc<CharacterTextures> = assets.load_sync("/systex.txa").unwrap();
    let points = native
        .iter()
        .flat_map(|n| &n.draws)
        .filter(|d| d.texture == 2)
        .map(|d| d.point)
        .collect::<Vec<_>>();
    let glyphs = points
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
        .collect::<HashMap<_, _>>();
    let mut frames = 0;
    for (i, (case, native)) in cases.iter().zip(&native).enumerate() {
        check_packets(case, native, &names, i);
        let mut view = View::new(font.clone());
        view.synchronize(&assets, &case.model).unwrap();
        assert_eq!(
            view.completion
                .as_ref()
                .unwrap()
                .layout
                .size
                .to_array()
                .map(f32::to_bits),
            native.size.map(f32::to_bits),
            "case{i} completion size"
        );
        for size in [(1280, 720), (1920, 1080)] {
            let actual = render_frame(&device, &queue, size, |pass| {
                view.render(
                    &case.model,
                    case.present.then_some(&atlas),
                    &system,
                    case.scrollbar,
                    pass,
                )
            });
            let expected = render_frame(&device, &queue, size, |pass| {
                replay(pass, native, &names, &atlas, &system, &glyphs)
            });
            let errors = actual
                .iter()
                .zip(&expected)
                .enumerate()
                .filter(|(_, (a, b))| a != b)
                .collect::<Vec<_>>();
            assert!(
                errors.is_empty(),
                "case{i} {size:?}:{} differing RGBA channels;first{:?}",
                errors.len(),
                errors.first()
            );
            frames += 1;
        }
    }
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    eprintln!(
        "TIPS parent:{frames} byte-identical full720p/1080p frames;real tipsmode/systex atlases,completion font,native parent/help/scrollbar/text replay pass"
    );
}

// Reuse the established native packet replay,with8-vertex scrollbar strips
// and its separate system atlas binding.
fn replay(
    pass: &mut RenderPass,
    native: &Native,
    names: &HashMap<u32, String>,
    atlas: &CharacterTextures,
    system: &CharacterTextures,
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
            let vertices: Vec<PosColTexVertex> = (0..d.vertices.len() / 6)
                .map(|i| PosColTexVertex {
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
                })
                .collect();
            let vertices = VertexSource::VertexData {
                vertices: &vertices,
            };
            let owner = if names[&d.point] == "scroll" {
                system
            } else {
                atlas
            };
            let texture = owner.get(&names[&d.point]).as_source();
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
