use super::*;
use crate::adv::higurashi::{
    browser_controls::{RowMetadata, Viewport},
    browser_history::Item,
    message_glyphs::tests::gpu::{gpu_assets, render_prepared_frame},
};
use shin_core::{format::text::write_sjis_string, primitives::color::FloatColor4};
use shin_render::shaders::types::vertices::TextVertex;
use std::{
    collections::HashMap,
    fmt::Write as _,
    io::Write as _,
    process::{Command as Process, Stdio},
};

struct Draw {
    program: u32,
    point: u32,
    blend: u32,
    stencil: [u32; 5],
    matrix: [u32; 16],
    extra: [u32; 16],
    color: [u32; 16],
    vertices: Vec<u32>,
}
enum Event {
    Draw(Draw),
    Clear([u32; 4]),
}
struct Cached {
    index: usize,
    tint: [u32; 4],
    size: [u32; 2],
    fill: [u32; 4],
}
struct Packet {
    events: Vec<Event>,
    cache: Vec<Cached>,
}
fn word(w: &mut std::str::SplitWhitespace<'_>) -> u32 {
    w.next().unwrap().parse().unwrap()
}
fn array<const N: usize>(w: &mut std::str::SplitWhitespace<'_>) -> [u32; N] {
    std::array::from_fn(|_| word(w))
}
fn hex(text: &str) -> String {
    let mut bytes = Vec::new();
    write_sjis_string(text, &mut bytes).unwrap();
    if bytes.is_empty() {
        "-".into()
    } else {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}
fn native(input: &str) -> Vec<Packet> {
    let mut child = Process::new(std::env::var("SHIN_HIGURASHI_NATIVE_NAVIGATION_VIEW").unwrap())
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
    let data = input.to_owned();
    let writer = std::thread::spawn(move || stdin.write_all(data.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    if let Ok(dir) = std::env::var("SHIN_HIGURASHI_NAVIGATION_VIEW_CAPTURE") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("input.txt"), input).unwrap();
        std::fs::write(dir.join("native.txt"), &output.stdout).unwrap();
        std::fs::write(dir.join("native.log"), &output.stderr).unwrap();
    }
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    let mut w = text.split_whitespace();
    let mut packets = Vec::new();
    while let Some(op) = w.next() {
        assert_eq!(op, "R");
        let mut events = Vec::new();
        loop {
            match w.next().unwrap() {
                "K" => events.push(Event::Clear(array(&mut w))),
                "Q" => {
                    let program = word(&mut w);
                    let point = word(&mut w);
                    let blend = word(&mut w);
                    let n = word(&mut w);
                    let stencil = array(&mut w);
                    let matrix = array(&mut w);
                    let extra = array(&mut w);
                    let color = array(&mut w);
                    let vertices = (0..n).map(|_| word(&mut w)).collect();
                    events.push(Event::Draw(Draw {
                        program,
                        point,
                        blend,
                        stencil,
                        matrix,
                        extra,
                        color,
                        vertices,
                    }));
                }
                "C" => break,
                other => panic!("unexpected navigation event {other}"),
            }
        }
        let n = word(&mut w);
        let mut cache = Vec::new();
        for _ in 0..n {
            assert_eq!(w.next(), Some("T"));
            cache.push(Cached {
                index: word(&mut w) as usize,
                tint: array(&mut w),
                size: array(&mut w),
                fill: array(&mut w),
            });
        }
        assert_eq!(w.next(), Some("E"));
        let glyphs = word(&mut w);
        assert_eq!(
            glyphs as usize,
            events
                .iter()
                .filter(|e| matches!(e,Event::Draw(d)if d.program>=3))
                .count()
        );
        let _buffers = word(&mut w);
        let _glyph_handles = word(&mut w);
        packets.push(Packet { events, cache });
    }
    packets
}
fn draw<'a>(events: &mut std::slice::Iter<'a, Event>) -> &'a Draw {
    let Event::Draw(d) = events.next().unwrap() else {
        panic!("expected draw")
    };
    d
}
fn check_part(d: &Draw, p: Part, matrix: Mat4, masked: bool) {
    assert_eq!(d.program, if p.texture { 2 } else { 1 });
    assert_eq!(d.point, if p.texture { 0 } else { u32::MAX });
    assert_eq!(d.blend, if masked { 0 } else { 2 });
    assert_eq!(
        d.stencil,
        if masked {
            [7, 0, 0, 3, 0xffffff]
        } else {
            [7, 0, 0, 0, 0xffff]
        }
    );
    assert_eq!(d.matrix, matrix.to_cols_array().map(f32::to_bits));
    if p.texture {
        assert_eq!(
            d.vertices,
            bytemuck::cast_slice::<_, u32>(&p.sprite_vertices())
        );
    } else {
        assert_eq!(d.vertices, bytemuck::cast_slice::<_, u32>(&p.vertices()));
    }
}
fn check(packet: &Packet, view: &View, model: &Navigation, browser: &BrowserControls) -> usize {
    let mut events = packet.events.iter();
    let matrix = transform(model.slide());
    for p in background() {
        check_part(draw(&mut events), p, matrix, false);
    }
    let Event::Clear(k) = events.next().unwrap() else {
        panic!("expected stencil clear")
    };
    assert_eq!(*k, [4, 0xff000000, 0, 1.0f32.to_bits()]);
    check_part(draw(&mut events), mask(), matrix, true);
    let range = browser.viewport();
    assert_eq!(packet.cache.len(), view.cache.entries.len());
    let mut glyphs = 0;
    for (c, n) in view.cache.entries.iter().zip(&packet.cache) {
        let label = &c.label;
        let layout = &c.value.layout;
        assert_eq!(n.index, label.index);
        assert_eq!(n.tint, label.tint.to_array().map(f32::to_bits));
        assert_eq!(n.size, layout.size.to_array().map(f32::to_bits));
        let fill = label.fill(
            model.selected(),
            model.entries()[label.index].row,
            range.first,
            range.end,
        );
        assert_eq!(n.fill, fill.to_array().map(f32::to_bits));
        for border in [true, false] {
            for glyph in &layout.glyphs {
                let d = draw(&mut events);
                assert_eq!(d.program, if border { 4 } else { 3 });
                assert_eq!(d.point, glyph.point as u32);
                assert_eq!(d.blend, 2);
                assert_eq!(d.stencil, [2, 0, 0, 0, 0x1ffff]);
                assert_eq!(
                    d.matrix,
                    layout
                        .native_transform(label.index, model.scroll(), model.slide())
                        .to_cols_array()
                        .map(f32::to_bits)
                );
                assert_eq!(d.vertices, bytemuck::cast_slice::<_, u32>(&glyph.vertices));
                let extra: Vec<_> = if border {
                    glyph.border.iter().flat_map(|v| v.to_array()).collect()
                } else {
                    fill.to_array().into_iter().chain([0.0; 12]).collect()
                };
                assert_eq!(
                    d.extra,
                    extra
                        .iter()
                        .map(|v| v.to_bits())
                        .collect::<Vec<_>>()
                        .as_slice()
                );
                let tint = if border {
                    vec4(0.0, 0.0, 0.0, 240.0 * f32::from_bits(0x3b808081))
                } else {
                    fill
                };
                let color: Vec<_> = tint
                    .to_array()
                    .into_iter()
                    .chain([0.0; 12])
                    .map(f32::to_bits)
                    .collect();
                assert_eq!(d.color, color.as_slice());
                glyphs += 1;
            }
        }
    }
    for p in foreground(model.entries().len(), model.scroll()) {
        check_part(draw(&mut events), p, matrix, false);
    }
    assert!(events.next().is_none());
    glyphs
}
fn replay(
    pass: &mut RenderPass,
    packet: &Packet,
    view: &View,
    handles: &HashMap<u32, crate::asset::font::GpuFontGlyphHandle>,
) {
    for event in &packet.events {
        let Event::Draw(d) = event else {
            pass.clear(None, Some(0), None);
            continue;
        };
        let stencil = d.stencil;
        let request = builder(
            match stencil[0] {
                7 => StencilFunction::Always,
                2 => StencilFunction::Equal,
                _ => panic!("comparison"),
            },
            match stencil[3] {
                0 => StencilOperation::Keep,
                3 => StencilOperation::Increment,
                _ => panic!("stencil op"),
            },
            (stencil[4] >> 16) as u8,
            if d.blend == 0 {
                ColorBlendType::NoColor
            } else {
                ColorBlendType::Layer1
            },
        );
        let transform = HigurashiTransform::convert_clip_depth(Mat4::from_cols_array(
            &d.matrix.map(f32::from_bits),
        ));
        let vec = |w: &[u32]| Vec4::from_array(std::array::from_fn(|i| f32::from_bits(w[i])));
        match d.program {
            1 => {
                let vertices: Vec<_> = d
                    .vertices
                    .chunks_exact(4)
                    .map(|v| PosColVertex {
                        position: vec3(
                            f32::from_bits(v[0]),
                            f32::from_bits(v[1]),
                            f32::from_bits(v[2]),
                        ),
                        color: UnormColor(v[3]),
                    })
                    .collect();
                pass.run(request.build(
                    RenderProgramWithArguments::Fill {
                        vertices: VertexSource::VertexData {
                            vertices: &vertices,
                        },
                        transform,
                    },
                    DrawPrimitive::TrianglesStrip,
                ));
            }
            2 => {
                let vertices: Vec<_> = d
                    .vertices
                    .chunks_exact(6)
                    .map(|v| PosColTexVertex {
                        position: vec3(
                            f32::from_bits(v[0]),
                            f32::from_bits(v[1]),
                            f32::from_bits(v[2]),
                        ),
                        color: UnormColor(v[3]),
                        texture_position: vec2(f32::from_bits(v[4]), f32::from_bits(v[5])),
                    })
                    .collect();
                pass.run(request.build(
                    RenderProgramWithArguments::Sprite {
                        vertices: VertexSource::VertexData {
                            vertices: &vertices,
                        },
                        sprite: view.atlas.get("chapter").as_source(),
                        transform,
                    },
                    DrawPrimitive::TrianglesStrip,
                ));
            }
            3 | 4 => {
                let vertices: [TextVertex; 4] = std::array::from_fn(|i| TextVertex {
                    position: vec(&d.vertices[5 * i..]),
                    color: f32::from_bits(d.vertices[5 * i + 4]),
                });
                let glyph = handles[&d.point].as_texture_source().unwrap();
                let color = |v: Vec4| FloatColor4::from_rgba(v.x, v.y, v.z, v.w);
                let vertices = VertexSource::VertexData {
                    vertices: &vertices,
                };
                let p = if d.program == 3 {
                    RenderProgramWithArguments::Font {
                        vertices,
                        glyph,
                        transform,
                        color1: color(vec(&d.extra)),
                        color2: color(vec(&d.color)),
                    }
                } else {
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
                        color: color(vec(&d.color)),
                    }
                };
                pass.run(request.build(p, DrawPrimitive::TrianglesStrip));
            }
            _ => panic!("program"),
        }
    }
}

struct Case {
    model: Navigation,
    browser: BrowserControls,
    view: View,
    frames: Vec<(f32, f32, usize)>,
}
#[test]
#[ignore = "full updated7c31c packets and real browser atlas/glyphs on offscreen Metal at720p/1080p; no audio or saves"]
fn higurashi_navigation_full_view_matches_native_packets_and_pixels_at_720p_and_1080p() {
    let (device, queue, server) = gpu_assets();
    let mut input = String::new();
    let mut cases = Vec::new();
    for count in [1, 8, 20, 64] {
        for profile in 0..2 {
            let rows: Arc<[Row]> = (0..count)
                .map(|i| Row {
                    boundary: i as u32,
                    index: i as u32,
                    item: match i % 4 {
                        0 => Item::Message {
                            text: "本文".into(),
                            style: 0,
                            argument: None,
                        },
                        1 => Item::Choice {
                            title: if profile == 0 {
                                "選択 @r二行\n候補"
                            } else {
                                ""
                            }
                            .into(),
                            text: "ignored".into(),
                        },
                        2 => Item::Checkpoint {
                            title: "章".into(),
                            text: if profile == 0 { "詳細な説明" } else { "" }.into(),
                        },
                        _ => Item::CharacterChoice { response: 3 },
                    },
                })
                .collect::<Vec<_>>()
                .into();
            let markers: Vec<_> = (0..count)
                .filter(|i| profile == 0 || i % 4 != 0)
                .map(|i| i as u32)
                .collect();
            let indices = (0..count as u32).collect::<Vec<_>>();
            let selected = count / 2;
            let current = if profile == 0 { count - 1 } else { selected };
            let first = if profile == 0 { 0 } else { count / 3 };
            let end = if profile == 0 {
                count
            } else {
                (count * 2 / 3 + 1).min(count)
            };
            let mut browser = BrowserControls::new(
                rows.iter()
                    .map(|r| RowMetadata {
                        kind: match r.item {
                            Item::Message { .. } => 0,
                            Item::Choice { .. } => 1,
                            Item::Checkpoint { .. } => 2,
                            Item::CharacterChoice { .. } => 3,
                        },
                        voice_count: 0,
                        top: r.index as f32 * 60.0,
                        height: 60.0,
                    })
                    .collect(),
                Viewport {
                    first,
                    end,
                    minimum: 0.0,
                    extent: count as f32 * 60.0,
                },
                selected,
                0.0,
            )
            .unwrap();
            let model = Navigation::new(&indices, &markers, current, &mut browser).unwrap();
            let view = shin_tasks::block_on(View::load(&server, rows.clone())).unwrap();
            let max = (model.entries().len() as f32 * 60.0 - 720.0).max(0.0);
            let frames = vec![
                (0.0, 0.0, 0),
                (0.25, 0.35, model.entries().len() / 2),
                (max, 1.0, model.entries().len() - 1),
                (max * 0.5, 1.0, model.entries().len() / 2),
                (0.0, 1.0, 0),
            ];
            writeln!(input, "N {count} {selected} {current} {}", markers.len()).unwrap();
            for r in rows.iter() {
                let (kind, title, text) = match &r.item {
                    Item::Message { .. } => (0, "", ""),
                    Item::Choice { title, text } => (1, title.as_str(), text.as_str()),
                    Item::Checkpoint { title, text } => (2, title.as_str(), text.as_str()),
                    Item::CharacterChoice { .. } => (3, "", ""),
                };
                writeln!(input, "{kind} {} {}", hex(title), hex(text)).unwrap();
            }
            for m in markers {
                write!(input, "{m} ").unwrap();
            }
            input.push('\n');
            for (scroll, slide, sel) in &frames {
                writeln!(input, "R {scroll} {slide} {sel} {first} {end}").unwrap();
            }
            input.push_str("D\n");
            cases.push(Case {
                model,
                browser,
                view,
                frames,
            });
        }
    }
    let packets = native(&input);
    let mut packets = packets.iter();
    let mut frames = 0;
    let mut glyphs = 0;
    let mut pixels = 0;
    let mut hashes = String::new();
    for case in &mut cases {
        for &(scroll, slide, selected) in &case.frames {
            case.model.state.scroll.fast_forward_to(scroll);
            case.model.state.slide = slide;
            case.model.state.selected = selected;
            case.view.prepare(&server, &case.model).unwrap();
            let packet = packets.next().unwrap();
            glyphs += check(packet, &case.view, &case.model, &case.browser);
            let points: Vec<_> = packet
                .events
                .iter()
                .filter_map(|e| match e {
                    Event::Draw(d) if d.program >= 3 => Some(d.point as u16 as u32),
                    _ => None,
                })
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect();
            let chars: Vec<_> = points.iter().map(|p| char::from_u32(*p).unwrap()).collect();
            let loaded = case
                .view
                .font
                .clone()
                .load_glyphs(device.clone(), queue.clone(), &chars);
            let handles: HashMap<_, _> = points.into_iter().zip(loaded).collect();
            for size in [(1280, 720), (1920, 1080)] {
                let clear = UnormColor(0xff503020);
                let expected = render_prepared_frame(
                    &device,
                    &queue,
                    size,
                    clear,
                    |_| {},
                    |pass| replay(pass, packet, &case.view, &handles),
                );
                let actual = render_prepared_frame(
                    &device,
                    &queue,
                    size,
                    clear,
                    |_| {},
                    |pass| case.view.render(pass, &case.model, &case.browser),
                );
                let mismatch = actual.iter().zip(&expected).position(|(a, b)| a != b);
                assert_eq!(mismatch, None, "frame {frames} {size:?}");
                let hash = actual.iter().fold(0xcbf29ce484222325u64, |h, b| {
                    (h ^ *b as u64).wrapping_mul(0x100000001b3)
                });
                writeln!(hashes, "{frames} {} {} {hash:016x}", size.0, size.1).unwrap();
                pixels += size.0 as usize * size.1 as usize;
            }
            frames += 1;
        }
    }
    assert!(packets.next().is_none());
    if let Ok(dir) = std::env::var("SHIN_HIGURASHI_NAVIGATION_VIEW_CAPTURE") {
        std::fs::write(std::path::Path::new(&dir).join("frames.txt"), hashes).unwrap();
    }
    eprintln!(
        "Navigation full view:{} owners,{frames} logical frames,{} byte-identical720p/1080p frames,{glyphs} exact glyph packets,{pixels} compared pixels",
        cases.len(),
        frames * 2
    );
}
