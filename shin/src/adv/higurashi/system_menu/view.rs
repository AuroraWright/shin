//! Updated1bec4 rows and info/title/time panels, using the shared GPU programs.
use super::SystemMenu;
use crate::{
    adv::{characters::view::CharacterTextures, higurashi::message_glyphs::glyph_vertices},
    asset::{
        font::{GpuFontGlyphHandle, GpuFontLazy},
        system::AssetServer,
    },
    layer::message_layer::outline,
};
use anyhow::Result;
use glam::{Mat4, Vec2, Vec4, vec2, vec3, vec4};
use shin_core::{
    layout::{HigurashiTextLayouter, LayoutParams, MessageTextLayouterDefaults, commands::Command},
    primitives::color::FloatColor4,
};
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    gpu_texture::GpuTexture,
    render_pass::RenderPass,
    shaders::types::{
        buffer::VertexSource,
        vertices::{PosColTexVertex, TextVertex},
    },
};
use std::sync::Arc;

pub struct Text {
    size: Vec2,
    pub(super) glyphs: Vec<TextGlyph>,
}
pub(super) struct TextGlyph {
    pub(super) handle: GpuFontGlyphHandle,
    pub(super) vertices: [TextVertex; 4],
    pub(super) border: [Vec2; 8],
}
impl Text {
    pub fn prepare(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        font: Arc<GpuFontLazy>,
        text: &str,
    ) -> Self {
        let (commands, lines, size) = HigurashiTextLayouter::new(
            font.clone(),
            LayoutParams {
                layout_width: 486.0,
                text_size: 38.0,
                perform_soft_breaks: false,
                ..Default::default()
            },
            MessageTextLayouterDefaults {
                color: 0xfff,
                draw_speed: 50,
                fade: 200,
            },
        )
        .parse(text);
        let characters: Vec<_> = commands
            .iter()
            .filter_map(|command| match command {
                Command::Char(c) => Some(c),
                _ => None,
            })
            .collect();
        let points: Vec<_> = characters.iter().map(|c| c.codepoint).collect();
        let handles = font.load_glyphs(device.clone(), queue.clone(), &points);
        let glyphs = characters
            .into_iter()
            .zip(handles)
            .filter_map(|(c, handle)| {
                let info = handle.info();
                if info.actual_width == 0 || info.actual_height == 0 {
                    return None;
                }
                let mut vertices = glyph_vertices(c, &lines[c.line_index], info);
                for vertex in &mut vertices {
                    vertex.position = vertex.position + vec4(2.0, 2.0, 0.0, 0.0);
                }
                Some(TextGlyph {
                    vertices,
                    border: outline::distances_with_radius(
                        vec2(c.horizontal_scale, c.scale),
                        info.actual_size_f32(),
                        info.actual_size_normalized(),
                        1,
                        1.5,
                    ),
                    handle,
                })
            })
            .collect();
        // SimpleText params[0xc]=1 enables the two-pixel outline padding.
        Self {
            size: size + Vec2::splat(4.0),
            glyphs,
        }
    }
    pub fn size(&self) -> Vec2 {
        self.size
    }
    fn render(&self, pass: &mut RenderPass, transform: Mat4, fade: f32) {
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
                        color: FloatColor4::from_rgba(0.0, 0.0, 0.0, fade),
                    }
                } else {
                    RenderProgramWithArguments::Font {
                        vertices,
                        glyph: texture,
                        transform,
                        color1: FloatColor4::from_rgba(1.0, 1.0, 1.0, fade),
                        color2: FloatColor4::from_rgba(1.0, 1.0, 1.0, fade),
                    }
                };
                pass.run(builder.build(program, DrawPrimitive::TrianglesStrip));
            }
        }
    }
}

pub struct View {
    atlas: Arc<CharacterTextures>,
    font: Arc<GpuFontLazy>,
    title: Option<Text>,
    time: Text,
    time_string: String,
}
impl View {
    /// VM title is snapshotted on opening; global play time is refreshed later.
    pub async fn load(assets: &AssetServer, title: &str, play_ticks: f64) -> Result<Self> {
        let atlas = assets.load("/adv.txa").await?;
        let font: Arc<GpuFontLazy> = assets.load("/newrodin.fnt").await?;
        Ok(Self::from_loaded(assets, atlas, font, title, play_ticks))
    }
    /// Live ADV already owns this atlas and the startup font. Reuse them when
    /// constructing the menu at its prepared-screen capture boundary.
    pub fn from_loaded(
        assets: &AssetServer,
        atlas: Arc<CharacterTextures>,
        font: Arc<GpuFontLazy>,
        title: &str,
        play_ticks: f64,
    ) -> Self {
        let context = assets.context();
        let make = |text| {
            Text::prepare(
                &context.wgpu_device,
                &context.wgpu_queue,
                font.clone(),
                text,
            )
        };
        let time_string = super::play_time_text(play_ticks);
        Self {
            atlas,
            title: (!title.is_empty()).then(|| make(title)),
            time: make(&time_string),
            time_string,
            font,
        }
    }
    pub fn update_time(&mut self, assets: &AssetServer, play_ticks: f64) {
        let text = super::play_time_text(play_ticks);
        if text != self.time_string {
            let context = assets.context();
            self.time = Text::prepare(
                &context.wgpu_device,
                &context.wgpu_queue,
                self.font.clone(),
                &text,
            );
            self.time_string = text;
        }
    }
    pub fn render(&self, model: &SystemMenu, pass: &mut RenderPass) {
        if model.phase() < 1 {
            return;
        }
        render_rows(model, self.atlas.get("sysmenu"), pass);
        let slide = model.detail_slide();
        let fade = model.detail_fade();
        for (index, text) in [self.title.as_ref(), Some(&self.time)]
            .into_iter()
            .enumerate()
        {
            let Some(text) = text else { continue };
            if !(text.size.x > 0.0) {
                continue;
            }
            let y = if index == 0 { 795.0 } else { 930.0 };
            //1bec4 keeps these translations separate, then74cac composes
            // projection and glyph alignment with serial FMLA operations.
            let base = Mat4::from_translation(vec3(-48.0, y, 0.0));
            let slide = Mat4::from_translation(vec3(slide * 120.0, 0.0, 0.0));
            let transform = multiply(
                crate::render::top_left_projection_matrix(),
                multiply(slide, base),
            );
            render_quad(
                self.atlas.get("info"),
                pass,
                vec4(1176.0, 0.0, 756.0, 64.0),
                vec4(0.0, index as f32 * 0.5, 1.0, 0.5),
                fade,
                None,
                transform,
            );
            text.render(
                pass,
                multiply(
                    transform,
                    Mat4::from_translation(vec3(1920.0 - text.size.x, 54.0, 0.0)),
                ),
                fade,
            );
        }
    }
}
pub fn render_rows(model: &SystemMenu, atlas: &GpuTexture, pass: &mut RenderPass) {
    if model.phase() < 1 {
        return;
    }
    for (index, row) in model.rows().iter().enumerate() {
        render_quad(
            atlas,
            pass,
            vec4(
                24.0 + (row.slide.value() - 1.0) * 504.0,
                model.state.vertical_offset as f32 + index as f32 * 128.0,
                396.0,
                120.0,
            ),
            vec4(
                if index == model.selected() { 0.0 } else { 0.5 },
                row.item as f32 * 0.125,
                0.5,
                0.125,
            ),
            row.slide.value(),
            Some(row.flash.value()),
            crate::render::top_left_projection_matrix(),
        );
    }
}
fn render_quad(
    texture: &GpuTexture,
    pass: &mut RenderPass,
    rect: Vec4,
    uv: Vec4,
    opacity: f32,
    flash: Option<f32>,
    transform: Mat4,
) {
    let color = FloatColor4::from_rgba(1.0, 1.0, 1.0, opacity).into_unorm();
    let vertices = shin_render::quad_vertices::build_quad_vertices(|corner| PosColTexVertex {
        position: vec3(rect.x + rect.z * corner.x, rect.y + rect.w * corner.y, 0.0),
        texture_position: vec2(uv.x + uv.z * corner.x, uv.y + uv.w * corner.y),
        color,
    });
    let vertices = VertexSource::VertexData {
        vertices: &vertices,
    };
    //9ba18 selects ordinary sprite when flash<=0, otherwise program5.
    let program = if let Some(flash) = flash.filter(|value| *value > 0.0) {
        RenderProgramWithArguments::Button {
            vertices,
            texture: texture.as_source(),
            transform,
            flash1: Vec4::ZERO,
            flash2: vec4(1.0, 1.0, 1.0, flash),
        }
    } else {
        RenderProgramWithArguments::Sprite {
            vertices,
            sprite: texture.as_source(),
            transform,
        }
    };
    pass.run(
        RenderRequestBuilder::new()
            .color_blend_type(ColorBlendType::Layer1)
            .build(program, DrawPrimitive::TrianglesStrip),
    );
}

//74cac matrix product: multiply first term, then three fused accumulations.
fn multiply(left: Mat4, right: Mat4) -> Mat4 {
    let left = left.to_cols_array();
    let right = right.to_cols_array();
    Mat4::from_cols_array(&std::array::from_fn(|i| {
        let row = i % 4;
        let column = i / 4 * 4;
        let first = left[row] * right[column];
        let second = left[row + 4].mul_add(right[column + 1], first);
        let third = left[row + 8].mul_add(right[column + 2], second);
        left[row + 12].mul_add(right[column + 3], third)
    }))
}

#[cfg(test)]
mod tests;
