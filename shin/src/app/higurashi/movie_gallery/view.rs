//! Actual93e28/94470 MovieMode parent packets. The owner subsequently draws its
//! private ModeStack snapshot (including First's program6/18 capture overlay).
use super::ViewState;
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
    // Actual150d24 has three 12-byte entries and exactly one key each.
    const HINTS: [(i32, i32, i32, i32); 3] = [(0, -1, 0, 214), (2, -1, 48, 76), (1, -1, 96, 78)];
    let (first, second, row, width) = HINTS[id];
    let mut left = right - width as f32;
    let v = row as f32 / 144.;
    quad(
        out,
        "help_moviemode",
        p,
        [left, 1012., left + width as f32, 1060.],
        [
            0.,
            v,
            width as f32 / 320. + 0.,
            v + f32::from_bits(0x3eaaaaab),
        ],
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
    // 940a0..940f4 composes the parent with (1848,210) first; c0574 then
    // applies its own16px cap offset. Preserve both rounded products rather
    // than folding those translations before the projection multiply.
    multiply(
        multiply(p, Mat4::from_translation(vec3(1848., 210., 0.))),
        Mat4::from_translation(vec3(0., 16., 0.)),
    )
}
pub(super) fn completion_layout(font: impl FontMetrics + Clone, text: &str) -> Result<Layout> {
    // Original938a0 SimpleText width640,size40,outline1. d19f4 insets by4;
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
pub(super) fn packets(state: &ViewState, size: Vec2, swapped: bool) -> Vec<Draw> {
    if !state.ready {
        return vec![];
    }
    let p = projection();
    let mut out = vec![];
    quad(&mut out, "bg", p, [0., 0., 1920., 1080.], FULL, u32::MAX);
    if let Some(Draw::Quad(bg)) = out.last_mut() {
        bg.opaque = true;
    }
    let origin = state.scroll + -210.;
    for row in &state.rows {
        let grow = (row.focus + -0.5) * 0.5;
        let width = grow * 75.;
        let height = grow * 42.;
        let left = (row.x + 138.) - width * 0.5;
        let top = (row.y + 0.) - origin - height * 0.5;
        let rect = [left, top, (width + 384.) + left, (height + 216.) + top];
        let gray = ((row.focus.max(0.) * 255.) as u32).min(255);
        let color = 0xff000000 | gray | (gray << 8) | (gray << 16);
        quad(
            &mut out,
            if row.locked {
                "logo".to_owned()
            } else {
                format!("mv{:02}", u32::from(row.movie_id) + 1)
            },
            p,
            rect,
            FULL,
            color,
        );
        // 93ec0's blend1 survives the entire thumbnail loop until94084.
        if let Some(Draw::Quad(q)) = out.last_mut() {
            q.opaque = true;
        }
    }
    // Authored12 rows produce720 exactly; greater heights are only explicit
    // component seams until an actual catalog can reach that branch.
    if state.document_height > 720 {
        for vertices in browser_decorations::scrollbar(720., state.scrollbar) {
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
        close.flash = state.close_flash;
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
    for id in [1, 2, 0] {
        let disabled = id == 2 && state.rows.get(state.selected).is_some_and(|r| r.locked);
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
    /// published. These19 names are actually requested by native93e28/94470.
    pub fn validate_atlas(atlas: &CharacterTextures) -> Result<()> {
        for name in [
            "bg",
            "logo",
            "obi",
            "caption",
            "close",
            "help_moviemode",
            "key",
        ] {
            ensure!(atlas.contains(name), "Movie archive is missing {name}");
        }
        for id in 1..=12 {
            let name = format!("mv{id:02}");
            ensure!(atlas.contains(&name), "Movie archive is missing {name}");
        }
        Ok(())
    }
    pub fn synchronize(&mut self, server: &AssetServer, state: &ViewState) -> Result<()> {
        if self
            .completion
            .as_ref()
            .is_some_and(|(t, _)| t == &state.completion)
        {
            return Ok(());
        }
        let layout = completion_layout(self.font.clone(), &state.completion)?;
        let c = server.context();
        let glyphs = Glyphs::from_layout(&c.wgpu_device, &c.wgpu_queue, self.font.clone(), layout);
        self.completion = Some((state.completion.clone(), glyphs));
        Ok(())
    }
    /// Parent draws are atlas-gated; private First drawing belongs AFTER this
    /// method and is permitted while the worker is still pending/unpublished.
    pub fn render(
        &self,
        state: &ViewState,
        atlas: Option<&CharacterTextures>,
        system: &CharacterTextures,
        swapped: bool,
        pass: &mut RenderPass,
    ) -> Result<()> {
        ensure!(
            atlas.is_none() || self.completion.is_some(),
            "Movie completion text is not synchronized"
        );
        let size = self
            .completion
            .as_ref()
            .map_or(Vec2::ZERO, |(_, g)| g.layout.size);
        ensure!(
            state.ready == atlas.is_some(),
            "Movie atlas publication and view state disagree"
        );
        ensure!(
            !state.ready || state.selected < state.rows.len(),
            "Movie selected row outside gallery"
        );
        for draw in packets(state, size, swapped) {
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
#[path = "view_tests.rs"]
mod tests;
