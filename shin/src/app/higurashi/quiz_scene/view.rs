//! Original updated9d1b0 parent composition. Child captured overlays belong
//! to the scene adapter and render after this complete parent sequence.
use super::{ANSWER_POSITIONS, Model, motion::Motion, resources::Prepared};
use crate::{
    adv::{
        character_view::CharacterTextures,
        higurashi::browser_navigation::labels::{Glyphs, Layout},
    },
    app::higurashi::kakera_menu::view::Packet,
    asset::{font::GpuFontLazy, system::AssetServer},
    layer::render_params::{higurashi::HigurashiTransform, multiply_transforms as multiply},
};
use anyhow::{Result, ensure};
use glam::{Mat4, Vec4, vec3, vec4};
use shin_core::{layout::LayoutParams, primitives::color::UnormColor};
use shin_render::{
    ColorBlendType, PassKind, RenderRequestBuilder, quad_vertices::QuadVertices,
    render_pass::RenderPass,
};
use std::sync::Arc;

pub(super) enum Draw {
    Quad(Packet),
    Bustup {
        index: usize,
        opaque: bool,
    },
    Question {
        index: usize,
        packet: Packet,
    },
    Answers(Mat4),
    /// Actuald147c borrows the scope's last poll snapshot after parent drawing.
    Children,
}
const FULL: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
const MARKS: [[i32; 4]; 10] = [
    [576, 0, 336, 96],
    [900, 0, 240, 240],
    [1035, 240, 96, 312],
    [912, 552, 216, 240],
    [576, 696, 336, 96],
    [240, 696, 336, 96],
    [12, 552, 240, 240],
    [12, 232, 96, 336],
    [12, 0, 240, 240],
    [240, 0, 336, 96],
];
const ANSWER_ORIGINS: [[f32; 2]; 4] = [
    [624.0, 784.0],
    [1212.0, 860.0],
    [624.0, 936.0],
    [40.0, 860.0],
];
fn projection() -> Mat4 {
    Mat4::from_cols_array(&[
        2.0 / 1920.0,
        0.0,
        0.0,
        0.0,
        0.0,
        -2.0 / 1080.0,
        0.0,
        0.0,
        0.0,
        0.0,
        -1.0,
        0.0,
        -1.0,
        1.0,
        -0.0,
        1.0,
    ])
}
fn packet(
    texture: impl Into<String>,
    matrix: Mat4,
    rect: [f32; 4],
    uv: [f32; 4],
    color: u32,
) -> Packet {
    Packet {
        texture: texture.into(),
        matrix,
        rect,
        uv,
        color,
        opaque: false,
        flash: 0.0,
    }
}
fn alpha(value: f32) -> u32 {
    u32::from((value.max(0.0) * 255.0) as u8) << 24
}
fn slice(digit: i32, width: i32, total: f32, span: f32) -> [f32; 4] {
    let left = (digit * width) as f32 / total;
    [left, 0.0, left + span, 1.0]
}
pub(super) fn packets(motion: &Motion) -> Vec<Draw> {
    let m = &motion.model;
    let p = projection();
    let mut out = Vec::new();
    if motion.ready {
        let mut bg = packet("bg", p, [0.0, 0.0, 1920.0, 1080.0], FULL, 0xff808080);
        bg.opaque = true;
        out.push(Draw::Quad(bg));
        for opaque in [true, false] {
            out.push(Draw::Bustup {
                index: m.reaction_variant as usize,
                opaque,
            });
        }
        let mut quad =
            |name: &str, rect, uv, color| out.push(Draw::Quad(packet(name, p, rect, uv, color)));
        quad("window", [390.0, 12.0, 1530.0, 804.0], FULL, u32::MAX);
        quad("quota", [1547.0, 480.0, 1883.0, 816.0], FULL, u32::MAX);
        quad(
            "qnum",
            [1682.0, 642.0, 1754.0, 762.0],
            slice(m.clamped_argument, 72, 720.0, 0.1),
            u32::MAX,
        );
        if m.current_question >= 0 {
            quad("q", [900.0, 124.0, 1020.0, 244.0], FULL, u32::MAX);
        }
        if motion.question_elapsed >= 0.0 {
            let elapsed = motion.question_elapsed as i32;
            let remaining = 3600 - elapsed;
            let seconds = remaining / 60;
            let subsecond = remaining % 60;
            let hundredths = subsecond * 100 / 60;
            // Original9d5e8..9d5fc suppresses the leading digit after tick3000.
            if elapsed <= 3000 {
                quad(
                    "sec",
                    [588.0, 236.0, 780.0, 716.0],
                    slice(remaining / 600, 192, 1920.0, 0.1),
                    u32::MAX,
                );
            }
            if seconds % 10 >= 0 {
                quad(
                    "sec",
                    [780.0, 236.0, 972.0, 716.0],
                    slice(seconds % 10, 192, 1920.0, 0.1),
                    u32::MAX,
                );
            }
            quad("period", [972.0, 508.0, 1092.0, 718.0], FULL, u32::MAX);
            quad(
                "msec",
                [1092.0, 508.0, 1212.0, 718.0],
                slice(subsecond / 6, 120, 1200.0, 0.1),
                u32::MAX,
            );
            quad(
                "msec",
                [1212.0, 508.0, 1332.0, 718.0],
                slice(hundredths % 10, 120, 1200.0, 0.1),
                u32::MAX,
            );
        }
        for (i, [x, y, w, h]) in MARKS.into_iter().enumerate() {
            let (x, y) = (x as f32 + 390.0, y as f32 + 12.0);
            out.push(Draw::Quad(packet(
                format!(
                    "sig{}_{}",
                    i + 1,
                    if m.marks[i] == 0 { "off" } else { "on" }
                ),
                p,
                [x, y, x + w as f32, y + h as f32],
                FULL,
                u32::MAX,
            )));
        }
        let color = 0x00ffffff | alpha(m.clocks[2].value());
        let scale = m.clocks[1].value();
        if m.current_question >= 0 {
            let matrix = multiply(
                p,
                multiply(
                    Mat4::from_translation(vec3(960.0, 476.0, 0.0)),
                    Mat4::from_scale(vec3(scale, scale, 1.0)),
                ),
            );
            out.push(Draw::Question {
                index: m.current_question as usize,
                packet: packet("", matrix, [-480.0, -240.0, 480.0, 240.0], FULL, color),
            });
        }
        if motion.countdown_phase >= 0 {
            let phase = motion.countdown_phase;
            let matrix = multiply(
                p,
                multiply(
                    Mat4::from_translation(vec3(960.0, 408.0, 0.0)),
                    Mat4::from_scale(vec3(scale, scale, 1.0)),
                ),
            );
            let (name, rect, uv) = if phase <= 2 {
                (
                    "count",
                    [-140.0, -270.0, 142.0, 270.0],
                    slice(phase, 282, 846.0, f32::from_bits(0x3eaaaaab)),
                )
            } else {
                (
                    if phase == 3 { "start" } else { "timeup" },
                    [-480.0, -136.0, 480.0, 134.0],
                    FULL,
                )
            };
            out.push(Draw::Quad(packet(name, matrix, rect, uv, color)));
        }
        out.push(Draw::Quad(packet(
            "logo",
            p,
            [1473.0, 30.0, 1905.0, 162.0],
            FULL,
            u32::MAX,
        )));
        for (i, [x, y]) in ANSWER_ORIGINS.into_iter().enumerate() {
            out.push(Draw::Quad(packet(
                "answer_off",
                p,
                [x, y, x + 672.0, y + 132.0],
                FULL,
                u32::MAX,
            )));
            out.push(Draw::Quad(packet(
                ["triangle", "circle", "cross", "square"][i],
                p,
                [x + 70.0, y + 40.0, x + 124.0, y + 94.0],
                FULL,
                u32::MAX,
            )));
        }
        if m.current_question >= 0 {
            if m.clocks[2].is_idle() {
                out.push(Draw::Answers(p));
            }
            if m.chosen_answer >= 0 {
                let [x, y] = ANSWER_ORIGINS[m.chosen_answer as usize];
                out.push(Draw::Quad(packet(
                    "answer_on",
                    p,
                    [x, y, x + 672.0, y + 132.0],
                    FULL,
                    u32::MAX,
                )));
                let (x, y) = (x + 276.0, y + 6.0);
                out.push(Draw::Quad(packet(
                    if m.chosen_answer == m.correct_answer {
                        "ok"
                    } else {
                        "ng"
                    },
                    p,
                    [x, y, x + 120.0, y + 120.0],
                    FULL,
                    u32::MAX,
                )));
            }
        }
    }
    let fade = m.clocks[0].value();
    if fade > 0.0 {
        out.push(Draw::Quad(packet(
            "",
            p,
            [0.0, 0.0, 1920.0, 1080.0],
            FULL,
            alpha(fade),
        )));
    }
    out.push(Draw::Children);
    out
}

pub(super) struct View {
    font: Arc<GpuFontLazy>,
    text: Option<[&'static str; 4]>,
    pub answers: Vec<Glyphs>,
}
impl View {
    pub fn new(font: Arc<GpuFontLazy>) -> Self {
        Self {
            font,
            text: None,
            answers: Vec::new(),
        }
    }
    pub fn synchronize(&mut self, server: &AssetServer, model: &Model) -> Result<()> {
        let Some(answers) = model.answers() else {
            return Ok(());
        };
        if self.text == Some(answers) {
            return Ok(());
        }
        let context = server.context();
        self.answers = answers
            .into_iter()
            .map(|text| {
                let layout = Layout::from_script(
                    self.font.clone(),
                    text,
                    LayoutParams {
                        layout_width: 470.0,
                        text_size: 45.0,
                        perform_soft_breaks: true,
                        ..Default::default()
                    },
                )?;
                Ok(Glyphs::from_layout(
                    &context.wgpu_device,
                    &context.wgpu_queue,
                    self.font.clone(),
                    layout,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        self.text = Some(answers);
        Ok(())
    }
    pub fn render(
        &self,
        motion: &Motion,
        atlas: Option<&CharacterTextures>,
        prepared: Option<&Prepared>,
        pass: &mut RenderPass,
    ) -> Result<()> {
        ensure!(
            !motion.ready || (atlas.is_some() && prepared.is_some()),
            "Ready quiz renderer requires published resources"
        );
        for draw in packets(motion) {
            match draw {
                Draw::Quad(p) => p.render(pass, atlas),
                Draw::Bustup { index, opaque } => prepared.unwrap().bustups[index].render(
                    pass,
                    &HigurashiTransform::default(),
                    1,
                    if opaque {
                        PassKind::Opaque
                    } else {
                        PassKind::Transparent
                    },
                )?,
                Draw::Question { index, packet: p } => {
                    let [l, t, r, b] = p.rect;
                    let [u, v, s, w] = p.uv;
                    let bytes = p.color.to_le_bytes();
                    QuadVertices::new()
                        .with_box(l, t, r, b)
                        .with_tex_box(u, v, s, w)
                        .with_color(UnormColor::from_rgba(
                            bytes[2], bytes[1], bytes[0], bytes[3],
                        ))
                        .render_sprite(
                            pass,
                            RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1),
                            prepared.unwrap().questions[index]
                                .first_texture()
                                .as_source(),
                            HigurashiTransform::convert_clip_depth(p.matrix),
                        );
                }
                Draw::Answers(parent) => {
                    ensure!(
                        self.answers.len() == 4,
                        "Quiz answer glyph owners must synchronize before drawing"
                    );
                    for (glyphs, [x, y]) in self.answers.iter().zip(ANSWER_POSITIONS) {
                        let native = multiply(
                            parent,
                            multiply(
                                Mat4::from_translation(vec3(x, y, 0.0)),
                                Mat4::from_scale(vec3(1.0, 1.0, 0.0)),
                            ),
                        );
                        glyphs.render_at(
                            pass,
                            native,
                            Vec4::ONE,
                            vec4(0.0, 0.0, 0.0, 240.0 * f32::from_bits(0x3b808081)),
                            RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1),
                        );
                    }
                }
                Draw::Children => {}
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests;
