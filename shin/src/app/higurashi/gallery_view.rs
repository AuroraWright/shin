//! Packet primitives and the completion-text owner shared by the native
//! MovieMode (93e28/94470) and BGMMode (3001c/308cc) parent views.
use crate::{
    adv::{
        characters::view::CharacterTextures,
        higurashi::browser_navigation::labels::{Glyphs, Layout},
    },
    app::higurashi::kakera_menu::view::Packet,
    asset::{font::GpuFontLazy, system::AssetServer},
    layer::render_params::{higurashi::HigurashiTransform, multiply_transforms as multiply},
};
use anyhow::{Result, ensure};
use glam::{Mat4, Vec2, Vec4, vec3, vec4};
use shin_core::layout::{LayoutParams, font::FontMetrics};
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, vertices::PosColTexVertex},
};
use std::sync::Arc;

pub(super) enum Draw {
    Quad(Packet),
    Scrollbar {
        matrix: Mat4,
        vertices: [PosColTexVertex; 8],
    },
    Completion(Mat4),
}
pub(super) const FULL: [f32; 4] = [0., 0., 1., 1.];
pub(super) fn quad(
    out: &mut Vec<Draw>,
    name: impl Into<String>,
    matrix: Mat4,
    rect: [f32; 4],
    uv: [f32; 4],
    color: u32,
) {
    out.push(Draw::Quad(Packet {
        texture: name.into(),
        matrix,
        rect,
        uv,
        color,
        opaque: false,
        flash: 0.,
    }));
}
pub(super) fn alpha(value: f32) -> u32 {
    ((value.max(0.) * 255.) as u32).min(255) << 24
}
/// A 320px-wide help sheet of 48px rows; each record is
/// (first key, second key, row y, width).
pub(super) struct HelpSheet {
    pub texture: &'static str,
    pub height: f32,
    pub hints: &'static [(i32, i32, i32, i32)],
}
pub(super) fn hint(
    out: &mut Vec<Draw>,
    p: Mat4,
    sheet: &HelpSheet,
    right: f32,
    id: usize,
    color: u32,
    swapped: bool,
) -> f32 {
    let (first, second, row, width) = sheet.hints[id];
    let mut left = right - width as f32;
    let v = row as f32 / sheet.height;
    quad(
        out,
        sheet.texture,
        p,
        [left, 1012., left + width as f32, 1060.],
        [0., v, width as f32 / 320. + 0., v + 48. / sheet.height],
        color,
    );
    for key in [first, second].into_iter().filter(|k| *k >= 0) {
        let key = if swapped {
            match key {
                1 => 2,
                2 => 1,
                _ => key,
            }
        } else {
            key
        };
        left += -48.;
        let u = (key * 48) as f32 / 288.;
        quad(
            out,
            "key",
            p,
            [left, 1012., left + 48., 1060.],
            [u, 0., u + f32::from_bits(0x3e2aaaab), 1.],
            color,
        );
    }
    left + -10.
}
pub(super) fn text_matrix(p: Mat4, size: Vec2) -> Mat4 {
    let anchor = Mat4::from_translation(vec3(-size.x * 0., -size.y * 0., 0.));
    let local = multiply(Mat4::from_scale(vec3(1., 1., 0.)), anchor);
    multiply(
        p,
        multiply(Mat4::from_translation(vec3(48., 1012., 0.)), local),
    )
}
/// The parent is composed with (1848,top) first; c0574 then applies its own
/// 16px cap offset. Preserve both rounded products rather than folding those
/// translations before the projection multiply.
pub(super) fn scrollbar_matrix(p: Mat4, top: f32) -> Mat4 {
    multiply(
        multiply(p, Mat4::from_translation(vec3(1848., top, 0.))),
        Mat4::from_translation(vec3(0., 16., 0.)),
    )
}
pub(super) fn completion_layout(font: impl FontMetrics + Clone, text: &str) -> Result<Layout> {
    // Original938a0/2f554 SimpleText width640,size40,outline1. d19f4 insets
    // by4; d1c20 adds2 to positions; Layout::from_script preserves both.
    Layout::from_script(
        font,
        text,
        LayoutParams {
            layout_width: 636.,
            text_size: 40.,
            perform_soft_breaks: false,
            ..Default::default()
        },
    )
}
pub(super) struct CompletionText {
    font: Arc<GpuFontLazy>,
    cached: Option<(String, Glyphs)>,
}
impl CompletionText {
    pub fn new(font: Arc<GpuFontLazy>) -> Self {
        Self { font, cached: None }
    }
    pub fn synchronize(&mut self, server: &AssetServer, text: &str) -> Result<()> {
        if self.cached.as_ref().is_some_and(|(t, _)| t == text) {
            return Ok(());
        }
        let layout = completion_layout(self.font.clone(), text)?;
        let c = server.context();
        let glyphs = Glyphs::from_layout(&c.wgpu_device, &c.wgpu_queue, self.font.clone(), layout);
        self.cached = Some((text.to_owned(), glyphs));
        Ok(())
    }
    /// Text is required once the atlas is published; before that the parent
    /// draws nothing and the size is zero.
    pub fn size(&self, atlas: bool, owner: &str) -> Result<Vec2> {
        ensure!(
            !atlas || self.cached.is_some(),
            "{owner} completion text is not synchronized"
        );
        Ok(self
            .cached
            .as_ref()
            .map_or(Vec2::ZERO, |(_, g)| g.layout.size))
    }
    pub fn render(
        &self,
        draws: Vec<Draw>,
        atlas: Option<&CharacterTextures>,
        system: &CharacterTextures,
        pass: &mut RenderPass,
    ) {
        for draw in draws {
            match draw {
                Draw::Quad(p) => p.render(pass, atlas),
                Draw::Scrollbar { matrix, vertices } => pass.run(
                    RenderRequestBuilder::new()
                        .color_blend_type(ColorBlendType::Layer1)
                        .build(
                            RenderProgramWithArguments::Sprite {
                                vertices: VertexSource::VertexData {
                                    vertices: &vertices,
                                },
                                sprite: system.get("scroll").as_source(),
                                transform: HigurashiTransform::convert_clip_depth(matrix),
                            },
                            DrawPrimitive::TrianglesStrip,
                        ),
                ),
                Draw::Completion(matrix) => {
                    if let Some((_, glyphs)) = &self.cached {
                        glyphs.render_at(
                            pass,
                            matrix,
                            Vec4::ONE,
                            vec4(0., 0., 0., 240. * f32::from_bits(0x3b808081)),
                            RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1),
                        );
                    }
                }
            }
        }
    }
}
