//! Original af2f0/afc50/afe4c/b0058/b01c0/c0574 SaveMenu drawing.
//! Saved-VM hydration and preview pixels are supplied by the owning service.
use crate::render::NATIVE_UI_PROJECTION;
use super::{
    model::{Mode, ViewState},
    resources::{Labels, Rows, visible},
};
use crate::{
    adv::{
        characters::view::CharacterTextures,
        higurashi::{
            browser_controls::Scrollbar,
            browser_decorations,
            browser_navigation::labels::{Glyph, Glyphs, Layout},
        },
    },
    asset::{font::GpuFontLazy, system::AssetServer},
    layer::{
        message_layer::outline,
        render_params::{higurashi::HigurashiTransform, multiply_transforms as multiply},
    },
};
use anyhow::{Context, Result};
use glam::{Mat4, Vec4, vec2, vec3, vec4};
use shin_core::{
    layout::{
        HigurashiTextLayouter, LayoutParams, MessageTextLayouterDefaults, commands::Command,
        font::FontMetrics,
    },
    primitives::color::UnormColor,
};
use shin_render::{
    ColorBlendType, RenderRequestBuilder,
    quad_vertices::QuadVertices,
    render_pass::RenderPass,
    shaders::types::{
        texture::TextureSource,
        vertices::{PosColTexVertex, TextVertex},
    },
};
use std::sync::Arc;

#[derive(Clone, Copy, Debug)]
pub(super) enum Texture {
    Atlas(&'static str),
    Thumbnail(usize),
    Fill,
}
pub(super) struct Quad {
    pub texture: Texture,
    pub matrix: Mat4,
    pub rect: [f32; 4],
    pub uv: [f32; 4],
    /// Original packed BGRA; convert the bytes at GPU submission.
    pub color: u32,
    pub opaque: bool,
    pub flash: f32,
}
pub(super) enum Draw {
    Quad(Quad),
    Labels {
        index: usize,
        matrix: Mat4,
    },
    Scroll {
        matrix: Mat4,
        vertices: [PosColTexVertex; 8],
    },
}
const FULL: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
const BLUE: u32 = 0xff005fbe;
/// Unpadded SimpleText (outline flag zero); browser labels use outline=true
/// and different vertices.
fn label_layout(font: impl FontMetrics + Clone, text: &str) -> Result<Layout> {
    Layout::unpadded(
        font,
        text,
        LayoutParams {
            layout_width: 1240.0,
            text_size: 60.0,
            perform_soft_breaks: false,
            ..Default::default()
        },
    )
}
fn text_matrix(parent: Mat4, y: f32) -> Mat4 {
    // d2240: zero anchor, unit XY scale with Z zero, then text position.
    let local = multiply(
        Mat4::from_translation(vec3(354.0, y, 0.0)),
        Mat4::from_scale(vec3(1.0, 1.0, 0.0)),
    );
    multiply(parent, local)
}
fn quad(texture: Texture, matrix: Mat4, rect: [f32; 4], uv: [f32; 4], color: u32) -> Draw {
    Draw::Quad(Quad {
        texture,
        matrix,
        rect,
        uv,
        color,
        opaque: false,
        flash: 0.0,
    })
}
fn sprite(name: &'static str, matrix: Mat4, rect: [f32; 4], uv: [f32; 4], color: u32) -> Draw {
    quad(Texture::Atlas(name), matrix, rect, uv, color)
}
fn number(out: &mut Vec<Draw>, matrix: Mat4, mut x: i32, text: &str, big: bool, align_right: bool) {
    // Native tables152138/1521f8/152298, on the384 x120 'num' sheet.
    let digit_width = if big { 36 } else { 24 };
    let punctuation_width = if big { 24 } else { 16 };
    let width: i32 = text
        .bytes()
        .map(|b| {
            if b.is_ascii_digit() {
                digit_width
            } else if big && b != b'.' {
                20
            } else {
                punctuation_width
            }
        })
        .sum();
    if align_right {
        x -= width;
    }
    for point in text.bytes() {
        let glyph = if point.is_ascii_digit() {
            Some((
                (point - b'0') as i32 * digit_width,
                if big { 48 } else { 0 },
                digit_width,
            ))
        } else if big && point == b'.' {
            Some((360, 48, 24))
        } else if !big && point == b':' {
            Some((240, 0, 16))
        } else if !big && point == b'/' {
            Some((256, 0, 16))
        } else {
            None
        };
        if let Some((u, v, w)) = glyph {
            let h = if big { 48 } else { 30 };
            let (u, v) = (u as f32 / 384.0, v as f32 / 120.0);
            out.push(sprite(
                "num",
                matrix,
                [x as f32, 2.0, (x + w) as f32, (h + 2) as f32],
                [u, v, u + w as f32 / 384.0, v + h as f32 / 120.0],
                BLUE,
            ));
            x += w;
        } else {
            x += if big { 20 } else { 16 };
        }
    }
}
/// Children are drawn by the owner after this complete parent sequence.
/// `thumbnails` describes only published real texture views.
pub(super) fn packets(
    state: &ViewState,
    rows: &Rows,
    atlas_present: bool,
    thumbnails: &[bool; 100],
) -> Vec<Draw> {
    if !atlas_present {
        return Vec::new();
    }
    let mut out = Vec::new();
    let projection = NATIVE_UI_PROJECTION;
    let open_x = 1920.0 - state.progress * 1920.0;
    let body = multiply(
        projection,
        Mat4::from_translation(vec3(open_x, -(state.scroll - 162.0), 0.0)),
    );
    out.push(quad(
        Texture::Fill,
        body,
        [150.0, 12.0, 1750.0, 20606.0],
        FULL,
        0xc0c0c0c0,
    ));
    out.push(sprite(
        "line",
        body,
        [150.0, 0.0, 1750.0, 12.0],
        FULL,
        u32::MAX,
    ));
    out.push(sprite(
        "line",
        body,
        [150.0, 20606.0, 1750.0, 20618.0],
        FULL,
        u32::MAX,
    ));
    for (index, row) in rows
        .iter()
        .enumerate()
        .filter(|(i, _)| visible(*i, state.scroll))
    {
        let y = (12 + index * 206) as f32 + 4.0;
        let matrix = multiply(body, Mat4::from_translation(vec3(150.0, y, 0.0)));
        if index != 0 {
            out.push(sprite(
                "separator",
                matrix,
                [0.0, -10.0, 1600.0, -4.0],
                FULL,
                u32::MAX,
            ));
        }
        number(
            &mut out,
            matrix,
            354,
            &format!("{}.", index + 1),
            true,
            false,
        );
        if let Some(date) = row.date {
            number(
                &mut out,
                matrix,
                1594,
                &format!(
                    "{:04}/{:02}/{:02} {:2}:{:02}",
                    date.year, date.month, date.day, date.hour, date.minute
                ),
                false,
                true,
            );
        }
        out.push(Draw::Labels { index, matrix });
        let mut thumbnail = quad(
            if thumbnails[index] {
                Texture::Thumbnail(index)
            } else {
                Texture::Atlas("nodata")
            },
            matrix,
            [6.0, 0.0, 348.0, 192.0],
            FULL,
            u32::MAX,
        );
        if let Draw::Quad(q) = &mut thumbnail {
            q.opaque = true;
        }
        out.push(thumbnail);
        if index as i32 == state.selected {
            for (top, bottom, v, w) in [
                (-4.0, 8.0, 0.0, 0.375),
                (8.0, 184.0, 0.375, 0.625),
                (184.0, 196.0, 0.625, 1.0),
            ] {
                out.push(sprite(
                    "focus",
                    matrix,
                    [0.0, top, 1600.0, bottom],
                    [0.0, v, 1.0, w],
                    0xc0ffffff,
                ));
            }
        }
        let flash = state.row_flash[index];
        if flash > 0.0 {
            let color = 0x00ffffff | (u32::from((flash.max(0.0) * 255.0) as u8) << 24);
            out.push(quad(
                Texture::Fill,
                matrix,
                [0.0, 0.0, 1600.0, 192.0],
                FULL,
                color,
            ));
        }
    }
    let parent = multiply(
        projection,
        Mat4::from_translation(vec3(open_x + 1848.0, 162.0, 0.0)),
    );
    let matrix = browser_decorations::scrollbar_transform(parent, 0.0, 0.0);
    for vertices in browser_decorations::scrollbar(818.0, state.scrollbar) {
        out.push(Draw::Scroll { matrix, vertices });
    }
    let header = state.progress * 150.0 + -150.0;
    out.push(sprite(
        "obi",
        projection,
        [0.0, header, 1920.0, header + 150.0],
        [0.0, 0.0, 1.0, 0.625],
        0xd8ffffff,
    ));
    out.push(sprite(
        if state.mode == Mode::Save {
            "caption_save"
        } else {
            "caption_load"
        },
        projection,
        [72.0, header, 672.0, header + 144.0],
        FULL,
        u32::MAX,
    ));
    let mut close = sprite(
        "close",
        projection,
        [1764.0, header + 30.0, 1848.0, (header + 30.0) + 84.0],
        FULL,
        u32::MAX,
    );
    if let Draw::Quad(q) = &mut close {
        q.flash = state.close_flash;
    }
    out.push(close);
    let footer = 1080.0 - state.progress * 88.0;
    out.push(sprite(
        "obi",
        projection,
        [0.0, footer, 1920.0, footer + 88.0],
        [0.0, f32::from_bits(0x3f222222), 1.0, 1.0],
        0xd8ffffff,
    ));
    const HINTS: [(i32, i32, u32, u32); 5] = [
        (5, 0, 0, 214),
        (2, -1, 48, 76),
        (1, -1, 96, 112),
        (1, -1, 144, 110),
        (4, -1, 192, 78),
    ];
    let mut right = 1872.0;
    let baseline = footer + 20.0;
    let ids: &[usize] = if state.mode == Mode::Save {
        &[1, 2, 4, 0]
    } else {
        &[1, 3, 0]
    };
    for &id in ids {
        let (first, second, row, width) = HINTS[id];
        let valid = rows
            .get(state.selected as usize)
            .is_some_and(|r| r.date.is_some());
        let color = if (id == 3 || id == 4) && !valid {
            0xff808080
        } else {
            u32::MAX
        };
        let left = right - width as f32;
        let v = row as f32 / 240.0;
        out.push(sprite(
            "help_saveload",
            projection,
            [left, baseline, left + width as f32, baseline + 48.0],
            [
                0.0,
                v,
                width as f32 / 320.0 + 0.0,
                v + f32::from_bits(0x3e4ccccd),
            ],
            color,
        ));
        right = left;
        // Original cc940 is zero for Nintendo;27560 preserves these keys.
        for key in [first, second].into_iter().filter(|k| *k >= 0) {
            right += -48.0;
            let u = (key * 48) as f32 / 288.0;
            out.push(sprite(
                "key",
                projection,
                [right, baseline, right + 48.0, baseline + 48.0],
                [u, 0.0, u + f32::from_bits(0x3e2aaaab), 1.0],
                color,
            ));
        }
        right += -10.0;
    }
    out
}
struct Cached {
    labels: Labels,
    glyphs: Vec<(Glyphs, f32)>,
}
pub struct View {
    font: Arc<GpuFontLazy>,
    cached: [Option<Cached>; 100],
}
impl View {
    pub fn new(font: Arc<GpuFontLazy>) -> Self {
        Self {
            font,
            cached: std::array::from_fn(|_| None),
        }
    }
    /// Native dirty290 is cleared after labels refresh. Comparing the retained
    /// metadata also catches overwrite/rollback changes supplied by the owner.
    pub fn synchronize(
        &mut self,
        server: &AssetServer,
        state: &ViewState,
        rows: &Rows,
    ) -> Result<()> {
        for (index, row) in rows.iter().enumerate() {
            if !visible(index, state.scroll) {
                self.cached[index] = None;
                continue;
            }
            if self.cached[index]
                .as_ref()
                .is_some_and(|c| c.labels == row.labels)
            {
                continue;
            }
            let texts: Vec<(&str, f32)> = match &row.labels {
                Labels::Empty => vec![("データがありません", 97.0)],
                Labels::Loading => vec![("loading...", 97.0)],
                Labels::Ready { title, subtitle } => vec![(title, 67.0), (subtitle, 127.0)],
            };
            let mut glyphs = Vec::with_capacity(texts.len());
            for (text, y) in texts {
                let layout = label_layout(self.font.clone(), text)?;
                let ctx = server.context();
                glyphs.push((
                    Glyphs::from_layout(
                        &ctx.wgpu_device,
                        &ctx.wgpu_queue,
                        self.font.clone(),
                        layout,
                    ),
                    y,
                ));
            }
            self.cached[index] = Some(Cached {
                labels: row.labels.clone(),
                glyphs,
            });
        }
        Ok(())
    }
    pub fn render(
        &self,
        state: &ViewState,
        rows: &Rows,
        atlas: Option<&CharacterTextures>,
        system: &CharacterTextures,
        thumbnails: &[Option<TextureSource<'_>>; 100],
        pass: &mut RenderPass,
    ) {
        let presence = std::array::from_fn(|i| thumbnails[i].is_some());
        for draw in packets(state, rows, atlas.is_some(), &presence) {
            match draw {
                Draw::Quad(q) => q.render(pass, atlas, thumbnails),
                Draw::Scroll { matrix, vertices } => {
                    browser_decorations::sprite(pass, system, "scroll", &vertices, matrix)
                }
                Draw::Labels { index, matrix } => {
                    if let Some(cached) = &self.cached[index] {
                        let tint = vec4(
                            0.0,
                            95.0 * f32::from_bits(0x3b808081),
                            190.0 * f32::from_bits(0x3b808081),
                            1.0,
                        );
                        for (glyphs, y) in &cached.glyphs {
                            let matrix = text_matrix(matrix, *y);
                            glyphs.render_at(
                                pass,
                                matrix,
                                tint,
                                Vec4::ZERO,
                                RenderRequestBuilder::new()
                                    .color_blend_type(ColorBlendType::Layer1),
                            );
                        }
                    }
                }
            }
        }
    }
}
impl Quad {
    fn render(
        &self,
        pass: &mut RenderPass,
        atlas: Option<&CharacterTextures>,
        thumbnails: &[Option<TextureSource<'_>>; 100],
    ) {
        let [l, t, r, b] = self.rect;
        let [u, v, s, w] = self.uv;
        let c = self.color.to_le_bytes();
        let vertices = QuadVertices::new()
            .with_box(l, t, r, b)
            .with_tex_box(u, v, s, w)
            .with_color(UnormColor::from_rgba(c[2], c[1], c[0], c[3]));
        let transform = HigurashiTransform::convert_clip_depth(self.matrix);
        let builder = RenderRequestBuilder::new().color_blend_type(if self.opaque {
            ColorBlendType::Opaque
        } else {
            ColorBlendType::Layer1
        });
        let texture = match self.texture {
            Texture::Fill => {
                vertices.fill(pass, builder, transform);
                return;
            }
            Texture::Atlas(name) => atlas.expect("SaveMenu atlas").get(name).as_source(),
            Texture::Thumbnail(index) => thumbnails[index].expect("SaveMenu published thumbnail"),
        };
        if self.flash > 0.0 {
            vertices.render_button(
                pass,
                builder,
                texture,
                transform,
                vec4(1.0, 1.0, 1.0, self.flash),
                Vec4::ZERO,
            );
        } else {
            vertices.render_sprite(pass, builder, texture, transform);
        }
    }
}

#[cfg(test)]
mod tests;
