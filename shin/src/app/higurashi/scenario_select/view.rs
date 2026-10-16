//! Original b4e04/b5524: baked atlas rows, static shell, blackout then children.
use super::ViewState;
use crate::{
    adv::character_view::CharacterTextures, layer::render_params::higurashi::HigurashiTransform,
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
const FULL: [f32; 4] = [0., 0., 1., 1.];
fn alpha(v: f32) -> u32 {
    ((v.clamp(0., 1.) * 255.) as u32) << 24
}
fn quad(out: &mut Vec<Packet>, name: impl Into<String>, rect: [f32; 4], uv: [f32; 4], color: u32) {
    out.push(Packet {
        matrix: projection(),
        texture: name.into(),
        rect,
        uv,
        color,
        opaque: false,
        flash: 0.,
    });
}
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
        _ => unreachable!(),
    };
    right -= width;
    quad(
        out,
        "help_snrsel",
        [right, y, right + width, y + 48.],
        [
            0.,
            offset / 144.,
            width / 320.,
            offset / 144. + f32::from_bits(0x3eaaaaab),
        ],
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
/// Parent only: the concrete owner draws its last borrowed child snapshot last.
#[cfg(test)]
pub(super) fn packets(state: &ViewState, swapped: bool) -> Vec<Packet> {
    packets_with_size(state, swapped, (2160, 1800))
}
fn packets_with_size(state: &ViewState, swapped: bool, size: (u32, u32)) -> Vec<Packet> {
    let mut out = vec![];
    if state.ready {
        quad(&mut out, "ks_bg", [0., 0., 1920., 1080.], FULL, u32::MAX);
        out.last_mut().unwrap().opaque = true;
        for (i, row) in state.rows.iter().enumerate() {
            let x = if i & 1 == 0 { 930. } else { 270. };
            let y = state.base_y + i as f32 * 60.;
            let band = if state.mask & (1 << i) == 0 {
                12
            } else {
                row.art_band
            };
            if !super::art_band_available(band, size) {
                continue;
            }
            let uv = label_uv(band, size);
            quad(
                &mut out,
                "sce_menu",
                [x, y, x + 720., y + 120.],
                uv,
                0xffffff | alpha(row.opacity),
            );
            out.last_mut().unwrap().flash = row.flash;
            quad(
                &mut out,
                "cursor",
                [x, y, x + 720., y + 120.],
                FULL,
                0xffffff | alpha(row.highlight),
            );
        }
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
            if state.mask & (1 << (state.selected as u32)) == 0 {
                0xff808080
            } else {
                u32::MAX
            },
            swapped,
        );
        hint(&mut out, right, 1012., 0, u32::MAX, swapped);
    }
    if state.blackout > 0. {
        quad(
            &mut out,
            "",
            [0., 0., 1920., 1080.],
            FULL,
            alpha(state.blackout),
        );
    }
    out
}
fn label_uv(band: u32, (width, height): (u32, u32)) -> [f32; 4] {
    let u = ((band / 15) * 720) as f32 / width as f32;
    let v = ((band % 15) * 120) as f32 / height as f32;
    // Match the accepted updated native arithmetic exactly when dimensions agree.
    [u, v, u + 720. / width as f32, v + 120. / height as f32]
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
                    .expect("Ready ScenarioSelect atlas")
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
                    .expect("Ready ScenarioSelect atlas")
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
        let size = atlas
            .map(|atlas| {
                let size = atlas.get("sce_menu").size();
                (size.width, size.height)
            })
            .unwrap_or((2160, 1800));
        for packet in packets_with_size(state, button_layout_swapped, size) {
            packet.render(atlas, pass);
        }
    }
}
#[cfg(test)]
mod tests;
