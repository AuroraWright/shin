//! Native SaveMenu row text d6a14..d6f60 (generic outlined style1).
use super::character_text::{CharacterText, CharacterTextGlyph, CharacterTextLayout};
use crate::asset::{font::GpuFontLazy, system::AssetServer};
use glam::{Mat4, Vec2, vec2, vec4};
use shin_core::{
    layout::{
        LayoutParams, MessageTextLayouter, MessageTextLayouterDefaults, commands::Command,
        font::FontMetrics,
    },
    primitives::color::FloatColor4,
};
use shin_render::{RenderRequestBuilder, render_pass::RenderPass};
use std::sync::Arc;

pub const EMPTY_SLOT: &str = "データがありません";
pub const LOADING_SLOT: &str = "loading...";
pub const TEXT_WIDTH: f32 = 1272.0;
pub const TEXT_SIZE: f32 = 40.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveSlotLabels {
    Empty,
    Loading,
    Ready { title: String, subtitle: String },
}

pub fn layout(labels: &SaveSlotLabels, font: &impl FontMetrics) -> CharacterTextLayout {
    let lines: Vec<(&str, f32)> = match labels {
        SaveSlotLabels::Empty => vec![(EMPTY_SLOT, 87.0)],
        SaveSlotLabels::Loading => vec![(LOADING_SLOT, 87.0)],
        SaveSlotLabels::Ready { title, subtitle } => vec![(title, 60.0), (subtitle, 114.0)],
    };
    let mut glyphs = Vec::new();
    let mut first_count = 0;
    for (index, (text, y)) in lines.into_iter().enumerate() {
        // Native style1 subtracts its six-pixel border from width1278.
        let (commands, _, _) = MessageTextLayouter::new(
            font,
            font,
            LayoutParams {
                layout_width: TEXT_WIDTH,
                text_size: TEXT_SIZE,
                rubi_size: TEXT_SIZE * 0.4,
                perform_soft_breaks: false,
                always_leave_space_for_rubi: false,
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
            let Command::Char(ch) = command else {
                continue;
            };
            let info = font.get_glyph_info(ch.codepoint).unwrap();
            if info.actual_width == 0 || info.actual_height == 0 {
                continue;
            }
            let scale = vec2(ch.horizontal_scale, ch.scale);
            let pixels = info.actual_size_f32() * scale;
            let uv = info.actual_size_normalized();
            let radius = uv * (Vec2::splat(3.0) / pixels);
            let pos = vec2(317.0, y) + ch.position + info.bearing_screenspace_f32() * scale;
            let color = FloatColor4::from_unorm(ch.color);
            glyphs.push(CharacterTextGlyph {
                codepoint: ch.codepoint,
                rect: vec4(pos.x, pos.y, pixels.x + 6.0, pixels.y + 6.0),
                uv: vec4(-radius.x, -radius.y, uv.x + radius.x, uv.y + radius.y),
                distances: [
                    vec2(-0.707, -0.707),
                    vec2(0.0, -1.0),
                    vec2(0.707, -0.707),
                    vec2(-1.0, 0.0),
                    vec2(1.0, 0.0),
                    vec2(-0.707, 0.707),
                    vec2(0.0, 1.0),
                    vec2(0.707, 0.707),
                ]
                .map(|d| d * radius),
                color: color,
            });
        }
        if index == 0 {
            first_count = glyphs.len();
        }
    }
    CharacterTextLayout {
        glyphs,
        name_glyph_count: first_count,
        name_height: 0.0,
        height: 172.0,
    }
}

pub struct SaveSlotText {
    pub labels: SaveSlotLabels,
    text: CharacterText,
}
impl SaveSlotText {
    pub fn load(server: &AssetServer, labels: SaveSlotLabels) -> anyhow::Result<Self> {
        let font: Arc<GpuFontLazy> = server.load_sync("/system.fnt")?;
        let text = CharacterText::from_layout(server, font.clone(), layout(&labels, font.as_ref()));
        Ok(Self { labels, text })
    }
    pub fn render(&self, pass: &mut RenderPass, transform: Mat4, builder: RenderRequestBuilder) {
        self.text.render_with_builder(pass, transform, builder);
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
    fn native_empty_loading_and_two_line_labels_keep_their_baselines() {
        for labels in [SaveSlotLabels::Empty, SaveSlotLabels::Loading] {
            let result = layout(&labels, &Metrics);
            assert!(!result.glyphs.is_empty());
            assert_eq!(result.name_glyph_count, result.glyphs.len());
            assert_eq!(result.glyphs[0].rect, vec4(317.5, 87.0, 25.0, 44.0));
        }
        let result = layout(
            &SaveSlotLabels::Ready {
                title: "AB".into(),
                subtitle: "CD".into(),
            },
            &Metrics,
        );
        assert_eq!(result.name_glyph_count, 2);
        assert_eq!(result.glyphs.len(), 4);
        assert_eq!(result.glyphs[0].rect.y, 60.0);
        assert_eq!(result.glyphs[2].rect.y, 114.0);
        assert!(result.glyphs.iter().all(|g| g.uv.x < 0.0 && g.uv.y < 0.0));
    }
    #[test]
    fn long_titles_shrink_to_native_width_without_wrapping() {
        let result = layout(
            &SaveSlotLabels::Ready {
                title: "名".repeat(100),
                subtitle: "章".into(),
            },
            &Metrics,
        );
        assert_eq!(result.name_glyph_count, 100);
        assert!(
            result.glyphs[..100]
                .iter()
                .all(|g| g.rect.y == 60.0 && g.rect.x + g.rect.z <= 1595.0)
        );
        assert_eq!(result.glyphs[100].rect.y, 114.0);
    }
}
