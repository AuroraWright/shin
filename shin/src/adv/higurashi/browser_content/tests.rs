use super::*;
use crate::{
    adv::higurashi::{
        browser_glyphs::{self, glyph_color},
        browser_history::Row,
        message_glyphs::tests::gpu::{gpu_assets, render_prepared_frame},
    },
    asset::font::GpuFontLazy,
};
use shin_core::{
    format::text::write_sjis_string, layout::HigurashiBacklogVoices, primitives::color::FloatColor4,
};
use shin_render::shaders::types::vertices::TextVertex;
use std::{
    collections::HashMap,
    fmt::Write as _,
    io::Write as _,
    process::{Command as Process, Stdio},
    sync::Arc,
};

struct Draw {
    program: u32,
    texture: u32,
    count: usize,
    matrix: [u32; 16],
    extra: [u32; 16],
    color: [u32; 4],
    vertices: Vec<u32>,
}
struct Packet {
    draws: Vec<Draw>,
    cached: Vec<u32>,
    voices: Vec<u8>,
}
#[derive(Clone, Copy)]
struct Frame {
    scroll: f32,
    slide: f32,
    selected: usize,
    scrollbar: Scrollbar,
    jump_row: u32,
    jump: f32,
    voice_row: u32,
    voice: f32,
}
impl Frame {
    fn flashes(&self, row: u32) -> (f32, f32) {
        (
            if row == self.jump_row { self.jump } else { 0.0 },
            if row == self.voice_row {
                self.voice
            } else {
                0.0
            },
        )
    }
}
struct Case {
    rows: Vec<PositionedRow>,
    outline: u8,
    frames: Vec<Frame>,
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
fn word(w: &mut std::str::SplitWhitespace<'_>) -> u32 {
    w.next().unwrap().parse().unwrap()
}
fn array<const N: usize>(w: &mut std::str::SplitWhitespace<'_>) -> [u32; N] {
    std::array::from_fn(|_| word(w))
}
fn native(input: &str) -> Vec<Packet> {
    let mut child = Process::new(std::env::var("SHIN_HIGURASHI_NATIVE_BROWSER_CONTENT").unwrap())
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
    let data = input.to_owned();
    let writer = std::thread::spawn(move || stdin.write_all(data.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    if let Ok(dir) = std::env::var("SHIN_HIGURASHI_BROWSER_CONTENT_CAPTURE") {
        std::fs::create_dir_all(&dir).unwrap();
        for (file, data) in [
            ("input.txt", input.as_bytes()),
            ("native.txt", &output.stdout),
            ("native.log", &output.stderr),
        ] {
            std::fs::write(std::path::Path::new(&dir).join(file), data).unwrap();
        }
    }
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    text.lines()
        .map(|line| {
            let mut w = line.split_whitespace();
            assert_eq!(w.next(), Some("R"));
            let mut draws = Vec::new();
            while let Some(op) = w.next() {
                if op == "C" {
                    break;
                }
                assert_eq!(op, "Q");
                let program = word(&mut w);
                let texture = word(&mut w);
                let count = word(&mut w) as usize;
                let matrix = array(&mut w);
                let extra = array(&mut w);
                let color = array(&mut w);
                let vertices = (0..count * if program == 3 || program == 4 { 5 } else { 6 })
                    .map(|_| word(&mut w))
                    .collect();
                draws.push(Draw {
                    program,
                    texture,
                    count,
                    matrix,
                    extra,
                    color,
                    vertices,
                });
            }
            let cached = (0..word(&mut w)).map(|_| word(&mut w)).collect();
            assert_eq!(w.next(), Some("M"));
            let voices = (0..word(&mut w)).map(|_| word(&mut w) as u8).collect();
            assert_eq!(w.next(), Some("E"));
            assert_eq!(word(&mut w) as usize, draws.len());
            assert!(w.next().is_none());
            Packet {
                draws,
                cached,
                voices,
            }
        })
        .collect()
}
fn cases(font: Arc<GpuFontLazy>) -> (Vec<Case>, String) {
    let mut cases = Vec::new();
    let mut input = String::new();
    for count in [1, 8, 20] {
        for profile in 0..4 {
            let mut top = -112.25;
            let outline = profile as u8;
            writeln!(input, "N {count} {outline}").unwrap();
            let rows: Vec<_> = (0..count)
                .map(|i| {
                    let item = match i % 8 {
                        0 => Item::Message {
                            text: "名前@r@vvoice1.本文@vvoice2.続く".into(),
                            style: if profile % 2 == 0 { 0 } else { 3 },
                            argument: Some(9),
                        },
                        1 => Item::Message {
                            text: "@r@z60.小".into(),
                            style: 3,
                            argument: Some(if profile == 1 { -1 } else { 0 }),
                        },
                        2 => Item::Choice {
                            title: "選択".into(),
                            text: "返答@r次".into(),
                        },
                        3 => Item::Checkpoint {
                            title: "章".into(),
                            text: "説明".into(),
                        },
                        4 => Item::CharacterChoice { response: profile },
                        5 => Item::Message {
                            text: "@r@bかな.@<漢字@>続き".into(),
                            style: 0,
                            argument: Some(-1),
                        },
                        6 => Item::Message {
                            text: "@r@o50.@vvoice.@x20.音@v.".into(),
                            style: 4,
                            argument: Some(9),
                        },
                        _ => Item::Message {
                            text: format!("@r{}", "@vvoice.".repeat(256)),
                            style: 0,
                            argument: Some(9),
                        },
                    };
                    let row = Row {
                        index: i as u32,
                        boundary: i as u32 + 1,
                        item,
                    };
                    let measured = browser_glyphs::layout(
                        &row,
                        font.clone(),
                        HigurashiBacklogVoices::Count(0),
                    )
                    .unwrap();
                    let height = if matches!(row.item, Item::Message {argument:Some(n),..} if n>=0)
                    {
                        measured.size.y.max(76.0)
                    } else {
                        measured.size.y
                    } + 36.0;
                    let (kind, style, arg, title, text) = match &row.item {
                        Item::Message {
                            text,
                            style,
                            argument,
                        } => (0, *style, argument.unwrap_or(-1), "", text.as_str()),
                        Item::Choice { title, text } => (1, 0, 0, title.as_str(), text.as_str()),
                        Item::Checkpoint { title, text } => {
                            (2, 0, 0, title.as_str(), text.as_str())
                        }
                        Item::CharacterChoice { response } => (3, 0, *response, "", ""),
                    };
                    writeln!(
                        input,
                        "{kind} {style} {arg} {top} {height} {} {}",
                        hex(title),
                        hex(text)
                    )
                    .unwrap();
                    let result = PositionedRow {
                        row,
                        top,
                        height,
                        voice_count: measured.voice_count,
                    };
                    top += height;
                    result
                })
                .collect();
            let minimum = rows[0].top;
            let middle = rows[count / 2].top;
            let scrolls = [
                minimum,
                minimum + 166.0,
                middle,
                top - 818.0,
                minimum + 180.25,
                top + 162.0,
                minimum,
                minimum - 918.0,
                minimum,
                middle,
                minimum,
            ];
            let frames = scrolls
                .into_iter()
                .enumerate()
                .map(|(f, scroll)| {
                    let position = scroll as i32;
                    let end = top.ceil() as i32;
                    let scrollbar = Scrollbar {
                        position,
                        visible: if position.wrapping_add(818) > end {
                            end.wrapping_sub(position)
                        } else {
                            818
                        },
                        first: minimum.floor() as i32,
                        end,
                    };
                    let slide = [1.0, 0.5, 0.999, 0.25, 0.0, 1.0, 1.0, 1.0, 1.0, 0.5, 1.0][f];
                    let selected = [0, count / 2, count - 1][f % 3];
                    let jump = [0.0, 0.125, 0.5, 1.0][f % 4];
                    let voice = [0.0, 0.25, 0.75, 1.0][(f + profile as usize) % 4];
                    let frame = Frame {
                        scroll,
                        slide,
                        selected,
                        scrollbar,
                        jump_row: selected as u32,
                        jump,
                        voice_row: 0,
                        voice,
                    };
                    writeln!(
                        input,
                        "R {scroll} {slide} {selected} {} {} {} {} {} {jump} 0 {voice}",
                        scrollbar.position,
                        scrollbar.visible,
                        scrollbar.first,
                        scrollbar.end,
                        selected
                    )
                    .unwrap();
                    frame
                })
                .collect();
            input.push_str("D\n");
            cases.push(Case {
                rows,
                outline,
                frames,
            });
        }
    }
    (cases, input)
}
fn compare_sprite(d: &Draw, texture: u32, vertices: &[PosColTexVertex], matrix: Mat4) -> usize {
    assert_eq!(d.program, 2);
    compare_geometry(d, texture, bytemuck::cast_slice(vertices), matrix)
}
fn compare_geometry(d: &Draw, texture: u32, vertices: &[u32], matrix: Mat4) -> usize {
    assert_eq!(d.texture, texture);
    assert_eq!(d.matrix, matrix.to_cols_array().map(f32::to_bits));
    assert_eq!(d.vertices, vertices);
    16 + vertices.len()
}
fn check(
    packet: &Packet,
    cache: &BrowserGlyphCache,
    rows: &[PositionedRow],
    f: Frame,
) -> (usize, usize, usize) {
    assert_eq!(packet.cached, cache.indices().collect::<Vec<_>>());
    assert_eq!(
        packet.voices,
        rows.iter().map(|r| r.voice_count).collect::<Vec<_>>()
    );
    let mut draws = packet.draws.iter();
    let mut words = 0;
    let mut glyphs = 0;
    let mut decorations_count = 0;
    let matrix = parent_transform(f.slide);
    visit(
        cache,
        rows,
        f.scroll,
        f.selected,
        |row| f.flashes(row),
        |command| match command {
            Command::Cursor { row, alpha } => {
                for vertices in decorations::cursor(
                    cursor_rect(row, f.scroll),
                    FloatColor4::WHITE.with_alpha(alpha),
                ) {
                    words += compare_sprite(draws.next().unwrap(), 11, &vertices, matrix);
                    decorations_count += 1;
                }
            }
            Command::Glyphs(row) => {
                let model = cache.get(row.row.index).unwrap();
                let matrix = native_transform(row.top, f.scroll, f.slide);
                for border in [true, false] {
                    for glyph in &model.glyphs {
                        let d = draws.next().unwrap();
                        assert_eq!(d.program, if border { 4 } else { 3 });
                        assert_eq!(d.count, 4);
                        words += compare_geometry(
                            d,
                            glyph.character.codepoint as u32,
                            bytemuck::cast_slice(&glyph.vertices),
                            matrix,
                        );
                        let color = glyph_color(glyph.character.color);
                        if border {
                            assert_eq!(
                                d.extra,
                                std::array::from_fn(|i| glyph.border[i / 2][i % 2].to_bits())
                            );
                            assert_eq!(d.color, [0, 0, 0, 1.0f32.to_bits()]);
                            words += 20;
                        } else {
                            let top = if matches!(row.row.item, Item::Message { .. }) {
                                FloatColor4::WHITE
                            } else {
                                color
                            };
                            assert_eq!(d.extra[..4], top.into_array().map(f32::to_bits));
                            assert_eq!(d.color, color.into_array().map(f32::to_bits));
                            words += 8;
                        }
                        glyphs += 1;
                    }
                }
            }
            Command::Voice { row, flash } => {
                let d = draws.next().unwrap();
                assert_eq!(d.program, if flash > 0.0 { 5 } else { 2 });
                words += compare_geometry(
                    d,
                    13,
                    bytemuck::cast_slice(&voice_vertices(row.height)),
                    native_transform(row.top, f.scroll, f.slide),
                );
                if flash > 0.0 {
                    assert_eq!(
                        d.extra[..4],
                        vec4(1.0, 1.0, 1.0, flash).to_array().map(f32::to_bits)
                    );
                    words += 4;
                }
                decorations_count += 1;
            }
            Command::Line(y) => {
                for vertices in line_vertices(y) {
                    words += compare_sprite(draws.next().unwrap(), 14, &vertices, matrix);
                    decorations_count += 1;
                }
            }
        },
    );
    let matrix = decorations::scrollbar_transform(
        crate::layer::render_params::multiply_transforms(
            matrix,
            Mat4::from_translation(glam::vec3((1920.0 - f.slide * 1920.0) + 1848.0, 162.0, 0.0)),
        ),
        0.0,
        0.0,
    );
    for vertices in decorations::scrollbar(818.0, f.scrollbar) {
        words += compare_sprite(draws.next().unwrap(), 12, &vertices, matrix);
        decorations_count += 1;
    }
    assert!(draws.next().is_none());
    (words, glyphs, decorations_count)
}
fn replay(
    pass: &mut RenderPass,
    packet: &Packet,
    atlas: &CharacterTextures,
    system: &CharacterTextures,
    textures: &HashMap<u32, crate::asset::font::GpuFontGlyphHandle>,
) {
    for d in &packet.draws {
        let matrix = Mat4::from_cols_array(&d.matrix.map(f32::from_bits));
        let transform = HigurashiTransform::convert_clip_depth(matrix);
        let request = RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1);
        let color = |v: [u32; 4]| {
            FloatColor4::from_rgba(
                f32::from_bits(v[0]),
                f32::from_bits(v[1]),
                f32::from_bits(v[2]),
                f32::from_bits(v[3]),
            )
        };
        if d.program == 3 || d.program == 4 {
            let vertices: Vec<TextVertex> = d
                .vertices
                .chunks_exact(5)
                .map(|v| TextVertex {
                    position: Vec4::from_array(std::array::from_fn(|i| f32::from_bits(v[i]))),
                    color: f32::from_bits(v[4]),
                })
                .collect();
            let vertices = VertexSource::VertexData {
                vertices: &vertices,
            };
            let glyph = textures[&d.texture].as_texture_source().unwrap();
            let program = if d.program == 4 {
                RenderProgramWithArguments::FontBorder {
                    vertices,
                    glyph,
                    transform,
                    distances: std::array::from_fn(|i| {
                        glam::vec2(
                            f32::from_bits(d.extra[2 * i]),
                            f32::from_bits(d.extra[2 * i + 1]),
                        )
                    }),
                    color: color(d.color),
                }
            } else {
                RenderProgramWithArguments::Font {
                    vertices,
                    glyph,
                    transform,
                    color1: color(d.extra[..4].try_into().unwrap()),
                    color2: color(d.color),
                }
            };
            pass.run(request.build(program, DrawPrimitive::TrianglesStrip));
        } else {
            let vertices: Vec<PosColTexVertex> = d
                .vertices
                .chunks_exact(6)
                .map(|v| PosColTexVertex {
                    position: glam::vec3(
                        f32::from_bits(v[0]),
                        f32::from_bits(v[1]),
                        f32::from_bits(v[2]),
                    ),
                    color: UnormColor(v[3]),
                    texture_position: glam::vec2(f32::from_bits(v[4]), f32::from_bits(v[5])),
                })
                .collect();
            let name = match d.texture {
                11 => "cursor",
                12 => "scroll",
                13 => "voice",
                14 => "line",
                _ => panic!("atlas"),
            };
            let source = if d.texture == 12 { system } else { atlas };
            let vertices = VertexSource::VertexData {
                vertices: &vertices,
            };
            let texture = source.get(name).as_source();
            let program = if d.program == 5 {
                RenderProgramWithArguments::Button {
                    vertices,
                    texture,
                    transform,
                    flash1: Vec4::ZERO,
                    flash2: Vec4::from_array(std::array::from_fn(|i| f32::from_bits(d.extra[i]))),
                }
            } else {
                RenderProgramWithArguments::Sprite {
                    vertices,
                    sprite: texture,
                    transform,
                }
            };
            pass.run(request.build(program, DrawPrimitive::TrianglesStrip));
        }
    }
}
#[test]
#[ignore = "original77254 populated-cache/voice/separator/scrollbar packets and actual archive/font offscreen720p/1080p; no audio/window/saves"]
fn higurashi_browser_content_matches_native_retained_packets_and_pixels() {
    let (device, queue, assets) = gpu_assets();
    let font: Arc<GpuFontLazy> = assets.load_sync("/newrodin.fnt").unwrap();
    let atlas: Arc<CharacterTextures> = assets.load_sync("/logview.txa").unwrap();
    let system: Arc<CharacterTextures> = assets.load_sync("/systex.txa").unwrap();
    for name in ["cursor", "voice", "line"] {
        assert!(atlas.contains(name));
    }
    assert!(system.contains("scroll"));
    let (cases, input) = cases(font.clone());
    let packets = native(&input);
    let mut packets = packets.iter();
    let mut total_words = 0;
    let mut glyph_packets = 0;
    let mut decoration_packets = 0;
    let mut frames = 0;
    let mut pixels = 0;
    let mut unsorted = false;
    let mut hashes = String::new();
    for case in &cases {
        let mut cache = BrowserGlyphCache::default();
        for &frame in &case.frames {
            let previous: HashMap<_, _> = cache
                .indices()
                .map(|i| (i, cache.get(i).unwrap().glyphs.as_ptr()))
                .collect();
            cache
                .synchronize(
                    &device,
                    &queue,
                    font.clone(),
                    &case.rows,
                    frame.scroll,
                    case.outline,
                )
                .unwrap();
            for (&i, &pointer) in &previous {
                if let Some(row) = cache.get(i) {
                    assert_eq!(
                        row.glyphs.as_ptr(),
                        pointer,
                        "retained populated glyph owner"
                    );
                }
            }
            let packet = packets.next().unwrap();
            let (words, glyphs, decorations) = check(packet, &cache, &case.rows, frame);
            total_words += words;
            glyph_packets += glyphs;
            decoration_packets += decorations;
            unsorted |= packet.cached.windows(2).any(|v| v[0] > v[1]);
            let points: Vec<_> = packet
                .draws
                .iter()
                .filter(|d| d.program == 3 || d.program == 4)
                .map(|d| char::from_u32(d.texture).unwrap())
                .collect();
            let handles = font
                .clone()
                .load_glyphs(device.clone(), queue.clone(), &points);
            let textures: HashMap<_, _> =
                points.into_iter().map(|c| c as u32).zip(handles).collect();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while textures.values().any(|h| h.as_texture_source().is_err()) {
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            for size in [(1280, 720), (1920, 1080)] {
                let clear = UnormColor(0xff503020);
                let expected = render_prepared_frame(
                    &device,
                    &queue,
                    size,
                    clear,
                    |_| {},
                    |pass| replay(pass, packet, &atlas, &system, &textures),
                );
                let actual = render_prepared_frame(
                    &device,
                    &queue,
                    size,
                    clear,
                    |_| {},
                    |pass| {
                        render(
                            pass,
                            &atlas,
                            &system,
                            &cache,
                            &case.rows,
                            frame.scroll,
                            frame.slide,
                            frame.selected,
                            |row| frame.flashes(row),
                            frame.scrollbar,
                        )
                    },
                );
                assert_eq!(
                    actual.iter().zip(&expected).position(|(a, b)| a != b),
                    None,
                    "content frame{frames} {size:?}"
                );
                pixels += size.0 as usize * size.1 as usize;
                let hash = actual.iter().fold(0xcbf29ce484222325u64, |h, b| {
                    (h ^ *b as u64).wrapping_mul(0x100000001b3)
                });
                writeln!(hashes, "{frames} {} {} {hash:016x}", size.0, size.1).unwrap();
            }
            frames += 1;
        }
        cache
            .synchronize(
                &device,
                &queue,
                font.clone(),
                &case.rows,
                100000.0,
                case.outline,
            )
            .unwrap();
        assert!(cache.is_empty() && cache.indices().next().is_none());
    }
    assert!(packets.next().is_none() && unsorted);
    if let Ok(dir) = std::env::var("SHIN_HIGURASHI_BROWSER_CONTENT_CAPTURE") {
        std::fs::write(std::path::Path::new(&dir).join("frames.txt"), hashes).unwrap();
    }
    eprintln!(
        "Browser content:{} owners,{frames} logical frames,{} byte-identical720p/1080p frames,{glyph_packets} exact glyph packets,{decoration_packets} decorations,{total_words} exact words,{pixels} compared pixels; retained insertion order and balanced native cull",
        cases.len(),
        frames * 2
    );
}
