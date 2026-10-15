use super::super::resources::{retire_invisible_thumbnails, thumbnail_size};
use super::*;
use crate::adv::higurashi::{reference_gpu_assets as gpu_assets, reference_render_to_texture};
use crate::asset::font::GpuFontGlyphHandle;
use glam::vec2;
use shin_core::{
    format::{font::read_font_metrics, save::higurashi::SaveDate},
    layout::font::FontMetrics,
};
use shin_render::{
    DrawPrimitive, RenderProgramWithArguments,
    shaders::types::{
        buffer::VertexSource,
        vertices::{PosColVertex, TextVertex},
    },
};
use std::{
    collections::HashMap,
    fmt::Write as _,
    io::{BufReader, Write as _},
    process::{Command, Stdio},
};

struct Case {
    state: ViewState,
    rows: Rows,
    thumbnails: [bool; 100],
    present: bool,
    pixels: bool,
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
struct Expected {
    program: u32,
    blend: u32,
    texture: String,
    matrix: Mat4,
    vertices: Vec<u32>,
    tint: Option<Vec4>,
    flash: f32,
}
fn state(mode: Mode, selected: i32, scroll: f32, progress: f32) -> ViewState {
    ViewState {
        mode,
        selected,
        progress,
        scroll,
        close_flash: 0.0,
        row_flash: [0.0; 100],
        scrollbar: Scrollbar {
            position: scroll as i32,
            visible: 818,
            first: 0,
            end: 20618,
        },
    }
}
fn rows(pattern: usize) -> Rows {
    std::array::from_fn(|i| {
        if (i + pattern) % 4 == 0 {
            return Default::default();
        }
        let date = SaveDate {
            year: if i % 17 == 0 {
                65535
            } else {
                2025 + (i % 2) as u16
            },
            month: (i % 13) as u8,
            day: (i % 32) as u8,
            hour: (i % 25) as u8,
            minute: (i % 61) as u8,
            second: 59,
        };
        let labels = if (i + pattern) % 4 == 1 {
            Labels::Loading
        } else {
            Labels::Ready {
                title: Arc::from(if i % 2 == 0 {
                    "鬼隠し編@r祭りの夜"
                } else {
                    "綿流し編"
                }),
                subtitle: Arc::from(if i % 3 == 0 {
                    "@c900.赤字@c. 本文"
                } else {
                    "保存した場面"
                }),
            }
        };
        super::super::resources::Row {
            date: Some(date),
            labels,
        }
    })
}
fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for mode in [Mode::Save, Mode::Load] {
        for selected in 0..100 {
            let scroll = ((selected * 206 + 12) as f32 - 409.0 + 100.0).clamp(0.0, 19800.0);
            let mut state = state(
                mode,
                selected as i32,
                scroll,
                [0.0, 0.375, 1.0][selected % 3],
            );
            state.close_flash = [0.0, 0.35, 1.0][selected % 3];
            for (i, flash) in state.row_flash.iter_mut().enumerate() {
                if i % 3 == selected % 3 {
                    *flash = [0.0, 0.2, 0.7][i % 3];
                }
            }
            let mut thumbnails = [false; 100];
            for (i, present) in thumbnails.iter_mut().enumerate() {
                *present = i % 3 == selected % 3;
            }
            cases.push(Case {
                state,
                rows: rows(selected % 4),
                thumbnails,
                present: true,
                pixels: [0, 1, 4, 5, 50, 99].contains(&selected),
            });
        }
    }
    for scroll in [0.0, 367.999, 368.0, 368.001, 19800.0] {
        let mut s = state(Mode::Load, 0, scroll, 0.625);
        s.scrollbar.position = ((scroll as i32) - 1).max(0);
        cases.push(Case {
            state: s,
            rows: rows(0),
            thumbnails: [false; 100],
            present: true,
            pixels: false,
        });
    }
    // Readiness gates only the parent: the native empty child traversal remains.
    cases.push(Case {
        state: state(Mode::Load, 99, 19800.0, 0.0),
        rows: rows(3),
        thumbnails: [true; 100],
        present: false,
        pixels: true,
    });
    cases
}
fn hex(text: &str) -> String {
    if text.is_empty() {
        "-".into()
    } else {
        text.bytes().map(|b| format!("{b:02x}")).collect()
    }
}
fn run_native(input: &str) -> (String, String) {
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_SAVE_MENU_RENDER").unwrap())
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
    let sent = input.to_owned();
    let writer = std::thread::spawn(move || stdin.write_all(sent.as_bytes()));
    let output = child.wait_with_output().unwrap();
    let sent = writer.join().unwrap();
    if let Ok(dir) = std::env::var("SHIN_HIGURASHI_SAVE_MENU_RENDER_CAPTURE") {
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
    (
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap(),
    )
}
fn native(cases: &[Case]) -> (Vec<Vec<NativeDraw>>, HashMap<u32, String>) {
    let mut input = String::new();
    for c in cases {
        let s = &c.state;
        // One optional field exercises the native pre-clock scrollbar snapshot.
        writeln!(
            input,
            "R {} {} {} {:x} {:x} {:x} {} 100",
            u32::from(c.present),
            u32::from(s.mode == Mode::Load),
            s.selected,
            s.progress.to_bits(),
            s.scroll.to_bits(),
            s.close_flash.to_bits(),
            s.scrollbar.position
        )
        .unwrap();
        for (i, row) in c.rows.iter().enumerate() {
            let d = row.date.unwrap_or(SaveDate {
                year: 0,
                month: 0,
                day: 0,
                hour: 0,
                minute: 0,
                second: 0,
            });
            let (kind, title, subtitle) = match &row.labels {
                Labels::Empty => (0, "", ""),
                Labels::Loading => (1, "", ""),
                Labels::Ready { title, subtitle } => (2, title.as_ref(), subtitle.as_ref()),
            };
            writeln!(
                input,
                "{} {} {} {} {} {} {} {} {} {:x} {} {}",
                i,
                u32::from(row.date.is_some()),
                d.year,
                d.month,
                d.day,
                d.hour,
                d.minute,
                u32::from(c.thumbnails[i]),
                kind,
                s.row_flash[i].to_bits(),
                hex(title),
                hex(subtitle)
            )
            .unwrap();
        }
    }
    let (out, err) = run_native(&input);
    let names = err
        .lines()
        .filter_map(|s| {
            let (id, name) = s.strip_prefix("Texture")?.split_once(':')?;
            Some((id.parse().unwrap(), name.to_owned()))
        })
        .collect();
    let mut words = out.split_whitespace();
    let mut frames = Vec::new();
    while let Some(op) = words.next() {
        assert_eq!(op, "R");
        let mut draws = Vec::new();
        loop {
            let op = words.next().unwrap();
            let mut n = || words.next().unwrap().parse::<u32>().unwrap();
            if op == "E" {
                assert_eq!(n() as usize, draws.len());
                assert_eq!(n(), 1);
                let reads = n();
                assert_eq!(
                    reads as usize,
                    cases[frames.len()]
                        .thumbnails
                        .iter()
                        .enumerate()
                        .filter(|(i, p)| **p
                            && cases[frames.len()].present
                            && visible(*i, cases[frames.len()].state.scroll))
                        .count()
                );
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
        frames.push(draws);
    }
    assert_eq!(frames.len(), cases.len());
    (frames, names)
}
fn expected(case: &Case, font: impl FontMetrics + Clone) -> Vec<Expected> {
    let mut out = Vec::new();
    for draw in packets(&case.state, &case.rows, case.present, &case.thumbnails) {
        match draw {
            Draw::Quad(q) => {
                let fill = matches!(q.texture, Texture::Fill);
                let texture = match q.texture {
                    Texture::Atlas(name) => name.to_owned(),
                    Texture::Thumbnail(i) => format!("thumbnail{i}"),
                    Texture::Fill => String::new(),
                };
                let [l, t, r, b] = q.rect;
                let [u, v, s, w] = q.uv;
                let vertices = (0..4)
                    .flat_map(|i| {
                        let mut vtx = vec![
                            (if i % 2 == 0 { l } else { r }).to_bits(),
                            (if i < 2 { t } else { b }).to_bits(),
                            0,
                            q.color,
                        ];
                        if !fill {
                            vtx.extend([
                                (if i % 2 == 0 { u } else { s }).to_bits(),
                                (if i < 2 { v } else { w }).to_bits(),
                            ]);
                        }
                        vtx
                    })
                    .collect();
                out.push(Expected {
                    program: if fill {
                        1
                    } else if q.flash > 0.0 {
                        5
                    } else {
                        2
                    },
                    blend: if q.opaque { 1 } else { 2 },
                    texture,
                    matrix: q.matrix,
                    vertices,
                    tint: None,
                    flash: q.flash,
                });
            }
            Draw::Scroll { matrix, vertices } => out.push(Expected {
                program: 2,
                blend: 2,
                texture: "scroll".into(),
                matrix,
                vertices: vertices
                    .iter()
                    .flat_map(|v| {
                        let position = v.position;
                        let texture_position = v.texture_position;
                        position
                            .to_array()
                            .map(f32::to_bits)
                            .into_iter()
                            .chain([v.color.0])
                            .chain(texture_position.to_array().map(f32::to_bits))
                    })
                    .collect(),
                tint: None,
                flash: 0.0,
            }),
            Draw::Labels { index, matrix } => {
                let texts: Vec<(&str, f32)> = match &case.rows[index].labels {
                    Labels::Empty => vec![("データがありません", 97.0)],
                    Labels::Loading => vec![("loading...", 97.0)],
                    Labels::Ready { title, subtitle } => vec![(title, 67.0), (subtitle, 127.0)],
                };
                for (text, y) in texts {
                    let layout = label_layout(font.clone(), text).unwrap();
                    for glyph in layout.glyphs {
                        out.push(Expected {
                            program: 3,
                            blend: 2,
                            texture: format!("glyph{}", glyph.point as u32),
                            matrix: text_matrix(matrix, y),
                            vertices: glyph
                                .vertices
                                .iter()
                                .flat_map(|v| {
                                    let position = v.position;
                                    position
                                        .to_array()
                                        .map(f32::to_bits)
                                        .into_iter()
                                        .chain([v.color.to_bits()])
                                })
                                .collect(),
                            tint: Some(vec4(
                                0.0,
                                95.0 * f32::from_bits(0x3b808081),
                                190.0 * f32::from_bits(0x3b808081),
                                1.0,
                            )),
                            flash: 0.0,
                        });
                    }
                }
            }
        }
    }
    out
}
fn check_packets(
    case: &Case,
    native: &[NativeDraw],
    names: &HashMap<u32, String>,
    font: impl FontMetrics + Clone,
    index: usize,
) -> usize {
    let expected = expected(case, font);
    assert_eq!(expected.len(), native.len(), "case{index} draw count");
    for (j, (a, n)) in expected.iter().zip(native).enumerate() {
        let name = match n.texture {
            1 => String::new(),
            0 => names[&n.point].clone(),
            2 => format!("glyph{}", n.point),
            3 => format!("thumbnail{}", n.point),
            _ => panic!("unknown native texture"),
        };
        assert_eq!(
            (a.program, a.blend),
            (n.program, n.blend),
            "case{index} draw{j} {name} program/blend"
        );
        assert_eq!(a.texture, name, "case{index} draw{j} texture");
        assert_eq!(
            a.matrix.to_cols_array().map(f32::to_bits),
            n.matrix,
            "case{index} draw{j} {name} matrix"
        );
        assert_eq!(
            a.vertices, n.vertices,
            "case{index} draw{j} {name} vertices"
        );
        if let Some(tint) = a.tint {
            assert_eq!(
                tint.to_array().map(f32::to_bits),
                n.extra[..4],
                "case{index} glyph fill1"
            );
            assert_eq!(
                tint.to_array().map(f32::to_bits),
                n.color[..4],
                "case{index} glyph fill2"
            );
        }
        if a.program == 5 {
            assert_eq!([1.0, 1.0, 1.0, a.flash].map(f32::to_bits), n.extra[..4]);
        }
    }
    expected.len()
}
#[test]
#[ignore = "original af2f0/helper/real-font packets, both modes, all100 rows and exact visible metadata"]
fn higurashi_save_menu_packets_match_native() {
    let cases = cases();
    let (native, names) = native(&cases);
    let font = Arc::new(
        read_font_metrics(&mut BufReader::new(
            std::fs::File::open(std::env::var("SHIN_HIGURASHI_FONT").unwrap()).unwrap(),
        ))
        .unwrap(),
    );
    let mut draws = 0;
    for (i, (case, native)) in cases.iter().zip(&native).enumerate() {
        draws += check_packets(case, native, &names, font.clone(), i);
    }
    eprintln!(
        "SaveMenu: {} native profiles/{draws} exact parent/glyph packets; both modes/all100 rows, date/empty/loading/ready metadata, culling, three-strip focus, flashes, slide/header/footer/help/scrollbar pass",
        cases.len()
    );
}
#[test]
#[ignore = "actual ae778 populated thumbnail/text cache eviction and retained hydrated VM pointers"]
fn higurashi_save_menu_visible_resource_lifetimes_match_native() {
    let positions = [0.0f32, 367.999, 368.0, 368.001, 1000.25, 19800.0];
    let mut input = String::new();
    for position in positions {
        writeln!(input, "L {:x}", position.to_bits()).unwrap();
    }
    let extents = [(1280, 720), (1920, 1080), (1921, 1081)];
    for (width, height) in extents {
        writeln!(input, "S {width} {height}").unwrap();
    }
    let (out, _) = run_native(&input);
    let mut words = out.split_whitespace();
    for position in positions {
        assert_eq!(words.next(), Some("L"));
        let mut thumbs: [Option<usize>; 100] = std::array::from_fn(Some);
        retire_invisible_thumbnails(&mut thumbs, position);
        for (i, thumb) in thumbs.iter().enumerate() {
            let actual: [u32; 5] = std::array::from_fn(|_| words.next().unwrap().parse().unwrap());
            let seen = visible(i, position);
            assert_eq!(
                actual,
                [
                    u32::from(thumb.is_some()),
                    1,
                    u32::from(!seen),
                    u32::from(seen),
                    u32::from(seen)
                ],
                "scroll{position} row{i} thumbnail/VM/dirty/text ownership"
            );
        }
    }
    for extent in extents {
        assert_eq!(words.next(), Some("S"));
        let actual = (
            words.next().unwrap().parse().unwrap(),
            words.next().unwrap().parse().unwrap(),
        );
        assert_eq!(
            thumbnail_size(extent),
            actual,
            "canvas{extent:?} original scale/fcvtpu"
        );
    }
    assert!(words.next().is_none());
}
fn frame(
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
            "Higurashi native SaveMenu reference",
        );
        pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
        draw(&mut pass);
    })
}
#[test]
#[ignore = "real saveload/systex/NewRodin assets and original parent packet replay720p/1080p; supplied thumbnail texture boundary"]
fn higurashi_save_menu_pixels_match_native_at_720p_and_1080p() {
    let cases = cases().into_iter().filter(|c| c.pixels).collect::<Vec<_>>();
    let (native, names) = native(&cases);
    let (device, queue, assets) = gpu_assets();
    let font: Arc<GpuFontLazy> = assets.load_sync("/newrodin.fnt").unwrap();
    let atlas: Arc<CharacterTextures> = assets.load_sync("/saveload.txa").unwrap();
    let system: Arc<CharacterTextures> = assets.load_sync("/systex.txa").unwrap();
    let points = native
        .iter()
        .flat_map(|n| n)
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
    let mut view = View::new(font.clone());
    for (i, (case, native)) in cases.iter().zip(&native).enumerate() {
        check_packets(case, native, &names, font.clone(), i);
        view.synchronize(&assets, &case.state, &case.rows).unwrap();
        for index in 0..100 {
            assert_eq!(
                view.cached[index].is_some(),
                visible(index, case.state.scroll),
                "case{i} row{index} retained glyph cache"
            );
            if let Some(cache) = &view.cached[index] {
                assert_eq!(
                    cache.labels, case.rows[index].labels,
                    "case{i} row{index} refreshed metadata"
                );
            }
        }
        // The native a9680 service returns a supplied texture view. Reuse real
        // nodata artwork as that input; no saved-scene preview is fabricated.
        let thumbnails =
            std::array::from_fn(|i| case.thumbnails[i].then(|| atlas.get("nodata").as_source()));
        for size in [(1280, 720), (1920, 1080)] {
            let actual = frame(&device, &queue, size, |pass| {
                view.render(
                    &case.state,
                    &case.rows,
                    case.present.then_some(&atlas),
                    &system,
                    &thumbnails,
                    pass,
                )
            });
            let expected = frame(&device, &queue, size, |pass| {
                replay(pass, native, &names, &atlas, &system, &thumbnails, &glyphs)
            });
            let errors = actual
                .iter()
                .zip(&expected)
                .enumerate()
                .filter(|(_, (a, b))| a != b)
                .collect::<Vec<_>>();
            assert!(
                errors.is_empty(),
                "case{i} {size:?}: {} RGBA channel differences; first {:?}",
                errors.len(),
                errors.first()
            );
            frames += 1;
        }
    }
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    eprintln!(
        "SaveMenu: {frames} byte-identical native parent/text frames720p/1080p; one retained View across all cases verifies cache refresh/eviction/re-entry; real saveload/systex/NewRodin resources; supplied thumbnail view boundary, no reconstructed-preview claim"
    );
}
fn replay(
    pass: &mut RenderPass,
    native: &[NativeDraw],
    names: &HashMap<u32, String>,
    atlas: &CharacterTextures,
    system: &CharacterTextures,
    thumbnails: &[Option<TextureSource<'_>>; 100],
    glyphs: &HashMap<u32, GpuFontGlyphHandle>,
) {
    let tint = |words: &[u32]| {
        shin_core::primitives::color::FloatColor4::from_vec4(Vec4::from_array(std::array::from_fn(
            |i| f32::from_bits(words[i]),
        )))
    };
    let packed = |word: u32| {
        let c = word.to_le_bytes();
        UnormColor::from_rgba(c[2], c[1], c[0], c[3])
    };
    for d in native {
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
                color: packed(d.vertices[4 * i + 3]),
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
        } else if d.texture == 0 || d.texture == 3 {
            let vertices = (0..d.vertices.len() / 6)
                .map(|i| PosColTexVertex {
                    position: vec3(
                        f32::from_bits(d.vertices[6 * i]),
                        f32::from_bits(d.vertices[6 * i + 1]),
                        f32::from_bits(d.vertices[6 * i + 2]),
                    ),
                    color: packed(d.vertices[6 * i + 3]),
                    texture_position: vec2(
                        f32::from_bits(d.vertices[6 * i + 4]),
                        f32::from_bits(d.vertices[6 * i + 5]),
                    ),
                })
                .collect::<Vec<_>>();
            let texture = if d.texture == 3 {
                thumbnails[d.point as usize].unwrap()
            } else {
                let owner = if names[&d.point] == "scroll" {
                    system
                } else {
                    atlas
                };
                owner.get(&names[&d.point]).as_source()
            };
            let vertices = VertexSource::VertexData {
                vertices: &vertices,
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
            assert_eq!(d.program, 3);
            let vertices: [TextVertex; 4] = std::array::from_fn(|i| TextVertex {
                position: Vec4::from_array(std::array::from_fn(|j| {
                    f32::from_bits(d.vertices[5 * i + j])
                })),
                color: f32::from_bits(d.vertices[5 * i + 4]),
            });
            pass.run(builder.build(
                RenderProgramWithArguments::Font {
                    vertices: VertexSource::VertexData {
                        vertices: &vertices,
                    },
                    glyph: glyphs[&d.point].as_texture_source().unwrap(),
                    transform,
                    color1: tint(&d.extra),
                    color2: tint(&d.color),
                },
                DrawPrimitive::TrianglesStrip,
            ));
        }
    }
}
