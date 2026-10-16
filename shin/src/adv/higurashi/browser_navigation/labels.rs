//! Original7c31c visible-label cache and d19b4/d2240 SimpleText packets.
//! The owning view supplies the list's stencil mask and decorative panel.
use super::Entry;
use crate::{
    adv::higurashi::{
        browser_history::{Item, Row},
        message_glyphs::glyph_vertices,
    },
    asset::font::{GpuFontGlyphHandle, GpuFontLazy},
    layer::{
        message_layer::outline,
        render_params::{higurashi::HigurashiTransform, multiply_transforms as multiply},
    },
};
use anyhow::{Context, Result};
use glam::{Mat4, Vec2, Vec4, vec2, vec3, vec4};
use shin_core::{
    layout::{
        HigurashiTextLayouter, LayoutParams, LineInfo, MessageTextLayouterDefaults,
        commands::Command, font::FontMetrics,
    },
    primitives::color::FloatColor4,
};
use shin_render::{
    DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, vertices::TextVertex},
};
use std::sync::Arc;

pub struct Label {
    pub index: usize,
    pub text: String,
    pub tint: Vec4,
}
impl Label {
    pub fn new(index: usize, entry: &Entry, rows: &[Row]) -> Result<Self> {
        let blue = vec4(0.875, 0.875, 1.0, 1.0);
        let green = vec4(0.75, 1.0, 0.75, 1.0);
        let (text, tint) = match entry.kind {
            0 => ("先頭".into(), blue),
            97 => ("カーソル位置".into(), green),
            98 => ("ゲーム内現在位置".into(), green),
            99 => ("カーソル位置/ゲーム内現在位置".into(), green),
            100 => ("後尾".into(), blue),
            1 => match &rows
                .get(entry.marker as usize)
                .context("Navigation label marker exceeds history")?
                .item
            {
                Item::Choice { title, .. } => (title.clone(), vec4(1.0, 0.75, 0.75, 1.0)),
                Item::Checkpoint { title, text } if !text.is_empty() => {
                    (format!("{title} {text}"), vec4(0.75, 1.0, 1.0, 1.0))
                }
                Item::Checkpoint { title, .. } => (title.clone(), vec4(0.75, 1.0, 1.0, 1.0)),
                Item::CharacterChoice { .. } => ("ヒロイン選択".into(), vec4(1.0, 0.75, 0.75, 1.0)),
                Item::Message { .. } => (">>謎<<".into(), Vec4::ONE),
            },
            _ => (">>謎<<".into(), Vec4::ONE),
        };
        Ok(Self { index, text, tint })
    }
    /// Native navigation treats the parent's final selectable row inclusively.
    pub fn fill(&self, selected: usize, row: i32, first: usize, end: usize) -> Vec4 {
        let mut tint = self.tint;
        if self.index != selected {
            tint *= vec4(
                f32::from_bits(0x3f333333),
                f32::from_bits(0x3f333333),
                f32::from_bits(0x3f333333),
                1.0,
            );
        }
        if row < first as i32 || row > end as i32 {
            tint *= vec4(1.0, 1.0, 1.0, 0.5);
        }
        tint
    }
}
pub fn visible(index: usize, scroll: f32) -> bool {
    let y = (index * 60) as f32 - scroll;
    y + 60.0 > 0.0 && y < 720.0
}
pub struct Cached<T> {
    pub label: Label,
    pub value: T,
}
/// Keep native cache order: append missing visible entries, then retire every
/// invisible owner. Revisiting an evicted entry creates a fresh glyph owner.
pub struct Cache<T> {
    pub entries: Vec<Cached<T>>,
}
impl<T> Default for Cache<T> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
}
impl<T> Cache<T> {
    pub fn synchronize(
        &mut self,
        entries: &[Entry],
        rows: &[Row],
        scroll: f32,
        mut prepare: impl FnMut(&Label) -> Result<T>,
    ) -> Result<()> {
        for (index, entry) in entries
            .iter()
            .enumerate()
            .filter(|(i, _)| visible(*i, scroll))
        {
            if !self.entries.iter().any(|c| c.label.index == index) {
                let label = Label::new(index, entry, rows)?;
                let value = prepare(&label)?;
                self.entries.push(Cached { label, value });
            }
        }
        self.entries.retain(|c| visible(c.label.index, scroll));
        Ok(())
    }
}
pub struct Glyph {
    pub point: char,
    pub vertices: [TextVertex; 4],
    pub border: [Vec2; 8],
}
pub struct Layout {
    pub size: Vec2,
    pub glyphs: Vec<Glyph>,
}
impl Layout {
    pub fn new(font: impl FontMetrics + Clone, text: &str) -> Result<Self> {
        Self::with_params(
            font,
            text,
            LayoutParams {
                layout_width: 1276.0,
                text_size: 48.0,
                perform_soft_breaks: false,
                ..Default::default()
            },
        )
    }
    pub fn with_params(
        font: impl FontMetrics + Clone,
        text: &str,
        params: LayoutParams,
    ) -> Result<Self> {
        let text_size = params.text_size;
        let (commands, lines, size) = HigurashiTextLayouter::new(
            font.clone(),
            params,
            MessageTextLayouterDefaults {
                color: 0xfff,
                draw_speed: 50,
                fade: 200,
            },
        )
        .parse_plain(text, false);
        Self::from_commands(font, commands, lines, size, text_size)
    }
    /// d182c builds a MessageTextParser before d19b4; notification and other
    /// authored SimpleText owners retain its control syntax and character fixups.
    pub fn from_script(
        font: impl FontMetrics + Clone,
        text: &str,
        params: LayoutParams,
    ) -> Result<Self> {
        let text_size = params.text_size;
        let (commands, lines, size) = HigurashiTextLayouter::new(
            font.clone(),
            params,
            MessageTextLayouterDefaults {
                color: 0xfff,
                draw_speed: 50,
                fade: 200,
            },
        )
        .parse(text);
        Self::from_commands(font, commands, lines, size, text_size)
    }
    /// d19f4/d1a08 selects zero origin/size padding when the SimpleText
    /// outline flag is zero. d1f30..d1f6c then selects padding=-1, insetting
    /// each glyph by one logical pixel.
    pub fn unpadded(font: impl FontMetrics + Clone, text: &str, params: LayoutParams) -> Result<Self> {
        let text_size = params.text_size;
        let (commands, _, size) = HigurashiTextLayouter::new(
            font.clone(),
            params,
            MessageTextLayouterDefaults {
                color: 0xfff,
                draw_speed: 50,
                fade: 200,
            },
        )
        .parse(text);
        let mut glyphs = Vec::new();
        for command in commands {
            let Command::Char(c) = command else {
                continue;
            };
            let info = font
                .get_glyph_info(c.codepoint)
                .context("Label glyph missing from font")?;
            if info.actual_width == 0 || info.actual_height == 0 {
                continue;
            }
            let scale = vec2(c.horizontal_scale, c.scale);
            let scaled = scale * info.actual_size_f32();
            let local_top = scale * info.bearing_screenspace_f32() - -1.0;
            let local_bottom = (local_top + scaled) + -2.0;
            let ratio = (local_bottom - local_top) / scaled + -1.0;
            let normalized = info.actual_size_normalized();
            let uv_top = normalized * (ratio * -0.5);
            let uv_bottom = normalized * (ratio * 0.5 + 1.0);
            let top = c.position + local_top;
            let bottom = c.position + local_bottom;
            let (color_top, color_bottom) = if c.is_rubi {
                (0.0, 0.0)
            } else {
                // d2054 uses the unscaled screen-space bearing, then adds height
                // before float conversion. Ruby's two tint weights remain zero.
                let reciprocal = 1.0 / text_size;
                (
                    (-(info.bearing_y as i32)) as f32 * reciprocal,
                    (info.actual_height as i32 - info.bearing_y as i32) as f32 * reciprocal,
                )
            };
            glyphs.push(Glyph {
                point: c.codepoint,
                vertices: [
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
                ],
                border: outline::distances_with_radius(
                    scale,
                    info.actual_size_f32(),
                    normalized,
                    0,
                    1.5,
                ),
            });
        }
        Ok(Self { size, glyphs })
    }
    fn from_commands(
        font: impl FontMetrics,
        commands: Vec<Command>,
        lines: Vec<LineInfo>,
        size: Vec2,
        text_size: f32,
    ) -> Result<Self> {
        let mut glyphs = Vec::new();
        for command in commands {
            let Command::Char(mut c) = command else {
                continue;
            };
            let info = font
                .get_glyph_info(c.codepoint)
                .context("Navigation glyph missing from font")?;
            if info.actual_width == 0 || info.actual_height == 0 {
                continue;
            }
            // d1c20/d1c28 add padding to the laid-out character position
            // before d1f24 builds vertices. Adding it to final vertices loses
            // a bit at some fractional48px glyph positions.
            c.position += Vec2::splat(2.0);
            let mut vertices = glyph_vertices(&c, &lines[c.line_index], &info);
            // Native d2048 leaves both tint weights at zero for ruby. The
            // shared message vertices already contain that flat-color form.
            if !c.is_rubi {
                let reciprocal = 1.0 / text_size;
                let top = -(info.bearing_y as f32) * reciprocal;
                let bottom =
                    (info.actual_height as i32 - info.bearing_y as i32) as f32 * reciprocal;
                vertices[0].color = top;
                vertices[1].color = top;
                vertices[2].color = bottom;
                vertices[3].color = bottom;
            }
            glyphs.push(Glyph {
                point: c.codepoint,
                vertices,
                border: outline::distances_with_radius(
                    vec2(c.horizontal_scale, c.scale),
                    info.actual_size_f32(),
                    info.actual_size_normalized(),
                    0,
                    1.5,
                ),
            });
        }
        Ok(Self {
            size: size + Vec2::splat(4.0),
            glyphs,
        })
    }
    pub fn native_transform(&self, index: usize, scroll: f32, slide: f32) -> Mat4 {
        // 74d34 divides directly; keep its signed zero and serial matrix FMAs.
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
        let parent = multiply(
            projection,
            Mat4::from_translation(vec3(1920.0 - slide * 1920.0, 0.0, 0.0)),
        );
        let list = multiply(
            parent,
            Mat4::from_translation(vec3(0.0, -(scroll - 240.0), 0.0)),
        );
        let anchor = Mat4::from_translation(vec3(-self.size.x, 0.0, 0.0));
        multiply(
            list,
            multiply(
                Mat4::from_translation(vec3(1800.0, ((index as i32 * 60) | 3) as f32, 0.0)),
                multiply(Mat4::from_scale(vec3(1.0, 1.0, 0.0)), anchor),
            ),
        )
    }
}
pub struct Glyphs {
    pub layout: Layout,
    handles: Vec<GpuFontGlyphHandle>,
}
impl Glyphs {
    pub fn prepare(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        font: Arc<GpuFontLazy>,
        label: &Label,
    ) -> Result<Self> {
        let layout = Layout::new(font.clone(), &label.text)?;
        Ok(Self::from_layout(device, queue, font, layout))
    }
    pub fn from_layout(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        font: Arc<GpuFontLazy>,
        layout: Layout,
    ) -> Self {
        let points: Vec<_> = layout.glyphs.iter().map(|g| g.point).collect();
        let handles = font.load_glyphs(device.clone(), queue.clone(), &points);
        Self { layout, handles }
    }
    /// Draw inside the owner's active list mask. Geometry stays in the1920x1080
    /// logical canvas; the render target scales it for720p or1080p output.
    pub fn render(
        &self,
        pass: &mut RenderPass,
        index: usize,
        scroll: f32,
        slide: f32,
        fill: Vec4,
        builder: RenderRequestBuilder,
    ) {
        self.render_at(
            pass,
            self.layout.native_transform(index, scroll, slide),
            fill,
            vec4(0.0, 0.0, 0.0, 240.0 * f32::from_bits(0x3b808081)),
            builder,
        );
    }
    pub fn render_at(
        &self,
        pass: &mut RenderPass,
        native: Mat4,
        fill: Vec4,
        border_color: Vec4,
        builder: RenderRequestBuilder,
    ) {
        self.render_gradient_at(pass, native, fill, fill, border_color, builder);
    }
    pub fn render_gradient_at(
        &self,
        pass: &mut RenderPass,
        native: Mat4,
        fill1: Vec4,
        fill2: Vec4,
        border_color: Vec4,
        builder: RenderRequestBuilder,
    ) {
        let transform = HigurashiTransform::convert_clip_depth(native);
        for border in [true, false] {
            if (if border {
                border_color.w
            } else {
                fill1.w.max(fill2.w)
            }) <= 0.0
            {
                continue;
            }
            for (glyph, handle) in self.layout.glyphs.iter().zip(&self.handles) {
                let Ok(texture) = handle.as_texture_source() else {
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
                        color: FloatColor4::from_rgba(
                            border_color.x,
                            border_color.y,
                            border_color.z,
                            border_color.w,
                        ),
                    }
                } else {
                    RenderProgramWithArguments::Font {
                        vertices,
                        glyph: texture,
                        transform,
                        color1: FloatColor4::from_vec4(fill1),
                        color2: FloatColor4::from_vec4(fill2),
                    }
                };
                pass.run(builder.build(program, DrawPrimitive::TrianglesStrip));
            }
        }
    }
}

#[cfg(test)]
mod tests;
