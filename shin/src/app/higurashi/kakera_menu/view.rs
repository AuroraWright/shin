//! Updated63558 parent composition and641b0 controller hints.
use crate::render::NATIVE_UI_PROJECTION;
use super::{Model, catalog, motion::Motion};
use crate::{
    adv::{
        characters::view::CharacterTextures,
        higurashi::browser_navigation::labels::{Glyphs, Layout},
    },
    asset::{font::GpuFontLazy, system::AssetServer},
    layer::render_params::{higurashi::HigurashiTransform, multiply_transforms as multiply},
};
use anyhow::Result;
use glam::{Mat4, Vec4, vec3, vec4};
use shin_core::{layout::LayoutParams, primitives::color::UnormColor};
use shin_render::{
    ColorBlendType, RenderRequestBuilder, quad_vertices::QuadVertices, render_pass::RenderPass,
};
use std::sync::Arc;

pub(in crate::app::higurashi) struct Packet {
    pub texture: String,
    pub matrix: Mat4,
    pub rect: [f32; 4],
    pub uv: [f32; 4],
    /// Original packed BGRA, converted to RGBA only at GPU submission.
    pub color: u32,
    pub opaque: bool,
    pub flash: f32,
}
pub(super) enum Draw {
    Quad(Packet),
    Details(Mat4),
}
fn alpha(value: f32) -> u32 {
    u32::from((value.max(0.0) * 255.0) as u8) << 24
}
fn quad(
    texture: impl Into<String>,
    matrix: Mat4,
    rect: [f32; 4],
    uv: [f32; 4],
    color: u32,
) -> Draw {
    Draw::Quad(Packet {
        texture: texture.into(),
        matrix,
        rect,
        uv,
        color,
        opaque: false,
        flash: 0.0,
    })
}
const FULL: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
/// Native traversal appends child tasks after this complete parent sequence.
pub(super) fn packets(motion: &Motion, atlas_present: bool) -> Vec<Draw> {
    let m = &motion.model;
    let projection = NATIVE_UI_PROJECTION;
    let mut out = Vec::new();
    if atlas_present {
        let mut bg = quad(
            "ks_bg",
            projection,
            [0.0, 0.0, 1920.0, 1080.0],
            FULL,
            u32::MAX,
        );
        if let Draw::Quad(ref mut p) = bg {
            p.opaque = true;
        }
        out.push(bg);
        let page = multiply(
            projection,
            Mat4::from_translation(vec3(motion.clocks[7].value(), 0.0, 0.0)),
        );
        let suffix = if m.alternate { "_org" } else { "" };
        if m.any_artwork() { out.push(quad(
            format!("k_title{:02}{suffix}", m.page + 1),
            page,
            [526.0, 152.0, 1390.0, 278.0],
            FULL,
            u32::MAX,
        ));
        }
        for (cell, &fragment) in m.pages[m.page].iter().enumerate() {
            if !m.cell_available(m.page, cell) { continue; }
            let (left, top) = catalog::CELLS[cell];
            let (x, y) = (left as f32, top as f32);
            let status = m.statuses[fragment];
            out.push(quad(
                format!("{fragment:02}{suffix}"),
                page,
                [x, y, x + 432.0, y + 244.0],
                FULL,
                if status > 0 { u32::MAX } else { 0xff808080 },
            ));
            let paired = match (m.alternate, fragment) {
                (false, 60) => Some(103),
                (false, 62) => Some(104),
                (true, 49) => Some(52),
                _ => None,
            };
            let paired_new = paired.is_some_and(|id| m.statuses[id] == 1);
            if status == 1 || paired_new {
                let badge =
                    ["new", "new2", "new3"][usize::from(m.alternate) + usize::from(paired_new)];
                out.push(quad(
                    badge,
                    page,
                    [x + 286.0, y + 176.0, x + 432.0, y + 242.0],
                    FULL,
                    u32::MAX,
                ));
            }
            if motion.weights[cell] > 0.0 {
                out.push(quad(
                    "cursor",
                    page,
                    [x - 9.0, y - 9.0, x + 441.0, y + 253.0],
                    FULL,
                    0x00ffffff | alpha(motion.weights[cell]),
                ));
            }
            if motion.clocks[1 + cell].value() > 0.0 {
                out.push(quad(
                    "",
                    page,
                    [x, y, x + 432.0, y + 244.0],
                    FULL,
                    0x00ffffff | alpha(motion.clocks[1 + cell].value()),
                ));
            }
        }
        if !m.details().is_empty() {
            out.push(quad(
                "window",
                page,
                [0.0, 794.0, 1920.0, 1130.0],
                FULL,
                u32::MAX,
            ));
            out.push(quad(
                "hint",
                page,
                [468.0, 794.0, 1452.0, 866.0],
                FULL,
                u32::MAX,
            ));
            out.push(Draw::Details(page));
        }
        let notice = motion.clocks[9].value();
        if notice > 0.0 {
            let transform = multiply(
                page,
                multiply(
                    Mat4::from_translation(vec3(0.0, 526.0, 0.0)),
                    Mat4::from_scale(vec3(1.0, notice, 1.0)),
                ),
            );
            out.push(quad(
                "warning",
                transform,
                [0.0, -148.0, 1920.0, 146.0],
                FULL,
                0x00ffffff | alpha(notice),
            ));
        }
        out.push(quad(
            "obi",
            projection,
            [0.0, 0.0, 1920.0, 150.0],
            [0.0, 0.0, 1.0, 0.625],
            0xd8ffffff,
        ));
        out.push(quad(
            "caption",
            projection,
            [72.0, 0.0, 672.0, 144.0],
            FULL,
            u32::MAX,
        ));
        if m.can_exit() {
            let mut back = quad(
                "close",
                projection,
                [1764.0, 30.0, 1848.0, 114.0],
                FULL,
                u32::MAX,
            );
            if let Draw::Quad(ref mut p) = back {
                p.flash = motion.clocks[0].value();
            }
            out.push(back);
        }
        out.push(quad(
            "obi",
            projection,
            [0.0, 992.0, 1920.0, 1080.0],
            [0.0, f32::from_bits(0x3f222222), 1.0, 1.0],
            0xd8ffffff,
        ));
        let mut right = 1872.0;
        let hints: &[usize] = if !m.can_exit() {
            &[2, 3, 0]
        } else {
            &[1, 2, 3, 0]
        };
        // cc940 returns0 in this Switch executable:27560 preserves key IDs.
        const HINTS: [(u32, u32, u32); 4] = [(0, 0, 214), (2, 48, 76), (1, 96, 78), (5, 144, 248)];
        for &id in hints {
            let (key, row, width) = HINTS[id];
            let color = if id == 2 && (m.statuses[m.selected_fragment()] == 0 || !m.cell_available(m.page, m.selected)) {
                0xff808080
            } else {
                u32::MAX
            };
            let left = right - width as f32;
            let v = row as f32 / 192.0;
            out.push(quad(
                "help_tsumugi",
                projection,
                [left, 1012.0, right, 1060.0],
                [0.0, v, width as f32 / 320.0, v + 0.25],
                color,
            ));
            let left = left - 48.0;
            let u = (key * 48) as f32 / 288.0;
            out.push(quad(
                "key",
                projection,
                [left, 1012.0, left + 48.0, 1060.0],
                [u, 0.0, u + f32::from_bits(0x3e2aaaab), 1.0],
                color,
            ));
            right = left - 10.0;
        }
    }
    if motion.clocks[8].value() > 0.0 {
        out.push(quad(
            "",
            projection,
            [0.0, 0.0, 1920.0, 1080.0],
            FULL,
            alpha(motion.clocks[8].value()),
        ));
    }
    out
}
impl Packet {
    pub fn render(&self, pass: &mut RenderPass, atlas: Option<&CharacterTextures>) {
        let [l, t, r, b] = self.rect;
        let [u, v, s, w] = self.uv;
        let bytes = self.color.to_le_bytes();
        let vertices = QuadVertices::new()
            .with_box(l, t, r, b)
            .with_tex_box(u, v, s, w)
            .with_color(UnormColor::from_rgba(
                bytes[2], bytes[1], bytes[0], bytes[3],
            ));
        let transform = HigurashiTransform::convert_clip_depth(self.matrix);
        let builder = RenderRequestBuilder::new().color_blend_type(if self.opaque {
            ColorBlendType::Opaque
        } else {
            ColorBlendType::Layer1
        });
        if self.texture.is_empty() {
            vertices.fill(pass, builder, transform);
        } else if self.flash > 0.0 {
            vertices.render_button(
                pass,
                builder,
                atlas.expect("Kakera atlas").get(&self.texture).as_source(),
                transform,
                vec4(1.0, 1.0, 1.0, self.flash),
                Vec4::ZERO,
            );
        } else {
            vertices.render_sprite(
                pass,
                builder,
                atlas.expect("Kakera atlas").get(&self.texture).as_source(),
                transform,
            );
        }
    }
}
pub(super) struct View {
    font: Arc<GpuFontLazy>,
    selected: Option<(bool, usize)>,
    pub details: Vec<(Glyphs, Vec4)>,
}
impl View {
    pub fn new(font: Arc<GpuFontLazy>) -> Self {
        Self {
            font,
            selected: None,
            details: Vec::new(),
        }
    }
    pub fn synchronize(&mut self, server: &AssetServer, model: &Model) -> Result<()> {
        let selected = (model.alternate, model.selected_fragment());
        if self.selected == Some(selected) {
            return Ok(());
        }
        let context = server.context();
        let details = model
            .details()
            .into_iter()
            .map(|detail| {
                let layout = Layout::from_script(
                    self.font.clone(),
                    detail.title,
                    LayoutParams {
                        layout_width: 636.0,
                        text_size: 45.0,
                        perform_soft_breaks: true,
                        ..Default::default()
                    },
                )?;
                let glyphs = Glyphs::from_layout(
                    &context.wgpu_device,
                    &context.wgpu_queue,
                    self.font.clone(),
                    layout,
                );
                let rgb = if detail.grey {
                    f32::from_bits(0x3f333333)
                } else {
                    1.0
                };
                Ok((glyphs, vec4(rgb, rgb, rgb, 1.0)))
            })
            .collect::<Result<Vec<_>>>()?;
        self.details = details;
        self.selected = Some(selected);
        Ok(())
    }
    pub fn render(
        &self,
        motion: &Motion,
        atlas: Option<&CharacterTextures>,
        pass: &mut RenderPass,
    ) {
        for draw in packets(motion, atlas.is_some()) {
            match draw {
                Draw::Quad(mut p) => {
                    if matches!(p.texture.as_str(), "new" | "new2" | "new3") {
                        if let Some(atlas) = atlas {
                            if !atlas.contains(&p.texture) {
                                continue;
                            }
                            let size = atlas.get(&p.texture).size_vec();
                            p.rect[0] = p.rect[2] - size.x;
                            p.rect[1] = p.rect[3] - size.y;
                        }
                    }
                    p.render(pass, atlas);
                }
                Draw::Details(parent) => {
                    for (i, (glyphs, color)) in self.details.iter().enumerate() {
                        let native = multiply(
                            parent,
                            multiply(
                                Mat4::from_translation(vec3(
                                    if i % 2 == 0 { 300.0 } else { 1050.0 },
                                    if i < 2 { 872.0 } else { 932.0 },
                                    0.0,
                                )),
                                Mat4::from_scale(vec3(1.0, 1.0, 0.0)),
                            ),
                        );
                        glyphs.render_at(
                            pass,
                            native,
                            *color,
                            vec4(0.0, 0.0, 0.0, 240.0 * f32::from_bits(0x3b808081)),
                            RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1),
                        );
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
