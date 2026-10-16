//! Updated daf38/db88c TIPS parent packets and retained completion glyphs.
use crate::render::NATIVE_UI_PROJECTION;
use super::Model;
use crate::{
    adv::{
        characters::view::CharacterTextures,
        higurashi::{
            browser_controls::Scrollbar,
            browser_decorations,
            browser_navigation::labels::{Glyphs, Layout},
        },
    },
    app::higurashi::kakera_menu::view::Packet,
    asset::{font::GpuFontLazy, system::AssetServer},
    layer::render_params::multiply_transforms as multiply,
};
use anyhow::Result;
use glam::{Mat4, Vec4, vec3, vec4};
use shin_core::layout::LayoutParams;
use shin_render::{
    RenderRequestBuilder, render_pass::RenderPass, shaders::types::vertices::PosColTexVertex,
};
use std::sync::Arc;

pub(super) enum Draw {
    Quad(Packet),
    Scroll {
        matrix: Mat4,
        vertices: [PosColTexVertex; 8],
    },
    Completion(Mat4),
}
const FULL: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
fn quad(texture: impl Into<String>, rect: [f32; 4], uv: [f32; 4], color: u32) -> Draw {
    Draw::Quad(Packet {
        texture: texture.into(),
        matrix: NATIVE_UI_PROJECTION,
        rect,
        uv,
        color,
        opaque: false,
        flash: 0.0,
    })
}
fn alpha(value: f32) -> u32 {
    (u32::from((value.max(0.0) * 255.0) as u8) << 24) | 0x00ffffff
}
/// Scrollbar metrics are the original pre-clock-update c0568 snapshot. The
/// owning scene samples those before advancing scroll;do not resample here.
pub(super) fn packets(model: &Model, atlas_present: bool, scrollbar: Scrollbar) -> Vec<Draw> {
    let mut out = Vec::new();
    if !atlas_present {
        return out;
    }
    let mut background = quad("bg", [0.0, 0.0, 1920.0, 1080.0], FULL, u32::MAX);
    if let Draw::Quad(ref mut p) = background {
        p.opaque = true;
    }
    out.push(background);
    for (index, row) in model.rows.iter().enumerate() {
        let y = (index / 2 * 92) as f32 - (model.scroll.value() - 162.0);
        if !(-92.0..1080.0).contains(&y) {
            continue;
        }
        let x = (index % 2 * 820 + 150) as f32;
        let y = y + 2.0;
        let rect = [x, y, x + 800.0, y + 88.0];
        let v = if row.current { 0.5 } else { 0.0 };
        out.push(quad("plate", rect, [0.0, v, 1.0, v + 0.5], u32::MAX));
        if index == model.selected {
            let angle = model.background_phase * f32::from_bits(0x40490fd0);
            let opacity =
                (angle + angle).sin() * f32::from_bits(0x3e4ccccd) + f32::from_bits(0x3ecccccd);
            out.push(quad("cur", rect, FULL, alpha(opacity)));
        }
        if row.status == 1 && row.artwork_available {
            out.push(quad(
                "new",
                [x, y + 26.0, x + 108.0, y + 62.0],
                FULL,
                u32::MAX,
            ));
        }
        let id = if row.status > 0 && row.artwork_available {
            row.atlas_id
        } else {
            0
        };
        let v = f32::from(id & 31) * 64.0 * f32::from_bits(0x3a000000);
        out.push(quad(
            format!("title{}", id >> 5),
            [x + 80.0, y + 12.0, x + 720.0, y + 76.0],
            [0.0, v, 1.0, v + f32::from_bits(0x3d000000)],
            u32::MAX,
        ));
        if row.flash.value() > 0.0 {
            out.push(quad("flash", rect, FULL, alpha(row.flash.value())));
        }
    }
    let parent = multiply(
        NATIVE_UI_PROJECTION,
        Mat4::from_translation(vec3(1848.0, 162.0, 0.0)),
    );
    let matrix = browser_decorations::scrollbar_transform(parent, 0.0, 0.0);
    for vertices in browser_decorations::scrollbar(818.0, scrollbar) {
        out.push(Draw::Scroll { matrix, vertices });
    }
    out.push(quad(
        "obi",
        [0.0, 0.0, 1920.0, 150.0],
        [0.0, 0.0, 1.0, 0.625],
        0xd8ffffff,
    ));
    out.push(quad("caption", [0.0, 0.0, 600.0, 144.0], FULL, u32::MAX));
    let mut close = quad("close", [1764.0, 30.0, 1848.0, 114.0], FULL, u32::MAX);
    if let Draw::Quad(ref mut p) = close {
        p.flash = model.feedback.value();
    }
    out.push(close);
    out.push(quad(
        "obi",
        [0.0, 992.0, 1920.0, 1080.0],
        [0.0, f32::from_bits(0x3f222222), 1.0, 1.0],
        0xd8ffffff,
    ));
    const HINTS: [(i32, i32, u32, u32); 3] = [(5, 0, 0, 214), (2, -1, 48, 76), (1, -1, 96, 78)];
    let mut right = 1872.0;
    for id in [1, 2, 0] {
        let (key, second, row, width) = HINTS[id];
        let color = if id == 2
            && (model.rows[model.selected].status == 0
                || !model.rows[model.selected].artwork_available)
        {
            0xff808080
        } else {
            u32::MAX
        };
        let left = right - width as f32;
        let v = row as f32 / 192.0;
        out.push(quad(
            "help_tipsmode",
            [left, 1012.0, right, 1060.0],
            [0.0, v, width as f32 / 320.0, v + 0.25],
            color,
        ));
        right = left;
        for key in [key, second].into_iter().filter(|k| *k >= 0) {
            right -= 48.0;
            let u = (key * 48) as f32 / 288.0;
            out.push(quad(
                "key",
                [right, 1012.0, right + 48.0, 1060.0],
                [u, 0.0, u + f32::from_bits(0x3e2aaaab), 1.0],
                color,
            ));
        }
        right -= 10.0;
    }
    let local = multiply(
        Mat4::from_translation(vec3(48.0, 1012.0, 0.0)),
        Mat4::from_scale(vec3(1.0, 1.0, 0.0)),
    );
    out.push(Draw::Completion(multiply(NATIVE_UI_PROJECTION, local)));
    out
}
pub(super) struct View {
    font: Arc<GpuFontLazy>,
    text: String,
    pub completion: Option<Glyphs>,
}
impl View {
    pub fn new(font: Arc<GpuFontLazy>) -> Self {
        Self {
            font,
            text: String::new(),
            completion: None,
        }
    }
    pub fn synchronize(&mut self, server: &AssetServer, model: &Model) -> Result<()> {
        if self.text == model.completion_text {
            return Ok(());
        }
        let layout = Layout::from_script(
            self.font.clone(),
            &model.completion_text,
            LayoutParams {
                layout_width: 640.0,
                text_size: 40.0,
                perform_soft_breaks: true,
                ..Default::default()
            },
        )?;
        let context = server.context();
        self.completion = Some(Glyphs::from_layout(
            &context.wgpu_device,
            &context.wgpu_queue,
            self.font.clone(),
            layout,
        ));
        self.text.clone_from(&model.completion_text);
        Ok(())
    }
    pub fn render(
        &self,
        model: &Model,
        atlas: Option<&CharacterTextures>,
        system: &CharacterTextures,
        scrollbar: Scrollbar,
        pass: &mut RenderPass,
    ) {
        for draw in packets(model, atlas.is_some(), scrollbar) {
            match draw {
                Draw::Quad(p) => p.render(pass, atlas),
                Draw::Scroll { matrix, vertices } => {
                    browser_decorations::sprite(pass, system, "scroll", &vertices, matrix)
                }
                Draw::Completion(matrix) => {
                    if let Some(glyphs) = &self.completion {
                        glyphs.render_at(
                            pass,
                            matrix,
                            Vec4::ONE,
                            vec4(0.0, 0.0, 0.0, 240.0 * f32::from_bits(0x3b808081)),
                            RenderRequestBuilder::new()
                                .color_blend_type(shin_render::ColorBlendType::Layer1),
                        );
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
