//! Original3cbd4 grid/shell +3d3fc bitmap digits +3d5ec key hints.
use crate::render::NATIVE_UI_PROJECTION;
use super::ViewState;
use crate::{
    adv::characters::view::CharacterTextures, layer::render_params::higurashi::HigurashiTransform,
};
use glam::{Mat4, Vec4, vec4};
use shin_core::primitives::color::UnormColor;
use shin_render::{
    ColorBlendType, RenderRequestBuilder, quad_vertices::QuadVertices, render_pass::RenderPass,
};
pub(super) struct Packet {
    pub matrix: Mat4,
    pub texture: String,
    pub rect: [f32; 4],
    pub uv: [f32; 4],
    pub color: u32,
    pub opaque: bool,
    pub flash: f32,
}
const FULL: [f32; 4] = [0., 0., 1., 1.];
fn alpha(v: f32) -> u32 {
    ((v.clamp(0., 1.) * 255.) as u32) << 24
}
fn quad(out: &mut Vec<Packet>, name: impl Into<String>, rect: [f32; 4], uv: [f32; 4], color: u32) {
    out.push(Packet {
        matrix: NATIVE_UI_PROJECTION,
        texture: name.into(),
        rect,
        uv,
        color,
        opaque: false,
        flash: 0.,
    });
}

const DIGITS: [[f32; 4]; 11] = [
    [0., 0., 38., 48.],
    [40., 0., 31., 48.],
    [80., 0., 36., 48.],
    [120., 0., 36., 48.],
    [160., 0., 40., 48.],
    [200., 0., 36., 48.],
    [240., 0., 38., 48.],
    [280., 0., 36., 48.],
    [320., 0., 38., 48.],
    [360., 0., 38., 48.],
    [400., 0., 34., 48.],
];
fn hint(
    out: &mut Vec<Packet>,
    mut right: f32,
    y: f32,
    row: usize,
    color: u32,
    swapped: bool,
) -> f32 {
    let (mut key, offset, width) = match row {
        0 => (0, 0., 214.),
        1 => (2, 48., 76.),
        2 => (1, 96., 78.),
        3 => (5, 144., 248.),
        _ => unreachable!(),
    };
    right -= width;
    quad(
        out,
        "help_cgmode",
        [right, y, right + width, y + 48.],
        [0., offset / 192., width / 320., offset / 192. + 0.25],
        color,
    );
    right += -48.;
    if swapped {
        if key == 1 {
            key = 2;
        } else if key == 2 {
            key = 1;
        }
    }
    let u = (key * 48) as f32 / 288.;
    quad(
        out,
        "key",
        [right, y, right + 48., y + 48.],
        [u, 0., u + f32::from_bits(0x3e2aaaab), 1.],
        color,
    );
    right + -10.
}
fn page_number(out: &mut Vec<Packet>, text: &str) {
    let mut x = 1870.
        - text
            .bytes()
            .map(|c| DIGITS[if c == b'/' { 10 } else { (c - b'0') as usize }][2] - 4.)
            .sum::<f32>();
    for c in text.bytes() {
        let d = DIGITS[if c == b'/' { 10 } else { (c - b'0') as usize }];
        let left = x + -2.;
        quad(
            out,
            "num",
            [left, 158., left + d[2], 158. + d[3]],
            [
                d[0] / 440.,
                d[1] / 48.,
                d[0] / 440. + d[2] / 440.,
                d[1] / 48. + d[3] / 48.,
            ],
            u32::MAX,
        );
        x += d[2] - 4.;
    }
}
/// Baked parent only. Actual d2240 completion glyphs at(48,1012) and the
/// borrowed private scope are rendered by the concrete owner after these packets.
pub(super) fn packets(state: &ViewState, swapped: bool) -> Vec<Packet> {
    let mut out = vec![];
    if !state.ready {
        return out;
    }
    quad(&mut out, "bg", [0., 0., 1920., 1080.], FULL, u32::MAX);
    out.last_mut().unwrap().opaque = true;
    for (i, cell) in state.cells.iter().enumerate() {
        let Some(entry) = state.entries.get(state.page * 12 + i) else {
            break;
        };
        let y = (222 + (i >> 2) * 252) as f32;
        let grow = (cell.focus + (-0.5)) * 0.5;
        let gx = grow * 75.;
        let gy = grow * 42.;
        let x = cell.x - gx * 0.5;
        let y = y - gy * 0.5;
        let rect = [x, y, (gx + 384.) + x, (gy + 216.) + y];
        quad(
            &mut out,
            if entry.locked() { "logo" } else { &entry.name },
            rect,
            FULL,
            u32::MAX,
        );
        let p = out.last_mut().unwrap();
        p.opaque = true;
        p.flash = cell.flash;
        if state.cell == i {
            let angle = state.spin * f32::from_bits(0x40490fd0);
            let a = (angle + angle).sin() * f32::from_bits(0x3e4ccccd) + f32::from_bits(0x3ecccccd);
            quad(&mut out, "", rect, FULL, 0xffffff | alpha(a));
        }
    }
    let entry = &state.entries[state.page * 12 + state.cell];
    if entry.pictures.len() >= 2 && !entry.locked() {
        let mut x = (1830 - 30 * entry.pictures.len() as i32) as f32;
        for unlocked in &entry.unlocked {
            quad(
                &mut out,
                "check",
                [x, 948., x + 32., 988.],
                FULL,
                if *unlocked { u32::MAX } else { 0xff808080 },
            );
            x += 30.;
        }
    }
    page_number(&mut out, &format!("{}/{}", state.page + 1, state.pages));
    quad(
        &mut out,
        "obi",
        [0., 0., 1920., 150.],
        [0., 0., 1., 0.625],
        0xd8ffffff,
    );
    quad(&mut out, "caption", [72., 0., 672., 144.], FULL, u32::MAX);
    quad(&mut out, "close", [1764., 30., 1848., 114.], FULL, u32::MAX);
    out.last_mut().unwrap().flash = state.close_flash;
    quad(
        &mut out,
        "obi",
        [0., 992., 1920., 1080.],
        [0., f32::from_bits(0x3f222222), 1., 1.],
        0xd8ffffff,
    );
    let right = hint(&mut out, 1872., 1012., 1, u32::MAX, swapped);
    let right = hint(
        &mut out,
        right,
        1012.,
        2,
        if entry.locked() { 0xff808080 } else { u32::MAX },
        swapped,
    );
    let right = hint(&mut out, right, 1012., 3, u32::MAX, swapped);
    hint(&mut out, right, 1012., 0, u32::MAX, swapped);
    out
}
impl Packet {
    fn render(&self, atlas: Option<&CharacterTextures>, pass: &mut RenderPass) {
        let c = self.color.to_le_bytes();
        let vertices = QuadVertices::new()
            .with_box(self.rect[0], self.rect[1], self.rect[2], self.rect[3])
            .with_tex_box(self.uv[0], self.uv[1], self.uv[2], self.uv[3])
            .with_color(UnormColor::from_rgba(c[2], c[1], c[0], c[3]));
        let builder = RenderRequestBuilder::new().color_blend_type(if self.opaque {
            ColorBlendType::Opaque
        } else {
            ColorBlendType::Layer1
        });
        let matrix = HigurashiTransform::convert_clip_depth(self.matrix);
        if self.texture.is_empty() {
            vertices.fill(pass, builder, matrix);
        } else if self.flash > 0. {
            vertices.render_button(
                pass,
                builder,
                atlas
                    .expect("Ready CG gallery atlas")
                    .get(&self.texture)
                    .as_source(),
                matrix,
                vec4(1., 1., 1., self.flash),
                Vec4::ZERO,
            );
        } else {
            vertices.render_sprite(
                pass,
                builder,
                atlas
                    .expect("Ready CG gallery atlas")
                    .get(&self.texture)
                    .as_source(),
                matrix,
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
            packet.render(atlas, pass);
        }
    }
}
#[cfg(test)]
mod tests;
