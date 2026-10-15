//! Later-engine MessageLayer glyph preparation and SDF passes.
//! Layout/style selection, message clocks and window/clipping remain caller-owned.
use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use glam::{Mat4, Vec2, vec2, vec4};
use shin_core::{
    format::font::gerokasu2::texture::GlyphTextureGeometry,
    layout::{
        LineInfo,
        commands::{CharFontType, Command},
    },
    primitives::color::{FloatColor4, UnormColor},
};
use shin_render::{
    DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    render_pass::RenderPass,
    shaders::types::{
        buffer::{OwnedVertexBuffer, VertexSource},
        vertices::TextVertex,
    },
};

use crate::asset::font::gerokasu2::{GpuGerokasu2Font, GpuGerokasu2GlyphHandle};

pub struct MessageGlyphDraw {
    pub vertices: [TextVertex; 4],
    pub outline_thresholds: Vec2,
    pub fill_thresholds: Vec2,
}

impl MessageGlyphDraw {
    pub fn new(
        glyph: &GlyphTextureGeometry,
        position: Vec2,
        scale: Vec2,
        baseline_ascent: f32,
        line_height: f32,
        is_ruby: bool,
    ) -> Result<Self> {
        ensure!(
            position.is_finite() && scale.is_finite() && scale.x >= 0.0 && scale.y > 0.0,
            "FNT6 message position/scale is invalid"
        );
        ensure!(
            baseline_ascent.is_finite()
                && line_height.is_finite()
                && line_height > 0.0
                && glyph.extra_metric.is_finite()
                && glyph.extra_metric > 0.0,
            "FNT6 message line/font metrics are invalid"
        );
        ensure!(
            glyph
                .dimensions
                .into_iter()
                .all(|n| n > 0 && n <= i32::MAX as u32),
            "FNT6 message glyph dimensions are invalid"
        );
        let pixels = vec2(glyph.dimensions[0] as f32, glyph.dimensions[1] as f32) * scale;
        // Native8d704/8d710 explicitly fuse bearing*scale-2. The far margin
        // adds scaled padded size first, then4, before adding position.
        let near = vec2(
            (glyph.origin[0] as f32).mul_add(scale.x, -2.0),
            (glyph.origin[1] as f32).mul_add(scale.y, -2.0),
        );
        let far = (pixels + near) + Vec2::splat(4.0);
        let ratio = (((far - near) / pixels) + Vec2::splat(-1.0)) * 0.5;
        let uv_scale = Vec2::from_array(glyph.scale);
        let uv_near = -(ratio * uv_scale);
        let uv_far = (ratio + Vec2::ONE) * uv_scale;
        let near = position + near;
        let far = position + far;
        let (top, bottom) = if is_ruby {
            (0.0, 0.0)
        } else {
            let inverse = 1.0 / line_height;
            let effective = baseline_ascent + glyph.origin[1] as f32;
            (
                effective * inverse,
                (effective + glyph.dimensions[1] as f32) * inverse,
            )
        };
        let vertices = [
            TextVertex {
                position: vec4(near.x, near.y, uv_near.x, uv_near.y),
                color: top,
            },
            TextVertex {
                position: vec4(far.x, near.y, uv_far.x, uv_near.y),
                color: top,
            },
            TextVertex {
                position: vec4(near.x, far.y, uv_near.x, uv_far.y),
                color: bottom,
            },
            TextVertex {
                position: vec4(far.x, far.y, uv_far.x, uv_far.y),
                color: bottom,
            },
        ];
        let center = (-4.0 / glyph.extra_metric) / scale.y + 0.0;
        let radius = (1.5 / glyph.extra_metric) / scale.y;
        let outline_thresholds = vec2((center - radius).max(-1.0), (radius + center).max(-1.0));
        let fill = (1.2 / glyph.extra_metric) / scale.y;
        let fill_thresholds = vec2((-fill).max(-1.0), fill.max(-1.0));
        Ok(Self {
            vertices,
            outline_thresholds,
            fill_thresholds,
        })
    }

    /// Native8f57c/8f660 skips nonpositive and unordered (NaN) fade values.
    pub fn colors(color: UnormColor, progress: f32) -> Option<[FloatColor4; 2]> {
        if !(progress > 0.0) {
            return None;
        }
        let inverse = f32::from_bits(0x3b808081); // native5007c/50098: multiply1/255
        let outline = FloatColor4::from_rgba(0.0, 0.0, 0.0, progress * 0.75);
        let fill = FloatColor4::from_rgba(
            (color.0 & 255) as f32 * inverse,
            ((color.0 >> 8) & 255) as f32 * inverse,
            ((color.0 >> 16) & 255) as f32 * inverse,
            progress, // Native overwrites packed alpha, including color0.
        );
        Some([outline, fill])
    }
}

struct PreparedCharacter {
    command_index: usize,
    draw: MessageGlyphDraw,
    glyph: GpuGerokasu2GlyphHandle,
    color: UnormColor,
    progress: f32,
}

/// Owns shared layout commands, cached FNT6 handles and one immutable quad buffer.
/// The ADV message controller supplies fade progress and a window transform.
pub struct Gerokasu2MessageText {
    commands: Vec<Command>,
    lines: Vec<LineInfo>,
    layout_size: Vec2,
    characters: Vec<PreparedCharacter>,
    vertex_buffer: Option<OwnedVertexBuffer<TextVertex>>,
}

impl Gerokasu2MessageText {
    pub fn layout_and_prepare(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        fonts: [&Arc<GpuGerokasu2Font>; 2],
        text: &str,
        window_mode: i32,
        alignment: shin_core::vm::command::types::MessageTextLayout,
        message_speed: u8,
    ) -> Result<Self> {
        let (commands, lines, layout_size) = shin_core::layout::gerokasu2::message_layouter(
            fonts[0].metrics(),
            fonts[1].metrics(),
            window_mode,
            alignment,
            message_speed,
        )
        .parse(text);
        Self::prepare(device, queue, fonts, commands, lines, layout_size)
    }

    pub fn prepare(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        fonts: [&Arc<GpuGerokasu2Font>; 2],
        commands: Vec<Command>,
        lines: Vec<LineInfo>,
        layout_size: Vec2,
    ) -> Result<Self> {
        let mut requested = [Vec::new(), Vec::new()];
        for command in &commands {
            if let Command::Char(c) = command {
                requested[font_index(c.font)].push(c.codepoint);
                ensure!(
                    c.line_index < lines.len(),
                    "FNT6 message character has no line"
                );
            }
        }
        let mut glyphs = [0, 1].map(|i| {
            fonts[i]
                .clone()
                .load_glyphs(device.clone(), queue.clone(), &requested[i])
                .into_iter()
        });
        let mut vertices = Vec::new();
        let mut characters = Vec::new();
        for (command_index, command) in commands.iter().enumerate() {
            if let Command::Char(c) = command {
                let glyph = glyphs[font_index(c.font)]
                    .next()
                    .context("FNT6 message glyph batch is incomplete")?;
                let line = &lines[c.line_index];
                let draw = MessageGlyphDraw::new(
                    glyph.geometry()?,
                    c.position,
                    vec2(c.horizontal_scale, c.scale),
                    line.baseline_ascent,
                    line.line_height,
                    c.is_rubi,
                )?;
                vertices.extend_from_slice(&draw.vertices);
                characters.push(PreparedCharacter {
                    command_index,
                    draw,
                    glyph,
                    color: c.color,
                    progress: 0.0,
                });
            }
        }
        let vertex_buffer = (!vertices.is_empty()).then(|| {
            OwnedVertexBuffer::allocate_vertex(
                device,
                &vertices,
                Some("Gerokasu2MessageText/vertices"),
            )
        });
        Ok(Self {
            commands,
            lines,
            layout_size,
            characters,
            vertex_buffer,
        })
    }

    pub fn character_count(&self) -> usize {
        self.characters.len()
    }

    pub fn layout_size(&self) -> Vec2 {
        self.layout_size
    }

    pub fn commands(&self) -> &[Command] {
        &self.commands
    }

    pub fn lines(&self) -> &[LineInfo] {
        &self.lines
    }

    pub fn character_command_index(&self, index: usize) -> Option<usize> {
        self.characters.get(index).map(|c| c.command_index)
    }

    pub fn set_progress(&mut self, index: usize, progress: f32) -> Result<()> {
        self.characters
            .get_mut(index)
            .context("FNT6 message character index is invalid")?
            .progress = progress;
        Ok(())
    }

    pub fn render(
        &self,
        pass: &mut RenderPass,
        builder: RenderRequestBuilder,
        transform: Mat4,
    ) -> Result<()> {
        let Some(buffer) = &self.vertex_buffer else {
            return Ok(());
        };
        // Original8f538..8f714 draws every visible outline before any fill.
        for outline in [true, false] {
            for (index, character) in self.characters.iter().enumerate() {
                let Some(colors) = MessageGlyphDraw::colors(character.color, character.progress)
                else {
                    continue;
                };
                let (color, thresholds) = if outline {
                    (colors[0], character.draw.outline_thresholds)
                } else {
                    (colors[1], character.draw.fill_thresholds)
                };
                pass.run(builder.build(
                    RenderProgramWithArguments::FontSdf {
                        vertices: VertexSource::VertexBuffer {
                            vertices: buffer.as_sliced_buffer_ref(index * 4, 4),
                        },
                        glyph: character.glyph.as_texture_source()?,
                        transform,
                        color1: color,
                        color2: color,
                        thresholds,
                    },
                    DrawPrimitive::TrianglesStrip,
                ));
            }
        }
        Ok(())
    }
}

fn font_index(font: CharFontType) -> usize {
    match font {
        CharFontType::Regular => 0,
        CharFontType::Bold => 1,
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod gpu_tests;

#[cfg(test)]
mod clock_gpu_tests;
