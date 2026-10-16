//! Actual older48de8 Chart parent composition. The owner draws the mixed child
//! scope after these packets; this view does not fabricate a Question overlay.
use crate::render::NATIVE_UI_PROJECTION;
use super::model::Model;
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
    layer::render_params::{higurashi::HigurashiTransform, multiply_transforms as multiply},
};
use anyhow::{Result, ensure};
use glam::{Mat4, Vec2, Vec4, vec3, vec4};
use shin_core::layout::{LayoutParams, font::FontMetrics};
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    render_pass::RenderPass,
    shaders::types::{
        buffer::VertexSource,
        vertices::PosColTexVertex,
    },
};
use std::sync::Arc;
pub(super) enum Draw {
    Quad(Packet),
    Scrollbar {
        matrix: Mat4,
        vertices: [PosColTexVertex; 8],
    },
    Text {
        index: usize,
        matrix: Mat4,
    },
}
const FULL: [f32; 4] = [0., 0., 1., 1.];
const PALETTE: [u32; 7] = [
    0xffff99cb, 0xffffd8eb, 0xff14c3f5, 0xffeb2020, 0xfff6e024, 0xfff49922, 0xff29e66b,
];
fn quad(out: &mut Vec<Draw>, name: &str, matrix: Mat4, rect: [f32; 4], uv: [f32; 4], color: u32) {
    out.push(Draw::Quad(Packet {
        texture: name.to_owned(),
        matrix,
        rect,
        uv,
        color,
        opaque: false,
        flash: 0.,
    }));
}
fn flash(out: &mut Vec<Draw>, name: &str, matrix: Mat4, rect: [f32; 4], uv: [f32; 4], value: f32) {
    // Actual9ba18 emits exactly one regular/flash draw, retaining packedwhite.
    quad(out, name, matrix, rect, uv, u32::MAX);
    if let Some(Draw::Quad(p)) = out.last_mut() {
        p.flash = value;
    }
}
fn packed(rgba: [f32; 4]) -> u32 {
    let c = rgba.map(|v| ((v.max(0.) * 255.) as u32).min(255));
    c[0] << 16 | c[1] << 8 | c[2] | c[3] << 24
}
fn shaded(color: u32, tint: f32) -> u32 {
    let f = f32::from_bits(0x3b808081);
    packed([
        ((color >> 16) & 255) as f32 * f * tint,
        ((color >> 8) & 255) as f32 * f * tint,
        (color & 255) as f32 * f * tint,
        ((color >> 24) & 255) as f32 * f,
    ])
}
fn hint(
    out: &mut Vec<Draw>,
    p: Mat4,
    right: f32,
    y: f32,
    id: usize,
    color: u32,
    swapped: bool,
) -> f32 {
    let (key, band, width) = [(0, 0, 214), (2, 48, 76), (1, 96, 246), (5, 144, 214)][id];
    let l = right - width as f32;
    let v = band as f32 / 192.;
    quad(
        out,
        "help_chart",
        p,
        [l, y, l + width as f32, y + 48.],
        [0., v, width as f32 / 320. + 0., v + 0.25],
        color,
    );
    let l = l - 48.;
    let key = if swapped {
        match key {
            1 => 2,
            2 => 1,
            k => k,
        }
    } else {
        key
    };
    let u = (key * 48) as f32 / 288.;
    quad(
        out,
        "key",
        p,
        [l, y, l + 48., y + 48.],
        [u, 0., u + f32::from_bits(0x3e2aaaab), 1.],
        color,
    );
    l + (-10.)
}
fn text_matrix(parent: Mat4, pos: [f32; 2], size: glam::Vec2) -> Mat4 {
    let anchor = Mat4::from_translation(vec3(-size.x * 0., -size.y * 0., 0.));
    let scaled = multiply(Mat4::from_scale(vec3(1., 1., 0.)), anchor);
    multiply(
        parent,
        multiply(Mat4::from_translation(vec3(pos[0], pos[1], 0.)), scaled),
    )
}
pub(super) fn packets(
    m: &Model,
    ready: bool,
    bar: Scrollbar,
    sizes: [glam::Vec2; 2],
    swapped: bool,
) -> Vec<Draw> {
    let mut out = Vec::new();
    if !ready {
        return out;
    }
    let p = NATIVE_UI_PROJECTION;
    let slide = 1920. - m.progress * 1920.;
    let graph = multiply(p, Mat4::from_translation(vec3(slide, 0., 0.)));
    let g = &m.groups[m.current_group as usize];
    let scroll = g.scroll.value() + (-165.);
    for pass in 0..4 {
        for node in &m.nodes {
            if node.group != m.current_group {
                continue;
            }
            let bright = m.title || node.current;
            if (bright && pass == 1) || (!bright && pass == 2) {
                continue;
            }
            let tint = if bright { 1. } else { 0.5 };
            let rec = &m.catalog.records[node.record];
            let x = g.origin_x as f32 + (node.x.wrapping_mul(36) as f32);
            let y = (g.origin_y.wrapping_add(node.y.wrapping_mul(56)) as f32) - scroll;
            let grey = packed([0.15625, 0.15625, 0.15625, 1.]);
            let color = packed([tint, tint, tint, 1.]);
            let bottom = y + if rec.kind == 3 {
                (i32::from(rec.value) * 56) as f32
            } else {
                56.
            };
            if !(y < 1084. && bottom >= -4.) {
                continue;
            }
            match rec.kind {
                1 if pass == 0 || pass == 3 => {
                    let mut u = if (rec.value as i16) < 0 { 384. } else { 96. };
                    let color = if pass == 0 {
                        u += 144.;
                        grey
                    } else {
                        shaded(PALETTE[rec.label.as_ref().unwrap().extra as usize], tint)
                    };
                    let l = (x + 18.) + (-63.);
                    let t = (y + 28.) + (-35.);
                    let u = (u + 4.) / 768.;
                    quad(
                        &mut out,
                        "parts",
                        graph,
                        [l, t, l + 126., t + 70.],
                        [
                            u,
                            f32::from_bits(0x3c17b426),
                            u + 0.1640625,
                            f32::from_bits(0x3e2f684b),
                        ],
                        color,
                    );
                }
                2 if pass <= 2 && rec.value & 15 != 0 => {
                    let ux = (rec.value & 15) as f32 * 48.;
                    let uy = (rec.value >> 4) as f32 * 72. + if pass == 0 { 288. } else { 144. };
                    let u = (ux + 4.) / 768.;
                    let v = (uy + 4.) / 432.;
                    quad(
                        &mut out,
                        "parts",
                        graph,
                        [x, y, x + 36., bottom],
                        [u, v, u + 0.046875, v + f32::from_bits(0x3e04bda1)],
                        if pass == 0 { grey } else { color },
                    );
                }
                3 if pass <= 2 => {
                    let v = if pass == 0 { 76. } else { 4. } / 432.;
                    quad(
                        &mut out,
                        "parts",
                        graph,
                        [x, y, x + 36., bottom],
                        [
                            f32::from_bits(0x3baaaaab),
                            v,
                            f32::from_bits(0x3d555555),
                            v + f32::from_bits(0x3e04bda1),
                        ],
                        if pass == 0 { grey } else { color },
                    );
                }
                4 if pass <= 2 => {
                    let v = if pass == 0 { 76. } else { 4. } / 432.;
                    quad(
                        &mut out,
                        "parts",
                        graph,
                        [x, y, x + (i32::from(rec.value) * 36) as f32, bottom],
                        [
                            f32::from_bits(0x3d8aaaab),
                            v,
                            f32::from_bits(0x3deaaaab),
                            v + f32::from_bits(0x3e04bda1),
                        ],
                        if pass == 0 { grey } else { color },
                    );
                }
                _ => {}
            }
        }
    }
    if g.selected >= 0 {
        let n = &m.nodes[g.selected as usize];
        let r = &m.catalog.records[n.record];
        let x = g.origin_x as f32 + n.x.wrapping_mul(36) as f32;
        let y = g.origin_y.wrapping_add(n.y.wrapping_mul(56)) as f32 - scroll;
        let l = (x + 18.) + (-63.);
        let t = (y + 28.) + (-35.);
        let u = if (r.value as i16) < 0 {
            f32::from_bits(0x3f315555)
        } else {
            f32::from_bits(0x3ea2aaab)
        };
        let pulse = ((((m.frame & 63) as f32 * (1. / 64.)) * f32::from_bits(0x40c90fd0)).sin()
            * 0.25)
            + 0.75;
        quad(
            &mut out,
            "parts",
            graph,
            [l, t, l + 126., t + 70.],
            [
                u,
                f32::from_bits(0x3c17b426),
                u + 0.1640625,
                f32::from_bits(0x3e2f684b),
            ],
            packed([1., 1., 1., pulse]),
        );
    }
    let bar_parent = multiply(p, Mat4::from_translation(vec3(slide + 1848., 165., 0.)));
    let matrix = browser_decorations::scrollbar_transform(bar_parent, 0., 0.);
    for vertices in browser_decorations::scrollbar(819., bar) {
        out.push(Draw::Scrollbar { matrix, vertices });
    }
    if m.tooltip >= 0 {
        let n = &m.nodes[m.tooltip as usize];
        let width = sizes[1].x;
        let raw_x = g.origin_x.wrapping_add(n.x.wrapping_mul(36)) as f32;
        let lower = (n.y.wrapping_mul(56).wrapping_add(56) as f32 - scroll) + g.origin_y as f32;
        let upper = (n.y.wrapping_mul(56).wrapping_sub(48) as f32 - scroll) + g.origin_y as f32;
        let y = if lower + 48. < 984. { lower } else { upper };
        let x = if raw_x + width > 1752. {
            1752. - width
        } else {
            raw_x
        };
        let right = width + x;
        for (rect, uv) in [
            ([x + (-24.), y, x, y + 48.], [0., 0., 0.5, 1.]),
            ([x, y, right, y + 48.], [0.5, 0., 0.5, 1.]),
            ([right, y, right + 24., y + 48.], [0.5, 0., 1., 1.]),
        ] {
            quad(&mut out, "window", graph, rect, uv, 0xa0000000);
        }
        out.push(Draw::Text {
            index: 1,
            matrix: text_matrix(graph, [x, y + 10.], sizes[1]),
        });
    }
    let header = m.progress * 150. + (-150.);
    quad(
        &mut out,
        "obi",
        p,
        [0., header, 1920., header + 150.],
        [0., 0., 1., 0.625],
        0xd8ffffff,
    );
    quad(
        &mut out,
        "caption",
        p,
        [72., header, 672., header + 144.],
        FULL,
        u32::MAX,
    );
    if m.title {
        let y = header + 54.;
        let v = m.current_group.wrapping_mul(72) as f32 / 360.;
        quad(
            &mut out,
            "books",
            p,
            [900., y, 1428., y + 72.],
            [0., v, 1., v + f32::from_bits(0x3e4ccccd)],
            u32::MAX,
        );
        if m.available.len() >= 2 {
            for (rect, uv, value) in [
                (
                    [828., y, 900., y + 72.],
                    [0., 0., 0.5, 1.],
                    m.flashes[1].value(),
                ),
                (
                    [1428., y, 1500., y + 72.],
                    [0.5, 0., 1., 1.],
                    m.flashes[2].value(),
                ),
            ] {
                flash(&mut out, "arrow", p, rect, uv, value);
            }
        }
    }
    let y = header + 30.;
    flash(
        &mut out,
        "close",
        p,
        [1764., y, 1848., y + 84.],
        FULL,
        m.flashes[0].value(),
    );
    let bottom = 1080. - m.progress * 88.;
    quad(
        &mut out,
        "obi",
        p,
        [0., bottom, 1920., bottom + 88.],
        [0., f32::from_bits(0x3f222222), 1., 1.],
        0xd8ffffff,
    );
    let y = bottom + 20.;
    let x = hint(&mut out, p, 1872., y, 1, u32::MAX, swapped);
    let valid = g.selected >= 0
        && (m.catalog.records[m.nodes[g.selected as usize].record].value as i16) >= 0;
    let mut x = hint(
        &mut out,
        p,
        x,
        y,
        2,
        if valid { u32::MAX } else { 0xff808080 },
        swapped,
    );
    if m.title {
        x = hint(
            &mut out,
            p,
            x,
            y,
            3,
            if m.available.len() > 1 {
                u32::MAX
            } else {
                0xff808080
            },
            swapped,
        );
    }
    hint(&mut out, p, x, y, 0, u32::MAX, swapped);
    out.push(Draw::Text {
        index: 0,
        matrix: text_matrix(
            multiply(p, Mat4::from_translation(vec3(48., y, 0.))),
            [0., 0.],
            sizes[0],
        ),
    });
    let x = g.x.value() + g.origin_x as f32;
    let y = (g.y.value() + g.origin_y as f32) - scroll;
    quad(
        &mut out,
        "cursor",
        p,
        [x, y, x + 48., y + 48.],
        FULL,
        u32::MAX,
    );
    out
}
fn label_layout(font: impl FontMetrics + Clone, text: &str) -> Result<Layout> {
    // The Chart tooltip SimpleText has zero measured size before a label
    // is assigned (native empty-tooltip profiles), rather than a blank line.
    if text.is_empty() {
        return Ok(Layout {
            size: Vec2::ZERO,
            glyphs: Vec::new(),
        });
    }
    Layout::unpadded(
        font,
        text,
        LayoutParams {
            layout_width: 640.0,
            text_size: 32.0,
            perform_soft_breaks: false,
            ..Default::default()
        },
    )
}
pub(super) fn completion_layout(font: impl FontMetrics + Clone, text: &str) -> Result<Layout> {
    //472a4..472b8 passes width640/font40 and outline1. Native d19f4
    //insets layout width by4, then adds2 to character positions and4 to size.
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
struct Cached {
    text: String,
    glyphs: Glyphs,
}
pub(super) struct View {
    font: Arc<GpuFontLazy>,
    cached: [Option<Cached>; 2],
}
impl View {
    pub fn new(font: Arc<GpuFontLazy>) -> Self {
        Self {
            font,
            cached: [None, None],
        }
    }
    pub fn synchronize(&mut self, server: &AssetServer, m: &Model) -> Result<()> {
        for (i, text) in [&m.completion_text, &m.tooltip_text]
            .into_iter()
            .enumerate()
        {
            if self.cached[i].as_ref().is_some_and(|v| &v.text == text) {
                continue;
            }
            let layout = if i == 0 {
                completion_layout(self.font.clone(), text)?
            } else {
                label_layout(self.font.clone(), text)?
            };
            let ctx = server.context();
            let glyphs =
                Glyphs::from_layout(&ctx.wgpu_device, &ctx.wgpu_queue, self.font.clone(), layout);
            self.cached[i] = Some(Cached {
                text: text.clone(),
                glyphs,
            });
        }
        Ok(())
    }
    pub fn render(
        &self,
        m: &Model,
        bar: Scrollbar,
        atlas: Option<&CharacterTextures>,
        system: &CharacterTextures,
        swapped: bool,
        pass: &mut RenderPass,
    ) -> Result<()> {
        let ready = atlas.is_some();
        ensure!(
            !ready || self.cached[0].is_some(),
            "Chart text has not been synchronized"
        );
        let sizes = std::array::from_fn(|i| {
            self.cached[i]
                .as_ref()
                .map_or(glam::Vec2::ZERO, |c| c.glyphs.layout.size)
        });
        for draw in packets(m, ready, bar, sizes, swapped) {
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
                Draw::Text { index, matrix } => {
                    if let Some(c) = &self.cached[index] {
                        c.glyphs.render_at(
                            pass,
                            matrix,
                            Vec4::ONE,
                            if index == 0 {
                                vec4(0., 0., 0., 240. * f32::from_bits(0x3b808081))
                            } else {
                                Vec4::ZERO
                            },
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
