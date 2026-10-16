use super::*;
use crate::adv::higurashi::{
    reference_gpu_assets as gpu_assets, reference_render_to_texture as render_to_texture,
};
use shin_core::primitives::color::{FloatColor4, UnormColor};
use shin_render::{
    DrawPrimitive, RenderProgramWithArguments,
    shaders::types::{buffer::VertexSource, vertices::TextVertex},
};
#[test]
#[ignore = "original music-title glyph packet replay vs production overlay at720p1080p, offscreen Metal/no audio/window/saves"]
fn higurashi_music_title_pixels_match_original_packets() {
    let (device, queue, server) = gpu_assets();
    let font: Arc<GpuFontLazy> = server.load_sync("/newrodin.fnt").unwrap();
    let mut pairs = 0;
    for (text, delta, hidden) in [
        ("日常", 20.0, false),
        ("Matsuribayashi", 60.0, false),
        ("見えない何か", 5.0, true),
    ] {
        let mut title = MusicTitle::default();
        let mut clock = MenuIndicator::default();
        title
            .show(&device, &queue, font.clone(), text, &mut clock)
            .unwrap();
        if hidden {
            clock.advance(Ticks::from_f32(20.0));
        }
        clock.indicator.hidden = hidden;
        clock.advance(Ticks::from_f32(delta));
        let input = format!(
            "N\nT {}\n{}H {}\nU {delta}\nR 1\nD\n",
            hex(text),
            if hidden { "U 20\n" } else { "" },
            hidden as u8
        );
        let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_BGM_TITLE").unwrap())
            .args([
                "higurashi/uncompressed_executable_update",
                &std::env::var("SHIN_HIGURASHI_TITLE_FONT").unwrap(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut pipe = child.stdin.take().unwrap();
        let writer = std::thread::spawn(move || pipe.write_all(input.as_bytes()).unwrap());
        let out = child.wait_with_output().unwrap();
        writer.join().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let output = String::from_utf8(out.stdout).unwrap();
        let raw = native_packets(output.lines().find(|l| l.starts_with("R ")).unwrap());
        assert!(!raw.is_empty());
        let points = raw
            .iter()
            .map(|p| char::from_u32(p[1]).unwrap())
            .collect::<Vec<_>>();
        let handles = font
            .clone()
            .load_glyphs(device.clone(), queue.clone(), &points);
        for size in [(1280, 720), (1920, 1080)] {
            let actual = render_to_texture(&device, &queue, size, |ctx, target| {
                let mut pass = ctx.begin_pass(
                    shin_render::shaders::types::texture::TextureTarget {
                        view: target,
                        kind:
                            shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
                    },
                    Some(ctx.depth_stencil),
                    "Music title owner",
                );
                pass.clear(Some(UnormColor::WHITE), Some(0), Some(1.0));
                title.render(&mut pass, &clock, true);
            });
            let expected = render_to_texture(&device, &queue, size, |ctx, target| {
                let mut pass = ctx.begin_pass(
                    shin_render::shaders::types::texture::TextureTarget {
                        view: target,
                        kind:
                            shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
                    },
                    Some(ctx.depth_stencil),
                    "Original music title packets",
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
                "Music title/native pixels differ{text:?} at{size:?}"
            );
            assert!(actual.chunks_exact(4).any(|p| p != [255, 255, 255, 255]));
            pairs += 1;
        }
    }
    println!(
        "Music title:{pairs} byte-identical original-packet/GPU pairs at720p/1080p;Japanese,entrance,squash,partial hidden alpha,cyan tint"
    );
}
