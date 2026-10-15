//! Actual3001c/308cc BGMMode parent packets. The owner subsequently draws its
//! private ModeStack snapshot (including First's program6/18 capture overlay).
use super::Model;
use crate::{
    adv::{
        character_view::CharacterTextures,
        higurashi::{
            browser_controls::Scrollbar,
            browser_decorations,
            browser_navigation::labels::{Glyphs, Layout},
        },
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
const FULL: [f32; 4] = [0., 0., 1., 1.];
fn projection() -> Mat4 {
    Mat4::from_cols_array(&[
        2. / 1920.,
        0.,
        0.,
        0.,
        0.,
        -2. / 1080.,
        0.,
        0.,
        0.,
        0.,
        -1.,
        0.,
        -1.,
        1.,
        -0.,
        1.,
    ])
}
fn quad(
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
fn alpha(value: f32) -> u32 {
    ((value.max(0.) * 255.) as u32).min(255) << 24
}
fn hint(out: &mut Vec<Draw>, p: Mat4, right: f32, id: usize, color: u32, swapped: bool) -> f32 {
    // Actual14bcfc; the fourth record has key4, not the later engine's key5.
    const HINTS: [(i32, i32, i32, i32); 4] = [
        (5, 0, 0, 214),
        (2, -1, 48, 76),
        (1, -1, 96, 78),
        (4, -1, 144, 78),
    ];
    let (first, second, row, width) = HINTS[id];
    let mut left = right - width as f32;
    let v = row as f32 / 192.;
    quad(
        out,
        "help_bgmmode",
        p,
        [left, 1012., left + width as f32, 1060.],
        [0., v, width as f32 / 320. + 0., v + 0.25],
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
fn text_matrix(p: Mat4, size: Vec2) -> Mat4 {
    let anchor = Mat4::from_translation(vec3(-size.x * 0., -size.y * 0., 0.));
    let local = multiply(Mat4::from_scale(vec3(1., 1., 0.)), anchor);
    multiply(
        p,
        multiply(Mat4::from_translation(vec3(48., 1012., 0.)), local),
    )
}
fn scrollbar_matrix(p: Mat4) -> Mat4 {
    // 30164..301a0 composes the parent with (1848,162) first; c0574 then
    // applies its own16px cap offset. Preserve both rounded products rather
    // than folding those translations before the projection multiply.
    multiply(
        multiply(p, Mat4::from_translation(vec3(1848., 162., 0.))),
        Mat4::from_translation(vec3(0., 16., 0.)),
    )
}
pub(super) fn completion_layout(font: impl FontMetrics + Clone, text: &str) -> Result<Layout> {
    // Original2f554 SimpleText width640,size40,outline1. d19f4 insets by4;
    // d1c20 adds2 to positions; Layout::from_script preserves both operations.
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
pub(super) fn packets(
    m: &Model,
    atlas: bool,
    bar: Scrollbar,
    size: Vec2,
    swapped: bool,
) -> Vec<Draw> {
    if !atlas {
        return vec![];
    }
    let p = projection();
    let mut out = vec![];
    quad(&mut out, "bg", p, [0., 0., 1920., 1080.], FULL, u32::MAX);
    if let Some(Draw::Quad(bg)) = out.last_mut() {
        bg.opaque = true;
    }
    let origin = m.scroll.value() + -162.;
    let t = m.elapsed * f32::from_bits(0x40490fd0);
    let pulse = (t + t).sin() * f32::from_bits(0x3e4ccccd) + f32::from_bits(0x3ecccccd);
    for (index, row) in m.rows.iter().enumerate() {
        let y = ((index / 2) * 92) as f32 - origin;
        if y < -92. || y >= 1080. {
            continue;
        }
        let x = 150. + ((index & 1) * 820) as f32;
        let top = y + 2.;
        let rect = [x, top, x + 800., top + 88.];
        let uv = if m.playing == index as i32 {
            [0., 0.5, 1., 1.]
        } else {
            [0., 0., 1., 0.5]
        };
        quad(&mut out, "plate", p, rect, uv, u32::MAX);
        if m.selected == index as i32 {
            quad(&mut out, "cur", p, rect, FULL, 0x00ffffff | alpha(pulse));
        }
        let v = ((u32::from(row.name) % 32) * 64) as f32 * f32::from_bits(0x3a000000);
        quad(
            &mut out,
            format!("title{}", row.name / 32),
            p,
            [x + 80., top + 12., x + 720., top + 76.],
            [0., v, 1., v + f32::from_bits(0x3d000000)],
            u32::MAX,
        );
        let f = row.flash.value();
        if f > 0. {
            quad(&mut out, "flash", p, rect, FULL, 0x00ffffff | alpha(f));
        }
    }
    if m.document_height > 817 {
        for vertices in browser_decorations::scrollbar(818., bar) {
            out.push(Draw::Scrollbar {
                matrix: scrollbar_matrix(p),
                vertices,
            });
        }
    }
    quad(
        &mut out,
        "obi",
        p,
        [0., 0., 1920., 150.],
        [0., 0., 1., 0.625],
        0xd8ffffff,
    );
    quad(
        &mut out,
        "caption",
        p,
        [72., 0., 672., 144.],
        FULL,
        u32::MAX,
    );
    quad(
        &mut out,
        "close",
        p,
        [1764., 30., 1848., 114.],
        FULL,
        u32::MAX,
    );
    if let Some(Draw::Quad(close)) = out.last_mut() {
        close.flash = m.close_flash.value();
    }
    quad(
        &mut out,
        "obi",
        p,
        [0., 992., 1920., 1080.],
        [0., f32::from_bits(0x3f222222), 1., 1.],
        0xd8ffffff,
    );
    let mut right = 1872.;
    for id in [1, 3, 2, 0] {
        let disabled =
            (id == 3 && m.playing < 0) || (id == 2 && !m.rows[m.selected as usize].unlocked);
        right = hint(
            &mut out,
            p,
            right,
            id,
            if disabled { 0xff808080 } else { u32::MAX },
            swapped,
        );
    }
    out.push(Draw::Completion(text_matrix(p, size)));
    out
}
pub(super) struct View {
    font: Arc<GpuFontLazy>,
    completion: Option<(String, Glyphs)>,
}
impl View {
    pub fn new(font: Arc<GpuFontLazy>) -> Self {
        Self {
            font,
            completion: None,
        }
    }
    /// Explicit desktop malformed-archive guard; no replacement artwork is
    /// published. These14 names are actually requested by native3001c/308cc.
    pub fn validate_atlas(atlas: &CharacterTextures) -> Result<()> {
        for name in [
            "bg",
            "plate",
            "cur",
            "flash",
            "obi",
            "caption",
            "close",
            "help_bgmmode",
            "key",
            "title0",
            "title1",
            "title2",
            "title3",
            "title4",
        ] {
            ensure!(atlas.contains(name), "Music archive is missing {name}");
        }
        Ok(())
    }
    pub fn synchronize(&mut self, server: &AssetServer, model: &Model) -> Result<()> {
        if self
            .completion
            .as_ref()
            .is_some_and(|(t, _)| t == &model.completion_text)
        {
            return Ok(());
        }
        let layout = completion_layout(self.font.clone(), &model.completion_text)?;
        let c = server.context();
        let glyphs = Glyphs::from_layout(&c.wgpu_device, &c.wgpu_queue, self.font.clone(), layout);
        self.completion = Some((model.completion_text.clone(), glyphs));
        Ok(())
    }
    /// Parent draws are atlas-gated; private First drawing belongs AFTER this
    /// method and is permitted while the worker is still pending/unpublished.
    pub fn render(
        &self,
        model: &Model,
        atlas: Option<&CharacterTextures>,
        system: &CharacterTextures,
        swapped: bool,
        pass: &mut RenderPass,
    ) -> Result<()> {
        ensure!(
            atlas.is_none() || self.completion.is_some(),
            "Music completion text is not synchronized"
        );
        let size = self
            .completion
            .as_ref()
            .map_or(Vec2::ZERO, |(_, g)| g.layout.size);
        for draw in packets(model, atlas.is_some(), model.scrollbar, size, swapped) {
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
                    if let Some((_, glyphs)) = &self.completion {
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
        Ok(())
    }
}
#[cfg(test)]
mod tests;
