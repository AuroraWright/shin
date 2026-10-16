//! Native Characters name/description layout and outlined text drawing.
use std::sync::Arc;

use glam::{Mat4, Vec2, Vec4, vec2, vec4};
use shin_core::{
    layout::{
        LayoutParams, MessageTextLayouter, MessageTextLayouterDefaults, commands::Command,
        font::FontMetrics,
    },
    primitives::color::FloatColor4,
};
use shin_render::{
    ColorBlendType, DepthStencilState, DrawPrimitive, RenderProgramWithArguments,
    RenderRequestBuilder, StencilFunction, StencilMask, StencilPipelineState, StencilState,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, vertices::TextVertex},
};

use super::{character_view::CharacterTextures, characters::CharacterState};
use crate::{
    asset::{
        font::{GpuFontGlyphHandle, GpuFontLazy},
        system::AssetServer,
    },
    layer::render_params::{DrawableClipMode, DrawableClipParams, TransformParams},
};

#[derive(Debug)]
pub struct CharacterTextGlyph {
    pub codepoint: char,
    /// Expanded three-pixel border quad, relative to the text viewport.
    pub rect: Vec4,
    pub uv: Vec4,
    pub distances: [Vec2; 8],
    pub color: FloatColor4,
}

#[derive(Debug)]
pub struct CharacterTextLayout {
    pub glyphs: Vec<CharacterTextGlyph>,
    pub name_glyph_count: usize,
    pub name_height: f32,
    pub height: f32,
}
impl CharacterTextLayout {
    /// Native0x504b0..0x50974; generic text style1 adds three pixels on each side.
    pub fn new(name: &str, description: &str, font: &impl FontMetrics) -> Self {
        Self::with_content_width(name, description, font, 960.0)
    }

    /// Generic style1 removes its six-pixel border from the native width
    /// parameter before layout. Characters passes966; Tips passes1222.
    pub(crate) fn with_content_width(
        name: &str,
        description: &str,
        font: &impl FontMetrics,
        content_width: f32,
    ) -> Self {
        let append = |text: &str, text_size: f32, body: bool, origin: Vec2| {
            let mut glyphs = Vec::new();
            let (commands, _, size) = MessageTextLayouter::new(
                font,
                font,
                LayoutParams {
                    layout_width: content_width,
                    text_size,
                    rubi_size: text_size * 0.4,
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
            .parse(text);
            for command in commands {
                let Command::Char(character) = command else {
                    continue;
                };
                let info = font.get_glyph_info(character.codepoint).unwrap();
                if info.actual_width == 0 || info.actual_height == 0 {
                    continue;
                }
                let scale = vec2(character.horizontal_scale, character.scale);
                let size = info.actual_size_f32() * scale;
                let normalized = info.actual_size_normalized();
                let overdraw = normalized * (Vec2::splat(3.0) / size);
                let pos = origin + character.position + info.bearing_screenspace_f32() * scale;
                let color = FloatColor4::from_unorm(character.color);
                let radius = overdraw;
                glyphs.push(CharacterTextGlyph {
                    codepoint: character.codepoint,
                    rect: vec4(pos.x, pos.y, size.x + 6.0, size.y + 6.0),
                    uv: vec4(
                        -overdraw.x,
                        -overdraw.y,
                        normalized.x + overdraw.x,
                        normalized.y + overdraw.y,
                    ),
                    distances: [
                        radius * vec2(-0.707, -0.707),
                        radius * vec2(0.0, -1.0),
                        radius * vec2(0.707, -0.707),
                        radius * vec2(-1.0, 0.0),
                        radius * vec2(1.0, 0.0),
                        radius * vec2(-0.707, 0.707),
                        radius * vec2(0.0, 1.0),
                        radius * vec2(0.707, 0.707),
                    ],
                    color: color,
                });
            }
            (glyphs, size.y + 6.0)
        };
        // The text object's inner three-pixel padding cancels border expansion.
        let (mut glyphs, name_height) = append(name, 48.0, false, vec2(5.0, 5.0));
        let name_height = name_height + 5.0;
        let name_glyph_count = glyphs.len();
        let (body, body_height) = append(description, 32.0, true, vec2(5.0, name_height + 3.0));
        glyphs.extend(body);
        Self {
            glyphs,
            name_glyph_count,
            name_height,
            height: name_height + 3.0 + body_height + 5.0,
        }
    }

    pub fn max_scroll(&self) -> f32 {
        (self.height - 730.0).max(0.0)
    }
    pub fn clamp_scroll(&self, scroll: f32) -> f32 {
        scroll.clamp(0.0, self.max_scroll())
    }
}

pub struct CharacterText {
    pub layout: CharacterTextLayout,
    handles: Vec<GpuFontGlyphHandle>,
}
impl CharacterText {
    pub fn load(server: &AssetServer, state: &CharacterState) -> Self {
        let font: Arc<GpuFontLazy> = server.load_sync("/system.fnt").unwrap();
        crate::text_stream::description(&state.name, &state.description);
        let layout = CharacterTextLayout::new(&state.name, &state.description, font.as_ref());
        Self::from_layout(server, font, layout)
    }

    pub(crate) fn from_layout(
        server: &AssetServer,
        font: Arc<GpuFontLazy>,
        layout: CharacterTextLayout,
    ) -> Self {
        let codepoints: Vec<_> = layout.glyphs.iter().map(|g| g.codepoint).collect();
        let handles = font.load_glyphs(
            server.context().wgpu_device.clone(),
            server.context().wgpu_queue.clone(),
            &codepoints,
        );
        Self { layout, handles }
    }

    pub fn render_divider(
        &self,
        pass: &mut RenderPass,
        atlas: &CharacterTextures,
        scroll: f32,
        transform: Mat4,
    ) {
        let (clip, builder) = Self::begin_clip(pass, transform, vec4(544.0, 188.0, 976.0, 730.0));
        atlas.render_text_divider(pass, self.layout.name_height, scroll, transform, builder);
        clip.end(pass);
    }

    pub(crate) fn begin_clip(
        pass: &mut RenderPass,
        transform: Mat4,
        area: Vec4,
    ) -> (DrawableClipParams, RenderRequestBuilder) {
        let clip = DrawableClipParams {
            mode: DrawableClipMode::Clip,
            area,
        };
        clip.begin(
            pass,
            &TransformParams {
                transform: crate::render::centered_projection_matrix().inverse() * transform,
                ..Default::default()
            },
        );
        let builder = RenderRequestBuilder::new()
            .color_blend_type(ColorBlendType::Layer1)
            .depth_stencil(DepthStencilState {
                stencil: StencilState {
                    stencil_reference: 0,
                    pipeline: StencilPipelineState {
                        function: StencilFunction::Equal,
                        stencil_read_mask: StencilMask::ClipOnly,
                        ..Default::default()
                    },
                },
                ..Default::default()
            });
        (clip, builder)
    }

    /// Native full-character artwork is drawn between the divider and the glyphs.
    pub fn render(&self, pass: &mut RenderPass, scroll: f32, transform: Mat4) {
        self.render_in_viewport(pass, scroll, transform, vec4(544.0, 188.0, 976.0, 730.0));
    }

    pub(crate) fn render_in_viewport(
        &self,
        pass: &mut RenderPass,
        scroll: f32,
        transform: Mat4,
        viewport: Vec4,
    ) {
        self.render_clipped(
            pass,
            scroll,
            transform,
            viewport.truncate().truncate(),
            viewport,
        );
    }

    pub(crate) fn render_clipped(
        &self,
        pass: &mut RenderPass,
        scroll: f32,
        transform: Mat4,
        origin: Vec2,
        area: Vec4,
    ) {
        let (clip, builder) = Self::begin_clip(pass, transform, area);
        let transform =
            transform * Mat4::from_translation(glam::vec3(origin.x, origin.y - scroll, 0.0));
        self.render_with_builder(pass, transform, builder);
        clip.end(pass);
    }

    /// Draw inside a stencil already owned by the caller (SaveMenu slot rows).
    pub(crate) fn render_with_builder(
        &self,
        pass: &mut RenderPass,
        transform: Mat4,
        builder: RenderRequestBuilder,
    ) {
        for range in [
            0..self.layout.name_glyph_count,
            self.layout.name_glyph_count..self.layout.glyphs.len(),
        ] {
            for border in [true, false] {
                for index in range.clone() {
                    let Ok(glyph_texture) = self.handles[index].as_texture_source() else {
                        continue;
                    };
                    let glyph = &self.layout.glyphs[index];
                    let r = glyph.rect;
                    let uv = glyph.uv;
                    let vertices = [
                        TextVertex {
                            position: vec4(r.x, r.y, uv.x, uv.y),
                            color: 0.0,
                        },
                        TextVertex {
                            position: vec4(r.x + r.z, r.y, uv.z, uv.y),
                            color: 0.0,
                        },
                        TextVertex {
                            position: vec4(r.x, r.y + r.w, uv.x, uv.w),
                            color: 0.0,
                        },
                        TextVertex {
                            position: vec4(r.x + r.z, r.y + r.w, uv.z, uv.w),
                            color: 0.0,
                        },
                    ];
                    let vertices = VertexSource::VertexData {
                        vertices: &vertices,
                    };
                    let program = if border {
                        RenderProgramWithArguments::FontBorder {
                            vertices,
                            glyph: glyph_texture,
                            transform,
                            distances: glyph.distances,
                            color: FloatColor4::from_rgba(
                                47.0 / 255.0,
                                25.0 / 255.0,
                                20.0 / 255.0,
                                1.0,
                            ),
                        }
                    } else {
                        RenderProgramWithArguments::Font {
                            vertices,
                            glyph: glyph_texture,
                            transform,
                            color1: glyph.color,
                            color2: glyph.color,
                        }
                    };
                    pass.run(builder.build(program, DrawPrimitive::TrianglesStrip));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Metrics;
    impl FontMetrics for Metrics {
        fn get_ascent(&self) -> u32 {
            60
        }
        fn get_descent(&self) -> u32 {
            20
        }
        fn get_glyph_info(&self, _: char) -> Option<shin_core::format::font::GlyphInfo> {
            Some(shin_core::format::font::GlyphInfo {
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
    fn names_stay_on_one_line_descriptions_wrap_and_scroll_is_bounded() {
        let layout = CharacterTextLayout::new(&"名".repeat(60), &"説明".repeat(700), &Metrics);
        assert_eq!(layout.name_glyph_count, 60);
        assert_eq!(layout.name_height, 59.0);
        assert!(layout.height > 730.0);
        assert_eq!(layout.clamp_scroll(-20.0), 0.0);
        assert_eq!(layout.clamp_scroll(10000.0), layout.height - 730.0);
        assert!(
            layout
                .glyphs
                .iter()
                .all(|g| g.rect.to_array().into_iter().all(f32::is_finite))
        );
        assert!(
            layout.glyphs[60..]
                .iter()
                .all(|g| g.rect.x + g.rect.z <= 976.0)
        );
        let short = CharacterTextLayout::new("名前", "@b説明@b", &Metrics);
        assert_eq!(short.max_scroll(), 0.0);
        assert_eq!(short.glyphs.len(), 4);
    }

    #[test]
    #[ignore = "requires supplied ROM (SHIN_REFERENCE_ROM)"]
    fn reference_all_character_text_resolves_font_and_layout() {
        use shin_core::{
            format::{rom::RomReader, scenario::Scenario},
            primitives::stateless_reader::StatelessFile,
        };
        use std::{
            collections::HashSet,
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
        let scenario = Scenario::new(read("/main.snr").into()).unwrap();
        let font =
            shin_core::format::font::read_lazy_font(&mut Cursor::new(read("/system.fnt"))).unwrap();
        let tables = scenario.info_tables();
        let mut texts = HashSet::new();
        let mut scrolling = 0;
        let mut glyphs = 0;
        for (id, grid) in tables.chars_grid_info.iter().enumerate() {
            let model = super::super::characters::CharacterModel::new(
                grid,
                &tables.chars_sprite_info,
                id,
                0,
            );
            for cell in &model.cells {
                for definition in &cell.definitions {
                    for state in &definition.states {
                        if !texts.insert((state.name.clone(), state.description.clone())) {
                            continue;
                        }
                        let layout =
                            CharacterTextLayout::new(&state.name, &state.description, &font);
                        assert!(layout.height.is_finite());
                        assert!(layout.name_height.is_finite());
                        for glyph in &layout.glyphs {
                            assert!(glyph.rect.z > 6.0 && glyph.rect.w > 6.0);
                            assert!(glyph.rect.to_array().into_iter().all(f32::is_finite));
                            assert!(glyph.uv.to_array().into_iter().all(f32::is_finite));
                        }
                        glyphs += layout.glyphs.len();
                        scrolling += usize::from(layout.max_scroll() > 0.0);
                    }
                }
            }
        }
        assert!(texts.len() > 100);
        assert!(scrolling > 0);
        eprintln!(
            "Character text: {} unique entries,{glyphs} drawable glyphs,{scrolling} scrolling entries",
            texts.len()
        );
    }

    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_character_text_layout() {
        let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&path)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
        for (a, value) in [
            (0x185cd0, 3.0f32),
            (0x159624, 960.0),
            (0x159a6c, 47.0 / 255.0),
            (0x1593e0, 25.0 / 255.0),
            (0x159478, 20.0 / 255.0),
            (0x159448, 544.0),
            (0x159384, 188.0),
            (0x159674, 976.0),
            (0x159618, 730.0),
            (0x159a70, -730.0),
            (0x159440, -188.0),
            (0x15990c, 0.4),
            (0x159468, -0.707),
            (0x159784, 0.707),
        ] {
            assert_eq!(word(a), value.to_bits(), "native character text{a:x}");
        }
        assert_eq!(&exe[0x15bcef + 256..0x15bcef + 268], b"/system.fnt\0");
        for (a, opcode) in [
            (0x65bdc, 0xf943b508u32),
            (0x50564, 0xf943b79c),
            (0x50544, 0x52a84808u32),
            (0x50700, 0x52a84008),
            (0x5070c, 0x52802028),
            (0x506cc, 0x1e28380d),
            (0x50704, 0xbd038260),
            (0x50970, 0xbd038668),
            (0x1011a4, 0xa903a408),
        ] {
            assert_eq!(word(a), opcode, "native character text{a:x}");
        }
    }
}

#[cfg(test)]
pub(crate) fn assert_reference_character_text(
    context: &mut crate::render::PreRenderContext,
    server: &Arc<AssetServer>,
    scenario: &shin_core::format::scenario::Scenario,
) {
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
        shin_core::format::texture_archive::read_texture_archive(&read("/chars.txa")).unwrap();
    let atlas: Arc<CharacterTextures> = server.load_sync("/chars.txa").unwrap();
    let tables = scenario.info_tables();
    let models: Vec<_> = (0..8)
        .map(|id| {
            super::characters::CharacterModel::new(
                &tables.chars_grid_info[id],
                &tables.chars_sprite_info,
                id,
                0,
            )
        })
        .collect();
    let first = models[0]
        .selected_cell()
        .unwrap()
        .definition()
        .state()
        .unwrap();
    let longest = models
        .iter()
        .flat_map(|m| &m.cells)
        .flat_map(|c| &c.definitions)
        .flat_map(|d| &d.states)
        .max_by_key(|s| {
            (CharacterTextLayout::new(&s.name, &s.description, &font).height * 100.0) as i32
        })
        .unwrap();
    let output = context.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Native Characters outlined text"),
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
    for (state, at_bottom) in [(first, false), (longest, false), (longest, true)] {
        let text = CharacterText::load(server, state);
        let scroll = if at_bottom {
            text.layout.max_scroll()
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
            text.render_divider(
                &mut pass,
                &atlas,
                scroll,
                crate::render::top_left_projection_matrix(),
            );
            text.render(
                &mut pass,
                scroll,
                crate::render::top_left_projection_matrix(),
            );
        }
        let actual = crate::layer::user::TileLayer::readback_reference_texture(context, &output);
        let mut expected = vec![[32.0 / 255.0, 64.0 / 255.0, 96.0 / 255.0, 1.0]; 1920 * 1080];
        // Expected geometry comes from the core parser and independently entered native
        // constants, not CharacterTextLayout's glyph quads or border-distance arrays.
        let parse = |message: &str, size: f32, body: bool| {
            MessageTextLayouter::new(
                &font,
                &font,
                LayoutParams {
                    layout_width: 960.0,
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
        let (name, _, name_size) = parse(&state.name, 48.0, false);
        let name_height = name_size.y + 11.0;
        let (body, _, _) = parse(&state.description, 32.0, true);
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
        for y in 188..918usize {
            if (y as f32 + 0.5) < bar_y || (y as f32 + 0.5) >= bar_y + 16.0 {
                continue;
            }
            for x in 544..1520usize {
                let sx = ((x as f32 + 0.5 - 544.0) / 976.0 * bar.width() as f32 - 0.5)
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
                    let left = 544.0
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
                    for y in ((top - 0.5).ceil().max(188.0) as usize)
                        ..((top + height - 0.5).ceil().clamp(188.0, 918.0) as usize)
                    {
                        for x in ((left - 0.5).ceil().max(544.0) as usize)
                            ..((left + width - 0.5).ceil().clamp(544.0, 1520.0) as usize)
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
                    "Character text {},scroll{scroll}: pixel{},{} channel{channel}: {pixel:?} vs {wanted:?}",
                    state.name,
                    index % 1920,
                    index / 1920
                );
            }
        }
        eprintln!(
            "Character text {}, scroll{scroll}: {} glyphs, worst channel error{worst}",
            state.name,
            text.layout.glyphs.len()
        );
        if at_bottom {
            let mut ppm = b"P6\n1920 1080\n255\n".to_vec();
            for pixel in actual.chunks_exact(4) {
                ppm.extend_from_slice(&pixel[..3]);
            }
            std::fs::write("/tmp/shin-character-text-reference.ppm", ppm).unwrap();
        }
    }
    // Compose the recovered draw paths in native order for a reviewable screen preview.
    let details = CharacterText::load(server, first);
    let path = format!(
        "/chars/{}.txa",
        first.variant().unwrap().full_sprite.to_ascii_lowercase()
    );
    let full: Arc<CharacterTextures> = server.load_sync(&path).unwrap();
    {
        let mut pass = context.begin_pass(
            TextureTarget {
                view: &view,
                kind: TextureTargetKind::Screen,
            },
            Some(context.depth_stencil),
            "Character screen preview",
        );
        pass.clear(
            Some(shin_core::primitives::color::UnormColor::BLACK),
            Some(128),
            None,
        );
        let transform = crate::render::top_left_projection_matrix();
        atlas.render_background(&mut pass, 1.0, transform);
        atlas.render_window(&mut pass, transform);
        atlas.render_grid(&mut pass, &models[0], 0.0, None, transform);
        atlas.render_controls(&mut pass, &models[0], [0.0; 5], transform);
        details.render_divider(&mut pass, &atlas, 0.0, transform);
        full.render_full_sprite(&mut pass, transform);
        details.render(&mut pass, 0.0, transform);
        atlas.render_header(&mut pass, 1.0, transform);
    }
    let pixels = crate::layer::user::TileLayer::readback_reference_texture(context, &output);
    let mut ppm = b"P6\n1920 1080\n255\n".to_vec();
    for pixel in pixels.chunks_exact(4) {
        ppm.extend_from_slice(&pixel[..3]);
    }
    std::fs::write("/tmp/shin-character-screen-reference.ppm", ppm).unwrap();
}
