//! Native LogMessage formatting and clipped message-browser glyphs.
use std::sync::Arc;

use glam::{Mat4, Vec2, vec2, vec4};
use shin_core::{
    layout::{
        BacklogTextLayouter, LayoutParams, LineInfo, MessageTextLayouterDefaults,
        commands::{CharFontType, Command},
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

use crate::adv::{assets::AdvFonts, backlog::BacklogItem, characters::text::CharacterTextGlyph};
use crate::{
    asset::font::{GpuFontGlyphHandle, GpuFontLazy},
    layer::{
        message_layer::outline,
        render_params::{DrawableClipMode, DrawableClipParams, TransformParams},
    },
    render::top_left_projection_matrix,
};

pub(crate) fn layout_message<Font: FontMetrics>(
    text: &str,
    color: i32,
    regular: Font,
    bold: Font,
) -> (Vec<Command>, Vec<LineInfo>, Vec2) {
    layout_display_text(text, color, regular, bold, false)
}

/// Only quiz history labels are defined in code; other rows contain SNR text.
pub(crate) fn localize_item(item: &BacklogItem) -> bool {
    matches!(item, BacklogItem::Quiz { .. })
}

pub(crate) fn layout_display_text<Font: FontMetrics>(
    text: &str,
    color: i32,
    regular: Font,
    bold: Font,
    localize: bool,
) -> (Vec<Command>, Vec<LineInfo>, Vec2) {
    // a3638..a3660/a5c38..a5c60: raw native LayoutParams, including
    // zero ruby size (base constructor derives 40% of text size).
    let layouter = BacklogTextLayouter::new(
        regular,
        bold,
        LayoutParams {
            layout_width: 1480.0,
            line_padding_above: 6.0,
            line_padding_below: 4.0,
            text_size: 40.0,
            base_font_horizontal_scale: f32::from_bits(0x3f783e42),
            always_leave_space_for_rubi: false,
            ..Default::default()
        },
        MessageTextLayouterDefaults {
            color,
            draw_speed: 50,
            fade: 200,
        },
    );
    if localize {
        layouter.parse(text)
    } else {
        layouter.parse_raw(text)
    }
}

pub(crate) struct BacklogText {
    glyphs: Vec<CharacterTextGlyph>,
    handles: Vec<GpuFontGlyphHandle>,
    pub height: f32,
}

pub(crate) fn display_text(item: &BacklogItem) -> Option<(String, i32)> {
    match item {
        BacklogItem::Message { text, .. } => Some((text.clone(), 999)),
        BacklogItem::Choice { title, text, .. } => Some((format!("@r《{title}》@r{text}"), 977)),
        BacklogItem::Checkpoint { title, text } => Some((format!("@r{title}@r{text}"), 799)),
        BacklogItem::Quiz { mode, response, .. } => {
            quiz_text((*mode)?, *response).map(|text| (text, 977))
        }
    }
}

// Authored browser response tables at native1da0d8/1da110/1da1f0.
const QUIZ_ANSWERS: [&str; 7] = [
    "Ａを選択",
    "Ｂを選択",
    "Ｃを選択",
    "Ｄを選択",
    "Ｅを選択",
    "Ｆを選択",
    "絵羽おばさんに相談",
];
const MURDERER_ANSWERS: [&str; 28] = [
    "物語を再読：第一の晩",
    "物語を再読：第二の晩",
    "物語を再読：第四の晩",
    "物語を再読：第五・六の晩",
    "物語を再読：第七の晩",
    "物語を再読：第八の晩",
    "ヒント：信用できる紫発言を探せ。",
    "ヒント：絶対に犯人でないのは誰？",
    "ヒント：ト書きは真実を語る。",
    "ヒント：南條と朱志香はシロ！",
    "ヒント：確実な死者をどんどん探せ。",
    "ヒント：９人の無実。",
    "ヒント：マスターキーが使えない？",
    "ヒント：赤き真実に違和感？",
    "ヒント：確実に６人を殺した？",
    "ヒント：犯人は閉じ込められた？",
    "ヒント：共犯の存在。",
    "ヒント：紗音は譲治以外の誰にでも殺せる。",
    "ヒント：死んだフリをした犯人がいる。そしてもう１人犯人がいる。",
    "ヒント：さらにもう１人犯人がいる？",
    "ヒント：譲治と真里亞はニワトリとタマゴ。",
    "ヒント：真里亞犯人説検証。",
    "ヒント：譲治犯人説検証。",
    "ヒント：譲治の紫発言。",
    "ヒント：大人２人、子供１人。",
    "ヒント：犯人の３人は家族。",
    "ヒント：子供の犯人は戦人。",
    "犯人特定：成功",
];

fn quiz_text(mode: i32, response: i32) -> Option<String> {
    match mode {
        0 => Some(format!(
            "@r《クイズ》@r{}",
            QUIZ_ANSWERS.get(usize::try_from(response).ok()?)?
        )),
        1 => Some("@r《ケーキ選択》".into()),
        2 => {
            let answer = if response > 27 {
                "犯人特定：失敗"
            } else {
                MURDERER_ANSWERS.get(usize::try_from(response).ok()?)?
            };
            Some(format!("@r《犯人当て》@r{answer}"))
        }
        3 => Some(format!(
            "@r《最後の選択》@r{}",
            ["魔法", "手品"].get(usize::try_from(response).ok()?)?
        )),
        // a3834 branches directly to return for an unknown native subtype.
        _ => Some(String::new()),
    }
}

impl BacklogText {
    pub fn load(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        fonts: &AdvFonts,
        item: &BacklogItem,
        outline_mode: u8,
    ) -> Option<Self> {
        let (text, color) = display_text(item)?;
        Some(Self::load_with_localization(
            device,
            queue,
            &fonts.medium_font,
            &fonts.bold_font,
            &text,
            color,
            outline_mode,
            localize_item(item),
        ))
    }

    pub fn load_raw(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        regular: &Arc<GpuFontLazy>,
        bold: &Arc<GpuFontLazy>,
        text: &str,
        color: i32,
        outline_mode: u8,
    ) -> Self {
        Self::load_with_localization(
            device,
            queue,
            regular,
            bold,
            text,
            color,
            outline_mode,
            false,
        )
    }

    fn load_with_localization(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        regular: &Arc<GpuFontLazy>,
        bold: &Arc<GpuFontLazy>,
        text: &str,
        color: i32,
        outline_mode: u8,
        localize: bool,
    ) -> Self {
        let (commands, _, size) =
            layout_display_text(text, color, regular.as_ref(), bold.as_ref(), localize);
        let mut glyphs = Vec::new();
        let mut handles = Vec::new();
        for command in commands {
            let Command::Char(ch) = command else {
                continue;
            };
            let font = match ch.font {
                CharFontType::Regular => regular,
                CharFontType::Bold => bold,
            };
            let info = font.get_glyph_info(ch.codepoint).unwrap();
            if info.actual_width == 0 || info.actual_height == 0 || ch.horizontal_scale == 0.0 {
                continue;
            }
            let scale = vec2(ch.horizontal_scale, ch.scale);
            let pixels = info.actual_size_f32() * scale;
            let uv = info.actual_size_normalized();
            let overdraw = uv * (Vec2::splat(2.0) / pixels);
            let pos = ch.position + info.bearing_screenspace_f32() * scale - Vec2::splat(2.0);
            let fill = FloatColor4::from_unorm(ch.color);
            glyphs.push(CharacterTextGlyph {
                codepoint: ch.codepoint,
                rect: vec4(pos.x, pos.y, pixels.x + 4.0, pixels.y + 4.0),
                uv: vec4(
                    -overdraw.x,
                    -overdraw.y,
                    uv.x + overdraw.x,
                    uv.y + overdraw.y,
                ),
                distances: outline::distances(scale, info.actual_size_f32(), uv, outline_mode),
                color: fill,
            });
            handles.extend(font.clone().load_glyphs(
                device.clone(),
                queue.clone(),
                &[ch.codepoint],
            ));
        }
        Self {
            glyphs,
            handles,
            height: size.y,
        }
    }

    /// Native a4f50..a502c: row x136, inner text x136/y10. Caller
    /// supplies the row's document position and current scroll, in pixels.
    pub fn render(&self, pass: &mut RenderPass, row_top: f32, scroll: f32, progress: f32) {
        let slide = 1920.0 * (1.0 - progress);
        let parent =
            top_left_projection_matrix() * Mat4::from_translation(glam::vec3(0.0, slide, 0.0));
        let clip = DrawableClipParams {
            mode: DrawableClipMode::Clip,
            area: vec4(136.0, 188.0, 1648.0, 730.0),
        };
        clip.begin(
            pass,
            &TransformParams {
                transform: crate::render::centered_projection_matrix().inverse() * parent,
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
        let transform =
            parent * Mat4::from_translation(glam::vec3(272.0, 198.0 + row_top - scroll, 0.0));
        for border in [true, false] {
            for (glyph, handle) in self.glyphs.iter().zip(&self.handles) {
                let Ok(glyph_texture) = handle.as_texture_source() else {
                    continue;
                };
                let r = glyph.rect;
                let uv = glyph.uv;
                let vertices = [
                    vec4(r.x, r.y, uv.x, uv.y),
                    vec4(r.x + r.z, r.y, uv.z, uv.y),
                    vec4(r.x, r.y + r.w, uv.x, uv.w),
                    vec4(r.x + r.z, r.y + r.w, uv.z, uv.w),
                ]
                .map(|position| TextVertex {
                    position,
                    color: 0.0,
                });
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
        clip.end(pass);
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

    struct LocalizingMetrics;
    impl FontMetrics for LocalizingMetrics {
        fn localize<'a>(&self, _: &'a str) -> std::borrow::Cow<'a, str> {
            std::borrow::Cow::Owned("English".into())
        }
        fn get_ascent(&self) -> u32 {
            Metrics.get_ascent()
        }
        fn get_descent(&self) -> u32 {
            Metrics.get_descent()
        }
        fn get_glyph_info(&self, c: char) -> Option<shin_core::format::font::GlyphInfo> {
            Metrics.get_glyph_info(c)
        }
    }
    #[test]
    fn localization_backlog_only_code_defined_quiz_rows_use_ui_overrides() {
        use shin_core::vm::command::types::MessageboxType;
        let authored = [
            BacklogItem::Message {
                message_id: 0,
                text: "確認".into(),
                messagebox_type: MessageboxType::Neutral,
            },
            BacklogItem::Choice {
                title: "確認".into(),
                text: "確認".into(),
                response: 0,
            },
            BacklogItem::Checkpoint {
                title: "確認".into(),
                text: "確認".into(),
            },
        ];
        for item in authored {
            assert!(!localize_item(&item));
            let (text, color) = display_text(&item).unwrap();
            assert_eq!(
                layout_display_text(
                    &text,
                    color,
                    LocalizingMetrics,
                    LocalizingMetrics,
                    localize_item(&item)
                ),
                layout_message(&text, color, Metrics, Metrics),
            );
        }
        let item = BacklogItem::Quiz {
            argument: 0,
            mode: Some(0),
            response: 0,
        };
        assert!(localize_item(&item));
        let (text, color) = display_text(&item).unwrap();
        assert_eq!(
            layout_display_text(
                &text,
                color,
                LocalizingMetrics,
                LocalizingMetrics,
                localize_item(&item)
            ),
            layout_message("English", color, Metrics, Metrics),
        );
    }

    #[test]
    fn quiz_subtype_and_checkpoint_labels_match_native_authored_forms() {
        assert_eq!(quiz_text(0, 0).as_deref(), Some("@r《クイズ》@rＡを選択"));
        assert_eq!(
            quiz_text(0, 6).as_deref(),
            Some("@r《クイズ》@r絵羽おばさんに相談")
        );
        assert_eq!(quiz_text(0, 7), None);
        assert_eq!(quiz_text(1, 999).as_deref(), Some("@r《ケーキ選択》"));
        assert_eq!(
            quiz_text(2, 27).as_deref(),
            Some("@r《犯人当て》@r犯人特定：成功")
        );
        assert_eq!(
            quiz_text(2, 28).as_deref(),
            Some("@r《犯人当て》@r犯人特定：失敗")
        );
        assert_eq!(
            quiz_text(2, 99).as_deref(),
            Some("@r《犯人当て》@r犯人特定：失敗")
        );
        assert_eq!(quiz_text(3, 0).as_deref(), Some("@r《最後の選択》@r魔法"));
        assert_eq!(quiz_text(3, 1).as_deref(), Some("@r《最後の選択》@r手品"));
        assert_eq!(quiz_text(3, 2), None);
        assert_eq!(quiz_text(-1, 0).as_deref(), Some(""));
        assert_eq!(
            display_text(&BacklogItem::Checkpoint {
                title: "Episode".into(),
                text: "Chapter".into()
            }),
            Some(("@rEpisode@rChapter".into(), 799))
        );
        assert_eq!(
            display_text(&BacklogItem::Quiz {
                argument: 17,
                mode: Some(3),
                response: 1
            }),
            Some(("@r《最後の選択》@r手品".into(), 977))
        );
    }

    #[test]
    fn backlog_name_is_full_size_bracketed_and_empty_name_uses_no_line() {
        let (commands, lines, size) =
            layout_message("戦人@r本文@vvoice_1.", 999, &Metrics, &Metrics);
        let chars: Vec<_> = commands
            .iter()
            .filter_map(|c| {
                if let Command::Char(c) = c {
                    Some(c)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(
            chars.iter().map(|c| c.codepoint).collect::<String>(),
            "【戦人】本文"
        );
        assert_eq!(lines.len(), 2);
        assert_eq!(size.y, 100.0);
        assert_eq!(chars[0].scale, chars[4].scale);
        assert_eq!(chars[0].scale, 0.5);
        assert_eq!(chars[0].position.x, 0.0);
        assert_eq!(chars[4].position.x, 0.0);
        assert!(chars[4].position.y > chars[0].position.y);
        assert!(!commands.iter().any(|c| matches!(c, Command::Voice(_))));
        let (commands, lines, size) = layout_message("@r本文", 999, &Metrics, &Metrics);
        assert_eq!(lines.len(), 1);
        assert_eq!(size.y, 50.0);
        assert_eq!(
            commands
                .iter()
                .filter_map(|c| if let Command::Char(c) = c {
                    Some(c.codepoint)
                } else {
                    None
                })
                .collect::<String>(),
            "本文"
        );
    }

    #[test]
    fn backlog_body_wraps_with_quote_indent_and_preserves_bold_ruby_and_color() {
        let text = format!(
            "名前@r「{}」@r@{{太字@}}@c900.赤@bルビ.@<漢字@>",
            "長い文章".repeat(80)
        );
        let (commands, lines, size) = layout_message(&text, 977, &Metrics, &Metrics);
        assert!(lines.len() >= 5);
        assert!(size.y > 730.0 / 4.0);
        let chars: Vec<_> = commands
            .iter()
            .filter_map(|c| {
                if let Command::Char(c) = c {
                    Some(c)
                } else {
                    None
                }
            })
            .collect();
        let opener = chars.iter().find(|c| c.codepoint == '「').unwrap();
        let continued = chars
            .iter()
            .find(|c| c.line_index == opener.line_index + 1 && !c.is_rubi)
            .unwrap();
        assert!((continued.position.x - opener.width).abs() < 0.001);
        assert!(chars.iter().any(|c| c.font == CharFontType::Bold));
        assert!(chars.iter().any(|c| c.is_rubi && c.scale < opener.scale));
        assert!(
            chars
                .iter()
                .all(|c| c.position.is_finite() && c.scale.is_finite())
        );
        assert!(lines.iter().all(|l| l.width <= 1480.1));
    }
    #[test]
    #[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
    fn reference_backlog_text_renders_and_clips_with_supplied_fonts() {
        use crate::asset::system::{
            AssetLoadContext, AssetServer, LayeredAssetIo, cache::AssetCache,
        };
        use shin_render::{
            dynamic_buffer::DynamicBuffer,
            pipelines::PipelineStorage,
            resize::{SurfaceResizeSource, ViewportParams},
            shaders::types::{
                buffer::BytesAddress,
                texture::{
                    DepthStencilTarget, TextureSamplerStore, TextureTarget, TextureTargetKind,
                },
            },
        };
        crate::init_test_task_pools();
        let instance = wgpu::Instance::default();
        let adapter =
            shin_tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .unwrap();
        let (device, queue) =
            shin_tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                required_features: if adapter.get_info().backend == wgpu::Backend::Vulkan {
                    adapter.features() & wgpu::Features::PASSTHROUGH_SHADERS
                } else {
                    wgpu::Features::empty()
                },
                ..Default::default()
            }))
            .unwrap();
        let mut io = LayeredAssetIo::new();
        if let Some(patch) = std::env::var_os("SHIN_REFERENCE_PATCH") {
            io.try_with_rom(patch).unwrap();
        }
        io.try_with_rom(std::env::var("SHIN_REFERENCE_ROM").unwrap())
            .unwrap();
        let server = AssetServer::new(
            io.into(),
            AssetLoadContext {
                game: crate::cli::Game::Umineko,
                higurashi_bustup_cache: Default::default(),
                wgpu_device: device.clone(),
                wgpu_queue: queue.clone(),
                bustup_cache: AssetCache::new(),
                picture_cache: Default::default(),
                font_cache: Default::default(),
            },
        );
        let regular = server
            .load_sync::<GpuFontLazy>("/newrodin-medium.fnt")
            .unwrap();
        let bold = server
            .load_sync::<GpuFontLazy>("/newrodin-bold.fnt")
            .unwrap();
        let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
            1920, 1080,
        )));
        let depth = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Choice test depth"),
            size: wgpu::Extent3d {
                width: 1920,
                height: 1080,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: shin_render::DEPTH_STENCIL_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let depth_view = depth.create_view(&Default::default());
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Choice test target"),
            size: wgpu::Extent3d {
                width: 1920,
                height: 1080,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let samplers = TextureSamplerStore::new(&device);
        let mut pipelines = PipelineStorage::new(device.clone(), format);
        let mut dynamic = DynamicBuffer::new(device.clone(), BytesAddress::new(1024 * 1024));
        let mut encoder = device.create_command_encoder(&Default::default());
        let mut context = crate::render::PreRenderContext {
            breakup_texture: None,
            device: &device,
            queue: &queue,
            resize_source: &resize,
            sampler_store: &samplers,
            depth_stencil: DepthStencilTarget { view: &depth_view },
            pipeline_storage: &mut pipelines,
            dynamic_buffer: &mut dynamic,
            encoder: &mut encoder,
        };

        let text = format!(
            "右代宮戦人@r「{}」@r@{{太字@}}@r@bうみねこ.@<海猫@>@r@c900.赤文字",
            "これはメッセージブラウザーの折り返し表示です。".repeat(40)
        );
        let normal = BacklogText::load_raw(&device, &queue, &regular, &bold, &text, 999, 0);
        let shifted = BacklogText::load_raw(&device, &queue, &regular, &bold, &text, 999, 1);
        assert!(normal.height > 730.0);
        assert!(normal.glyphs.len() > 900);
        let mut baseline = None;
        for (scroll, mode) in [(0.0, 0), (0.0, 1), (600.0, 0), (normal.height + 20.0, 0)] {
            let text = if mode == 0 { &normal } else { &shifted };
            {
                let mut pass = RenderPass::new(
                    context.pipeline_storage,
                    context.dynamic_buffer,
                    context.sampler_store,
                    context.device,
                    context.encoder,
                    TextureTarget {
                        kind: TextureTargetKind::Screen,
                        view: &view,
                    },
                    Some(context.depth_stencil),
                    None,
                    "Backlog text",
                );
                pass.clear(
                    Some(shin_core::primitives::color::UnormColor::BLACK),
                    Some(0),
                    Some(1.0),
                );
                text.render(&mut pass, 0.0, scroll, 1.0);
            }
            let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
            let mut changed = 0;
            for y in 0..1080 {
                for x in 0..1920 {
                    let offset = (y * 1920 + x) * 4;
                    let drawn = pixels[offset..offset + 3].iter().any(|c| *c != 0);
                    if drawn {
                        changed += 1;
                        assert!(
                            (136..1784).contains(&x) && (188..918).contains(&y),
                            "Text escaped native viewport: {x},{y}"
                        );
                    }
                }
            }
            if scroll < normal.height {
                assert!(changed > 10000, "No visible backlog glyphs: {changed}");
            } else {
                assert_eq!(changed, 0, "Scrolled-out message must leave viewport empty");
            }
            if mode == 1 {
                assert_ne!(
                    baseline.as_ref().unwrap(),
                    &pixels,
                    "Outline mode1 must shift browser border"
                );
            }
            if scroll == 0.0 && mode == 0 {
                let mut ppm = b"P6\n1920 1080\n255\n".to_vec();
                for pixel in pixels.chunks_exact(4) {
                    ppm.extend_from_slice(&pixel[..3]);
                }
                std::fs::write("/tmp/shin-backlog-text.ppm", ppm).unwrap();
                baseline = Some(pixels);
            }
        }
        let textures = server
            .load_sync::<crate::adv::backlog::view::BacklogTextures>("/logview.txa")
            .unwrap();
        let mut shell = crate::adv::backlog::view::BacklogShell::default();
        shell.update(shin_core::time::Ticks::from_f32(10.0));
        {
            let mut pass = RenderPass::new(
                context.pipeline_storage,
                context.dynamic_buffer,
                context.sampler_store,
                context.device,
                context.encoder,
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &view,
                },
                Some(context.depth_stencil),
                None,
                "Backlog shell with text",
            );
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::BLACK),
                Some(0),
                Some(1.0),
            );
            shell.render_background(&textures, &mut pass);
            normal.render(&mut pass, 0.0, 0.0, 1.0);
            shell.render_foreground(&textures, &mut pass);
        }
        let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
        let mut ppm = b"P6\n1920 1080\n255\n".to_vec();
        for pixel in pixels.chunks_exact(4) {
            ppm.extend_from_slice(&pixel[..3]);
        }
        std::fs::write("/tmp/shin-backlog-message-browser.ppm", ppm).unwrap();
        use crate::adv::{
            assets::AdvFonts,
            backlog::{Backlog, BacklogEntry},
            backlog::screen::BacklogScreen,
        };
        let history = Backlog {
            entries: (0..80)
                .map(|i| BacklogEntry {
                    save_position: i,
                    item: match i {
                        75..=78 => BacklogItem::Quiz {
                            argument: 17,
                            mode: Some(i as i32 - 75),
                            response: if i == 77 { 27 } else { 0 },
                        },
                        79 => BacklogItem::Checkpoint {
                            title: "第八話".into(),
                            text: "最後の選択".into(),
                        },
                        _ => BacklogItem::Message {
                            message_id: i,
                            text: format!("戦人@r「メッセージ{}」@vvoice_{i}.", i),
                            messagebox_type: shin_core::vm::command::types::MessageboxType::Neutral,
                        },
                    },
                })
                .collect(),
        };
        let textures = server
            .load_sync::<crate::adv::backlog::view::BacklogTextures>("/logview.txa")
            .unwrap();
        let fonts = AdvFonts {
            system_font: regular.clone(),
            medium_font: regular.clone(),
            bold_font: bold.clone(),
        };
        let mut browser = BacklogScreen::new(
            history,
            fonts,
            textures,
            server.load_sync("/systex.txa").unwrap(),
            79,
            0,
        )
        .unwrap();
        assert!(
            !browser.navigate(-4),
            "Opening browser must consume navigation"
        );
        assert_eq!(browser.cached_row_count(), 0);
        browser.update(shin_core::time::Ticks::from_f32(10.0));
        browser.prepare(&context);
        assert!(
            (1..10).contains(&browser.cached_row_count()),
            "An80-row history must allocate glyphs only near the visible rows"
        );
        assert_eq!(browser.model.selected, Some(79));
        let initial_scroll = browser.model.scroll();
        let mut frames = Vec::new();
        assert_eq!(
            browser.request_voice(),
            None,
            "Checkpoint has no replay voice"
        );
        for frame in 0..5 {
            if frame == 1 {
                assert!(browser.navigate(-4));
                for _ in 0..4 {
                    browser.update(shin_core::time::Ticks::from_f32(1.0));
                }
                browser.prepare(&context);
                assert_eq!(
                    browser.model.scroll(),
                    initial_scroll,
                    "A selected row already inside the viewport must not scroll"
                );
            } else if frame == 2 {
                assert!(browser.navigate(-4));
                for _ in 0..4 {
                    browser.update(shin_core::time::Ticks::from_f32(1.0));
                }
                browser.prepare(&context);
                assert!(browser.model.scroll() < initial_scroll);
            } else if frame == 3 {
                assert_eq!(browser.request_voice(), Some(vec!["voice_71".into()]));
                browser.update(shin_core::time::Ticks::from_f32(3.5));
                assert!((browser.voice_flash(71).unwrap() - 0.7).abs() < 1e-6);
            } else if frame == 4 {
                browser.update(shin_core::time::Ticks::from_f32(3.5));
                assert_eq!(browser.voice_flash(71), Some(0.0));
            }
            {
                let mut pass = RenderPass::new(
                    context.pipeline_storage,
                    context.dynamic_buffer,
                    context.sampler_store,
                    context.device,
                    context.encoder,
                    TextureTarget {
                        kind: TextureTargetKind::Screen,
                        view: &view,
                    },
                    Some(context.depth_stencil),
                    None,
                    "Saved-route browser rows",
                );
                pass.clear(
                    Some(shin_core::primitives::color::UnormColor::BLACK),
                    Some(0),
                    Some(1.0),
                );
                browser.render(&mut pass);
            }
            frames.push(crate::layer::user::readback_test_texture(
                &mut context,
                &target,
            ));
        }
        // Omit only the scrollbar; retain the actual document and cached rows.
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &view,
                },
                Some(context.depth_stencil),
                "Backlog no-overflow scrollbar control",
            );
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::BLACK),
                Some(0),
                Some(1.0),
            );
            browser.render_without_scrollbar(&mut pass);
        }
        let fitting = crate::layer::user::readback_test_texture(&mut context, &target);
        let mut changed = 0;
        for y in 188..918 {
            for x in 1788..1804 {
                let i = (y * 1920 + x) * 4;
                if fitting[i..i + 3] != frames[4][i..i + 3] {
                    changed += 1;
                }
            }
        }
        assert!(
            changed > 1000,
            "Backlog overflow must visibly render system scrollbar: {changed}"
        );
        let mut moved = 0;
        for y in 188..918 {
            for x in 1788..1804 {
                let i = (y * 1920 + x) * 4;
                if frames[0][i..i + 3] != frames[2][i..i + 3] {
                    moved += 1;
                }
            }
        }
        assert!(
            moved > 0,
            "Backlog thumb must move with the visible document"
        );
        eprintln!(
            "Backlog scrollbar: visible{changed} pixels, moved{moved} pixels; scrollbar-omission negative control passes"
        );
        assert_ne!(
            frames[0], frames[1],
            "Selection must move the focus even when scroll is unchanged"
        );
        assert_ne!(
            frames[1], frames[2],
            "Further navigation must move visible browser text"
        );
        assert_ne!(frames[2], frames[3], "Voice feedback must flash the icon");
        assert_eq!(
            frames[2], frames[4],
            "Completed feedback must return to the static icon"
        );
        let mut ppm = b"P6\n1920 1080\n255\n".to_vec();
        for pixel in frames[2].chunks_exact(4) {
            ppm.extend_from_slice(&pixel[..3]);
        }
        std::fs::write("/tmp/shin-backlog-focus-browser.ppm", ppm).unwrap();
        let mut source = shin_core::format::save::GameDataEntry {
            scenario_id: 1,
            random_seed: 12345,
            save_position: 1,
            route_data: Default::default(),
        };
        assert!(
            browser
                .prepare_rollback(&shin_core::format::save::GameData::from_entry(
                    source.clone()
                ))
                .is_err()
        );
        assert!(browser.confirmation_feedback_finished(71));
        source.save_position = 100;
        let (index, rollback) = browser
            .prepare_rollback(&shin_core::format::save::GameData::from_entry(
                source.clone(),
            ))
            .unwrap();
        assert_eq!(index, 71);
        assert_eq!(rollback.previous_position, 100);
        assert_eq!(source.save_position, 100);
        assert!(!browser.confirmation_feedback_finished(index));
        browser.update(shin_core::time::Ticks::from_f32(3.5));
        assert!(!browser.confirmation_feedback_finished(index));
        browser.prepare(&context);
        {
            let mut pass = RenderPass::new(
                context.pipeline_storage,
                context.dynamic_buffer,
                context.sampler_store,
                context.device,
                context.encoder,
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &view,
                },
                Some(context.depth_stencil),
                None,
                "Whole-row rollback confirmation feedback",
            );
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::BLACK),
                Some(0),
                Some(1.0),
            );
            browser.render(&mut pass);
        }
        let flashed = crate::layer::user::readback_test_texture(&mut context, &target);
        let row = browser.model.rows[index];
        let top = 188.0 + row.top - browser.model.scroll();
        let mut checked = 0;
        for y in (1..1080).step_by(7) {
            for x in (1..1920).step_by(11) {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let active = (136.0..1784.0).contains(&px)
                    && (188.0..918.0).contains(&py)
                    && (top..top + row.height).contains(&py);
                let alpha = if active { 178.0 / 255.0 } else { 0.0 };
                for channel in 0..3 {
                    let offset = (y * 1920 + x) * 4 + channel;
                    let expected = frames[4][offset] as f32 * (1.0 - alpha) + 255.0 * alpha;
                    assert!(
                        (flashed[offset] as f32 - expected).abs() <= 2.0,
                        "Browser whole-row flash {x},{y} channel{channel}"
                    );
                }
                checked += 1;
            }
        }
        let mut ppm = b"P6\n1920 1080\n255\n".to_vec();
        for pixel in flashed.chunks_exact(4) {
            ppm.extend_from_slice(&pixel[..3]);
        }
        std::fs::write("/tmp/shin-backlog-confirmation-row.ppm", ppm).unwrap();
        browser.update(shin_core::time::Ticks::from_f32(3.5));
        assert!(browser.confirmation_feedback_finished(index));
        println!(
            "Browser confirmation preparation and whole-row overlay: {checked} scalar samples"
        );
    }
    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
    fn reference_native_backlog_text_layout_and_fonts() {
        let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&path)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0x65c98, 0xf9442d08u32),
            (0x65d58, 0xf9443108u32),
            (0xa3638, 0xd2a84409u32),
            (0xa3648, 0x790073e8u32),
            (0xa364c, 0x3900ebe8u32),
            (0xa3650, 0xa902a7ffu32),
            (0xa3654, 0xd2a81809u32),
            (0xa3658, 0x52a89728u32),
            (0xa365c, 0xf2e81009u32),
            (0xa3694, 0x9400421bu32),
            (0xac2d4, 0x52860201u32),
            (0xac338, 0x52860221u32),
            (0xac358, 0x94001f1eu32),
            (0xac4a8, 0x94001ecau32),
            (0xac644, 0xb9402008u32),
            (0xac660, 0x54000241u32),
            (0xa4f64, 0x4eab1d60u32),
            (0xa5000, 0x1e249001u32),
            (0xa484c, 0x1e2d28a5u32),
            (0xa4954, 0x1e2119c4u32),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native LogMessage {address:x}"
            );
        }
        for (address, text) in [
            (0x15a979, "/newrodin-medium.fnt"),
            (0x161e9c, "/newrodin-bold.fnt"),
            (0x161eb9, "@r"),
        ] {
            assert_eq!(
                &exe[address + 256..address + 256 + text.len()],
                text.as_bytes()
            );
        }
        assert_eq!(
            f32::from_le_bytes(exe[0x159440 + 256..0x159440 + 260].try_into().unwrap()),
            -188.0
        );
        assert_eq!(
            f32::from_le_bytes(exe[0x159340 + 256..0x159340 + 260].try_into().unwrap()),
            136.0
        );
        assert_eq!(
            u32::from_le_bytes(exe[0x16dd80 + 256..0x16dd80 + 260].try_into().unwrap()),
            977
        );
    }
    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
    fn reference_native_backlog_quiz_and_checkpoint_labels() {
        let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&path)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0xab9cc, 0xf9106928u32),
            (0xabb18, 0x321f03e9u32),
            (0xabb28, 0xa901e119u32),
            (0xabfd8, 0x97fe227au32),
            (0xac004, 0xb98006a9u32),
            (0xac014, 0xb940c519u32),
            (0xac0f8, 0x320007e9u32),
            (0xac108, 0x29035119u32),
            (0xa3830, 0x71000d1fu32),
            (0xa3834, 0x54001e88u32),
            (0xa385c, 0x9401e4c9u32),
            (0xa386c, 0xf8697901u32),
            (0xa387c, 0xf00005e1u32),
            (0xa3a80, 0x913f6421u32),
            (0xa3c20, 0x913be421u32),
            (0xa3c30, 0x71006d1fu32),
            (0xa3c3c, 0x91044129u32),
            (0xa3c54, 0x910cb821u32),
            (0xa3c68, 0x9107c108u32),
            (0xa3c80, 0x91115c21u32),
            (0xa3c94, 0x91364c21u32),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native backlog quiz/checkpoint {address:x}"
            );
        }
        // kind 1 SELECT -> a387c (bracketed), kind 2 AUTOSAVE -> a37dc.
        assert_eq!(&exe[0x171476 + 256..0x17147a + 256], &[0, 56, 16, 36]);
        for (index, color) in [977u32, 799, 977, 979].into_iter().enumerate() {
            let offset = 0x16dd80 + 256 + index * 4;
            assert_eq!(
                u32::from_le_bytes(exe[offset..offset + 4].try_into().unwrap()),
                color
            );
        }
        let relocations: std::collections::HashMap<_, _> = exe[0x11e040 + 256..0x152eb8 + 256]
            .chunks_exact(24)
            .map(|bytes| {
                (
                    u64::from_le_bytes(bytes[..8].try_into().unwrap()),
                    u64::from_le_bytes(bytes[16..24].try_into().unwrap()),
                )
            })
            .collect();
        for (start, labels) in [
            (0x1da0d8u64, QUIZ_ANSWERS.as_slice()),
            (0x1da110, MURDERER_ANSWERS.as_slice()),
            (0x1da1f0, ["魔法", "手品"].as_slice()),
        ] {
            for (index, label) in labels.iter().enumerate() {
                let address = relocations[&(start + index as u64 * 8)] as usize + 256;
                assert_eq!(
                    &exe[address..address + label.len()],
                    label.as_bytes(),
                    "authored browser label {index}"
                );
                assert_eq!(exe[address + label.len()], 0);
            }
        }
        for (address, text) in [
            (0x15d443, "@r《クイズ》@r"),
            (0x15d457, "@r《ケーキ選択》"),
            (0x161ef9, "@r《犯人当て》@r"),
            (0x15e32e, "@r《最後の選択》@r"),
            (0x162d93, "犯人特定：失敗"),
            (0x161eb9, "@r"),
            (0x162160, "@r《"),
            (0x15dfd9, "》@r"),
        ] {
            assert_eq!(
                &exe[address + 256..address + 256 + text.len()],
                text.as_bytes()
            );
        }
    }
}
