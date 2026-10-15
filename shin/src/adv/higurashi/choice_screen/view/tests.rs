use super::*;
use crate::adv::higurashi::message_glyphs::tests::gpu::{gpu_assets, render_prepared_frame};
use shin_core::{format::scenario::instruction_elements::Register, primitives::color::UnormColor};
use shin_render::{
    DepthStencilState, StencilFunction, StencilOperation, StencilPipelineState, StencilState,
    shaders::types::vertices::TextVertex,
};
use std::{
    fmt::Write as _,
    io::Write as _,
    process::{Command, Stdio},
};

#[derive(Debug)]
struct Draw {
    program: u32,
    texture: u32,
    blend: u32,
    stencil: [u32; 5],
    matrix: [u32; 16],
    extra: [u32; 16],
    color: [u32; 16],
    vertices: Vec<u32>,
}
struct Frame {
    draws: Vec<Draw>,
    rows: Vec<[u32; 11]>,
    seeds: u32,
}
fn word(w: &mut std::str::SplitWhitespace<'_>) -> u32 {
    w.next().unwrap().parse().unwrap()
}
fn words<const N: usize>(w: &mut std::str::SplitWhitespace<'_>) -> [u32; N] {
    std::array::from_fn(|_| word(w))
}
fn native(input: String) -> Vec<Frame> {
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_CHOICE_RENDER").unwrap())
        .args([
            "higurashi/uncompressed_executable_update",
            "/tmp/higurashi-newrodin.fnt",
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
    if let Ok(dir) = std::env::var("SHIN_HIGURASHI_CHOICE_RENDER_CAPTURE") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("input.txt"), input).unwrap();
        std::fs::write(dir.join("output.txt"), &output.stdout).unwrap();
        std::fs::write(dir.join("stderr.txt"), &output.stderr).unwrap();
    }
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| {
            let mut w = line.split_whitespace();
            assert_eq!(w.next(), Some("R"));
            let mut draws = Vec::new();
            loop {
                match w.next().unwrap() {
                    "Q" => {
                        let program = word(&mut w);
                        let texture = word(&mut w);
                        let blend = word(&mut w);
                        let n = word(&mut w);
                        draws.push(Draw {
                            program,
                            texture,
                            blend,
                            stencil: words(&mut w),
                            matrix: words(&mut w),
                            extra: words(&mut w),
                            color: words(&mut w),
                            vertices: (0..n).map(|_| word(&mut w)).collect(),
                        });
                    }
                    "C" => break,
                    s => panic!("choice packet {s}"),
                }
            }
            let count = word(&mut w);
            let seeds = word(&mut w);
            let rows = (0..count)
                .map(|_| {
                    assert_eq!(w.next(), Some("T"));
                    words(&mut w)
                })
                .collect();
            assert_eq!(w.next(), Some("E"));
            assert_eq!(word(&mut w) as usize, draws.len());
            let _buffers = word(&mut w);
            let _handles = word(&mut w);
            assert!(w.next().is_none());
            Frame { draws, rows, seeds }
        })
        .collect()
}
fn vector(words: &[u32]) -> Vec4 {
    Vec4::from_array(std::array::from_fn(|i| f32::from_bits(words[i])))
}
fn native_color(color: u32) -> u32 {
    (color & 0xff00ff00) | ((color & 255) << 16) | ((color >> 16) & 255)
}
fn sprite(d: &Draw, texture: u32, matrix: Mat4, rect: Vec4, uv: Vec4, alpha: f32) {
    assert_eq!((d.program, d.texture, d.blend), (2, texture, 2));
    assert_eq!(
        d.matrix,
        matrix.to_cols_array().map(f32::to_bits),
        "sprite matrix for{texture}"
    );
    let mut vertices = d.vertices.clone();
    for v in vertices.chunks_exact_mut(6) {
        v[3] = native_color(v[3]);
    }
    assert_eq!(
        vertices,
        bytemuck::cast_slice::<_, u32>(&sprite_vertices(rect, uv, alpha)),
        "sprite vertices for{texture}"
    );
}
fn check(frame: &Frame, screen: &Screen, visibility: u32, kind: PassKind) -> usize {
    assert_eq!(frame.seeds, u32::from(visibility & 0x1000 != 0));
    assert_eq!(frame.rows.len(), screen.model.rows.len());
    for (native, row) in frame.rows.iter().zip(&screen.model.rows) {
        let size = screen.glyphs[&row.spec.original].layout.size;
        let tint = if row.spec.seen {
            vec4(0.75, 0.75, 1.0, 1.0)
        } else {
            Vec4::ONE
        };
        assert_eq!(
            *native,
            [
                row.id as u32,
                row.spec.original as u32,
                row.spec.badges as u32,
                row.y.to_bits(),
                row.spec.height.to_bits(),
                tint.x.to_bits(),
                tint.y.to_bits(),
                tint.z.to_bits(),
                tint.w.to_bits(),
                size.x.to_bits(),
                size.y.to_bits()
            ],
            "native choice row{}",
            row.spec.original
        );
    }
    if kind == PassKind::Opaque {
        assert!(frame.draws.is_empty());
        return 0;
    }
    let mut draws = frame.draws.iter();
    let mut glyphs = 0;
    let parent = screen.parent_transform();
    for row in &screen.model.rows {
        let matrix = screen.row_transform(parent, row);
        let alpha = row.alpha.value() * screen.model.opacity;
        for (y0, y1, v0, v1) in [
            (-8.0, 0.0, 0.0, 0.1),
            (0.0, row.spec.height, 0.1, 0.9),
            (row.spec.height, row.spec.height + 8.0, 0.9, 1.0),
        ] {
            sprite(
                draws.next().unwrap(),
                0,
                matrix,
                vec4(360.0, y0, 1560.0, y1),
                vec4(0.0, v0, 1.0, v1),
                alpha,
            );
        }
        let glyph = &screen.glyphs[&row.spec.original];
        let matrix = screen.text_transform(matrix, glyph.layout.size);
        let color = Vec4::new(
            row.focus.value(),
            row.focus.value(),
            row.focus.value(),
            alpha,
        );
        let tint = if row.spec.seen {
            vec4(0.75, 0.75, 1.0, 1.0)
        } else {
            Vec4::ONE
        };
        if alpha > 0.0 {
            for border in [true, false] {
                for g in &glyph.layout.glyphs {
                    let d = draws.next().unwrap();
                    assert_eq!(
                        (d.program, d.texture, d.blend),
                        (if border { 4 } else { 3 }, g.point as u32, 2)
                    );
                    assert_eq!(
                        d.matrix,
                        matrix.to_cols_array().map(f32::to_bits),
                        "glyph matrix {}",
                        g.point
                    );
                    assert_eq!(
                        d.vertices,
                        bytemuck::cast_slice::<_, u32>(&g.vertices),
                        "glyph vertices {}",
                        g.point
                    );
                    let extra: Vec<f32> = if border {
                        g.border.iter().flat_map(|v| v.to_array()).collect()
                    } else {
                        color.to_array().into_iter().chain([0.0; 12]).collect()
                    };
                    assert_eq!(
                        d.extra,
                        extra
                            .iter()
                            .map(|v| v.to_bits())
                            .collect::<Vec<_>>()
                            .as_slice(),
                        "glyph uniforms {}",
                        g.point
                    );
                    let fill = if border {
                        vec4(0.0, 0.0, 0.0, alpha)
                    } else {
                        color * tint
                    };
                    assert_eq!(
                        d.color,
                        fill.to_array()
                            .into_iter()
                            .chain([0.0; 12])
                            .map(f32::to_bits)
                            .collect::<Vec<_>>()
                            .as_slice()
                    );
                    glyphs += 1;
                }
            }
        }
    }
    if screen.model.selected >= 0 && screen.route_navigation {
        let row = &screen.model.rows[screen.model.selected as usize];
        let mask = row.spec.badges & 63;
        if mask != 0 {
            let y = row.y + (row.spec.height - 64.0) * 0.5;
            let x = screen.model.badge_slide.value() * -300.0 + 1480.0;
            let alpha = screen.model.badge_alpha.value() * screen.model.opacity;
            sprite(
                draws.next().unwrap(),
                1,
                parent,
                vec4(x - 4.0, y + 36.0, x - 4.0 + 420.0, y + 68.0),
                vec4(0.0, 0.0, 1.0, 1.0),
                alpha,
            );
            let mut left = x + (360 - mask.count_ones() as i32 * 60) as f32 * 0.5;
            for (i, (top, bottom)) in [
                (0x3b257eb5, 0x3e2814b0),
                (0x3e2d40a5, 0x3ea95fad),
                (0x3eabf5a8, 0x3efeb502),
                (0x3f00a57f, 0x3f2a052c),
                (0x3f2b5029, 0x3f54afd6),
                (0x3f55fad4, 0x3f7f5a81),
            ]
            .into_iter()
            .enumerate()
            {
                if mask & (1 << i) != 0 {
                    sprite(
                        draws.next().unwrap(),
                        2,
                        parent,
                        vec4(left, y, left + 64.0, y + 64.0),
                        vec4(
                            f32::from_bits(0x3c783e10),
                            f32::from_bits(top),
                            f32::from_bits(0x3f7c1f08),
                            f32::from_bits(bottom),
                        ),
                        alpha,
                    );
                    left += 60.0;
                }
            }
        }
    }
    assert!(draws.next().is_none());
    for d in &frame.draws {
        assert_eq!(d.stencil, [6, 0, 0, 2, 0x5ffff]);
    }
    glyphs
}
fn replay(
    pass: &mut RenderPass,
    frame: &Frame,
    screen: &Screen,
    handles: &HashMap<u32, crate::asset::font::GpuFontGlyphHandle>,
) {
    for d in &frame.draws {
        assert_eq!(&d.stencil[..4], &[6, 0, 0, 2]);
        let builder = RenderRequestBuilder::new()
            .color_blend_type(ColorBlendType::Layer1)
            .depth_stencil(DepthStencilState {
                stencil: StencilState {
                    pipeline: StencilPipelineState {
                        function: StencilFunction::GreaterOrEqual,
                        pass_operation: StencilOperation::Replace,
                        ..Default::default()
                    },
                    stencil_reference: (d.stencil[4] >> 16) as u8,
                },
                ..Default::default()
            });
        let transform = HigurashiTransform::convert_clip_depth(Mat4::from_cols_array(
            &d.matrix.map(f32::from_bits),
        ));
        if d.program == 2 {
            let vertices: Vec<_> = d
                .vertices
                .chunks_exact(6)
                .map(|v| PosColTexVertex {
                    position: vec3(
                        f32::from_bits(v[0]),
                        f32::from_bits(v[1]),
                        f32::from_bits(v[2]),
                    ),
                    color: UnormColor(native_color(v[3])),
                    texture_position: vec2(f32::from_bits(v[4]), f32::from_bits(v[5])),
                })
                .collect();
            let texture = match d.texture {
                0 => &screen.textures.selection,
                1 => &screen.textures.navigation_arrows,
                2 => &screen.textures.navigation_icons,
                _ => panic!("texture"),
            };
            pass.run(builder.build(
                RenderProgramWithArguments::Sprite {
                    vertices: VertexSource::VertexData {
                        vertices: &vertices,
                    },
                    sprite: texture.as_source(),
                    transform,
                },
                DrawPrimitive::TrianglesStrip,
            ));
        } else {
            let vertices: [TextVertex; 4] = std::array::from_fn(|i| TextVertex {
                position: vector(&d.vertices[5 * i..]),
                color: f32::from_bits(d.vertices[5 * i + 4]),
            });
            let vertices = VertexSource::VertexData {
                vertices: &vertices,
            };
            let glyph = handles[&d.texture].as_texture_source().unwrap();
            let program = if d.program == 3 {
                RenderProgramWithArguments::Font {
                    vertices,
                    glyph,
                    transform,
                    color1: FloatColor4::from_vec4(vector(&d.extra)),
                    color2: FloatColor4::from_vec4(vector(&d.color)),
                }
            } else {
                assert_eq!(d.program, 4);
                RenderProgramWithArguments::FontBorder {
                    vertices,
                    glyph,
                    transform,
                    distances: std::array::from_fn(|i| {
                        vec2(
                            f32::from_bits(d.extra[i * 2]),
                            f32::from_bits(d.extra[i * 2 + 1]),
                        )
                    }),
                    color: FloatColor4::from_vec4(vector(&d.color)),
                }
            };
            pass.run(builder.build(program, DrawPrimitive::TrianglesStrip));
        }
    }
}
#[derive(Clone)]
struct Values {
    selected: i32,
    opacity: f32,
    slide: f32,
    alpha: f32,
    route: bool,
    kind: PassKind,
    properties: Vec<(usize, f32)>,
    rows: Vec<(f32, f32, f32)>,
}
fn apply(screen: &mut Screen, v: &Values) {
    screen.model.selected = v.selected;
    screen.model.opacity = v.opacity;
    screen.model.badge_slide.fast_forward_to(v.slide);
    screen.model.badge_alpha.fast_forward_to(v.alpha);
    screen.route_navigation = v.route;
    for &(p, value) in &v.properties {
        screen.properties.control(p, value, Ticks::ZERO, 0).unwrap();
    }
    for (row, &(x, alpha, focus)) in screen.model.rows.iter_mut().zip(&v.rows) {
        row.x.fast_forward_to(x);
        row.alpha.fast_forward_to(alpha);
        row.focus.fast_forward_to(focus);
    }
}
#[test]
#[ignore = "complete original c0e30/c21c8 real font/layout/gradient/atlas/stencil packets and offscreen Metal720p/1080p pixels;no audio or saves"]
fn higurashi_choice_screen_renderer_matches_native_packets_and_pixels() {
    let (device, queue, server) = gpu_assets();
    let font: Arc<GpuFontLazy> = server.load_sync("/newrodin.fnt").unwrap();
    let textures: Arc<HigurashiMessageTextures> = server.load_sync("/msgtex.txa").unwrap();
    let specs = vec![
        (Vec::<String>::new(), 0x1000u32, 0),
        (vec!["".into()], 1, 0),
        (vec!["*05.左へ進む".into()], 1, 1),
        (
            vec![
                "*012345.ここに残る".into(),
                "@r二行@r三行".into(),
                "@bがっこう.@<学校@>へ行く".into(),
            ],
            7,
            6,
        ),
        (
            (0..12)
                .map(|i| {
                    if i == 3 {
                        format!("*3.{}", "長い選択肢".repeat(12))
                    } else {
                        format!("*{}.選択肢{i:02}", i % 6)
                    }
                })
                .collect(),
            0xfff,
            0xaaa,
        ),
        (
            vec![
                "*05.One".into(),
                "Two".into(),
                "*1234.Three".into(),
                "Fourth".into(),
            ],
            0x100d,
            5,
        ),
    ];
    let mut input = String::new();
    let mut cases = Vec::new();
    for (variants, mask, seen) in specs {
        writeln!(input, "N {} {mask:x} {seen:x} 42", variants.len()).unwrap();
        for text in &variants {
            writeln!(
                input,
                "{}",
                if text.is_empty() {
                    "-".into()
                } else {
                    text.bytes().map(|b| format!("{b:02x}")).collect::<String>()
                }
            )
            .unwrap();
        }
        let request = ChoiceRequest {
            base: 0,
            index: 0,
            destination: Register::from_regular_register(31),
            visibility: mask as i32,
            title: String::new(),
            variants,
        };
        let screen = Screen::new(
            &server,
            font.clone(),
            textures.clone(),
            &request,
            seen,
            [1, 2, 3, 4],
            (mask & 0x1000 != 0).then_some(42),
        )
        .unwrap();
        let count = screen.model.rows.len();
        let mut frames = Vec::new();
        for profile in 0..7 {
            let v = Values {
                selected: if count == 0 || profile == 0 {
                    -1
                } else if profile == 6 {
                    0 // All six badges visible with the retained property transform.
                } else {
                    (profile as usize % count) as i32
                },
                opacity: if profile == 2 {
                    0.35
                } else if profile == 3 {
                    0.0
                } else {
                    1.0
                },
                slide: if profile == 2 { 0.4 } else { 0.0 },
                alpha: if profile == 2 { 0.375 } else { 1.0 },
                route: profile != 4,
                kind: if profile == 5 {
                    PassKind::Opaque
                } else {
                    PassKind::Transparent
                },
                properties: if profile == 4 {
                    vec![
                        (0, 43.75),
                        (1, -12.25),
                        (12, 935.0),
                        (13, 1005.0),
                        (16, 37.0),
                        (10, -85.5),
                        (11, 3.25),
                    ]
                } else {
                    Vec::new()
                },
                rows: (0..count)
                    .map(|i| {
                        if profile == 2 {
                            (
                                -91.25 + i as f32 * 8.5,
                                if i % 3 == 0 { 0.0 } else { 0.375 },
                                0.725,
                            )
                        } else {
                            (0.0, 1.0, if i == 0 { 1.0 } else { 0.5 })
                        }
                    })
                    .collect(),
            };
            writeln!(
                input,
                "R {} {:x} {:x} {:x} {} 5 {} {}",
                v.selected,
                v.opacity.to_bits(),
                v.slide.to_bits(),
                v.alpha.to_bits(),
                u8::from(v.route),
                u8::from(v.kind == PassKind::Opaque),
                v.properties.len()
            )
            .unwrap();
            for &(p, value) in &v.properties {
                writeln!(input, "{p} {:x}", value.to_bits()).unwrap();
            }
            for &(x, a, f) in &v.rows {
                writeln!(
                    input,
                    "{:x} {:x} {:x}",
                    x.to_bits(),
                    a.to_bits(),
                    f.to_bits()
                )
                .unwrap();
            }
            frames.push(v);
        }
        input.push_str("D\n");
        cases.push((screen, mask, frames));
    }
    let frames = native(input);
    let mut frames = frames.iter();
    let mut pixels = 0;
    let mut number = 0;
    let mut glyphs = 0;
    let mut hashes = String::new();
    for (screen, mask, values) in &mut cases {
        for v in values {
            apply(screen, v);
            let frame = frames.next().unwrap();
            glyphs += check(frame, screen, *mask, v.kind);
            let points = frame
                .draws
                .iter()
                .filter(|d| d.program == 3 || d.program == 4)
                .map(|d| d.texture)
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let chars = points
                .iter()
                .map(|p| char::from_u32(*p).unwrap())
                .collect::<Vec<_>>();
            let loaded = font
                .clone()
                .load_glyphs(device.clone(), queue.clone(), &chars);
            let handles = points.into_iter().zip(loaded).collect::<HashMap<_, _>>();
            for size in [(1280, 720), (1920, 1080)] {
                let clear = UnormColor(0xff503020);
                let expected = render_prepared_frame(
                    &device,
                    &queue,
                    size,
                    clear,
                    |_| {},
                    |pass| replay(pass, frame, screen, &handles),
                );
                let actual = render_prepared_frame(
                    &device,
                    &queue,
                    size,
                    clear,
                    |_| {},
                    |pass| screen.render(pass, 5, v.kind).unwrap(),
                );
                assert_eq!(
                    actual.iter().zip(&expected).position(|(a, b)| a != b),
                    None,
                    "choice frame{number} {size:?}"
                );
                let hash = actual.iter().fold(0xcbf29ce484222325u64, |h, b| {
                    (h ^ *b as u64).wrapping_mul(0x100000001b3)
                });
                writeln!(hashes, "{number} {} {} {hash:016x}", size.0, size.1).unwrap();
                pixels += size.0 as usize * size.1 as usize;
            }
            number += 1;
        }
    }
    assert!(frames.next().is_none());
    if let Ok(path) = std::env::var("SHIN_HIGURASHI_CHOICE_PIXEL_HASHES") {
        std::fs::write(path, hashes).unwrap();
    }
    eprintln!(
        "Choice renderer:6 owners,{number} logical frames,{} byte-identical720p/1080p RGBA frames,{glyphs} exact glyph packets,{pixels} compared pixels;real font metrics/UTF8/ruby/multiline,empty/filter/shuffle,seen gradient,all6 badges/route toggle,opacity/slide/property transforms/opaque gating and native stencils pass",
        number * 2
    );
}
