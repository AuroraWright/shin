//! Updated722f4/72938 pages, full-byte stencil panel and baked shell/hints.
use super::ViewState;
use crate::{
    adv::character_view::CharacterTextures,
    layer::render_params::{higurashi::HigurashiTransform, multiply_transforms as multiply},
};
use glam::{Mat4, Vec4, vec3, vec4};
use shin_core::primitives::color::UnormColor;
use shin_render::{
    ColorBlendType, DepthStencilState, RenderRequestBuilder, StencilFunction, StencilOperation,
    StencilPipelineState, StencilState, quad_vertices::QuadVertices, render_pass::RenderPass,
};

pub(super) struct Packet {
    pub matrix: Mat4,
    pub texture: String,
    pub rect: [f32; 4],
    pub uv: [f32; 4],
    pub color: u32,
    pub flash: f32,
    /// Raw original five-word stencil descriptor; the last word packs masks/ref.
    pub stencil: [u32; 5],
}
const DEFAULT: [u32; 5] = [7, 0, 0, 0, 0xffff];
const CLIP: [u32; 5] = [7, 0, 0, 3, 0xffffff];
const PAGE: [u32; 5] = [3, 0, 0, 0, 0x1ffff];
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
    out: &mut Vec<Packet>,
    matrix: Mat4,
    texture: impl Into<String>,
    rect: [f32; 4],
    uv: [f32; 4],
    color: u32,
    stencil: [u32; 5],
) {
    out.push(Packet {
        matrix,
        texture: texture.into(),
        rect,
        uv,
        color,
        flash: 0.,
        stencil,
    });
}
fn hint(
    out: &mut Vec<Packet>,
    matrix: Mat4,
    mut right: f32,
    y: f32,
    row: usize,
    swapped: bool,
) -> f32 {
    let (buttons, label_y, width) = if row == 0 {
        ([5, 0], 0., 248.)
    } else {
        ([2, -1], 48., 76.)
    };
    right -= width;
    quad(
        out,
        matrix,
        "help_manual",
        [right, y, right + width, y + 48.],
        [0., label_y / 96., width / 320., label_y / 96. + 0.5],
        u32::MAX,
        DEFAULT,
    );
    for mut key in buttons {
        if key < 0 {
            continue;
        }
        if swapped {
            if key == 1 {
                key = 2;
            } else if key == 2 {
                key = 1;
            }
        }
        right -= 48.;
        let u = (key * 48) as f32 / 288.;
        quad(
            out,
            matrix,
            "key",
            [right, y, right + 48., y + 48.],
            [u, 0., u + f32::from_bits(0x3e2aaaab), 1.],
            u32::MAX,
            DEFAULT,
        );
    }
    right - 10.
}
pub(super) fn packets(state: &ViewState, button_layout_swapped: bool) -> Vec<Packet> {
    let mut out = vec![];
    if !state.ready {
        return out;
    }
    let shell = projection();
    // Native fsub1920-(progress*1920), followed by74cac multiplication.
    let page = multiply(
        shell,
        Mat4::from_translation(vec3(1920. - state.progress * 1920., 0., 0.)),
    );
    quad(
        &mut out,
        page,
        "",
        [0., 162., 1920., 980.],
        FULL,
        0xffff0000,
        CLIP,
    );
    for i in 0..state.count {
        let offset = (i as i32).wrapping_mul(1500) as f32 - state.scroll;
        if offset >= 1920. || offset + 1920. <= 0. {
            continue;
        }
        let alpha = (1. - ((offset / 1500.) * 0.5).abs()).max(0.5);
        let color = (((alpha * 255.) as u32).min(255) << 24) | 0xffffff;
        let x = offset + 240.;
        quad(
            &mut out,
            page,
            format!("{:03}", i + 1),
            [x, 166., x + 1440., 976.],
            FULL,
            color,
            PAGE,
        );
    }
    let top = state.progress * 150. + (-150.);
    quad(
        &mut out,
        shell,
        "obi",
        [0., top, 1920., top + 150.],
        [0., 0., 1., 0.625],
        0xd8ffffff,
        DEFAULT,
    );
    quad(
        &mut out,
        shell,
        "caption",
        [72., top, 672., top + 144.],
        FULL,
        u32::MAX,
        DEFAULT,
    );
    quad(
        &mut out,
        shell,
        "close",
        [1764., top + 30., 1848., (top + 30.) + 84.],
        FULL,
        u32::MAX,
        DEFAULT,
    );
    out.last_mut().unwrap().flash = state.close_flash;
    let bottom = 1080. - state.progress * 88.;
    quad(
        &mut out,
        shell,
        "obi",
        [0., bottom, 1920., bottom + 88.],
        [0., f32::from_bits(0x3f222222), 1., 1.],
        0xd8ffffff,
        DEFAULT,
    );
    let y = bottom + 20.;
    let right = hint(&mut out, shell, 1872., y, 1, button_layout_swapped);
    hint(&mut out, shell, right, y, 0, button_layout_swapped);
    out
}
impl Packet {
    fn render(&self, atlas: &CharacterTextures, pass: &mut RenderPass) {
        let bytes = self.color.to_le_bytes();
        let vertices = QuadVertices::new()
            .with_box(self.rect[0], self.rect[1], self.rect[2], self.rect[3])
            .with_tex_box(self.uv[0], self.uv[1], self.uv[2], self.uv[3])
            .with_color(UnormColor::from_rgba(
                bytes[2], bytes[1], bytes[0], bytes[3],
            ));
        let pipeline = if self.stencil == CLIP {
            StencilPipelineState {
                pass_operation: StencilOperation::Increment,
                ..Default::default()
            }
        } else if self.stencil == PAGE {
            StencilPipelineState {
                function: StencilFunction::LessOrEqual,
                ..Default::default()
            }
        } else {
            StencilPipelineState::default()
        };
        let builder = RenderRequestBuilder::new()
            .color_blend_type(if self.texture.is_empty() {
                ColorBlendType::NoColor
            } else {
                ColorBlendType::Layer1
            })
            .depth_stencil(DepthStencilState {
                stencil: StencilState {
                    pipeline,
                    stencil_reference: (self.stencil[4] >> 16) as u8,
                },
                ..Default::default()
            });
        let transform = HigurashiTransform::convert_clip_depth(self.matrix);
        if self.texture.is_empty() {
            pass.clear(None, Some(0), None);
            vertices.fill(pass, builder, transform);
        } else if self.flash > 0. {
            vertices.render_button(
                pass,
                builder,
                atlas.get(&self.texture).as_source(),
                transform,
                vec4(1., 1., 1., self.flash),
                Vec4::ZERO,
            );
        } else {
            vertices.render_sprite(
                pass,
                builder,
                atlas.get(&self.texture).as_source(),
                transform,
            );
        }
    }
}
pub(super) struct View;
impl View {
    pub fn new() -> Self {
        Self
    }
    pub fn render(
        &self,
        state: &ViewState,
        atlas: Option<&CharacterTextures>,
        button_layout_swapped: bool,
        pass: &mut RenderPass,
    ) {
        for packet in packets(state, button_layout_swapped) {
            packet.render(atlas.expect("Ready Manual atlas"), pass);
        }
    }
}
#[cfg(test)]
mod tests;
