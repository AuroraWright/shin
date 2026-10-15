//! Older message glyph preparation/drawing. Playback clocks, windows and page
//! ownership supply this renderer's per-glyph progress and composed transform.
use std::sync::Arc;

use anyhow::{Result, ensure};
use glam::{Vec2, vec2, vec4};
use shin_core::{
    format::font::GlyphInfo,
    layout::{
        HigurashiMessageLayouter, HigurashiMessageStyle, LineInfo,
        commands::{Char, Command},
    },
    primitives::color::FloatColor4,
};
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    render_pass::RenderPass,
    shaders::types::{
        buffer::{OwnedVertexBuffer, VertexSource},
        vertices::TextVertex,
    },
};

use crate::{
    asset::{
        font::{GpuFontGlyphHandle, GpuFontLazy},
        system::AssetServer,
    },
    layer::{message_layer::outline, render_params::higurashi::HigurashiTransform},
};

pub fn glyph_vertices(character: &Char, line: &LineInfo, glyph: &GlyphInfo) -> [TextVertex; 4] {
    let scale = vec2(character.horizontal_scale, character.scale);
    let pixels = glyph.actual_size_f32();
    let scaled = scale * pixels;
    let top = scale * glyph.bearing_screenspace_f32() - 2.0;
    let bottom = top + scaled + 4.0;
    let ratio = (bottom - top) / scaled - 1.0;
    let uv = glyph.actual_size_normalized();
    let uv_top = uv * (ratio * -0.5);
    let uv_bottom = uv * (ratio * 0.5 + 1.0);
    let top = character.position + top;
    let bottom = character.position + bottom;
    let (color_top, color_bottom) = if character.is_rubi {
        (0.0, 0.0)
    } else {
        // Native83730 deliberately uses unscaled glyph metrics for the tint.
        let reciprocal = 1.0 / line.line_height;
        let ascent = line.baseline_ascent - glyph.bearing_y as f32;
        (ascent * reciprocal, (ascent + pixels.y) * reciprocal)
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

pub fn message_offset(
    style: HigurashiMessageStyle,
    options: [i32; 2],
    natural_slide: f32,
    height: f32,
    size: Vec2,
) -> Vec2 {
    match style {
        HigurashiMessageStyle::Window | HigurashiMessageStyle::Mode2 => vec2(
            210.0,
            ((1080.0 - height) - 32.0) + (1.0 - natural_slide) * 64.0,
        ),
        HigurashiMessageStyle::Mode1 => vec2(210.0, ((1080.0 - size.y) * 0.35).max(32.0)),
        HigurashiMessageStyle::Transparent | HigurashiMessageStyle::Novel => vec2(
            options[0].wrapping_mul(50) as f32 + 210.0,
            options[1].wrapping_mul(60) as f32 + 80.0,
        ),
    }
}

pub struct Glyph {
    pub character: Char,
    pub block_index: usize,
    pub border: [Vec2; 8],
    handle: GpuFontGlyphHandle,
}

pub struct HigurashiMessageGlyphs {
    pub style: HigurashiMessageStyle,
    pub glyphs: Vec<Glyph>,
    pub lines: Vec<LineInfo>,
    pub size: Vec2,
    pub blocks: Vec<Command>,
    pub position_options: [i32; 2],
    vertices: Option<OwnedVertexBuffer<TextVertex>>,
}

impl HigurashiMessageGlyphs {
    pub async fn load_font(assets: &AssetServer) -> Result<Arc<GpuFontLazy>> {
        assets.load("/newrodin.fnt").await
    }

    pub fn prepare(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        font: Arc<GpuFontLazy>,
        msginit_style: i32,
        position_options: [i32; 2],
        text_speed: i32,
        outline_mode: u8,
        text: &str,
    ) -> Self {
        let (commands, lines, size) =
            HigurashiMessageLayouter::for_message(font.clone(), msginit_style, text_speed)
                .parse(text);
        let characters: Vec<_> = commands
            .iter()
            .filter_map(|command| match command {
                Command::Char(c) => Some(c.codepoint),
                _ => None,
            })
            .collect();
        let mut handles = font
            .load_glyphs(device.clone(), queue.clone(), &characters)
            .into_iter();
        let mut glyphs = Vec::with_capacity(characters.len());
        let mut vertices = Vec::with_capacity(characters.len() * 4);
        let mut blocks = Vec::new();
        for command in commands {
            match command {
                Command::Char(character) => {
                    let handle = handles.next().unwrap();
                    let info = handle.info();
                    let border = outline::distances_with_radius(
                        vec2(character.horizontal_scale, character.scale),
                        info.actual_size_f32(),
                        info.actual_size_normalized(),
                        outline_mode,
                        1.5,
                    );
                    vertices.extend(glyph_vertices(
                        &character,
                        &lines[character.line_index],
                        info,
                    ));
                    glyphs.push(Glyph {
                        character,
                        block_index: blocks.len(),
                        border,
                        handle,
                    });
                }
                block => blocks.push(block),
            }
        }
        Self {
            style: HigurashiMessageStyle::from_msginit(msginit_style),
            glyphs,
            lines,
            size,
            blocks,
            position_options,
            vertices: (!vertices.is_empty()).then(|| {
                OwnedVertexBuffer::allocate_vertex(
                    device,
                    &vertices,
                    Some("Higurashi message glyph vertices"),
                )
            }),
        }
    }

    pub fn offset(&self, natural_slide: f32, height: f32) -> Vec2 {
        message_offset(
            self.style,
            self.position_options,
            natural_slide,
            height,
            self.size,
        )
    }

    /// Window/cursor rendering and reveal/wait clocks are independent owners.
    pub fn render(
        &self,
        pass: &mut RenderPass,
        parent: &HigurashiTransform,
        stencil: u8,
        natural_slide: f32,
        modal_slide: f32,
        height: f32,
        progress: &[f32],
    ) -> Result<()> {
        ensure!(
            progress.len() == self.glyphs.len(),
            "Higurashi glyph progress count mismatch"
        );
        if natural_slide * modal_slide < 1.0 || self.style == HigurashiMessageStyle::Mode2 {
            return Ok(());
        }
        let transform = parent.compute_message_transform(self.offset(natural_slide, height));
        let builder = RenderRequestBuilder::new()
            .color_blend_type(ColorBlendType::Layer1)
            .depth_stencil_shorthand(stencil + 2, true, true);
        for border in [true, false] {
            if border && self.style == HigurashiMessageStyle::Novel {
                continue;
            }
            for (index, (glyph, &progress)) in self.glyphs.iter().zip(progress).enumerate() {
                if progress <= 0.0 {
                    continue;
                }
                let texture = glyph.handle.as_texture_source()?;
                let vertices = VertexSource::VertexBuffer {
                    vertices: self
                        .vertices
                        .as_ref()
                        .unwrap()
                        .as_sliced_buffer_ref(index * 4, 4),
                };
                let program = if border {
                    RenderProgramWithArguments::FontBorder {
                        vertices,
                        glyph: texture,
                        transform,
                        distances: glyph.border,
                        color: FloatColor4::from_rgba(0.0, 0.0, 0.0, progress),
                    }
                } else {
                    let color = FloatColor4::from_unorm(glyph.character.color).with_alpha(progress);
                    RenderProgramWithArguments::Font {
                        vertices,
                        glyph: texture,
                        transform,
                        color1: color,
                        color2: color,
                    }
                };
                pass.run(builder.build(program, DrawPrimitive::TrianglesStrip));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
pub(super) mod tests;
