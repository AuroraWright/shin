use super::purple_text::{CHAPTERS, PERSON_TEXT};
use crate::asset::{
    font::{GpuFontGlyphHandle, GpuFontLazy},
    system::AssetServer,
};
use crate::layer::render_params::multiply_transforms;
use glam::{Mat4, Vec2, vec2, vec4};
use shin_core::layout::font::FontMetrics;
use shin_core::{
    layout::{
        LayoutParams, MessageTextLayouter, MessageTextLayouterDefaults,
        commands::{CharFontType, Command},
    },
    primitives::color::FloatColor4,
};
use shin_render::{
    DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, vertices::TextVertex},
};
use std::sync::Arc;

struct Glyph {
    handle: GpuFontGlyphHandle,
    vertices: [TextVertex; 4],
    distances: [Vec2; 8],
    color: FloatColor4,
}
pub struct PurplePage {
    glyphs: Vec<Glyph>,
    pub height: f32,
}
pub struct PurplePages {
    pub chapters: Vec<PurplePage>,
    pub persons: Vec<PurplePage>,
}
impl PurplePages {
    pub async fn load(server: &AssetServer) -> Arc<Self> {
        let normal = server
            .load::<GpuFontLazy, _>("/newrodin-medium.fnt")
            .await
            .unwrap();
        let bold = server
            .load::<GpuFontLazy, _>("/newrodin-bold.fnt")
            .await
            .unwrap();
        let page = |text: &str| {
            let (commands, _, size) = MessageTextLayouter::new(
                normal.as_ref(),
                bold.as_ref(),
                LayoutParams {
                    layout_width: 1024.0,
                    text_size: 32.0,
                    always_leave_space_for_rubi: true,
                    ..Default::default()
                },
                MessageTextLayouterDefaults {
                    color: 999,
                    draw_speed: 50,
                    fade: 200,
                },
            )
            .parse(text);
            let mut glyphs = Vec::new();
            for command in commands {
                let Command::Char(character) = command else {
                    continue;
                };
                let font = match character.font {
                    CharFontType::Regular => &normal,
                    CharFontType::Bold => &bold,
                };
                let handle = font
                    .clone()
                    .load_glyphs(
                        server.context().wgpu_device.clone(),
                        server.context().wgpu_queue.clone(),
                        &[character.codepoint],
                    )
                    .remove(0);
                let info = handle.info();
                if info.actual_width == 0 || info.actual_height == 0 {
                    continue;
                }
                let scale = vec2(character.horizontal_scale, character.scale);
                let pixels = info.actual_size_f32();
                let scaled = pixels * scale;
                let bearing = info.bearing_screenspace_f32() * scale - 3.0;
                // Keep the native ordered operations; glyph UV rounding affects
                // the outline and text at fractional parent transforms.
                let end_bearing = (scaled + bearing) + 6.0;
                let position = character.position + 3.0;
                let top_left = position + bearing;
                let bottom_right = position + end_bearing;
                let expansion = ((end_bearing - bearing) / scaled - 1.0) * 0.5;
                let uv = info.actual_size_normalized();
                let uv_start = -expansion * uv;
                let uv_end = (expansion + 1.0) * uv;
                let ascent = font.get_ascent() as i32 - info.bearing_y as i32;
                let (top_color, bottom_color) = if character.is_rubi {
                    (0., 0.)
                } else {
                    (
                        ascent as f32 / 32.,
                        (ascent + info.actual_height as i32) as f32 / 32.,
                    )
                };
                let vertices = [
                    TextVertex {
                        position: vec4(top_left.x, top_left.y, uv_start.x, uv_start.y),
                        color: top_color,
                    },
                    TextVertex {
                        position: vec4(bottom_right.x, top_left.y, uv_end.x, uv_start.y),
                        color: top_color,
                    },
                    TextVertex {
                        position: vec4(top_left.x, bottom_right.y, uv_start.x, uv_end.y),
                        color: bottom_color,
                    },
                    TextVertex {
                        position: vec4(bottom_right.x, bottom_right.y, uv_end.x, uv_end.y),
                        color: bottom_color,
                    },
                ];
                let radius = Vec2::splat(3.) / scale / pixels * uv;
                let diagonal = f32::from_bits(0x3f34fdf4);
                let distances = [
                    vec2(-diagonal, -diagonal),
                    vec2(0., -1.),
                    vec2(diagonal, -diagonal),
                    vec2(-1., 0.),
                    vec2(1., 0.),
                    vec2(-diagonal, diagonal),
                    vec2(0., 1.),
                    vec2(diagonal, diagonal),
                ]
                .map(|direction| direction * radius);
                let rgba = character.color.0;
                let reciprocal = 1.0f32 / 255.0;
                let color = FloatColor4::from_rgba(
                    (rgba & 255) as f32 * reciprocal,
                    ((rgba >> 8) & 255) as f32 * reciprocal,
                    ((rgba >> 16) & 255) as f32 * reciprocal,
                    ((rgba >> 24) & 255) as f32 * reciprocal,
                );
                glyphs.push(Glyph {
                    handle,
                    vertices,
                    distances,
                    color,
                });
            }
            PurplePage {
                glyphs,
                height: size.y + 6.0,
            }
        };
        Arc::new(Self {
            chapters: CHAPTERS.iter().map(|entry| page(entry.text)).collect(),
            persons: PERSON_TEXT.iter().map(|text| page(text)).collect(),
        })
    }
}
impl PurplePage {
    pub fn render(
        &self,
        pass: &mut RenderPass,
        builder: RenderRequestBuilder,
        matrix: Mat4,
        scroll: f32,
        tint: FloatColor4,
    ) {
        let incoming = multiply_transforms(
            matrix,
            Mat4::from_translation(glam::vec3(726., -(scroll - 180.), 0.)),
        );
        // FontTextLayer has its own -3 horizontal position, applied after
        // the transcript scroll transform rather than folded into it.
        let local = multiply_transforms(
            Mat4::IDENTITY,
            Mat4::from_translation(glam::vec3(-0., -0., 0.)),
        );
        let local = multiply_transforms(Mat4::from_translation(glam::vec3(-3., 0., 0.)), local);
        let transform = multiply_transforms(incoming, local);
        for glyph in &self.glyphs {
            let Ok(glyph_texture) = glyph.handle.as_texture_source() else {
                continue;
            };
            pass.run(builder.build(
                RenderProgramWithArguments::FontBorder {
                    vertices: VertexSource::VertexData {
                        vertices: &glyph.vertices,
                    },
                    glyph: glyph_texture,
                    transform,
                    color: FloatColor4::from_rgba(0., 0., 0., 240.0 * (1.0f32 / 255.0)) * tint,
                    distances: glyph.distances,
                },
                DrawPrimitive::TrianglesStrip,
            ));
        }
        for glyph in &self.glyphs {
            let Ok(glyph_texture) = glyph.handle.as_texture_source() else {
                continue;
            };
            let color = glyph.color * tint;
            pass.run(builder.build(
                RenderProgramWithArguments::Font {
                    vertices: VertexSource::VertexData {
                        vertices: &glyph.vertices,
                    },
                    glyph: glyph_texture,
                    transform,
                    color1: color,
                    color2: color,
                },
                DrawPrimitive::TrianglesStrip,
            ));
        }
    }
}

#[cfg(test)]
pub(crate) fn reference_transcript_colors_render(
    context: &mut crate::render::PreRenderContext<'_, '_, '_, '_>,
    server: &Arc<AssetServer>,
) {
    use shin_core::primitives::color::UnormColor;
    use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
    let mut pages = Arc::try_unwrap(shin_tasks::block_on(PurplePages::load(server)))
        .ok()
        .expect("Reference owns the loaded transcript pages");
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    for name in ["uncompressed_executable", "uncompressed_executable_update"] {
        let executable = std::fs::read(root.join("umineko_switch_files").join(name)).unwrap();
        for text in [CHAPTERS[5].text, CHAPTERS[1].text, PERSON_TEXT[0]] {
            assert!(
                executable
                    .windows(text.len())
                    .any(|bytes| bytes == text.as_bytes()),
                "Transcript comes from {name}"
            );
        }
    }
    let target = context.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Murderer transcript color channel regression"),
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
    let view = target.create_view(&Default::default());
    let mut red = pages.chapters.remove(5);
    let mut purple_chapter = pages.chapters.remove(1);
    let mut purple_person = pages.persons.remove(0);
    // White names/punctuation are excluded; colored glyph positions and UVs
    // stay exactly as produced by the actual chapter/person page loader.
    purple_chapter
        .glyphs
        .retain(|g| g.color != FloatColor4::WHITE);
    purple_person
        .glyphs
        .retain(|g| g.color != FloatColor4::WHITE);
    for (name, page, rgb) in [
        ("chapter red", &mut red, [255u8, 0, 0]),
        ("chapter purple", &mut purple_chapter, [170, 113, 255]),
        ("person purple", &mut purple_person, [170, 113, 255]),
    ] {
        let original: Vec<_> = page.glyphs.iter().map(|g| g.color).collect();
        assert!(!original.is_empty());
        let mut draw =
            |override_color: Option<FloatColor4>,
             context: &mut crate::render::PreRenderContext<'_, '_, '_, '_>| {
                for (glyph, &color) in page.glyphs.iter_mut().zip(&original) {
                    glyph.color = override_color.unwrap_or(color);
                }
                {
                    let mut pass = context.begin_pass(
                        TextureTarget {
                            view: &view,
                            kind: TextureTargetKind::Screen,
                        },
                        Some(context.depth_stencil),
                        "Transcript color oracle",
                    );
                    pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
                    page.render(
                        &mut pass,
                        RenderRequestBuilder::new()
                            .depth_stencil_shorthand(0, true, false)
                            .color_blend_type(shin_render::ColorBlendType::Layer1),
                        crate::render::top_left_projection_matrix(),
                        0.0,
                        FloatColor4::WHITE,
                    );
                }
                crate::layer::user::readback_test_texture(context, &target)
            };
        let black = draw(Some(FloatColor4::BLACK), context);
        let white = draw(Some(FloatColor4::WHITE), context);
        let colored = draw(None, context);
        let mut count = 0;
        for i in 0..1920 * 1080usize {
            let offset = i * 4;
            for channel in 0..3 {
                let expected = black[offset + channel] as f32
                    + (white[offset + channel] as f32 - black[offset + channel] as f32)
                        * rgb[channel] as f32
                        / 255.0;
                assert!(
                    (colored[offset + channel] as f32 - expected).abs() <= 2.0,
                    "{name}: pixel{i},channel{channel} got{} expected{expected}",
                    colored[offset + channel]
                );
            }
            if white[offset..offset + 3] != black[offset..offset + 3] {
                count += 1;
            }
        }
        assert!(count > 1000, "Visible authored transcript glyphs required");
        eprintln!(
            "Murderer {name}: {count} glyph pixels, all2073600 pixels match independent black/white channel oracle"
        );
    }
}
