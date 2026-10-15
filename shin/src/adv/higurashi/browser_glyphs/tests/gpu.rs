use super::*;
use crate::adv::higurashi::message_glyphs::{
    HigurashiMessageGlyphs,
    tests::gpu::{gpu_assets, render_frame},
};

fn replay(pass: &mut RenderPass, packet: &Packet, glyphs: &BrowserGlyphs) {
    // Each native case renders twice before culling. Replay one retained frame.
    for draw in &packet.draws[..packet.draws.len() / 2] {
        let texture = glyphs.glyphs[draw.glyph - 1]
            .handle
            .as_texture_source()
            .unwrap();
        let vertices: [TextVertex; 4] = std::array::from_fn(|i| TextVertex {
            position: glam::Vec4::from_array(std::array::from_fn(|j| {
                f32::from_bits(draw.vertices[5 * i + j])
            })),
            color: f32::from_bits(draw.vertices[5 * i + 4]),
        });
        let native = Mat4::from_cols_array(&draw.matrix.map(f32::from_bits));
        let conversion = Mat4::from_cols_array(&[
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.5, 1.0,
        ]);
        let transform = conversion * native;
        let color = |v: &[u32]| {
            FloatColor4::from_rgba(
                f32::from_bits(v[0]),
                f32::from_bits(v[1]),
                f32::from_bits(v[2]),
                f32::from_bits(v[3]),
            )
        };
        let vertices = VertexSource::VertexData {
            vertices: &vertices,
        };
        let program = if draw.program == 4 {
            RenderProgramWithArguments::FontBorder {
                vertices,
                glyph: texture,
                transform,
                distances: std::array::from_fn(|i| {
                    vec2(
                        f32::from_bits(draw.extra[2 * i]),
                        f32::from_bits(draw.extra[2 * i + 1]),
                    )
                }),
                color: color(&draw.color),
            }
        } else {
            RenderProgramWithArguments::Font {
                vertices,
                glyph: texture,
                transform,
                color1: color(&draw.extra),
                color2: color(&draw.color),
            }
        };
        pass.run(
            RenderRequestBuilder::new()
                .color_blend_type(ColorBlendType::Layer1)
                .build(program, DrawPrimitive::TrianglesStrip),
        );
    }
}

#[test]
#[ignore = "loads actual Higurashi font and compares retained browser glyph packets on offscreen Metal"]
fn higurashi_browser_glyph_cache_renders_native_frames_at_720p_and_1080p() {
    let (device, queue, assets) = gpu_assets();
    let font = shin_tasks::block_on(HigurashiMessageGlyphs::load_font(&assets)).unwrap();
    let captures =
        std::path::PathBuf::from(std::env::var("SHIN_HIGURASHI_BROWSER_CAPTURES").unwrap());
    std::fs::create_dir_all(&captures).unwrap();
    let rows = [
        make_row(0, 0, 9, "名前@r@bかな.@<漢字@>、続く。"),
        make_row(3, 0, 0, "@r@r@z10.小@z200.大@z.戻"),
        make_row(0, 1, 0, "@c$F00.赤@c.戻@r選ぶ"),
        make_row(0, 2, 0, "本文 タイトル"),
        make_row(0, 3, 2, ""),
        make_row(0, 0, 9, "@r@o50.@va.声@x20.次@/後@y同期@|節@v."),
        make_row(0x13, 0, 9, "@r"),
    ];
    let mut frames = 0;
    for (case, row) in rows.into_iter().enumerate() {
        let outline = (case % 3) as u8;
        let measured = PositionedRow::measure(row.clone(), 0.0, font.clone()).unwrap();
        let rows = [measured];
        let mut cache = BrowserGlyphCache::default();
        cache
            .synchronize(&device, &queue, font.clone(), &rows, 0.0, outline)
            .unwrap();
        assert_eq!(cache.len(), 1);
        let address = cache.get(0).unwrap() as *const _;
        cache
            .synchronize(&device, &queue, font.clone(), &rows, 0.0, (outline + 1) % 3)
            .unwrap();
        assert_eq!(cache.get(0).unwrap() as *const _, address, "retained entry");
        let input: [String; 3] =
            [1.0, 0.5, 0.0].map(|slide| super::input(&row, outline, 0.0, 0.0, slide));
        let packets = native(&input.concat());
        for (slide, p) in [1.0, 0.5, 0.0].into_iter().zip(packets) {
            for size in [(1280, 720), (1920, 1080)] {
                let actual = render_frame(&device, &queue, size, |pass| {
                    cache.render(pass, &rows, 0.0, slide)
                });
                let expected = render_frame(&device, &queue, size, |pass| {
                    replay(pass, &p, cache.get(0).unwrap())
                });
                assert_eq!(actual, expected, "browser case{case} slide{slide} {size:?}");
                if slide == 1.0 {
                    std::fs::write(
                        captures.join(format!("case{case}-{}x{}.rgba", size.0, size.1)),
                        &actual,
                    )
                    .unwrap();
                }
                frames += 1;
            }
        }
        cache
            .synchronize(&device, &queue, font.clone(), &rows, 100000.0, outline)
            .unwrap();
        assert!(cache.is_empty());
        cache
            .synchronize(&device, &queue, font.clone(), &rows, 0.0, (outline + 1) % 3)
            .unwrap();
        assert_eq!(cache.len(), 1);
        let regenerated = cache.get(0).unwrap();
        let reference = native(&super::input(&row, (outline + 1) % 3, 0.0, 0.0, 1.0)).remove(0);
        for size in [(1280, 720), (1920, 1080)] {
            let actual = render_frame(&device, &queue, size, |pass| {
                cache.render(pass, &rows, 0.0, 1.0)
            });
            let expected = render_frame(&device, &queue, size, |pass| {
                replay(pass, &reference, regenerated)
            });
            assert_eq!(
                actual, expected,
                "browser case{case} regenerated outline {size:?}"
            );
            frames += 1;
        }
    }
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    assert_eq!(frames, 56);
    println!(
        "Higurashi browser GPU:56 whole RGBA frames match native glyph packet replay at720p/1080p; retained cache, cull/rebuild, seven row profiles and horizontal slide pass"
    );
}
