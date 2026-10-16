//! TipsMode1070a0/108028: authored headline/body layout and clipped drawing.
use std::sync::Arc;

use glam::{Mat4, Vec4, vec2, vec4};
use shin_core::{
    format::scenario::info::TipsInfoItem, layout::font::FontMetrics, primitives::color::UnormColor,
};
use shin_render::{ColorBlendType, quad_vertices::QuadVertices, render_pass::RenderPass};

use super::{
    character_text::{CharacterText, CharacterTextLayout},
    character_view::CharacterTextures,
};
use crate::asset::{font::GpuFontLazy, system::AssetServer};

pub const TIPS_TEXT_VIEWPORT: Vec4 = vec4(602.0, 188.0, 1232.0, 730.0);
/// Native107f6c..107fc8 clips both columns plus five-pixel vertical padding.
/// The left list is drawn before this stencil; it clips the divider and text.
pub const TIPS_TEXT_CLIP: Vec4 = vec4(86.0, 183.0, 1748.0, 740.0);

pub fn layout_tip(item: &TipsInfoItem, font: &impl FontMetrics) -> CharacterTextLayout {
    CharacterTextLayout::with_content_width(
        item.title.as_str(),
        item.content.as_str(),
        font,
        1216.0,
    )
}

pub struct TipsText {
    pub text: CharacterText,
}
impl TipsText {
    pub fn load(server: &AssetServer, item: &TipsInfoItem) -> Self {
        let font: Arc<GpuFontLazy> = server.load_sync("/system.fnt").unwrap();
        crate::text_stream::description(item.title.as_str(), item.content.as_str());
        let layout = layout_tip(item, font.as_ref());
        Self {
            text: CharacterText::from_layout(server, font, layout),
        }
    }
    /// Native bar covers the full viewport width and sits20px above the
    /// headline extent. Divider precedes the two text objects in draw order.
    pub fn divider(&self, scroll: f32) -> Vec4 {
        vec4(
            602.0,
            188.0 + self.text.layout.name_height - 20.0 - scroll,
            1232.0,
            16.0,
        )
    }
    pub fn render(
        &self,
        pass: &mut RenderPass,
        atlas: &CharacterTextures,
        scroll: f32,
        transform: Mat4,
    ) {
        let (clip, builder) = CharacterText::begin_clip(pass, transform, TIPS_TEXT_CLIP);
        let r = self.divider(scroll);
        QuadVertices::new()
            .with_box(r.x, r.y, r.x + r.z, r.y + r.w)
            .with_tex_box(0.0, 0.0, 1.0, 1.0)
            .with_color(UnormColor::WHITE)
            .render_sprite(
                pass,
                builder.color_blend_type(ColorBlendType::Opaque),
                atlas.get("bar").as_source(),
                transform,
            );
        clip.end(pass);
        self.text
            .render_clipped(pass, scroll, transform, vec2(602.0, 188.0), TIPS_TEXT_CLIP);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shin_core::{
        format::{font::GlyphInfo, text::U16String},
        layout::font::FontMetrics,
    };
    struct Metrics;
    impl FontMetrics for Metrics {
        fn get_ascent(&self) -> u32 {
            60
        }
        fn get_descent(&self) -> u32 {
            20
        }
        fn get_glyph_info(&self, _: char) -> Option<GlyphInfo> {
            Some(GlyphInfo {
                bearing_x: 1,
                bearing_y: 60,
                advance_width: 40,
                actual_width: 38,
                actual_height: 76,
                texture_width: 64,
                texture_height: 128,
            })
        }
    }
    #[test]
    fn tips_wrap_at_wider_width_and_keep_native_outline_and_offsets() {
        let item = TipsInfoItem {
            episode: 0,
            title_index: 0,
            title: U16String::new("名".repeat(100)),
            content: U16String::new("説".repeat(400)),
        };
        let layout = layout_tip(&item, &Metrics);
        assert_eq!(layout.name_glyph_count, 100);
        assert_eq!(layout.name_height, 59.0);
        assert_eq!(
            layout.glyphs[0].rect,
            vec4(5.304, 5.0, 17.552002, 51.600002)
        );
        assert!(layout.glyphs[..100].iter().all(|g| g.rect.y == 5.0)); // Long title squishes onto one line.
        assert!(
            layout.glyphs[100..]
                .iter()
                .all(|g| g.rect.x + g.rect.z <= 1232.0)
        );
        let narrow = CharacterTextLayout::new(item.title.as_str(), item.content.as_str(), &Metrics);
        assert!(layout.height < narrow.height);
        assert!(layout.max_scroll() == 0.0);
        let long = TipsInfoItem {
            content: U16String::new("説".repeat(2000)),
            ..item
        };
        let layout = layout_tip(&long, &Metrics);
        assert!(layout.max_scroll() > 0.0);
        assert_eq!(layout.clamp_scroll(f32::MAX), layout.height - 730.0);
        assert!((layout.glyphs[100].rect.y - 74.8).abs() < 1e-5);
    }
    #[test]
    #[ignore = "requires supplied ROM/executable (SHIN_REFERENCE_ROM)"]
    fn reference_native_tips_text_styles_clip_and_all_authored_glyphs() {
        use shin_core::{
            format::{rom::RomReader, scenario::Scenario},
            primitives::stateless_reader::StatelessFile,
        };
        use std::io::{Cursor, Read};
        let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&path)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, value) in [
            (0x185cd0, 3.0_f32),
            (0x15993c, 1216.0),
            (0x159a6c, 47.0 / 255.0),
            (0x1593e0, 25.0 / 255.0),
            (0x159478, 20.0 / 255.0),
            (0x159374, 602.0),
            (0x159384, 188.0),
            (0x1593d4, 1834.0),
        ] {
            assert_eq!(
                f32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                value
            );
        }
        for (address, expected) in [
            (0x1070f0, 0x52a84809),
            (0x1070f4, 0x1e282900),
            (0x1070f8, 0x1e21280e),
            (0x1072a4, 0xb900a7e8),
            (0x1072a8, 0x52802028),
            (0x1072ac, 0x1e212809),
            (0x100af0, 0x1e211000),
            (0x100b00, 0x7100051f),
            (0x100b04, 0x1e201d22),
            (0x100b34, 0x1e220d2d),
            (0x100b44, 0x1e2d2808),
            (0x100b48, 0x1e283820),
            (0x107f6c, 0xd2a85588),
            (0x107f70, 0xf2e866e8),
            (0x107f78, 0xd2880008),
            (0x107f7c, 0xf2a89ca8),
            (0x107f80, 0xf2d80008),
            (0x107f84, 0xf2e88cc8),
            (0x107fa0, 0xd2880008),
            (0x107fa4, 0xf2a89ca8),
            (0x107fa8, 0xf2e866e8),
            (0x108028, 0xbd430260),
            (0x10802c, 0x1e361001),
            (0x108030, 0x1e212800),
            (0x108034, 0x1e321001),
            (0x108038, 0x1e212800),
            (0x108050, 0x1e261000),
            (0x1080c0, 0x1e214181),
            (0x1080c4, 0x4eab1d60),
            (0x1080ec, 0x97ffe439),
            (0x1080f8, 0x97ffe436),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native{address:x}"
            );
        }
        let mut rom =
            RomReader::new(StatelessFile::new(std::fs::File::open(path).unwrap()).unwrap())
                .unwrap();
        let mut read = |path: &str| {
            let entry = rom.find_file(path).unwrap();
            let mut bytes = Vec::new();
            rom.open_file(entry)
                .unwrap()
                .read_to_end(&mut bytes)
                .unwrap();
            bytes
        };
        let scenario = Scenario::new(read("/main.snr").into()).unwrap();
        let font =
            shin_core::format::font::read_lazy_font(&mut Cursor::new(read("/system.fnt"))).unwrap();
        let mut glyphs = 0;
        let mut scrolling = 0;
        for (index, item) in scenario.info_tables().tips_info.iter().enumerate() {
            let layout = layout_tip(item, &font);
            assert!(layout.name_glyph_count > 0, "tip{index}");
            assert!(layout.height.is_finite() && layout.height > layout.name_height);
            for glyph in &layout.glyphs {
                assert!(glyph.rect.to_array().into_iter().all(f32::is_finite));
                assert!(glyph.uv.to_array().into_iter().all(f32::is_finite));
                assert!(glyph.rect.z > 6.0 && glyph.rect.w > 6.0);
            }
            for glyph in &layout.glyphs[layout.name_glyph_count..] {
                let info = font.get_glyph_info(glyph.codepoint).unwrap();
                let horizontal_scale = (glyph.rect.z - 6.0) / info.actual_width as f32;
                let logical_end = glyph.rect.x - 5.0 - info.bearing_x as f32 * horizontal_scale
                    + info.advance_width as f32 * horizontal_scale;
                assert!(
                    logical_end <= 1216.01,
                    "tip{index} {:?} logical end{logical_end}",
                    glyph.codepoint
                );
            }
            glyphs += layout.glyphs.len();
            scrolling += usize::from(layout.max_scroll() > 0.0);
        }
        assert!(glyphs > 1000);
        assert!(scrolling > 0);
        eprintln!("Tips text: {glyphs} drawable glyphs, {scrolling} scrolling entries");
        assert_eq!(TIPS_TEXT_CLIP, vec4(86.0, 183.0, 1748.0, 740.0));
    }
}

#[cfg(test)]
pub(crate) fn assert_reference_tips_text(
    context: &mut crate::render::PreRenderContext,
    server: &Arc<AssetServer>,
    scenario: &shin_core::format::scenario::Scenario,
) {
    use shin_core::layout::{
        LayoutParams, MessageTextLayouter, MessageTextLayouterDefaults, commands::Command,
    };
    use shin_core::primitives::color::FloatColor4;
    use shin_core::{
        format::{
            font::{GlyphMipLevel, GlyphTrait},
            rom::RomReader,
        },
        primitives::stateless_reader::StatelessFile,
    };
    use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
    use std::{
        collections::HashMap,
        fs::File,
        io::{Cursor, Read},
    };
    let mut rom = RomReader::new(
        StatelessFile::new(File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap())
            .unwrap(),
    )
    .unwrap();
    let mut read = |path: &str| {
        let entry = rom.find_file(path).unwrap();
        let mut bytes = Vec::new();
        rom.open_file(entry)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        bytes
    };
    let font =
        shin_core::format::font::read_lazy_font(&mut Cursor::new(read("/system.fnt"))).unwrap();
    let original =
        shin_core::format::texture_archive::read_texture_archive(&read("/tips.txa")).unwrap();
    let atlas: Arc<CharacterTextures> = server.load_sync("/tips.txa").unwrap();
    let items = &scenario.info_tables().tips_info;
    let first = &items[0];
    let longest = items
        .iter()
        .max_by_key(|item| (layout_tip(item, &font).height * 100.0) as i32)
        .unwrap();
    let output = context.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Native Tips outlined text"),
        size: wgpu::Extent3d {
            width: 1920,
            height: 1080,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = output.create_view(&Default::default());
    for (state, at_bottom) in [
        (first, false),
        (longest, false),
        (longest, true),
        (&items[1], false),
    ] {
        let text = TipsText::load(server, state);
        let scroll = if at_bottom {
            text.text.layout.max_scroll()
        } else {
            0.0
        };
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    view: &view,
                    kind: TextureTargetKind::Screen,
                },
                Some(context.depth_stencil),
                "Native character text",
            );
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::from_rgba(
                    32, 64, 96, 255,
                )),
                Some(128),
                None,
            );
            text.render(
                &mut pass,
                &atlas,
                scroll,
                crate::render::top_left_projection_matrix(),
            );
        }
        let actual = crate::layer::user::TileLayer::readback_reference_texture(context, &output);
        let mut expected = vec![[32.0 / 255.0, 64.0 / 255.0, 96.0 / 255.0, 1.0]; 1920 * 1080];
        // Expected geometry comes from the core parser and independently entered native
        // constants, not TipsText layout's glyph quads or border-distance arrays.
        let parse = |message: &str, size: f32, body: bool| {
            MessageTextLayouter::new(
                &font,
                &font,
                LayoutParams {
                    layout_width: 1216.0,
                    text_size: size,
                    rubi_size: size * 0.4,
                    perform_soft_breaks: body,
                    always_leave_space_for_rubi: body,
                    ..Default::default()
                },
                MessageTextLayouterDefaults {
                    color: 999,
                    draw_speed: 50,
                    fade: 200,
                },
            )
            .parse(message)
        };
        let (name, _, name_size) = parse(state.title.as_str(), 48.0, false);
        let name_height = name_size.y + 11.0;
        let (body, _, _) = parse(state.content.as_str(), 32.0, true);
        let bar = &original.textures[original.name_to_index["bar"]];
        let sample_gray =
            |glyph: &shin_core::format::font::Glyph, level: GlyphMipLevel, u: f32, v: f32| {
                let image = glyph.get_image(level);
                let sx = (u * image.width() as f32 - 0.5).clamp(0.0, image.width() as f32 - 1.0);
                let sy = (v * image.height() as f32 - 0.5).clamp(0.0, image.height() as f32 - 1.0);
                let x = sx.floor() as u32;
                let y = sy.floor() as u32;
                let dx = sx.fract();
                let dy = sy.fract();
                let a = image.get_pixel(x, y).0[0] as f32;
                let b = image.get_pixel((x + 1).min(image.width() - 1), y).0[0] as f32;
                let c = image.get_pixel(x, (y + 1).min(image.height() - 1)).0[0] as f32;
                let d = image
                    .get_pixel(
                        (x + 1).min(image.width() - 1),
                        (y + 1).min(image.height() - 1),
                    )
                    .0[0] as f32;
                ((a * (1.0 - dx) + b * dx) * (1.0 - dy) + (c * (1.0 - dx) + d * dx) * dy) / 255.0
            };
        let bar_y = 188.0 + name_height - 20.0 - scroll;
        for y in 183..923usize {
            if (y as f32 + 0.5) < bar_y || (y as f32 + 0.5) >= bar_y + 16.0 {
                continue;
            }
            for x in 602..1834usize {
                let sx = ((x as f32 + 0.5 - 602.0) / 1232.0 * bar.width() as f32 - 0.5)
                    .clamp(0.0, bar.width() as f32 - 1.0);
                let sy = ((y as f32 + 0.5 - bar_y) / 16.0 * bar.height() as f32 - 0.5)
                    .clamp(0.0, bar.height() as f32 - 1.0);
                let ix = sx.floor() as u32;
                let iy = sy.floor() as u32;
                let a = bar.get_pixel(ix, iy).0;
                let b = bar.get_pixel((ix + 1).min(bar.width() - 1), iy).0;
                let c = bar.get_pixel(ix, (iy + 1).min(bar.height() - 1)).0;
                let d = bar
                    .get_pixel(
                        (ix + 1).min(bar.width() - 1),
                        (iy + 1).min(bar.height() - 1),
                    )
                    .0;
                expected[y * 1920 + x] = std::array::from_fn(|i| {
                    let value = (a[i] as f32 * (1.0 - sx.fract()) + b[i] as f32 * sx.fract())
                        * (1.0 - sy.fract())
                        + (c[i] as f32 * (1.0 - sx.fract()) + d[i] as f32 * sx.fract())
                            * sy.fract();
                    value.round() / 255.0
                });
            }
        }
        let mut decompressed = HashMap::new();
        for (commands, offset) in [(&name, 5.0f32), (&body, name_height + 3.0)] {
            for border in [true, false] {
                for command in commands {
                    let Command::Char(character) = command else {
                        continue;
                    };
                    let info = font.get_glyph_info(character.codepoint).unwrap();
                    if info.actual_width == 0 || info.actual_height == 0 {
                        continue;
                    }
                    let glyph = decompressed.entry(character.codepoint).or_insert_with(|| {
                        font.get_glyph_for_character(character.codepoint as u16)
                            .decompress()
                    });
                    let lod = (1.0 / character.scale.min(character.horizontal_scale))
                        .log2()
                        .clamp(0.0, 3.0);
                    let levels = [
                        GlyphMipLevel::Level0,
                        GlyphMipLevel::Level1,
                        GlyphMipLevel::Level2,
                        GlyphMipLevel::Level3,
                    ];
                    let low = levels[lod.floor() as usize];
                    let high = levels[lod.ceil() as usize];
                    let sample = |u, v| {
                        sample_gray(glyph, low, u, v) * (1.0 - lod.fract())
                            + sample_gray(glyph, high, u, v) * lod.fract()
                    };
                    let left = 602.0
                        + 5.0
                        + character.position.x
                        + info.bearing_x as f32 * character.horizontal_scale;
                    let top = 188.0 - scroll + offset + character.position.y
                        - info.bearing_y as f32 * character.scale;
                    let width = info.actual_width as f32 * character.horizontal_scale + 6.0;
                    let height = info.actual_height as f32 * character.scale + 6.0;
                    let dx = 3.0 / (character.horizontal_scale * info.texture_width as f32);
                    let dy = 3.0 / (character.scale * info.texture_height as f32);
                    let tint = FloatColor4::from_unorm(character.color);
                    let rgb = if border {
                        [47.0 / 255.0, 25.0 / 255.0, 20.0 / 255.0]
                    } else {
                        [tint.b, tint.g, tint.r]
                    };
                    for y in ((top - 0.5).ceil().max(183.0) as usize)
                        ..((top + height - 0.5).ceil().clamp(183.0, 923.0) as usize)
                    {
                        for x in ((left - 0.5).ceil().max(86.0) as usize)
                            ..((left + width - 0.5).ceil().clamp(86.0, 1834.0) as usize)
                        {
                            let u = (x as f32 + 0.5 - left - 3.0)
                                / (character.horizontal_scale * info.texture_width as f32);
                            let v = (y as f32 + 0.5 - top - 3.0)
                                / (character.scale * info.texture_height as f32);
                            let alpha = if border {
                                let mut maximum = 0.0f32;
                                for (a, b) in [
                                    (-0.707, -0.707),
                                    (0.0, -1.0),
                                    (0.707, -0.707),
                                    (-1.0, 0.0),
                                    (1.0, 0.0),
                                    (-0.707, 0.707),
                                    (0.0, 1.0),
                                    (0.707, 0.707),
                                ] {
                                    maximum = maximum.max(sample(u + a * dx, v + b * dy));
                                }
                                (1.5 * maximum).min(1.0)
                            } else {
                                sample(u, v) * tint.a
                            };
                            let dst = &mut expected[y * 1920 + x];
                            for i in 0..3 {
                                dst[i] = ((rgb[i] * alpha + dst[i] * (1.0 - alpha)) * 255.0)
                                    .round()
                                    / 255.0;
                            }
                            dst[3] = ((alpha * (1.0 - dst[3]) + dst[3]) * 255.0).round() / 255.0;
                        }
                    }
                }
            }
        }
        let mut worst = 0.0f32;
        for (index, (pixel, wanted)) in actual.chunks_exact(4).zip(&expected).enumerate() {
            for channel in 0..4 {
                let error = (pixel[channel] as f32 - wanted[channel] * 255.0).abs();
                worst = worst.max(error);
                assert!(
                    error <= 4.1,
                    "Tips text {},scroll{scroll}: pixel{},{} channel{channel}: {pixel:?} vs {wanted:?}",
                    state.title.as_str(),
                    index % 1920,
                    index / 1920
                );
            }
        }
        eprintln!(
            "Tips text {}, scroll{scroll}: {} glyphs, worst channel error{worst}",
            state.title.as_str(),
            text.text.layout.glyphs.len()
        );
        if at_bottom {
            let mut ppm = b"P6\n1920 1080\n255\n".to_vec();
            for pixel in actual.chunks_exact(4) {
                ppm.extend_from_slice(&pixel[..3]);
            }
            std::fs::write("/tmp/shin-tips-text-reference.ppm", ppm).unwrap();
        }
    }
}
