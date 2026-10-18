use super::*;
use shin_core::{format::font::read_font_metrics, primitives::color::FloatColor4};
use std::{
    io::{BufReader, Write},
    process::{Command, Stdio},
};
fn hex(text: &str) -> String {
    if text.is_empty() {
        "-".into()
    } else {
        text.bytes().map(|b| format!("{b:02x}")).collect()
    }
}
fn native(input: String) -> String {
    let mut child =
        Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_SCENARIO_NOTIFICATION").unwrap())
            .args([
                "game-data/higurashi/uncompressed_executable_update",
                "/tmp/higurashi-newrodin.fnt",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
    let mut pipe = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || pipe.write_all(input.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    eprintln!("{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap()
}
fn layout(font: impl shin_core::layout::font::FontMetrics + Clone, text: &str) -> Layout {
    Layout::from_script(
        font,
        text,
        LayoutParams {
            layout_width: 1916.0,
            text_size: 48.0,
            perform_soft_breaks: false,
            ..Default::default()
        },
    )
    .unwrap()
}
fn matrix(size: glam::Vec2) -> glam::Mat4 {
    let projection = Mat4::from_cols_array(&[
        2.0 / 1920.0,
        0.0,
        0.0,
        0.0,
        0.0,
        2.0 / -1080.0,
        0.0,
        0.0,
        0.0,
        0.0,
        -1.0,
        0.0,
        -1.0,
        1.0,
        -0.0,
        1.0,
    ]);
    multiply(
        projection,
        multiply(
            Mat4::from_translation(vec3(4.0, 1076.0, 0.0)),
            multiply(
                Mat4::from_scale(vec3(1.0, 1.0, 0.0)),
                Mat4::from_translation(vec3(0.0, -size.y, 0.0)),
            ),
        ),
    )
}
fn snapshot(clock: &Indicator, layout: Option<&Layout>) -> Vec<u32> {
    let size = layout.map_or(glam::Vec2::ZERO, |l| l.size);
    vec![
        clock.hidden as u32,
        clock.opacity.to_bits(),
        clock.transition.value().to_bits(),
        clock.transition.is_waiting() as u32,
        size.x.to_bits(),
        size.y.to_bits(),
    ]
}
fn packets(clock: &Indicator, layout: &Layout, enabled: bool) -> Vec<Vec<u32>> {
    let alpha = clock.opacity * clock.transition.value();
    if !enabled || clock.transition.is_idle() || alpha <= 0.0 {
        return Vec::new();
    }
    let mut packets = Vec::new();
    for border in [true, false] {
        for glyph in &layout.glyphs {
            let mut p = vec![if border { 4 } else { 3 }, glyph.point as u32];
            p.extend(matrix(layout.size).to_cols_array().map(f32::to_bits));
            let color = if border {
                vec4(0.0, 0.0, 0.0, alpha)
            } else {
                vec4(1.0, 1.0, 1.0, alpha)
            };
            if border {
                p.extend(
                    glyph
                        .border
                        .iter()
                        .flat_map(|v| v.to_array())
                        .map(f32::to_bits),
                );
            } else {
                p.extend(color.to_array().map(f32::to_bits));
                p.extend([0; 12]);
            }
            p.extend(color.to_array().map(f32::to_bits));
            p.extend([0; 12]);
            for vertex in &glyph.vertices {
                let position = vertex.position;
                p.extend(position.to_array().map(f32::to_bits));
                p.push(vertex.color.to_bits());
            }
            packets.push(p);
        }
    }
    packets
}
fn native_packets(line: &str) -> Vec<Vec<u32>> {
    let mut words = line.split_whitespace();
    assert_eq!(words.next(), Some("R"));
    let mut packets = Vec::new();
    loop {
        match words.next() {
            Some("P") => packets.push(
                (0..70)
                    .map(|_| words.next().unwrap().parse().unwrap())
                    .collect(),
            ),
            Some("E") => {
                assert_eq!(
                    words.next().unwrap().parse::<usize>().unwrap(),
                    packets.len()
                );
                break;
            }
            x => panic!("notification packet{x:?}"),
        }
    }
    packets
}
#[test]
#[ignore = "original full notification ctor/text/update/stop/reset/render and exact SimpleText glyph packets"]
fn higurashi_autosave_notification_matches_original_state_and_glyph_packets() {
    let font = Arc::new(
        read_font_metrics(&mut BufReader::new(
            std::fs::File::open("/tmp/higurashi-newrodin.fnt").unwrap(),
        ))
        .unwrap(),
    );
    let long = "長い章タイトル ".repeat(40);
    let mut input = String::new();
    let mut states = Vec::new();
    let mut draws = Vec::new();
    let mut owners = 0;
    for text in [
        "",
        "Autosave Text",
        "鬼隠し編 第１章",
        "@r本文\n二行 @c900",
        "半角 ｱｲｳｴｵ && 日本語",
        long.as_str(),
    ] {
        for hidden in [false, true] {
            owners += 1;
            input.push_str("N\n");
            let mut clock = Indicator::default();
            states.push(snapshot(&clock, None));
            input.push_str(&format!("H {}\n", hidden as u32));
            clock.set_hidden(hidden as u32);
            states.push(snapshot(&clock, None));
            let l = layout(font.clone(), text);
            input.push_str(&format!("T {}\n", hex(text)));
            show(&mut clock);
            states.push(snapshot(&clock, Some(&l)));
            for d in [0.0, 0.125, 0.5, 1.0, 3.0, 17.0, 34.0, 239.0, 240.0, 1000.0] {
                input.push_str(&format!("U {d}\n"));
                clock.advance(Ticks::from_f32(d));
                states.push(snapshot(&clock, Some(&l)));
                for enabled in [false, true] {
                    input.push_str(&format!("R {}\n", enabled as u32));
                    draws.push(packets(&clock, &l, enabled));
                }
            }
            input.push_str("H 0\n");
            clock.set_hidden(0);
            states.push(snapshot(&clock, Some(&l)));
            input.push_str(&format!("T {}\nU 17\nS\n", hex(text)));
            show(&mut clock);
            states.push(snapshot(&clock, Some(&l)));
            clock.advance(Ticks::from_f32(17.0));
            states.push(snapshot(&clock, Some(&l)));
            if clock.transition.is_waiting() {
                clock
                    .transition
                    .control(0.0, Ticks::from_f32(f32::from_bits(0x3cf5c28f)), 0x140)
                    .unwrap();
            }
            states.push(snapshot(&clock, Some(&l)));
            for d in [1.0, 3.0, 30.0] {
                input.push_str(&format!("U {d}\nR 1\n"));
                clock.advance(Ticks::from_f32(d));
                states.push(snapshot(&clock, Some(&l)));
                draws.push(packets(&clock, &l, true));
            }
            input.push_str("Z\nD\n");
            clock.reset();
            states.push(snapshot(&clock, Some(&l)));
        }
    }
    let output = native(input);
    let actual = output
        .lines()
        .filter_map(|l| l.strip_prefix("T "))
        .map(|l| {
            l.split_whitespace()
                .map(|v| v.parse().unwrap())
                .collect::<Vec<u32>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(actual.len(), states.len());
    for (i, (a, e)) in actual.iter().zip(&states).enumerate() {
        assert_eq!(a, e, "notification state{i}");
    }
    let actual = output
        .lines()
        .filter(|l| l.starts_with("R "))
        .map(native_packets)
        .collect::<Vec<_>>();
    assert_eq!(actual.len(), draws.len());
    let count: usize = draws.iter().map(Vec::len).sum();
    for (i, (a, e)) in actual.iter().zip(&draws).enumerate() {
        assert_eq!(a, e, "notification draw{i}");
    }
    eprintln!(
        "Autosave notification:{owners} owners/{} exact states/{} frames/{count} full glyph packets;native entrance/240 hold/fade/hidden pause/stop/reset,setting gate,width1916/48px/anchor(0,1)/position(4,1076),text vertices/matrices/colors and retirement pass",
        states.len(),
        draws.len()
    );
}
#[test]
#[ignore = "original scenario-notification glyph packets against actual newrodin GPU rendering at720p/1080p"]
fn higurashi_autosave_notification_pixels_match_original_packets() {
    use crate::adv::higurashi::{
        reference_gpu_assets as gpu_assets, reference_render_to_texture as render_to_texture,
    };
    use shin_core::primitives::color::UnormColor;
    use shin_render::{
        DrawPrimitive, RenderProgramWithArguments,
        shaders::types::{buffer::VertexSource, vertices::TextVertex},
    };
    let (device, queue, server) = gpu_assets();
    let font: Arc<GpuFontLazy> = server.load_sync("/newrodin.fnt").unwrap();
    let mut pairs = 0;
    for (text, delta, hidden) in [
        ("鬼隠し編 第１章", 17.0, false),
        ("Autosave@rSecond line", 40.0, false),
        ("半角 ｱｲｳ 日本語", 7.0, true),
    ] {
        let mut notice = Notification::default();
        let mut clock = Indicator::default();
        notice
            .prepare(&device, &queue, font.clone(), text, &mut clock)
            .unwrap();
        clock.hidden = hidden;
        clock.advance(Ticks::from_f32(delta));
        let output = native(format!(
            "N\nT {}\nH {}\nU {delta}\nR 1\nD\n",
            hex(text),
            hidden as u32
        ));
        let raw = native_packets(output.lines().find(|l| l.starts_with("R ")).unwrap());
        let points = raw
            .iter()
            .map(|p| char::from_u32(p[1]).unwrap())
            .collect::<Vec<_>>();
        let handles = font
            .clone()
            .load_glyphs(device.clone(), queue.clone(), &points);
        for size in [(1280, 720), (1920, 1080)] {
            let actual = render_to_texture(&device, &queue, size, |context, target| {
                let mut pass = context.begin_pass(
                    shin_render::shaders::types::texture::TextureTarget {
                        view: target,
                        kind:
                            shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
                    },
                    Some(context.depth_stencil),
                    "Notification owner",
                );
                pass.clear(Some(UnormColor::WHITE), Some(0), Some(1.0));
                notice.render(&mut pass, &clock, true);
            });
            let expected = render_to_texture(&device, &queue, size, |context, target| {
                let mut pass = context.begin_pass(
                    shin_render::shaders::types::texture::TextureTarget {
                        view: target,
                        kind:
                            shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
                    },
                    Some(context.depth_stencil),
                    "Original notification packets",
                );
                pass.clear(Some(UnormColor::WHITE), Some(0), Some(1.0));
                for (p, handle) in raw.iter().zip(&handles) {
                    let floats =
                        |v: &[u32]| v.iter().map(|b| f32::from_bits(*b)).collect::<Vec<_>>();
                    let mut m: [f32; 16] = floats(&p[2..18]).try_into().unwrap();
                    for i in 0..4 {
                        m[4 * i + 2] = 0.5 * m[4 * i + 2] + 0.5 * m[4 * i + 3];
                    }
                    let transform = Mat4::from_cols_array(&m);
                    let vertices = p[50..70]
                        .chunks_exact(5)
                        .map(|v| TextVertex {
                            position: glam::Vec4::from_array(floats(&v[..4]).try_into().unwrap()),
                            color: f32::from_bits(v[4]),
                        })
                        .collect::<Vec<_>>();
                    let col = |v: &[u32]| {
                        let v = floats(v);
                        FloatColor4::from_rgba(v[0], v[1], v[2], v[3])
                    };
                    let glyph = handle.as_texture_source().unwrap();
                    let program = if p[0] == 4 {
                        RenderProgramWithArguments::FontBorder {
                            vertices: VertexSource::VertexData {
                                vertices: &vertices,
                            },
                            glyph,
                            transform,
                            distances: std::array::from_fn(|i| {
                                glam::vec2(
                                    f32::from_bits(p[18 + 2 * i]),
                                    f32::from_bits(p[19 + 2 * i]),
                                )
                            }),
                            color: col(&p[34..38]),
                        }
                    } else {
                        RenderProgramWithArguments::Font {
                            vertices: VertexSource::VertexData {
                                vertices: &vertices,
                            },
                            glyph,
                            transform,
                            color1: col(&p[18..22]),
                            color2: col(&p[34..38]),
                        }
                    };
                    pass.run(
                        RenderRequestBuilder::new()
                            .color_blend_type(ColorBlendType::Layer1)
                            .build(program, DrawPrimitive::TrianglesStrip),
                    );
                }
            });
            assert!(
                actual == expected,
                "notification/native pixels differ{text:?} at{size:?}"
            );
            pairs += 1;
        }
    }
    eprintln!(
        "Autosave notification:{pairs} byte-identical native-packet/GPU pairs at720p/1080p;Japanese/half-width/multiline,fractional entrance/full/hidden alpha pass"
    );
}
