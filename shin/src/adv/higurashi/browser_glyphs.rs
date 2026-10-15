//! Updated77254 MessageBrowser row glyph preparation, retained visible cache
//! and font packets. Decorative browser UI and voice transport are separate.
use std::{collections::BTreeMap, sync::Arc};

use super::browser_history::{Item, Row};
use crate::{
    asset::font::{GpuFontGlyphHandle, GpuFontLazy},
    layer::{
        message_layer::outline,
        render_params::{higurashi::HigurashiTransform, multiply_transforms},
    },
};
use anyhow::Result;
use glam::{Mat4, Vec2, vec2, vec3, vec4};
use shin_core::{
    format::font::GlyphInfo,
    layout::{
        HigurashiBacklogLayout, HigurashiBacklogLayouter, HigurashiBacklogVoice,
        HigurashiBacklogVoices,
        commands::{Char, Command},
        font::FontMetrics,
    },
    primitives::color::{FloatColor4, UnormColor},
};
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, vertices::TextVertex},
};

/// Original77608 uses white dialogue, fcc choices, cff checkpoints and fcc
/// heroine choices. The MSGSET argument does not supply the glyph fill color.
pub fn layout<Font: FontMetrics + Clone>(
    row: &Row,
    font: Font,
    voices: HigurashiBacklogVoices,
) -> Result<HigurashiBacklogLayout> {
    let (style, color) = match &row.item {
        Item::Message { style, .. } => (*style, 0xfff),
        Item::Choice { .. } | Item::CharacterChoice { .. } => (0, 0xfcc),
        Item::Checkpoint { .. } => (0, 0xcff),
    };
    Ok(HigurashiBacklogLayouter::new(font, style, color, voices).parse(&row.item.display_text()?))
}

pub struct PositionedRow {
    pub row: Row,
    pub top: f32,
    pub height: f32,
    pub voice_count: u8,
}
impl PositionedRow {
    /// Native worker height is the LogMessage extent plus36. The row's document
    /// position is supplied by the browser worker, whose anchor is independent.
    pub fn measure<Font: FontMetrics + Clone>(row: Row, top: f32, font: Font) -> Result<Self> {
        let measured = layout(&row, font, HigurashiBacklogVoices::Count(0))?;
        Ok(Self {
            row,
            top,
            height: measured.size.y + 36.0,
            voice_count: measured.voice_count,
        })
    }
    pub fn is_visible(&self, scroll: f32) -> bool {
        let relative = self.top - (scroll - 162.0);
        relative < 1080.0 && relative + self.height > 0.0
    }
}

/// Original776ec..77834:86-unit text inset,2px padding, and raw font metrics
/// for tint. Preserve the native floating point operation order.
pub fn glyph_vertices(
    character: &Char,
    glyph: &GlyphInfo,
    ascent: u32,
    descent: u32,
) -> [TextVertex; 4] {
    let scale = vec2(character.horizontal_scale, character.scale);
    let pixels = glyph.actual_size_f32();
    let scaled = scale * pixels;
    let bearing = scale * glyph.bearing_screenspace_f32();
    let top = vec2((bearing.x + 86.0) - 2.0, bearing.y - 2.0);
    let bottom = scaled + top + 4.0;
    let ratio = (bottom - top) / scaled - 1.0;
    let uv = glyph.actual_size_normalized();
    let uv_top = uv * (ratio * -0.5);
    let uv_bottom = uv * (ratio * 0.5 + 1.0);
    let top = character.position + top;
    let bottom = character.position + bottom;
    let (color_top, color_bottom) = if character.is_rubi {
        (0.0, 0.0)
    } else {
        let reciprocal = 1.0 / ascent.wrapping_add(descent) as f32;
        let bearing = (ascent as i32).wrapping_sub(i32::from(glyph.bearing_y)) as f32;
        (bearing * reciprocal, (bearing + pixels.y) * reciprocal)
    };
    [
        TextVertex {
            position: vec4(top.x, top.y, uv_top.x, uv_top.y),
            color: color_top,
        },
        TextVertex {
            position: vec4(bottom.x, top.y, uv_bottom.x, uv_top.y),
            color: color_top,
        },
        TextVertex {
            position: vec4(top.x, bottom.y, uv_top.x, uv_bottom.y),
            color: color_bottom,
        },
        TextVertex {
            position: vec4(bottom.x, bottom.y, uv_bottom.x, uv_bottom.y),
            color: color_bottom,
        },
    ]
}

pub fn native_transform(top: f32, scroll: f32, slide: f32) -> Mat4 {
    let projection =
        glam::camera::rh::proj::opengl::orthographic(0.0, 1920.0, 1080.0, 0.0, -1.0, 1.0);
    let parent = multiply_transforms(
        projection,
        Mat4::from_translation(vec3(1920.0 - slide * 1920.0, 0.0, 0.0)),
    );
    multiply_transforms(
        parent,
        Mat4::from_translation(vec3(197.0, (top + 18.0) - (scroll - 162.0), 0.0)),
    )
}

pub fn glyph_color(color: UnormColor) -> FloatColor4 {
    // Original4cccc multiplies bytes by the rounded reciprocal of255.
    let reciprocal = f32::from_bits(0x3b808081);
    FloatColor4::from_rgba(
        (color.0 & 255) as f32 * reciprocal,
        ((color.0 >> 8) & 255) as f32 * reciprocal,
        ((color.0 >> 16) & 255) as f32 * reciprocal,
        (color.0 >> 24) as f32 * reciprocal,
    )
}

pub struct BrowserGlyph {
    pub character: Char,
    pub vertices: [TextVertex; 4],
    pub border: [Vec2; 8],
    handle: GpuFontGlyphHandle,
}
pub struct BrowserGlyphs {
    pub glyphs: Vec<BrowserGlyph>,
    pub voices: Vec<HigurashiBacklogVoice>,
    dialogue: bool,
}
impl BrowserGlyphs {
    pub fn prepare(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        font: Arc<GpuFontLazy>,
        row: &Row,
        outline_mode: u8,
    ) -> Result<Self> {
        let parsed = layout(row, font.clone(), HigurashiBacklogVoices::Collect)?;
        let characters: Vec<_> = parsed
            .commands
            .into_iter()
            .filter_map(|c| match c {
                Command::Char(c) => Some(c),
                _ => None,
            })
            .collect();
        let codepoints: Vec<_> = characters.iter().map(|c| c.codepoint).collect();
        let handles = font
            .clone()
            .load_glyphs(device.clone(), queue.clone(), &codepoints);
        let glyphs = characters
            .into_iter()
            .zip(handles)
            .map(|(character, handle)| {
                let info = handle.info();
                let vertices =
                    glyph_vertices(&character, info, font.get_ascent(), font.get_descent());
                let border = outline::distances_with_radius(
                    vec2(character.horizontal_scale, character.scale),
                    info.actual_size_f32(),
                    info.actual_size_normalized(),
                    outline_mode,
                    1.5,
                );
                BrowserGlyph {
                    character,
                    vertices,
                    border,
                    handle,
                }
            })
            .collect();
        Ok(Self {
            glyphs,
            voices: parsed.voices,
            dialogue: matches!(row.item, Item::Message { .. }),
        })
    }
    pub fn render(&self, pass: &mut RenderPass, top: f32, scroll: f32, slide: f32) {
        let transform =
            HigurashiTransform::convert_clip_depth(native_transform(top, scroll, slide));
        let builder = RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1);
        for border in [true, false] {
            for glyph in &self.glyphs {
                let Ok(texture) = glyph.handle.as_texture_source() else {
                    continue;
                };
                let vertices = VertexSource::VertexData {
                    vertices: &glyph.vertices,
                };
                let program = if border {
                    RenderProgramWithArguments::FontBorder {
                        vertices,
                        glyph: texture,
                        transform,
                        distances: glyph.border,
                        color: FloatColor4::from_rgba(0.0, 0.0, 0.0, 1.0),
                    }
                } else {
                    let color = glyph_color(glyph.character.color);
                    RenderProgramWithArguments::Font {
                        vertices,
                        glyph: texture,
                        transform,
                        color1: if self.dialogue {
                            FloatColor4::from_rgba(1.0, 1.0, 1.0, 1.0)
                        } else {
                            color
                        },
                        color2: color,
                    }
                };
                pass.run(builder.build(program, DrawPrimitive::TrianglesStrip));
            }
        }
    }
}

#[derive(Default)]
pub struct BrowserGlyphCache {
    rows: BTreeMap<u32, BrowserGlyphs>,
    order: Vec<u32>,
}
impl BrowserGlyphCache {
    pub fn synchronize(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        font: Arc<GpuFontLazy>,
        rows: &[PositionedRow],
        scroll: f32,
        outline_mode: u8,
    ) -> Result<()> {
        self.rows.retain(|index, _| {
            rows.iter()
                .any(|r| r.row.index == *index && r.is_visible(scroll))
        });
        self.order.retain(|index| self.rows.contains_key(index));
        for row in rows.iter().filter(|r| r.is_visible(scroll)) {
            if let std::collections::btree_map::Entry::Vacant(entry) =
                self.rows.entry(row.row.index)
            {
                entry.insert(BrowserGlyphs::prepare(
                    device,
                    queue,
                    font.clone(),
                    &row.row,
                    outline_mode,
                )?);
                self.order.push(row.row.index);
            }
        }
        Ok(())
    }
    pub fn get(&self, index: u32) -> Option<&BrowserGlyphs> {
        self.rows.get(&index)
    }
    pub fn indices(&self) -> impl Iterator<Item = u32> + '_ {
        self.order.iter().copied()
    }
    pub fn len(&self) -> usize {
        self.rows.len()
    }
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
    pub fn render(&self, pass: &mut RenderPass, rows: &[PositionedRow], scroll: f32, slide: f32) {
        for index in self.indices() {
            if let Some(row) = rows.iter().find(|r| r.row.index == index) {
                self.rows[&index].render(pass, row.top, scroll, slide);
            }
        }
    }
}

#[cfg(test)]
mod tests;
