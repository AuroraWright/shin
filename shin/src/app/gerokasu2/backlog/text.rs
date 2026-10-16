//! MessageBrowser81A7C glyph passes, distinct from GenericText and MessageLayer.
use std::sync::Arc;

use crate::{
    adv::gerokasu2::backlog::view::glyph::{Draw, Geometry},
    asset::font::gerokasu2::{GpuGerokasu2Font, GpuGerokasu2GlyphHandle},
};
use anyhow::{Context, Result};
use glam::{Mat4, Vec2, Vec4};
use shin_core::{
    layout::{LineInfo, commands::Command},
    primitives::color::{FloatColor4, UnormColor},
};
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    render_pass::RenderPass,
    shaders::types::{
        buffer::{OwnedVertexBuffer, VertexSource},
        vertices::TextVertex,
    },
};

struct Character {
    glyph: GpuGerokasu2GlyphHandle,
    draw: Draw,
    color: UnormColor,
}
pub(super) struct Text {
    characters: Vec<Character>,
    vertices: Option<OwnedVertexBuffer<TextVertex>>,
}
impl Text {
    pub fn prepare(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        font: &Arc<GpuGerokasu2Font>,
        commands: Vec<Command>,
        lines: &[LineInfo],
    ) -> Result<Self> {
        let requested: Vec<_> = commands
            .iter()
            .filter_map(|command| match command {
                Command::Char(c) => Some(c.codepoint),
                _ => None,
            })
            .collect();
        // Native bootstrap aliases its normal and bold LogMessage font owners
        // to the same NewRodin font. Keep the shared asset/cache owner too.
        let mut glyphs = font
            .clone()
            .load_glyphs(device.clone(), queue.clone(), &requested)
            .into_iter();
        let mut vertices = Vec::new();
        let mut characters = Vec::new();
        for command in commands {
            let Command::Char(c) = command else {
                continue;
            };
            anyhow::ensure!(c.line_index < lines.len(), "Ger backlog glyph has no line");
            let glyph = glyphs
                .next()
                .context("Ger backlog glyph batch is incomplete")?;
            let g = glyph.geometry()?;
            let draw = Draw::new(
                &Geometry {
                    origin: g.origin,
                    dimensions: g.dimensions,
                    uv_scale: g.scale,
                    font_metrics: [g.font_metrics[0], g.font_metrics[1]],
                    extra_metric: g.extra_metric,
                },
                c.position.to_array(),
                [c.horizontal_scale, c.scale],
                c.is_rubi,
            )
            .map_err(anyhow::Error::msg)?;
            vertices.extend(draw.vertices.iter().map(|v| TextVertex {
                position: Vec4::from_array(v.position),
                color: v.color,
            }));
            characters.push(Character {
                glyph,
                draw,
                color: c.color,
            });
        }
        let vertices = (!vertices.is_empty()).then(|| {
            OwnedVertexBuffer::allocate_vertex(device, &vertices, Some("Ger backlog glyphs"))
        });
        Ok(Self {
            characters,
            vertices,
        })
    }
    pub fn render(&self, pass: &mut RenderPass, matrix: [f32; 16]) -> Result<()> {
        let Some(vertices) = self.vertices.as_ref() else {
            return Ok(());
        };
        let builder = RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1);
        for outline in [true, false] {
            for (index, character) in self.characters.iter().enumerate() {
                let (color, thresholds) = if outline {
                    (
                        FloatColor4::from_rgba(0., 0., 0., 0.75),
                        character.draw.outline,
                    )
                } else {
                    (fill_color(character.color), character.draw.fill)
                };
                pass.run(builder.build(
                    RenderProgramWithArguments::FontSdf {
                        vertices: VertexSource::VertexBuffer {
                            vertices: vertices.as_sliced_buffer_ref(index * 4, 4),
                        },
                        glyph: character.glyph.as_texture_source()?,
                        transform: Mat4::from_cols_array(&matrix),
                        color1: color,
                        color2: color,
                        thresholds: Vec2::from_array(thresholds),
                    },
                    DrawPrimitive::TrianglesStrip,
                ));
            }
        }
        Ok(())
    }
}
fn fill_color(color: UnormColor) -> FloatColor4 {
    let inverse = f32::from_bits(0x3b808081);
    let [r, g, b, a] = color.0.to_le_bytes();
    FloatColor4::from_rgba(
        r as f32 * inverse,
        g as f32 * inverse,
        b as f32 * inverse,
        a as f32 * inverse,
    )
}
