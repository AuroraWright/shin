//! Ger GenericText glyph renderer, reusing the native SELECT SDF path.
//! SaveMenu text caller supplies its44px size and1252px inner width.
use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use glam::{Mat4, Vec2, Vec4, vec2};
use shin_core::{
    layout::{LineInfo, commands::Command},
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

use crate::asset::font::gerokasu2::{GpuGerokasu2Font, GpuGerokasu2GlyphHandle, draw::GlyphDraw};

struct Character {
    glyph: GpuGerokasu2GlyphHandle,
    draw: GlyphDraw,
    color: UnormColor,
}

pub struct Text {
    characters: Vec<Character>,
    vertices: Option<OwnedVertexBuffer<TextVertex>>,
}

impl Text {
    pub fn prepare(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        font: &Arc<GpuGerokasu2Font>,
        commands: Vec<Command>,
        lines: Vec<LineInfo>,
        text_size: f32,
    ) -> Result<Self> {
        let requested = commands
            .iter()
            .filter_map(|command| match command {
                Command::Char(character) => Some(character.codepoint),
                _ => None,
            })
            .collect::<Vec<_>>();
        // GenericText uses the caller font for both regular and bold
        // characters and ignores timing/control commands.
        let mut glyphs = font
            .clone()
            .load_glyphs(device.clone(), queue.clone(), &requested)
            .into_iter();
        let mut vertices = Vec::new();
        let mut characters = Vec::new();
        for command in commands {
            let Command::Char(character) = command else {
                continue;
            };
            ensure!(
                character.line_index < lines.len(),
                "Ger GenericText character has no line"
            );
            let glyph = glyphs
                .next()
                .context("Ger GenericText glyph batch is incomplete")?;
            // D463C/D4658 translate parsed coordinates by the fixed border.
            // D484C supplies the caller text_size for gradient interpolation,
            // including characters with inline font-size changes.
            let draw = GlyphDraw::new(
                glyph.geometry()?,
                character.position + Vec2::splat(3.0),
                vec2(character.horizontal_scale, character.scale),
                text_size,
                character.is_rubi,
            )?;
            vertices.extend_from_slice(&draw.vertices);
            characters.push(Character {
                glyph,
                draw,
                color: character.color,
            });
        }
        let vertices = (!vertices.is_empty()).then(|| {
            OwnedVertexBuffer::allocate_vertex(
                device,
                &vertices,
                Some("Gerokasu2SaveMenuText/vertices"),
            )
        });
        Ok(Self {
            characters,
            vertices,
        })
    }

    pub fn render(
        &self,
        pass: &mut RenderPass,
        builder: RenderRequestBuilder,
        transform: Mat4,
        tint: Vec4,
    ) -> Result<()> {
        self.render_colored(pass, builder, transform, Vec4::new(1., 1., 1., tint.w))
    }

    /// ChapterSelector supplies an unquantized RGBA multiplier to GenericText.
    /// Other menu callers retain their original alpha-only `render` contract.
    pub fn render_colored(
        &self,
        pass: &mut RenderPass,
        builder: RenderRequestBuilder,
        transform: Mat4,
        tint: Vec4,
    ) -> Result<()> {
        let Some(vertices) = &self.vertices else {
            return Ok(());
        };
        // C2FB4..C3000 supplies white fill and black outline with row alpha.
        // The constructor's seen-choice RGB metadata is not read here.
        for outline in [true, false] {
            if !(tint.w > 0.0) {
                continue;
            }
            for (index, character) in self.characters.iter().enumerate() {
                let (color, thresholds) = if outline {
                    (
                        FloatColor4::from_rgba(0.0, 0.0, 0.0, tint.w),
                        character.draw.outline_thresholds,
                    )
                } else {
                    // Unlike MessageLayer, GenericText multiplies packed
                    // alpha too. Native50074+4FFA8 preserve color0 as zero.
                    (
                        fill_color(character.color, tint),
                        character.draw.fill_thresholds,
                    )
                };
                pass.run(builder.build(
                    RenderProgramWithArguments::FontSdf {
                        vertices: VertexSource::VertexBuffer {
                            vertices: vertices.as_sliced_buffer_ref(index * 4, 4),
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

fn fill_color(color: UnormColor, tint: Vec4) -> FloatColor4 {
    let inverse = f32::from_bits(0x3b808081);
    let packed = color.0;
    FloatColor4::from_rgba(
        ((packed & 255) as f32 * inverse) * tint.x,
        (((packed >> 8) & 255) as f32 * inverse) * tint.y,
        (((packed >> 16) & 255) as f32 * inverse) * tint.z,
        (((packed >> 24) & 255) as f32 * inverse) * tint.w,
    )
}
