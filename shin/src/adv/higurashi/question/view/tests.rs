use super::*;
use crate::adv::higurashi::message_glyphs::tests::gpu::{gpu_assets, render_prepared_frame};
use std::{
    collections::HashMap,
    io::Write,
    process::{Command as Process, Stdio},
};

struct Case {
    model: Question,
    view: View,
    prompt: &'static str,
    labels: &'static [&'static str],
}
struct Packet {
    size: [u32; 2],
    outer: [u32; 2],
    cache: Vec<Draw>,
    composite: Vec<Draw>,
}
struct Draw {
    program: u32,
    texture: u32,
    point: u32,
    matrix: [u32; 16],
    extra: [u32; 16],
    color: [u32; 16],
    vertices: Vec<u32>,
    indices: Vec<u16>,
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
    for c in cases {
        let q = &c.model;
        input += &format!(
            "R {} {:x} {:x} {:x} {:x} {} {}\n",
            q.selected(),
            q.origin().x.to_bits(),
            q.origin().y.to_bits(),
            q.scale().to_bits(),
            q.opacity().to_bits(),
            hex(c.prompt),
            hex(&c.labels.join("|"))
        );
        for b in q.buttons() {
            input += &format!(
                "{:x} {:x}\n",
                b.focus.value().to_bits(),
                b.flash.value().to_bits()
            );
        }
    }
    let mut child = Process::new(std::env::var("SHIN_HIGURASHI_NATIVE_QUESTION_RENDER").unwrap())
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
    eprintln!("{}", String::from_utf8_lossy(&output.stderr));
    let output = String::from_utf8(output.stdout).unwrap();
    if let Ok(path) = std::env::var("SHIN_HIGURASHI_QUESTION_RENDER_PACKETS") {
        std::fs::write(path, &output).unwrap();
    }
    let mut words = output.split_whitespace();
    let mut packets = Vec::new();
    while let Some(op) = words.next() {
        assert_eq!(op, "R");
        let mut number = || words.next().unwrap().parse::<u32>().unwrap();
        let size = [number(), number()];
        let outer = [number(), number()];
        assert_eq!(words.next(), Some("C"));
        assert_eq!(words.next(), Some("K"));
        assert_eq!(
            std::array::from_fn::<_, 4, _>(|_| words.next().unwrap().parse::<u32>().unwrap()),
            [1, 0, 0, 1.0f32.to_bits()]
        );
        let mut packet = Packet {
            size,
            outer,
            cache: Vec::new(),
            composite: Vec::new(),
        };
        let mut composite = false;
        loop {
            let op = words.next().unwrap();
            if op == "S" {
                assert!(!composite);
                composite = true;
                continue;
            }
            let mut number = || words.next().unwrap().parse::<u32>().unwrap();
            if op == "E" {
                assert!(composite);
                assert_eq!(
                    number() as usize,
                    packet.cache.len() + packet.composite.len()
                );
                assert_eq!(
                    number() as usize,
                    packet
                        .cache
                        .iter()
                        .filter(|p| !p.indices.is_empty())
                        .count()
                );
                assert!(number() > 0);
                break;
            }
            assert_eq!(op, "P");
            let program = number();
            let texture = number();
            let point = number();
            let count = number();
            let index_count = number();
            let draw = Draw {
                program,
                texture,
                point,
                matrix: std::array::from_fn(|_| number()),
                extra: std::array::from_fn(|_| number()),
                color: std::array::from_fn(|_| number()),
                vertices: (0..count).map(|_| number()).collect(),
                indices: (0..index_count)
                    .map(|_| u16::try_from(number()).unwrap())
                    .collect(),
            };
            if composite {
                packet.composite.push(draw);
            } else {
                packet.cache.push(draw);
            }
        }
        packets.push(packet);
    }
    assert_eq!(packets.len(), cases.len());
    packets
}
fn replay(pass: &mut RenderPass, draws: &[Draw], view: &View, glyphs: &HashMap<u32, &Glyph>) {
    for d in draws {
        let native = Mat4::from_cols_array(&d.matrix.map(f32::from_bits));
        let vec = |w: &[u32]| Vec4::from_array(std::array::from_fn(|i| f32::from_bits(w[i])));
        if d.program == 0 {
            assert_eq!(d.texture, 4);
            assert_eq!(d.vertices.len(), 9);
            let v: [PosVertex; 3] = std::array::from_fn(|i| PosVertex {
                position: vec3(
                    f32::from_bits(d.vertices[i * 3]),
                    f32::from_bits(d.vertices[i * 3 + 1]),
                    0.5,
                ),
            });
            pass.run(builder().build(
                RenderProgramWithArguments::Clear {
                    vertices: VertexSource::VertexData { vertices: &v },
                    color: color(vec(&d.extra)),
                },
                DrawPrimitive::Triangles,
            ));
        } else if d.program == 6 {
            let v: Vec<_> = d
                .vertices
                .chunks_exact(4)
                .map(|p| PosTexVertex {
                    position: vec2(f32::from_bits(p[0]), f32::from_bits(p[1])),
                    texture_position: vec2(f32::from_bits(p[2]), f32::from_bits(p[3])),
                })
                .collect();
            let vertices = if d.indices.is_empty() {
                VertexSource::VertexData { vertices: &v }
            } else {
                VertexSource::VertexAndIndexData {
                    vertices: &v,
                    indices: &d.indices,
                }
            };
            let texture = match d.texture {
                0 => view.textures.frame.as_source(),
                1 => view.textures.button.as_source(),
                3 => TextureSource {
                    view: &view.view,
                    sampler: TextureSampler::Linear,
                },
                _ => panic!("unexpected texture"),
            };
            textured(pass, vertices, texture, native, vec(&d.extra));
        } else {
            assert_eq!(d.texture, 2);
            assert_eq!(d.vertices.len(), 20);
            let v: [TextVertex; 4] = std::array::from_fn(|i| TextVertex {
                position: vec(&d.vertices[i * 5..]),
                color: f32::from_bits(d.vertices[i * 5 + 4]),
            });
            let vertices = VertexSource::VertexData { vertices: &v };
            let glyph = glyphs[&d.point].handle.as_texture_source().unwrap();
            let transform = HigurashiTransform::convert_clip_depth(native);
            let program = match d.program {
                3 => RenderProgramWithArguments::Font {
                    vertices,
                    glyph,
                    transform,
                    color1: color(vec(&d.extra)),
                    color2: color(vec(&d.color)),
                },
                4 => RenderProgramWithArguments::FontBorder {
                    vertices,
                    glyph,
                    transform,
                    distances: std::array::from_fn(|i| {
                        vec2(
                            f32::from_bits(d.extra[i * 2]),
                            f32::from_bits(d.extra[i * 2 + 1]),
                        )
                    }),
                    color: color(vec(&d.color)),
                },
                _ => panic!("unexpected font program"),
            };
            pass.run(builder().build(program, DrawPrimitive::TrianglesStrip));
        }
    }
}
fn check_text(
    text: &Text,
    parent: Mat4,
    position: Vec2,
    anchor: Vec2,
    fill: Vec4,
    border: Vec4,
    draws: &mut std::slice::Iter<Draw>,
) -> usize {
    let mut count = 0;
    let transform = text
        .native_transform(parent, position, anchor)
        .to_cols_array()
        .map(f32::to_bits);
    for outline in [true, false] {
        let tint = if outline { border } else { fill };
        if tint.w <= 0.0 {
            continue;
        }
        for g in &text.glyphs {
            let draw = draws.next().unwrap();
            assert_eq!(draw.program, if outline { 4 } else { 3 });
            assert_eq!(draw.texture, 2);
            assert_eq!(draw.matrix, transform, "glyph transform");
            assert_eq!(
                draw.vertices,
                bytemuck::cast_slice::<_, u32>(&g.vertices),
                "glyph geometry"
            );
            if outline {
                assert_eq!(
                    &draw.extra[..16],
                    bytemuck::cast_slice::<_, u32>(&g.border),
                    "glyph outline"
                );
                assert_eq!(&draw.color[..4], &tint.to_array().map(f32::to_bits));
            } else {
                assert_eq!(&draw.extra[..4], &tint.to_array().map(f32::to_bits));
                assert_eq!(&draw.color[..4], &tint.to_array().map(f32::to_bits));
            }
            count += 1;
        }
    }
    count
}
fn check_patch(draw: &Draw, kind: usize, rect: Vec4, tint: Vec4, native: Mat4) {
    assert_eq!(draw.program, 6);
    assert_eq!(draw.texture, if kind == 0 { 0 } else { 1 });
    assert_eq!(draw.indices, INDICES);
    assert_eq!(
        draw.vertices,
        bytemuck::cast_slice::<_, u32>(&patch_vertices(kind, rect)),
        "patch kind {kind}"
    );
    assert_eq!(&draw.extra[..4], &tint.to_array().map(f32::to_bits));
    assert_eq!(draw.matrix, native.to_cols_array().map(f32::to_bits));
}
fn assert_pixels(actual: &[u8], expected: &[u8], label: &str) {
    assert_eq!(actual.len(), expected.len());
    let mut count = 0;
    let mut first = None;
    for (index, (a, b)) in actual.iter().zip(expected).enumerate() {
        if a != b {
            count += 1;
            first.get_or_insert((index, *a, *b));
        }
    }
    assert_eq!(count, 0, "{label}: first differing channel {first:?}");
}

#[test]
#[ignore = "original updated dialog packets and real fonts/atlas on Metal; no audio or saves"]
fn higurashi_question_cache_and_pixels_match_native_at_720p_and_1080p() {
    let (device, queue, server) = gpu_assets();
    let prompts = [
        "シーンジャンプを実行します。@rよろしいですか？",
        "短い確認",
        "@c$F00.色と 空白@r二行@r三行",
        "保存した記録を読み込んで現在の状態を置き換えます。よろしいですか？",
    ];
    let labels: [&[&str]; 4] = [
        &["はい", "いいえ"],
        &["確認"],
        &["短い", "これは長い選択肢の文字列です", "別の選択肢"],
        &["@c$F00.そのまま", "改行\nを除去"],
    ];
    let mut cases = Vec::new();
    for i in 0..16 {
        let label = labels[i % 4];
        let (mut model, view) = shin_tasks::block_on(View::load(
            &server,
            prompts[i % 4],
            label,
            (i % label.len()) as i32,
            true,
        ))
        .unwrap();
        model.state.phase = -1;
        model.state.selected = if i == 3 {
            -1
        } else if i == 7 {
            -2
        } else {
            (i % label.len()) as i32
        };
        model.state.origin = vec2(
            [545.0, -77.25, 1380.125, 560.5][i % 4],
            [400.5, 192.25, -45.0, 970.75][i % 4],
        );
        model
            .state
            .scale
            .fast_forward_to([1.0, 1.4, 0.5, 0.83][i % 4]);
        model
            .state
            .opacity
            .fast_forward_to([1.0, 0.37, 0.0, 0.73][i % 4]);
        for (n, b) in model.state.buttons.iter_mut().enumerate() {
            b.focus.fast_forward_to([0.0, 0.37, 1.0][(i + n) % 3]);
            b.flash.fast_forward_to([0.0, 0.35, 0.7][(i + 2 * n) % 3]);
            let f = b.focus.value();
            let lerp = |a: Vec4, b: Vec4| a + (b - a) * f;
            b.background = lerp(vec4(0.0, 0.0, 0.0, 0.5), Vec4::splat(0.7));
            b.fill = lerp(vec4(1.0, 1.0, 1.0, 0.7), Vec4::ONE);
        }
        cases.push(Case {
            model,
            view,
            prompt: prompts[i % 4],
            labels: label,
        });
    }
    let packets = native(&cases);
    let mut glyph_draws = 0;
    let mut frames = 0;
    for (id, (c, p)) in cases.iter().zip(&packets).enumerate() {
        let q = &c.model;
        let v = &c.view;
        assert_eq!(p.size, [v.texture.width(), v.texture.height()]);
        assert_eq!(p.outer, q.outer().to_array().map(f32::to_bits));
        let proj = projection(q.outer());
        let content = multiply(proj, Mat4::from_translation(vec3(30.0, 30.0, 0.0)));
        let mut draws = p.cache.iter();
        check_patch(
            draws.next().unwrap(),
            0,
            vec4(0.0, 0.0, q.outer().x, q.outer().y),
            Vec4::ONE,
            proj,
        );
        glyph_draws += check_text(
            &v.prompt,
            content,
            Vec2::ZERO,
            Vec2::ZERO,
            Vec4::ONE,
            vec4(0.0, 0.0, 0.0, 0.8),
            &mut draws,
        );
        for (n, (b, t)) in q.buttons().iter().zip(&v.labels).enumerate() {
            check_patch(draws.next().unwrap(), 1, b.rect, b.background, content);
            glyph_draws += check_text(
                t,
                content,
                vec2(b.rect.x, b.rect.y) + vec2(b.rect.z, b.rect.w) * 0.5,
                Vec2::splat(0.5),
                b.fill,
                b.outline,
                &mut draws,
            );
            if q.selected() == n as i32 {
                check_patch(draws.next().unwrap(), 2, b.rect, Vec4::ONE, content);
            }
            if b.flash.value() > 0.0 {
                check_patch(
                    draws.next().unwrap(),
                    3,
                    b.rect,
                    vec4(1.0, 1.0, 1.0, b.flash.value()),
                    content,
                );
            }
        }
        assert!(draws.next().is_none());
        assert_eq!(p.composite.len(), 2);
        assert_eq!(
            p.composite[1].matrix,
            v.native_transform(q).to_cols_array().map(f32::to_bits)
        );
        assert_eq!(
            &p.composite[1].extra[..4],
            &vec4(1.0, 1.0, 1.0, q.opacity())
                .to_array()
                .map(f32::to_bits)
        );
        let glyphs: HashMap<_, _> = v
            .prompt
            .glyphs
            .iter()
            .chain(v.labels.iter().flat_map(|t| &t.glyphs))
            .map(|g| (g.point as u32, g))
            .collect();
        let clear = UnormColor::from_rgba(0, 0, 0, 0);
        let actual = render_prepared_frame(
            &device,
            &queue,
            (p.size[0], p.size[1]),
            clear,
            |_| {},
            |pass| v.paint(pass, q),
        );
        let expected = render_prepared_frame(
            &device,
            &queue,
            (p.size[0], p.size[1]),
            clear,
            |_| {},
            |pass| replay(pass, &p.cache, v, &glyphs),
        );
        assert_pixels(&actual, &expected, &format!("cache case{id}"));
        frames += 1;
        for size in [(1280, 720), (1920, 1080)] {
            let actual = render_prepared_frame(
                &device,
                &queue,
                size,
                UnormColor::WHITE,
                |ctx| v.prepare(ctx, q),
                |pass| v.render(pass, q),
            );
            let expected = render_prepared_frame(
                &device,
                &queue,
                size,
                UnormColor::WHITE,
                |ctx| {
                    let mut pass = ctx.begin_pass(
                        TextureTarget {
                            kind: TextureTargetKind::RenderTexture,
                            view: &v.view,
                        },
                        None,
                        "native question cache replay",
                    );
                    pass.clear(Some(clear), None, None);
                    replay(&mut pass, &p.cache, v, &glyphs);
                },
                |pass| replay(pass, &p.composite, v, &glyphs),
            );
            assert_pixels(
                &actual,
                &expected,
                &format!("composite case{id} at{size:?}"),
            );
            frames += 1;
        }
    }
    eprintln!(
        "Higurashi question:{} cases; {glyph_draws} exact glyph packets; {frames} byte-identical cache/720p/1080p frames",
        cases.len()
    );
}
